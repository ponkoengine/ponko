//! The windowed game loop (winit + wgpu), behind the `window` feature.

use std::sync::Arc;
use web_time::Instant;

use glam::Vec2;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{CursorGrabMode, Window, WindowId};

use super::*;
use crate::error::{Error, ErrorKind};
use crate::input::{Key, MouseButton};
use crate::render::{FrameStatus, Renderer};

/// `Game::run`: opens the window and runs until it closes.
pub(super) fn run<S, U>(mut game: Game, setup: S) -> Result<(), Error>
where
    S: FnOnce(&mut Ctx) -> U,
    U: SetupResult,
{
    #[cfg(feature = "profiling")]
    let _puffin_server = start_profiler();

    #[cfg(target_arch = "wasm32")]
    crate::web::show_panics_in_console();
    connect_hotpatch();
    #[cfg_attr(not(target_os = "android"), allow(unused_mut))]
    let mut builder = EventLoop::<Ready>::with_user_event();
    // Android: the app android-activity started the game with.
    #[cfg(target_os = "android")]
    {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        let Some(app) = crate::platform::android::app() else {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "no Android app to run in: package the game with `ponko package --target android`",
            ));
        };
        builder.with_android_app(app.clone());
    }
    let event_loop = builder.build().map_err(|e| {
        let error = Error::new(ErrorKind::Unsupported, format!("could not start the event loop: {e}"));
        #[cfg(target_arch = "wasm32")]
        crate::web::finish_startup(Err(error.clone()));
        error
    })?;

    let clock = Clock::new(game.fixed_hz);
    let mut ctx = game.build_ctx(false);
    let control = game.start_control(&mut ctx, false);
    ctx.perf.profiler = crate::profiler::Profiler::from_env();
    // A benchmark measures how fast the game can run, not the display.
    if ctx.perf.profiler.benchmarking() {
        game.vsync = false;
    }
    match std::env::var("PONKO_VSYNC").as_deref() {
        Ok("0") => game.vsync = false,
        Ok("1") => game.vsync = true,
        _ => {}
    }
    let mut app = App {
        #[cfg(target_arch = "wasm32")]
        proxy: Some(event_loop.create_proxy()),
        #[cfg(target_arch = "wasm32")]
        startup_context: None,
        control,
        #[cfg(all(target_arch = "wasm32", feature = "web-control"))]
        browser_control: None,
        config: game,
        setup: Some(setup),
        started: false,
        error: None,
        window: None,
        cursor_sources: Default::default(),
        layout: None,
        ctx,
        clock,
        last_frame: Instant::now(),
        occluded: false,
        #[cfg(feature = "inspector")]
        inspector: None,
        #[cfg(feature = "accessibility")]
        access: None,
        #[cfg(feature = "accessibility")]
        access_gap_until: None,
        #[cfg(target_os = "android")]
        android: AndroidFollow::default(),
    };

    event_loop.run_app(&mut app).map_err(|e| {
        Error::new(
            ErrorKind::Unsupported,
            format!("the event loop stopped with an error: {e}"),
        )
    })?;
    // While the run's crash reports are still on (they end with `app`).
    if let Some(error) = &app.error {
        crate::crash::report_error(error);
    }
    app.error.map_or(Ok(()), Err)
}

/// Starts the puffin server; the returned handle keeps it running.
#[cfg(feature = "profiling")]
fn start_profiler() -> Option<puffin_http::Server> {
    const ADDR: &str = "127.0.0.1:8585";
    puffin::set_scopes_on(true);
    match puffin_http::Server::new(ADDR) {
        Ok(server) => {
            eprintln!("ponko: profiler running; view it with `puffin_viewer --url {ADDR}`");
            Some(server)
        }
        Err(e) => {
            eprintln!("ponko: could not start the profiler on {ADDR}: {e}");
            None
        }
    }
}

/// The end of a benchmark report: the meshes that cost the most triangles.
#[cfg(feature = "3d")]
fn print_mesh_costs(ctx: &Ctx) {
    let costs = ctx.gfx.mesh_costs();
    if costs.is_empty() {
        return;
    }
    eprintln!("-- heaviest meshes (camera models / triangles, shadow models / triangles) --");
    for c in costs.iter().take(15) {
        eprintln!(
            "{:>7} {:>7.2}M  {:>7} {:>7.2}M  {}",
            c.models,
            c.triangles as f64 / 1e6,
            c.shadow_models,
            c.shadow_triangles as f64 / 1e6,
            c.name.as_deref().unwrap_or("(made in code)")
        );
    }
    // Every mesh, for sorting and grouping elsewhere.
    if let Some(path) = std::env::var_os("PONKO_BENCH_MESHES") {
        let mut csv = String::from("models,triangles,shadow_models,shadow_triangles,name\n");
        for c in &costs {
            csv.push_str(&format!(
                "{},{},{},{},{}\n",
                c.models,
                c.triangles,
                c.shadow_models,
                c.shadow_triangles,
                c.name.as_deref().unwrap_or("")
            ));
        }
        if let Err(e) = std::fs::write(&path, csv) {
            eprintln!("ponko: could not write {}: {e}", path.to_string_lossy());
        }
    }
}

/// The GPU is ready (with the assets, on the web): setup can run.
struct Ready(Result<Renderer, Error>);

/// What the window loop follows on Android between frames.
#[cfg(target_os = "android")]
#[derive(Default)]
struct AndroidFollow {
    /// Frames run, to space out the JNI reads.
    frames: u32,
    /// A call rings or is on: the game's sound waits.
    in_call: bool,
}

struct App<S> {
    /// Delivers [`Ready`] from the task that opens the GPU (web builds).
    #[cfg(target_arch = "wasm32")]
    proxy: Option<winit::event_loop::EventLoopProxy<Ready>>,
    /// Startup's lost-context listener (which stops startup), then the
    /// running game's (which reports a failure to the page).
    #[cfg(target_arch = "wasm32")]
    startup_context: Option<crate::web::ContextLostListener>,
    config: Game,
    setup: Option<S>,
    /// Setup has run; frames may run.
    started: bool,
    error: Option<Error>,
    window: Option<Arc<Window>>,
    cursor_sources: super::window_service::CursorQueue,
    /// Follows the player's keyboard layout, for key labels.
    layout: Option<crate::input::key_labels::watch::LayoutWatch>,
    ctx: Ctx,
    clock: Clock,
    last_frame: Instant,
    /// Window hidden/minimized: stop drawing, poll slowly instead of spinning.
    occluded: bool,
    /// inspector overlay, when the game has it on.
    #[cfg(feature = "inspector")]
    inspector: Option<crate::inspector::Inspector>,
    /// Gives `ctx.ui` to the system's accessibility API (screen readers).
    #[cfg(feature = "accessibility")]
    access: Option<crate::ui::UiAccessAdapter>,
    /// While assistive technology is reading the window: when the next
    /// frame may start (see [`ACCESS_GAP`]).
    #[cfg(feature = "accessibility")]
    access_gap_until: Option<Instant>,
    /// What the engine follows on Android between frames.
    #[cfg(target_os = "android")]
    android: AndroidFollow,
    /// Kept for the whole run: dropping it closes the endpoint. The last
    /// field, so it is dropped after `ctx`: a reply `ctx` still holds (a
    /// screenshot waiting for its frame) is let go first, its client is told
    /// the game is shutting down, and the endpoint's exit doesn't wait for
    /// an answer that can no longer come.
    control: Option<Control>,
    #[cfg(all(target_arch = "wasm32", feature = "web-control"))]
    browser_control: Option<crate::control::web::BrowserControl>,
}

impl<S, U> App<S>
where
    S: FnOnce(&mut Ctx) -> U,
    U: SetupResult,
{
    fn logical_window_size(window: &Window) -> Vec2 {
        let size: LogicalSize<f32> = crate::window::surface_size(window).to_logical(window.scale_factor());
        Vec2::new(size.width, size.height)
    }

    /// Vsync off on a desktop: frames run back to back from `about_to_wait`,
    /// because some systems (macOS) deliver redraw requests only at the
    /// display's refresh rate, which would cap the game there anyway.
    fn unpaced(&self) -> bool {
        !self.config.vsync && cfg!(not(target_arch = "wasm32"))
    }

    /// A frame, unless the app is suspended (no drawing in the background).
    fn run_frame(&mut self, event_loop: &ActiveEventLoop) {
        if self.ctx.window.suspended() {
            return;
        }
        self.frame(event_loop);
    }

    /// A lifecycle event from the system ([`crate::window::WindowEvent::Suspended`],
    /// `Resumed`, `LowMemory`): recorded and sent to the game's systems.
    /// Whether it changed anything (a repeated resume doesn't).
    fn lifecycle(&mut self, event: crate::window::WindowEvent) -> bool {
        if !self.ctx.window.feed_event(&event) {
            return false;
        }
        self.ctx.events.send(event);
        #[cfg(target_os = "android")]
        self.follow_android_audio();
        true
    }

    /// Android: the window's insets (system bars, the display cutout, the
    /// on-screen keyboard), read through JNI, as the safe area and the
    /// keyboard inset.
    #[cfg(target_os = "android")]
    fn follow_android_insets(&mut self) {
        let Some(window) = &self.window else { return };
        let Some((edges, keyboard)) = crate::platform::android::window_insets() else {
            return;
        };
        let scale = window.scale_factor() as f32;
        let [top, right, bottom, left] = edges.map(|px| px as f32 / scale);
        self.ctx
            .window
            .feed_platform_safe_insets(crate::window::SafeInsets::new(top, right, bottom, left));
        self.ctx.window.feed_keyboard_inset(keyboard as f32 / scale);
    }

    /// Android: the game's sound has the device while the app is in front
    /// and no call is on; else the output closes (no stream runs in the
    /// background). Audio focus follows the session category: every
    /// category but `Ambient` asks for it, so the player's music pauses.
    #[cfg(target_os = "android")]
    fn follow_android_audio(&mut self) {
        let away = self.ctx.window.suspended();
        self.ctx.audio.hold_output(away || self.android.in_call);
        let focus = !away && self.ctx.audio.session_category() != crate::audio::AudioSessionCategory::Ambient;
        crate::platform::android::audio_focus(focus);
    }

    /// Android, once a frame: the insets (every frame while the keyboard
    /// may be open, so it is followed as it slides; else twice a second)
    /// and the phone's call state (each second).
    #[cfg(target_os = "android")]
    fn follow_android(&mut self) {
        self.android.frames = self.android.frames.wrapping_add(1);
        if self.ctx.input.ime_allowed()
            || self.ctx.window.keyboard_inset() > 0.0
            || self.android.frames.is_multiple_of(30)
        {
            self.follow_android_insets();
        }
        if self.android.frames.is_multiple_of(60) {
            let in_call = crate::platform::android::in_call();
            if in_call != self.android.in_call {
                self.android.in_call = in_call;
                eprintln!(
                    "ponko: {}",
                    if in_call {
                        "a call has the audio"
                    } else {
                        "the call ended"
                    }
                );
            }
            self.follow_android_audio();
        }
    }

    /// iOS reports its safe area as the window's inner frame (the outer
    /// one is the screen's).
    #[cfg(target_os = "ios")]
    fn follow_ios_insets(&mut self, window: &Window) {
        let (Ok(outer), Ok(inner)) = (window.outer_position(), window.inner_position()) else {
            return;
        };
        let (outer_size, inner_size) = (window.outer_size(), window.inner_size());
        let frame = |p: PhysicalPosition<i32>, s: winit::dpi::PhysicalSize<u32>| {
            (
                Vec2::new(p.x as f32, p.y as f32),
                Vec2::new(s.width as f32, s.height as f32),
            )
        };
        let insets = crate::window::insets_of_frame(
            frame(outer, outer_size),
            frame(inner, inner_size),
            window.scale_factor() as f32,
        );
        self.ctx.window.feed_platform_safe_insets(insets);
    }

    /// How much of the window the on-screen keyboard covers, from its last
    /// frame (both in screen points).
    #[cfg(target_os = "ios")]
    fn follow_ios_keyboard(&mut self) {
        let (Some(window), Some((top, height))) = (&self.window, crate::platform::ios::keyboard_frame()) else {
            return;
        };
        let scale = window.scale_factor();
        let at = window.outer_position().unwrap_or_default().to_logical::<f32>(scale);
        let size = window.outer_size().to_logical::<f32>(scale);
        let cover = crate::window::keyboard_cover(at.y, size.height, top, height);
        self.ctx.window.feed_keyboard_inset(cover);
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        if !self.started {
            return;
        }
        #[cfg(target_os = "ios")]
        self.follow_ios_keyboard();
        #[cfg(target_os = "android")]
        self.follow_android();

        if let Some(control) = &self.control {
            control.poll(&mut self.ctx);
        }
        #[cfg(all(target_arch = "wasm32", feature = "web-control"))]
        if let Some(control) = &self.browser_control {
            control.poll(&mut self.ctx);
        }
        let now = Instant::now();
        let frame_dt = now.duration_since(self.last_frame).as_secs_f64();
        self.last_frame = now;
        let since_start = |t: Instant| t.duration_since(now).as_secs_f32() * 1000.0;

        #[cfg(feature = "profiling")]
        puffin::GlobalProfiler::lock().new_frame();
        let timing = frame_timing(&mut self.clock, &mut self.ctx, frame_dt);
        self.ctx.begin_frame(timing);
        // A layout switched or answered since the last frame relabels the keys.
        if let Some(labels) = self.layout.as_mut().and_then(|l| l.poll()) {
            self.ctx.input.set_key_labels(labels);
        }
        // In a browser, what the hidden IME text box saw since the last frame.
        #[cfg(target_arch = "wasm32")]
        crate::web::ime::drain(&mut self.ctx.input);
        self.ctx.frame_prepare_services();
        #[cfg(feature = "3d")]
        self.ctx.gfx.apply_shader_checks();
        let systems_start = Instant::now();
        self.config.run_systems(&mut self.ctx, timing.fixed_steps);
        let systems_ms = systems_start.elapsed().as_secs_f32() * 1000.0;
        // What the systems left in the UI goes to the screen reader, and its
        // requests are queued for the UI's next frame.
        #[cfg(feature = "accessibility")]
        if let (Some(access), Some(window)) = (&mut self.access, &self.window) {
            access.update(&mut self.ctx.ui, window.scale_factor() as f32);
        }
        self.ctx.input.end_frame();
        #[cfg(feature = "inspector")]
        if let (Some(inspector), Some(window)) = (&mut self.inspector, &self.window) {
            inspector.frame(window, &mut self.ctx);
        }

        if let Some(source) = self.cursor_sources.borrow_mut().take()
            && let Some(window) = &self.window
        {
            window.set_cursor(event_loop.create_custom_cursor(source));
        }
        if self.ctx.quit_requested() || self.ctx.window.close_requested() {
            event_loop.exit();
            return;
        }
        // Back that no system canceled: the app leaves, as Android apps do.
        #[cfg(target_os = "android")]
        if self.ctx.window.take_back() {
            crate::platform::android::leave();
        }
        if self.ctx.input.cursor_lock_changed
            && let Some(window) = &self.window
        {
            self.ctx.input.cursor_lock_changed = false;
            apply_cursor_lock(window, self.ctx.input.cursor_locked());
        }
        if let Some(window) = &self.window {
            self.ctx.input.apply_ime(window);
        }

        // With vsync, presenting blocks until the display is ready, which
        // paces the loop. So we can ask for the next frame right away.
        self.ctx.draw_perf_hud();
        let render_start = Instant::now();
        let status = self.ctx.render_frame();
        let render_ms = render_start.elapsed().as_secs_f32() * 1000.0;
        #[cfg(all(target_arch = "wasm32", feature = "web-control"))]
        if let Some(control) = &self.browser_control {
            use winit::platform::web::WindowExtWebSys;
            let canvas = || self.window.as_ref().and_then(|window| window.canvas());
            control.after_frame(canvas, status == FrameStatus::Presented);
        }
        match status {
            FrameStatus::Presented | FrameStatus::Skipped | FrameStatus::Offscreen => self.occluded = false,
            FrameStatus::Occluded => self.occluded = true,
        }
        let sample = crate::profiler::FrameSample {
            frame_ms: frame_dt as f32 * 1000.0,
            systems_start_ms: since_start(systems_start),
            systems_ms,
            render_start_ms: since_start(render_start),
            render_ms,
            wait_ms: self.ctx.perf.display_wait_ms(),
            gpu_ms: self.ctx.perf.last_render_stats.gpu_ms,
            presented: matches!(status, FrameStatus::Presented | FrameStatus::Offscreen),
        };
        if self.ctx.record_frame(sample) {
            #[cfg(feature = "3d")]
            print_mesh_costs(&self.ctx);
            event_loop.exit();
            return;
        }
        if !self.occluded
            && !self.unpaced()
            && let Some(window) = &self.window
        {
            // A screen reader is asking: leave the event loop idle for a
            // moment before the next frame (`about_to_wait` asks for it).
            #[cfg(feature = "accessibility")]
            if let Some(until) = access_gap(
                self.access.as_ref().is_some_and(|a| a.answers_on_event_loop()),
                Instant::now(),
            ) {
                self.access_gap_until = Some(until);
                return;
            }
            window.request_redraw();
        }
    }
}

/// How long the event loop is left idle between two vsync-paced frames
/// while assistive technology is reading the window.
///
/// A screen reader's questions arrive through the event loop, one per pass.
/// A paced frame spends nearly all of its time waiting for the display
/// inside the frame, so without a pause the game answers one question per
/// frame: a screen reader that asks hundreds waits seconds and reports the
/// game as not responding. During the pause every waiting question is
/// answered. The frame still ends on the display's next refresh, as long as
/// its own work fits in what is left of the refresh interval.
#[cfg(feature = "accessibility")]
const ACCESS_GAP: std::time::Duration = std::time::Duration::from_millis(4);

/// When the next paced frame may start: `None` for right away, or the end
/// of the pause that lets assistive technology be answered.
#[cfg(feature = "accessibility")]
fn access_gap(read_by_assistive_technology: bool, now: Instant) -> Option<Instant> {
    read_by_assistive_technology.then(|| now + ACCESS_GAP)
}

#[cfg(all(test, feature = "accessibility"))]
mod access_gap_tests {
    use super::*;

    #[test]
    fn a_window_nobody_reads_starts_its_next_frame_right_away() {
        assert_eq!(access_gap(false, Instant::now()), None);
    }

    #[test]
    fn a_window_a_screen_reader_reads_leaves_the_event_loop_idle_before_the_next_frame() {
        let now = Instant::now();
        let until = access_gap(true, now).expect("a pause");
        assert_eq!(until - now, ACCESS_GAP);
        // Short enough to keep a 60 Hz frame on its refresh, long enough to
        // answer many questions (each takes well under a millisecond).
        assert!(ACCESS_GAP >= std::time::Duration::from_millis(2));
        assert!(ACCESS_GAP <= std::time::Duration::from_millis(8));
    }
}

impl<S, U> App<S>
where
    S: FnOnce(&mut Ctx) -> U,
    U: SetupResult,
{
    /// The GPU is open: run setup and start the frames.
    fn start(&mut self, renderer: Renderer) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let mut renderer = renderer;
        // Benchmarks draw offscreen: a window is shown at most once per display
        // refresh, so above that rate presenting stalls now and then.
        // PONKO_BENCH_PRESENT=1 draws to the window when it can take a frame.
        let present = std::env::var("PONKO_BENCH_PRESENT").is_ok_and(|v| v == "1");
        let benchmarking = self.ctx.perf.profiler.benchmarking();
        renderer.set_draw_hidden(benchmarking, benchmarking && !present);
        self.ctx.gfx.renderer = Some(renderer);
        // A crash report from here on (setup too) names the GPU.
        crate::crash::note_gpu(&mut self.ctx);
        self.ctx.set_window_size(Self::logical_window_size(&window));
        self.ctx.set_scale_factor(window.scale_factor() as f32);
        #[cfg(target_arch = "wasm32")]
        self.ctx
            .window
            .feed_platform_safe_insets(crate::window::web_safe_insets(&window));
        // Edit mode (`ponko editor`): the inspector's panels are the window.
        #[cfg(feature = "editor")]
        let edit_mode = crate::inspector::app::requested();
        #[cfg(not(feature = "editor"))]
        crate::game::warn_without_editor();
        #[cfg(feature = "inspector")]
        {
            #[cfg(feature = "editor")]
            let (on, open) = if edit_mode.is_some() {
                (true, true)
            } else {
                crate::inspector::enabled(self.config.inspector)
            };
            #[cfg(not(feature = "editor"))]
            let (on, open) = crate::inspector::enabled(self.config.inspector);
            if on {
                let max_side = self.ctx.gfx.renderer.as_ref().map_or(2048, Renderer::max_texture_size);
                let mut inspector = crate::inspector::Inspector::new(&window, max_side as usize);
                inspector.open = open;
                #[cfg(feature = "editor")]
                if let Some(start) = edit_mode {
                    inspector.panels.app = Some(crate::inspector::app::App::new(start));
                    // Setup runs paused: the editor shows the world it made.
                    self.ctx.time.pause();
                }
                self.inspector = Some(inspector);
            }
        }

        // The keys' labels on the system's layout, for prompts, followed
        // while the window is open.
        let mut layout = crate::input::key_labels::watch::LayoutWatch::new(&window);
        if let Some(labels) = layout.poll() {
            self.ctx.input.set_key_labels(labels);
        }
        self.layout = Some(layout);

        // The built-in controllers, unless the game brought its own backend
        // (setup can still replace them with `set_gamepad_backend`): SDL's
        // with `gamepad-sdl` (gyro, trigger rumble, rumble on macOS), else
        // gilrs's; gilrs's too when SDL can't start and `gamepad` is on.
        #[cfg(all(feature = "gamepad-sdl", not(target_arch = "wasm32")))]
        if !self.ctx.has_gamepad_backend() {
            match crate::gamepad_sdl_sys::Sdl::start() {
                Ok(sdl) => self.ctx.set_gamepad_backend(crate::gamepad_sdl::SdlGamepads::new(sdl)),
                Err(e) => eprintln!("ponko: SDL's gamepads unavailable ({e})"),
            }
        }
        #[cfg(all(feature = "gamepad", not(target_arch = "wasm32")))]
        if !self.ctx.has_gamepad_backend() {
            self.ctx.set_gamepad_backend(crate::gamepad::GilrsBackend::new());
        }
        // On the web, the browser's gamepads read on the page's thread,
        // keeping no browser object (see `gamepad_web`).
        #[cfg(all(feature = "gamepad", target_arch = "wasm32"))]
        if !self.ctx.has_gamepad_backend() {
            self.ctx
                .set_gamepad_backend(crate::gamepad_web::WebGamepads::new(crate::gamepad_web::BrowserPads));
        }

        // The web's storage is read now (see `resumed`); a canvas has no
        // default-sized window to show first.
        #[cfg(target_arch = "wasm32")]
        if self.config.restore_window {
            match self.ctx.window.restore_settings(&self.ctx.storage) {
                // The title belongs to the game, as on the desktop.
                Ok(()) => {
                    let _ = self.ctx.window.set_title(self.config.title.clone());
                }
                Err(e) => eprintln!("ponko: opening the window with its default settings: {e}"),
            }
        }
        if let Some(setup) = self.setup.take() {
            self.ctx.set_in_setup(true);
            if let Err(error) = run_setups(std::mem::take(&mut self.config.setups), &mut self.ctx) {
                self.ctx.set_in_setup(false);
                self.error = Some(error);
                return;
            }
            let frame_fn = setup(&mut self.ctx).into_frame_fn();
            self.ctx.set_in_setup(false);
            self.ctx.apply_commands();
            // A sample count set in setup, before the first frame is drawn.
            self.ctx.follow_msaa();
            if let Some(f) = frame_fn {
                self.config.add_setup_closure(f);
            }
            self.started = true;
            #[cfg(all(target_arch = "wasm32", feature = "web-control"))]
            {
                self.browser_control = crate::control::web::BrowserControl::from_page();
            }
        }

        // Don't count setup time (e.g. asset loading) as the first frame's dt.
        self.last_frame = Instant::now();
        window.request_redraw();
    }
}

impl<S, U> ApplicationHandler<Ready> for App<S>
where
    S: FnOnce(&mut Ctx) -> U,
    U: SetupResult,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.error.is_some() {
            event_loop.exit();
            return;
        }
        if let Some(window) = self.window.clone() {
            // Back in front (iOS and Android; the first resume is the
            // launch and changes nothing).
            #[cfg(not(target_arch = "wasm32"))]
            self.lifecycle(crate::window::WindowEvent::Resumed);
            #[cfg(target_os = "ios")]
            self.follow_ios_insets(&window);
            #[cfg(target_os = "android")]
            self.follow_android_insets();
            if let Some(renderer) = &mut self.ctx.gfx.renderer {
                renderer.surface_event(crate::render::surface_lifecycle::SurfaceEvent::Suspended(false));
                // Android made a new native surface for the window.
                #[cfg(target_os = "android")]
                renderer.restore_window_surface();
                let size = crate::window::surface_size(&window);
                renderer.resize(size.width, size.height);
            }
            window.request_redraw();
            return;
        }

        let attributes = Window::default_attributes().with_title(self.config.title.clone());
        // A game has the whole screen: no status bar over it.
        #[cfg(target_os = "ios")]
        let attributes = {
            use winit::platform::ios::WindowAttributesExtIOS;
            attributes.with_prefers_status_bar_hidden(true)
        };
        // The first window opens with its final settings (the saved ones with
        // `restore_window_settings`), so no default-sized window shows first.
        // The web reads its storage after the canvas exists (see `start`).
        #[cfg(not(target_arch = "wasm32"))]
        let (attributes, settings) = {
            let handles: Vec<_> = event_loop.available_monitors().collect();
            let monitors: Vec<_> = handles.iter().map(super::window_service::monitor_info).collect();
            let mut settings = self.config.first_window_settings(&self.ctx.storage, &monitors);
            let fullscreen = super::window_service::fullscreen_on(&handles, &settings.mode).unwrap_or_else(|e| {
                eprintln!("ponko: restoring window settings in windowed mode: {e}");
                settings.mode = crate::window::WindowMode::Windowed;
                None
            });
            (
                super::window_service::first_window_attributes(attributes, &settings, fullscreen),
                settings,
            )
        };
        #[cfg(target_arch = "wasm32")]
        let (attributes, settings) = {
            let settings = self.config.default_window_settings();
            let attributes = attributes.with_inner_size(LogicalSize::new(self.config.width, self.config.height));
            (crate::web::canvas_attributes(attributes), settings)
        };
        // The accessibility adapter has to be on the view (macOS) or the
        // window (Windows) before it is first shown, so the window starts
        // hidden and is shown right after.
        #[cfg(all(feature = "accessibility", any(target_os = "macos", target_os = "windows")))]
        let attributes = attributes.with_visible(false);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => window,
            Err(e) => {
                let error = Error::new(ErrorKind::Unsupported, format!("could not create the window: {e}"));
                #[cfg(target_arch = "wasm32")]
                crate::web::finish_startup(Err(error.clone()));
                self.error = Some(error);
                event_loop.exit();
                return;
            }
        };
        #[cfg(feature = "accessibility")]
        {
            self.access = Some(crate::ui::UiAccessAdapter::for_window(&window, &self.config.title));
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            window.set_visible(true);
        }
        let window = Arc::new(window);
        self.ctx.window.attach(
            super::window_service::Backend::new(window.clone(), self.cursor_sources.clone()),
            settings,
        );
        self.window = Some(window.clone());
        self.ctx
            .input
            .set_platform_touch(crate::input::touch::platform_has_touch());
        #[cfg(target_os = "ios")]
        {
            crate::platform::ios::adopt_scene(&window);
            self.follow_ios_insets(&window);
            crate::platform::ios::watch_keyboard();
        }
        #[cfg(target_os = "android")]
        {
            self.follow_android_insets();
            self.follow_android_audio();
        }
        #[cfg(target_arch = "wasm32")]
        crate::input::touch::prepare_canvas(&window);
        let display = event_loop.owned_display_handle();
        let vsync = self.config.vsync;
        // The 3D frame's samples per pixel so far (setup hasn't run yet: the
        // default). A count set by a setup callback applies right after it
        // (`Ctx::follow_msaa`), before the first frame.
        #[cfg(feature = "3d")]
        let msaa = self.ctx.scene3d.post.msaa;
        #[cfg(not(feature = "3d"))]
        let msaa = 4;
        // HDR output as `Game::hdr_output` set it: the window opens for it.
        let hdr = self.ctx.gfx.hdr_output;

        // Desktop: open the GPU and start right away.
        #[cfg(not(target_arch = "wasm32"))]
        match Renderer::new(window, display, vsync, msaa, hdr) {
            Ok(renderer) => self.start(renderer),
            Err(error) => self.error = Some(error),
        }
        if self.error.is_some() {
            event_loop.exit();
        }

        // Web: the browser can't block, so fetch the assets, read the
        // player's stored files (`ctx.storage`) and open the GPU
        // in a task, which sends `Ready` when done.
        #[cfg(target_arch = "wasm32")]
        if let Some(proxy) = self.proxy.take() {
            let context_proxy = proxy.clone();
            let context_lost = std::rc::Rc::new(std::cell::Cell::new(false));
            let listener_lost = context_lost.clone();
            self.startup_context = crate::web::ContextLostListener::new(&window, move || {
                listener_lost.set(true);
                let _ = context_proxy.send_event(Ready(Err(crate::web::context_lost())));
            });
            #[cfg(target_feature = "atomics")]
            let threads = self.config.web_threads;
            wasm_bindgen_futures::spawn_local(async move {
                // Worker threads first: setup and every frame after it may
                // use them, and none may run before they are up (the
                // page's thread can't wait for a worker to start).
                #[cfg(target_feature = "atomics")]
                crate::web_threads::start(threads).await;
                crate::web::fetch_assets().await;
                crate::storage::preload_web().await;
                // A context-loss event can stop startup while assets are loading.
                if context_lost.get() {
                    return;
                }
                let renderer = Renderer::new_async(window, display, vsync, msaa, hdr)
                    .await
                    .map_err(|error| error.context("Could not initialize WebGPU or WebGL2"));
                let _ = proxy.send_event(Ready(renderer));
            });
        }
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        // The app is losing the screen (iOS: about to go inactive; Android:
        // its surface is going). One more frame while it still may draw,
        // so the game's systems see `Suspended` (and can save); then none
        // until it resumes. The web's page hide is left as it was.
        #[cfg(not(target_arch = "wasm32"))]
        if self.lifecycle(crate::window::WindowEvent::Suspended) {
            self.frame(event_loop);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = event_loop;
        if let Some(renderer) = &mut self.ctx.gfx.renderer {
            renderer.surface_event(crate::render::surface_lifecycle::SurfaceEvent::Suspended(true));
            // Its native surface is destroyed once this returns.
            #[cfg(target_os = "android")]
            renderer.release_window_surface();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, Ready(renderer): Ready) {
        if self.started || self.error.is_some() {
            return;
        }
        match renderer {
            Ok(renderer) => self.start(renderer),
            Err(error) => self.error = Some(error),
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.startup_context = None;
            crate::web::finish_startup(self.error.clone().map_or(Ok(()), Err));
            // While playing, a lost context stops the game for good: wgpu
            // can't draw on a restored one. The page says so.
            if self.started
                && let Some(window) = &self.window
            {
                self.startup_context = crate::web::ContextLostListener::new(window, || {
                    crate::web::report_failure(crate::web::context_lost_while_playing());
                });
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(error) = &self.error {
            eprintln!("{error}");
        }
        if self.error.is_some() {
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(window) = self.window.clone() else {
            return;
        };

        // Browsers allow sound only from the first click, tap or key press
        // on (the audio output opens then); on the desktop it does nothing.
        if is_user_gesture(&event) {
            self.ctx.audio.backend.user_gesture();
        }

        #[cfg(feature = "inspector")]
        if let Some(inspector) = &mut self.inspector {
            if let WindowEvent::KeyboardInput { event: key, .. } = &event
                && key.state == ElementState::Pressed
                && !key.repeat
                && key.physical_key == PhysicalKey::Code(crate::inspector::TOGGLE_CODE)
            {
                inspector.open = !inspector.open;
                // Its text fields may have changed the window's IME.
                self.ctx.input.reapply_ime();
                window.request_redraw();
                return;
            }
            if inspector.on_event(&window, &event) {
                return;
            }
        }

        #[cfg(feature = "accessibility")]
        if let Some(access) = &mut self.access {
            match &event {
                WindowEvent::Focused(focused) => access.set_window_focused(*focused),
                // AT-SPI is told where the window is (the others ask it).
                WindowEvent::Moved(_) | WindowEvent::Resized(_) => access.follow_window(&window),
                _ => {}
            }
        }

        let engine_event = match &event {
            WindowEvent::CloseRequested => Some(crate::window::WindowEvent::CloseRequested),
            WindowEvent::Focused(v) => Some(crate::window::WindowEvent::Focused(*v)),
            WindowEvent::DroppedFile(p) => Some(crate::window::WindowEvent::FileDropped(p.clone())),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                Some(crate::window::WindowEvent::ScaleFactorChanged(*scale_factor as f32))
            }
            WindowEvent::Resized(_) => Some(crate::window::WindowEvent::Minimized(
                window.is_minimized().unwrap_or(false),
            )),
            _ => None,
        };
        if let Some(event) = engine_event
            && self.ctx.window.feed_event(&event)
        {
            self.ctx.events.send(event);
            window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => {
                window.request_redraw();
            }

            WindowEvent::RedrawRequested => self.run_frame(event_loop),

            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                let size = crate::window::surface_size(&window);
                if let Some(renderer) = self.ctx.gfx.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                if crate::render::surface_lifecycle::SurfaceLifecycle::nonzero_size(size.width, size.height).is_some() {
                    self.ctx.set_window_size(Self::logical_window_size(&window));
                }
                self.ctx.set_scale_factor(window.scale_factor() as f32);
                // Turning the phone moves the notch.
                #[cfg(target_arch = "wasm32")]
                self.ctx
                    .window
                    .feed_platform_safe_insets(crate::window::web_safe_insets(&window));
                #[cfg(target_os = "ios")]
                self.follow_ios_insets(&window);
                #[cfg(target_os = "android")]
                self.follow_android_insets();
                window.request_redraw();
            }

            WindowEvent::Occluded(occluded) => {
                if let Some(renderer) = &mut self.ctx.gfx.renderer {
                    renderer.surface_event(crate::render::surface_lifecycle::SurfaceEvent::Occluded(occluded));
                }
                // A benchmark draws offscreen while hidden and keeps going.
                self.occluded = occluded && !self.ctx.perf.profiler.benchmarking();
                if !occluded {
                    window.request_redraw();
                }
            }

            WindowEvent::Focused(false) => self.ctx.input.release_all(),

            WindowEvent::Focused(true) => {
                // The system lets go of the cursor while the window is away.
                if self.ctx.input.cursor_locked() {
                    apply_cursor_lock(&window, true);
                }
                // The player may have switched keyboard layouts meanwhile.
                if let Some(layout) = &mut self.layout {
                    layout.refresh();
                }
            }

            WindowEvent::Ime(ime) => self.ctx.input.feed_ime(crate::input::ImeEvent::from_winit(ime)),
            // Android's Back (button or gesture), as winit names it.
            #[cfg(target_os = "android")]
            WindowEvent::KeyboardInput { event, .. }
                if event.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::BrowserBack) =>
            {
                if event.state == ElementState::Pressed && !event.repeat {
                    let back = crate::window::WindowEvent::BackRequested;
                    self.ctx.window.feed_event(&back);
                    self.ctx.events.send(back);
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && let Some(text) = &event.text
                {
                    self.ctx.input.add_text(text);
                }
                if event.repeat {
                    return;
                }
                if let PhysicalKey::Code(code) = event.physical_key
                    && let Some(key) = Key::from_winit(code)
                {
                    // What the key types names it in prompts (Z for
                    // `Key::W` on a French keyboard).
                    if event.state == ElementState::Pressed {
                        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                        let typed = {
                            use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
                            crate::input::key_labels::typed_char(&event.key_without_modifiers(), false)
                        };
                        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
                        let typed = crate::input::key_labels::typed_char(&event.logical_key, true);
                        if let Some(c) = typed {
                            self.ctx.input.learn_key_label(key, c);
                        }
                    }
                    self.ctx.input.set_key(key, event.state == ElementState::Pressed);
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(button) = MouseButton::from_winit(button) {
                    self.ctx.input.set_mouse_button(button, state == ElementState::Pressed);
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let pos = position.to_logical::<f32>(window.scale_factor());
                self.ctx.input.set_mouse_pos(Vec2::new(pos.x, pos.y));
            }

            WindowEvent::Touch(touch) => {
                let pos = touch.location.to_logical::<f32>(window.scale_factor());
                self.ctx
                    .input
                    .feed_touch(touch.id, touch_phase(touch.phase), Vec2::new(pos.x, pos.y));
            }

            // A Mac's trackpad: its pinch and rotation are the touch
            // screen's (macOS has no touch screens; see the touch guide).
            WindowEvent::PinchGesture { delta, phase, .. } => {
                self.ctx
                    .input
                    .feed_trackpad(crate::input::Gesture::Pinch, touch_phase(phase), delta as f32);
            }
            WindowEvent::RotationGesture { delta, phase, .. } => {
                // Degrees counterclockwise; the engine turns clockwise in radians.
                self.ctx
                    .input
                    .feed_trackpad(crate::input::Gesture::Rotate, touch_phase(phase), -delta.to_radians());
            }
            WindowEvent::DoubleTapGesture { .. } => {
                self.ctx
                    .input
                    .feed_trackpad(crate::input::Gesture::DoubleTap, crate::input::TouchPhase::Ended, 0.0)
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(x, y),
                    MouseScrollDelta::PixelDelta(p) => {
                        let p: PhysicalPosition<f64> = p;
                        let logical = p.to_logical::<f32>(window.scale_factor());
                        Vec2::new(logical.x, logical.y) / PIXELS_PER_SCROLL_LINE
                    }
                };
                self.ctx.input.add_mouse_wheel(lines);
            }

            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (x, y) } = event
            && self.window.as_ref().is_some_and(|w| w.has_focus())
        {
            self.ctx.input.add_mouse_motion(Vec2::new(x as f32, y as f32));
        }
    }

    /// winit 0.30's iOS loop drops a redraw asked for while it delivers
    /// redraws (each frame's own request for the next, and requests made
    /// in `about_to_wait`): ask for the frame as each pass of the polling
    /// loop starts instead, where it is kept.
    #[cfg(target_os = "ios")]
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, _cause: winit::event::StartCause) {
        if self.started
            && !self.ctx.window.suspended()
            && let Some(window) = &self.window
        {
            window.request_redraw();
        }
    }

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        self.lifecycle(crate::window::WindowEvent::LowMemory);
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // iOS ignores `exit`: an app may not quit itself. A failed start
        // there ends the process instead of showing a blank screen.
        #[cfg(target_os = "ios")]
        if let Some(error) = &self.error {
            eprintln!("{error}");
            crate::crash::report_error(error);
            std::process::exit(1);
        }
        // Suspended: nothing to draw or simulate until the system resumes it.
        if self.ctx.window.suspended() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        // Between two frames while a screen reader is asking: stay idle
        // (answering it) until the pause is over, then ask for the frame.
        #[cfg(feature = "accessibility")]
        if let Some(until) = self.access_gap_until {
            if Instant::now() < until {
                event_loop.set_control_flow(ControlFlow::WaitUntil(until));
                return;
            }
            self.access_gap_until = None;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        if self.occluded {
            // Hidden: check back now and then instead of spinning a core.
            // The simulation pauses while hidden (dt is clamped on return).
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + OCCLUDED_POLL));
        } else if self.unpaced() && self.started {
            event_loop.set_control_flow(ControlFlow::Poll);
            self.run_frame(event_loop);
        } else if cfg!(target_os = "ios") && self.started {
            // On iOS each frame is asked for as a pass of the loop starts
            // (`new_events`): poll, so passes keep coming. Presenting waits
            // for the display's next drawable, so frames stay paced by vsync.
            event_loop.set_control_flow(ControlFlow::Poll);
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

/// Hides the cursor and holds it in place (or in the window, where the
/// system can't hold it still), or frees it.
fn apply_cursor_lock(window: &Window, locked: bool) {
    if locked {
        if window.set_cursor_grab(CursorGrabMode::Locked).is_err() {
            let _ = window.set_cursor_grab(CursorGrabMode::Confined);
        }
    } else {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
    }
    window.set_cursor_visible(!locked);
}

/// The engine's phase of a touch or trackpad gesture.
fn touch_phase(phase: winit::event::TouchPhase) -> crate::input::TouchPhase {
    use winit::event::TouchPhase as W;
    match phase {
        W::Started => crate::input::TouchPhase::Started,
        W::Moved => crate::input::TouchPhase::Moved,
        W::Ended => crate::input::TouchPhase::Ended,
        W::Cancelled => crate::input::TouchPhase::Cancelled,
    }
}

/// Whether `event` lets a browser page start sound (a "user activation"):
/// a mouse button or key going down (not Escape) or a touch lifting.
fn is_user_gesture(event: &WindowEvent) -> bool {
    match event {
        WindowEvent::MouseInput { state, .. } => *state == ElementState::Pressed,
        WindowEvent::KeyboardInput { event, .. } => {
            event.state == ElementState::Pressed
                && event.physical_key != PhysicalKey::Code(winit::keyboard::KeyCode::Escape)
        }
        WindowEvent::Touch(touch) => touch.phase == winit::event::TouchPhase::Ended,
        _ => false,
    }
}
