//! Window control tested entirely through a fake backend, without an event loop.
#![deny(missing_docs)]
use ponko::{
    error::{Error, ErrorKind},
    math::{IVec2, UVec2, Vec2},
    prelude::Game,
    window::*,
};
use std::{cell::RefCell, rc::Rc};
#[derive(Default)]
struct Fake {
    requests: Vec<WindowRequest>,
    state: WindowState,
    clipboard: String,
    fail: bool,
    displays: bool,
}
struct Backend(Rc<RefCell<Fake>>);
impl WindowBackend for Backend {
    fn request(&mut self, r: &WindowRequest) -> Result<(), Error> {
        let mut f = self.0.borrow_mut();
        if f.fail {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "fake request failed; enable support",
            ));
        }
        match r {
            WindowRequest::Mode(v) => f.state.mode = v.clone(),
            WindowRequest::Settings(s) => f.state.mode = s.mode.clone(),
            _ => {}
        }
        f.requests.push(r.clone());
        Ok(())
    }
    fn state(&self) -> WindowState {
        self.0.borrow().state.clone()
    }
    fn monitors(&self) -> Vec<Monitor> {
        if !self.0.borrow().displays {
            return vec![];
        }
        let mut m = Monitor::default();
        m.name = Some("Display".into());
        m.position = IVec2::new(-1920, 0);
        m.size = UVec2::new(1920, 1080);
        m.scale_factor = 2.0;
        m.refresh_millihertz = Some(60000);
        m.modes = vec![mode()];
        vec![m]
    }
    fn clipboard_read(&mut self) -> Result<Option<String>, Error> {
        Ok(Some(self.0.borrow().clipboard.clone()))
    }
    fn clipboard_write(&mut self, s: &str) -> Result<(), Error> {
        self.0.borrow_mut().clipboard = s.into();
        Ok(())
    }
}
fn mode() -> VideoMode {
    VideoMode::new(1920, 1080, 60000, 32)
}
fn fake() -> (WindowService, Rc<RefCell<Fake>>) {
    let mut f = Fake {
        displays: true,
        ..Default::default()
    };
    f.state.physical_size = UVec2::new(1920, 1080);
    f.state.monitor = Some(0);
    let f = Rc::new(RefCell::new(f));
    (WindowService::with_backend(Backend(f.clone())), f)
}
#[test]
fn direct_calls_preserve_other_settings_and_report_observed_mode_and_monitor() {
    let (mut w, f) = fake();
    for m in [
        WindowMode::Windowed,
        WindowMode::Borderless(Some(0)),
        WindowMode::Exclusive(0, mode()),
    ] {
        w.set_mode(m.clone()).unwrap();
        assert_eq!(w.mode(), m);
    }
    w.set_title("Play").unwrap();
    w.set_size(Vec2::new(960.0, 540.0)).unwrap();
    w.set_position(IVec2::new(-960, 0)).unwrap();
    w.set_resize_limits(Some(Vec2::new(320.0, 200.0)), Some(Vec2::new(1920.0, 1080.0)))
        .unwrap();
    w.set_always_on_top(true).unwrap();
    w.set_decorations(false).unwrap();
    assert_eq!(w.settings().title, "Play");
    assert_eq!(w.settings().size, Vec2::new(960.0, 540.0));
    assert_eq!(w.current_monitor(), Some(0));
    assert_eq!(w.exclusive_modes(0).unwrap(), vec![mode()]);
    assert_eq!(f.borrow().requests.len(), 9);
    let settings: WindowSettings = serde_json::from_slice(&serde_json::to_vec(w.settings()).unwrap()).unwrap();
    assert_eq!(&settings, w.settings());
}
#[test]
fn invalid_sizes_bounds_images_and_hotspots_fail_without_mutating_backend() {
    let (mut w, f) = fake();
    for size in [Vec2::ZERO, Vec2::splat(f32::NAN), Vec2::splat(-1.0)] {
        assert_eq!(w.set_size(size).unwrap_err().kind(), ErrorKind::Invalid);
    }
    assert!(
        w.set_resize_limits(Some(Vec2::splat(800.0)), Some(Vec2::splat(200.0)))
            .is_err()
    );
    assert!(w.set_icon(Some(WindowImage::new(u32::MAX, u32::MAX, vec![]))).is_err());
    assert!(
        w.set_cursor(Cursor::Custom(WindowImage::new(1, 1, vec![255; 4]), UVec2::new(1, 0)))
            .is_err()
    );
    assert!(f.borrow().requests.is_empty());
    f.borrow_mut().fail = true;
    let old = w.settings().clone();
    assert_eq!(w.set_title("Refused").unwrap_err().kind(), ErrorKind::Unsupported);
    assert_eq!(w.settings(), &old);
    assert_eq!(w.exclusive_modes(2).unwrap_err().kind(), ErrorKind::NotFound);
}
#[test]
fn every_system_cursor_and_custom_image_visibility_and_capture_reach_backend() {
    let (mut w, f) = fake();
    for icon in CursorIcon::ALL {
        w.set_cursor(Cursor::System(*icon)).unwrap();
    }
    let image = WindowImage::new(2, 2, vec![255; 16]);
    w.set_icon(Some(image.clone())).unwrap();
    w.set_icon(None).unwrap();
    w.set_cursor(Cursor::Custom(image, UVec2::new(1, 1))).unwrap();
    for v in [false, true] {
        w.set_cursor_visible(v).unwrap();
    }
    for g in [CursorGrab::None, CursorGrab::Confined, CursorGrab::Locked] {
        w.set_cursor_grab(g).unwrap();
    }
    assert_eq!(f.borrow().requests.len(), CursorIcon::ALL.len() + 8);
}
#[test]
fn clipboard_text_round_trips_unicode_through_the_backend() {
    let (mut w, f) = fake();
    w.clipboard_write("hello 世界").unwrap();
    assert_eq!(f.borrow().clipboard, "hello 世界");
    assert_eq!(w.clipboard_read().unwrap().as_deref(), Some("hello 世界"));
}
#[test]
fn focus_and_minimized_events_are_only_sent_when_state_changes() {
    Game::new("events", 1, 1).run_headless(0, |ctx| {
        for e in [
            WindowEvent::Focused(false),
            WindowEvent::Focused(true),
            WindowEvent::Focused(true),
            WindowEvent::Minimized(false),
            WindowEvent::Minimized(true),
            WindowEvent::Minimized(true),
            WindowEvent::Minimized(false),
            WindowEvent::Minimized(false),
        ] {
            if ctx.window.feed_event(&e) {
                ctx.events.send(e);
            }
        }
        assert_eq!(
            ctx.events.peek::<WindowEvent>(),
            &[
                WindowEvent::Focused(true),
                WindowEvent::Minimized(true),
                WindowEvent::Minimized(false)
            ]
        );
        ctx.window.feed_event(&WindowEvent::CloseRequested);
        assert!(ctx.window.close_requested());
        ctx.window.cancel_close();
        assert!(!ctx.window.close_requested());
    });
}
#[test]
fn headless_window_calls_never_open_a_display() {
    let mut w = WindowService::default();
    w.set_mode(WindowMode::Borderless(None)).unwrap();
    w.clipboard_write("ignored").unwrap();
    assert_eq!(w.clipboard_read().unwrap(), None);
    assert!(w.monitors().is_empty());
    assert_eq!(w.state(), WindowState::default());
}
#[test]
fn restart_with_the_monitor_gone_falls_back_to_windowed() {
    let dir = std::env::temp_dir().join(format!("ponko-window-restore-{}", std::process::id()));
    Game::new("settings", 800, 600).data_dir(&dir).run_headless(0, |ctx| {
        let (w, _) = fake();
        ctx.window = w;
        ctx.window.set_mode(WindowMode::Exclusive(0, mode())).unwrap();
        ctx.window.set_title("Remember me").unwrap();
        ctx.window.save_settings(&mut ctx.storage).unwrap();
    });
    Game::new("settings", 800, 600).data_dir(&dir).run_headless(0, |ctx| {
        let (w, f) = fake();
        f.borrow_mut().displays = false;
        ctx.window = w;
        ctx.window.restore_settings(&ctx.storage).unwrap();
        assert_eq!(ctx.window.mode(), WindowMode::Windowed);
        assert_eq!(ctx.window.settings().title, "Remember me");
    });
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn restart_with_a_missing_mode_falls_back_and_valid_settings_restore() {
    let dir = std::env::temp_dir().join(format!("ponko-window-mode-{}", std::process::id()));
    Game::new("settings", 800, 600).data_dir(&dir).run_headless(0, |ctx| {
        let (w, _) = fake();
        ctx.window = w;
        ctx.window.set_mode(WindowMode::Exclusive(0, mode())).unwrap();
        ctx.window.save_settings(&mut ctx.storage).unwrap();
        ctx.window.set_mode(WindowMode::Windowed).unwrap();
        ctx.window.restore_settings(&ctx.storage).unwrap();
        assert_eq!(ctx.window.mode(), WindowMode::Exclusive(0, mode()));
        ctx.window
            .set_mode(WindowMode::Exclusive(0, VideoMode::new(1, 1, 1, 1)))
            .unwrap();
        ctx.window.save_settings(&mut ctx.storage).unwrap();
        ctx.window.restore_settings(&ctx.storage).unwrap();
        assert_eq!(ctx.window.mode(), WindowMode::Windowed);
    });
    std::fs::remove_dir_all(dir).unwrap();
}
