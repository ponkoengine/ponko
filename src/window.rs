//! Window requests and observed state, available even without a display.
#![deny(missing_docs)]
use crate::{
    error::{Error, ErrorKind},
    storage::Storage,
};
use glam::{IVec2, UVec2, Vec2};
use serde::{Deserialize, Serialize};

/// A display mode in physical pixels and millihertz.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct VideoMode {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Refresh rate in millihertz.
    pub refresh_millihertz: u32,
    /// Color depth in bits.
    pub bit_depth: u16,
}
/// Monitor snapshot. Indices are valid only until the next refresh.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Monitor {
    /// OS display name, when available.
    pub name: Option<String>,
    /// Physical desktop origin.
    pub position: IVec2,
    /// Physical size.
    pub size: UVec2,
    /// Logical-to-physical pixel multiplier.
    pub scale_factor: f32,
    /// Current refresh rate, when available.
    pub refresh_millihertz: Option<u32>,
    /// Exclusive fullscreen choices.
    pub modes: Vec<VideoMode>,
}
/// Runtime presentation choice. Web supports borderless only after a user gesture.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WindowMode {
    /// Ordinary desktop window.
    #[default]
    Windowed,
    /// Desktop-sized window; None chooses the current display.
    Borderless(Option<usize>),
    /// A monitor index and an exact advertised mode.
    Exclusive(usize, VideoMode),
}
/// Persistable window preferences. Sizes use logical window pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WindowSettings {
    /// Fullscreen choice.
    pub mode: WindowMode,
    /// Client size in logical pixels.
    pub size: Vec2,
    /// Desktop position in physical pixels; None lets the OS choose.
    pub position: Option<IVec2>,
    /// Minimum logical client size.
    pub min_size: Option<Vec2>,
    /// Maximum logical client size.
    pub max_size: Option<Vec2>,
    /// Title (browser canvas has no title bar).
    pub title: String,
    /// Whether the OS should keep the window above others.
    pub always_on_top: bool,
    /// Whether the window has OS decorations.
    pub decorations: bool,
}
impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            mode: WindowMode::Windowed,
            size: Vec2::new(800.0, 600.0),
            position: None,
            min_size: None,
            max_size: None,
            title: "Ponko".into(),
            always_on_top: false,
            decorations: true,
        }
    }
}
/// An RGBA8 icon or cursor image. Invalid dimensions/hotspots are rejected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WindowImage {
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
    /// Row-major RGBA8 bytes.
    pub rgba: Vec<u8>,
}
impl WindowImage {
    fn valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && (self.width as usize)
                .checked_mul(self.height as usize)
                .and_then(|n| n.checked_mul(4))
                == Some(self.rgba.len())
    }
}
/// Cursor grab request. Unsupported grabs return an error; lock falls back to confinement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CursorGrab {
    /// Release capture.
    None,
    /// Keep cursor inside the client area.
    Confined,
    /// Lock for relative motion.
    Locked,
}
/// A system cursor, independent of the window feature. More icons may be added.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CursorIcon {
    /// The Default system cursor.
    #[default]
    Default,
    /// The ContextMenu system cursor.
    ContextMenu,
    /// The Help system cursor.
    Help,
    /// The Pointer system cursor.
    Pointer,
    /// The Progress system cursor.
    Progress,
    /// The Wait system cursor.
    Wait,
    /// The Cell system cursor.
    Cell,
    /// The Crosshair system cursor.
    Crosshair,
    /// The Text system cursor.
    Text,
    /// The VerticalText system cursor.
    VerticalText,
    /// The Alias system cursor.
    Alias,
    /// The Copy system cursor.
    Copy,
    /// The Move system cursor.
    Move,
    /// The NoDrop system cursor.
    NoDrop,
    /// The NotAllowed system cursor.
    NotAllowed,
    /// The Grab system cursor.
    Grab,
    /// The Grabbing system cursor.
    Grabbing,
    /// The EResize system cursor.
    EResize,
    /// The NResize system cursor.
    NResize,
    /// The NeResize system cursor.
    NeResize,
    /// The NwResize system cursor.
    NwResize,
    /// The SResize system cursor.
    SResize,
    /// The SeResize system cursor.
    SeResize,
    /// The SwResize system cursor.
    SwResize,
    /// The WResize system cursor.
    WResize,
    /// The EwResize system cursor.
    EwResize,
    /// The NsResize system cursor.
    NsResize,
    /// The NeswResize system cursor.
    NeswResize,
    /// The NwseResize system cursor.
    NwseResize,
    /// The ColResize system cursor.
    ColResize,
    /// The RowResize system cursor.
    RowResize,
    /// The AllScroll system cursor.
    AllScroll,
    /// The ZoomIn system cursor.
    ZoomIn,
    /// The ZoomOut system cursor.
    ZoomOut,
    /// The DndAsk system cursor.
    DndAsk,
    /// The AllResize system cursor.
    AllResize,
}
impl CursorIcon {
    /// Every icon known by this release, in native binding order.
    pub const ALL: &[Self] = &[
        Self::Default,
        Self::ContextMenu,
        Self::Help,
        Self::Pointer,
        Self::Progress,
        Self::Wait,
        Self::Cell,
        Self::Crosshair,
        Self::Text,
        Self::VerticalText,
        Self::Alias,
        Self::Copy,
        Self::Move,
        Self::NoDrop,
        Self::NotAllowed,
        Self::Grab,
        Self::Grabbing,
        Self::EResize,
        Self::NResize,
        Self::NeResize,
        Self::NwResize,
        Self::SResize,
        Self::SeResize,
        Self::SwResize,
        Self::WResize,
        Self::EwResize,
        Self::NsResize,
        Self::NeswResize,
        Self::NwseResize,
        Self::ColResize,
        Self::RowResize,
        Self::AllScroll,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::DndAsk,
        Self::AllResize,
    ];
    #[cfg(feature = "window")]
    pub(crate) fn to_winit(self) -> winit::window::CursorIcon {
        match self {
            Self::Default => winit::window::CursorIcon::Default,
            Self::ContextMenu => winit::window::CursorIcon::ContextMenu,
            Self::Help => winit::window::CursorIcon::Help,
            Self::Pointer => winit::window::CursorIcon::Pointer,
            Self::Progress => winit::window::CursorIcon::Progress,
            Self::Wait => winit::window::CursorIcon::Wait,
            Self::Cell => winit::window::CursorIcon::Cell,
            Self::Crosshair => winit::window::CursorIcon::Crosshair,
            Self::Text => winit::window::CursorIcon::Text,
            Self::VerticalText => winit::window::CursorIcon::VerticalText,
            Self::Alias => winit::window::CursorIcon::Alias,
            Self::Copy => winit::window::CursorIcon::Copy,
            Self::Move => winit::window::CursorIcon::Move,
            Self::NoDrop => winit::window::CursorIcon::NoDrop,
            Self::NotAllowed => winit::window::CursorIcon::NotAllowed,
            Self::Grab => winit::window::CursorIcon::Grab,
            Self::Grabbing => winit::window::CursorIcon::Grabbing,
            Self::EResize => winit::window::CursorIcon::EResize,
            Self::NResize => winit::window::CursorIcon::NResize,
            Self::NeResize => winit::window::CursorIcon::NeResize,
            Self::NwResize => winit::window::CursorIcon::NwResize,
            Self::SResize => winit::window::CursorIcon::SResize,
            Self::SeResize => winit::window::CursorIcon::SeResize,
            Self::SwResize => winit::window::CursorIcon::SwResize,
            Self::WResize => winit::window::CursorIcon::WResize,
            Self::EwResize => winit::window::CursorIcon::EwResize,
            Self::NsResize => winit::window::CursorIcon::NsResize,
            Self::NeswResize => winit::window::CursorIcon::NeswResize,
            Self::NwseResize => winit::window::CursorIcon::NwseResize,
            Self::ColResize => winit::window::CursorIcon::ColResize,
            Self::RowResize => winit::window::CursorIcon::RowResize,
            Self::AllScroll => winit::window::CursorIcon::AllScroll,
            Self::ZoomIn => winit::window::CursorIcon::ZoomIn,
            Self::ZoomOut => winit::window::CursorIcon::ZoomOut,
            Self::DndAsk => winit::window::CursorIcon::DndAsk,
            Self::AllResize => winit::window::CursorIcon::AllResize,
        }
    }
}

/// Cursor appearance: a system icon or a custom RGBA image.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Cursor {
    /// A typed system cursor.
    System(CursorIcon),
    /// Custom pixels and hotspot in physical image pixels.
    Custom(WindowImage, UVec2),
}
/// A request recorded by the window service.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WindowRequest {
    /// Set presentation mode only.
    Mode(WindowMode),
    /// Set the title only.
    Title(String),
    /// Request a logical client size.
    Size(Vec2),
    /// Set physical desktop position.
    Position(IVec2),
    /// Set optional minimum and maximum logical client sizes.
    ResizeLimits(Option<Vec2>, Option<Vec2>),
    /// Set desktop topmost level.
    AlwaysOnTop(bool),
    /// Set desktop decorations.
    Decorations(bool),
    /// Apply all preferences after validation.
    Settings(WindowSettings),
    /// Set a window icon; None restores the default.
    Icon(Option<WindowImage>),
    /// Set cursor appearance.
    Cursor(Cursor),
    /// Set visibility.
    CursorVisible(bool),
    /// Change cursor capture.
    CursorGrab(CursorGrab),
    /// Re-enumerate monitors (also use after hot-plug).
    RefreshMonitors,
}
/// Window lifecycle event, also sent through `ctx.events`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WindowEvent {
    /// Keyboard focus changed.
    Focused(bool),
    /// Minimized state changed.
    Minimized(bool),
    /// Close requested; call `cancel_close` before the next frame ends to veto.
    CloseRequested,
    /// File dropped onto the window.
    FileDropped(std::path::PathBuf),
    /// DPI multiplier changed.
    ScaleFactorChanged(f32),
}
/// Last observed display state (requests are not proof the OS accepted them).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WindowState {
    /// Physical client size.
    pub physical_size: UVec2,
    /// DPI multiplier.
    pub scale_factor: f32,
    /// Current focus state.
    pub focused: bool,
    /// OS minimized state if known.
    pub minimized: bool,
    /// Actual fullscreen state.
    pub mode: WindowMode,
    /// Index of the current monitor, if the platform exposes it.
    pub monitor: Option<usize>,
}
impl Default for WindowState {
    fn default() -> Self {
        Self {
            physical_size: UVec2::ZERO,
            scale_factor: 1.0,
            monitor: None,
            focused: false,
            minimized: false,
            mode: WindowMode::Windowed,
        }
    }
}
/// Display backend seam. Implementations must never construct an event loop here.
/// The default backend is inert; tests may install a recording implementation.
pub trait WindowBackend {
    /// Apply a validated request or report a platform restriction.
    fn request(&mut self, request: &WindowRequest) -> Result<(), Error>;
    /// Query observed state.
    fn state(&self) -> WindowState;
    /// Enumerate connected displays.
    fn monitors(&self) -> Vec<Monitor>;
    /// Read text. Web returns None while permission/promise is pending.
    fn clipboard_read(&mut self) -> Result<Option<String>, Error>;
    /// Write text; web completion is asynchronous, polled by `clipboard_read`.
    fn clipboard_write(&mut self, text: &str) -> Result<(), Error>;
}
/// An inert backend: no OS calls, no window dependencies, no stored clipboard.
#[derive(Default)]
#[non_exhaustive]
pub struct HeadlessWindow;
impl WindowBackend for HeadlessWindow {
    fn request(&mut self, _: &WindowRequest) -> Result<(), Error> {
        Ok(())
    }
    fn state(&self) -> WindowState {
        WindowState::default()
    }
    fn monitors(&self) -> Vec<Monitor> {
        Vec::new()
    }
    fn clipboard_read(&mut self) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn clipboard_write(&mut self, _: &str) -> Result<(), Error> {
        Ok(())
    }
}
/// Window service. Requests act immediately; settings retain requested preferences.
/// No backend work occurs until a method is called. No-window builds stay inert.
#[non_exhaustive]
pub struct WindowService {
    backend: Box<dyn WindowBackend>,
    settings: WindowSettings,
    close: bool,
    last_focus: Option<bool>,
    last_minimized: Option<bool>,
    pub(crate) paste_target: Option<crate::ui::UiId>,
    /// The window's size in logical pixels (as `Ctx::window_size`), and
    /// what the platform covers of it ([`safe_area`](Self::safe_area)).
    logical_size: Vec2,
    safe_insets: SafeInsets,
}
impl Default for WindowService {
    fn default() -> Self {
        Self::with_backend(HeadlessWindow)
    }
}
impl WindowService {
    /// Construct with a fake or platform backend; does not open a display.
    pub fn with_backend(backend: impl WindowBackend + 'static) -> Self {
        let state = backend.state();
        Self {
            backend: Box::new(backend),
            settings: WindowSettings::default(),
            close: false,
            last_focus: Some(state.focused),
            last_minimized: Some(state.minimized),
            paste_target: None,
            logical_size: Vec2::ZERO,
            safe_insets: SafeInsets::NONE,
        }
    }
    /// Replace the inert backend with an already-created window adapter.
    #[cfg(feature = "window")]
    pub(crate) fn attach(&mut self, backend: impl WindowBackend + 'static, settings: WindowSettings) {
        let state = backend.state();
        self.last_focus = Some(state.focused);
        self.last_minimized = Some(state.minimized);
        self.backend = Box::new(backend);
        self.settings = settings;
    }
    /// Requested preferences, suitable for `Storage::write_json`.
    pub fn settings(&self) -> &WindowSettings {
        &self.settings
    }
    /// Observed state; headless returns a default snapshot.
    pub fn state(&self) -> WindowState {
        self.backend.state()
    }
    /// Current monitors. Do not persist their indices across restarts/hot-plug.
    pub fn monitors(&self) -> Vec<Monitor> {
        self.backend.monitors()
    }
    /// Apply a request. Invalid images, dimensions and bounds never reach the backend.
    pub fn request(&mut self, request: WindowRequest) -> Result<(), Error> {
        validate_request(&request)?;
        self.backend.request(&request)?;
        match request {
            WindowRequest::Settings(s) => self.settings = s,
            WindowRequest::Mode(v) => self.settings.mode = v,
            WindowRequest::Title(v) => self.settings.title = v,
            WindowRequest::Size(v) => self.settings.size = v,
            WindowRequest::Position(v) => self.settings.position = Some(v),
            WindowRequest::ResizeLimits(a, b) => {
                self.settings.min_size = a;
                self.settings.max_size = b;
            }
            WindowRequest::AlwaysOnTop(v) => self.settings.always_on_top = v,
            WindowRequest::Decorations(v) => self.settings.decorations = v,
            _ => {}
        }
        Ok(())
    }
    /// Read clipboard text; None means unavailable headless or pending on web.
    pub fn clipboard_read(&mut self) -> Result<Option<String>, Error> {
        self.backend.clipboard_read()
    }
    /// Write clipboard text. Headless does nothing; web requires permission and a gesture.
    pub fn clipboard_write(&mut self, text: &str) -> Result<(), Error> {
        self.backend.clipboard_write(text)
    }
    /// Feed an OS/fake lifecycle event. The game loop also emits it through Events.
    pub fn feed_event(&mut self, event: &WindowEvent) -> bool {
        let last = match event {
            WindowEvent::Focused(_) => Some(&mut self.last_focus),
            WindowEvent::Minimized(_) => Some(&mut self.last_minimized),
            _ => None,
        };
        if let Some(last) = last {
            let value = match event {
                WindowEvent::Focused(v) | WindowEvent::Minimized(v) => *v,
                _ => unreachable!(),
            };
            if *last == Some(value) {
                return false;
            }
            *last = Some(value);
        }
        if matches!(event, WindowEvent::CloseRequested) {
            self.close = true;
        }
        true
    }
    /// The part of the window nothing covers, in logical pixels (as
    /// [`Ctx::window_size`](crate::ctx::Ctx::window_size)): the window less
    /// a phone's notch or camera cutout, its home bar and rounded corners,
    /// where the platform reports them (see [`SafeInsets`]). The whole
    /// window where nothing covers it. Touch controls, the arrange screen
    /// and UI roots made with
    /// [`inside_safe_area`](crate::ui::UiBuilder::inside_safe_area) stay in it.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::window::SafeInsets;
    /// Game::new("notch", 800, 400).run_headless(0, |ctx| {
    ///     assert_eq!(ctx.window.safe_area().size(), vec2(800.0, 400.0), "nothing covered");
    ///     // As a phone held sideways reports it: the notch left, the home bar below.
    ///     ctx.window.feed_safe_insets(SafeInsets::new(0.0, 0.0, 21.0, 47.0));
    ///     assert_eq!(ctx.window.safe_area(), Rect::new(47.0, 0.0, 753.0, 379.0));
    /// });
    /// ```
    pub fn safe_area(&self) -> crate::camera::Rect {
        self.safe_insets.area(self.logical_size)
    }
    /// How far in from each window edge the platform covers it: what
    /// [`safe_area`](Self::safe_area) leaves out.
    pub fn safe_insets(&self) -> SafeInsets {
        self.safe_insets
    }
    /// Feeds the window's safe area insets, as the platform reports them:
    /// a test of a notched phone, a platform layer of the game's own. The
    /// engine feeds them itself where it reads them (the browser, on a
    /// page with `viewport-fit=cover`), again whenever the window changes
    /// size. Negative or not finite insets count as 0. The UI lays out with
    /// them from the next frame.
    pub fn feed_safe_insets(&mut self, insets: SafeInsets) {
        self.safe_insets = insets.sanitized();
    }
    /// The window's size in logical pixels, which the safe area is cut from.
    pub(crate) fn set_logical_size(&mut self, size: Vec2) {
        self.logical_size = size;
    }
    /// Veto the current close request. Explicit `ctx.quit()` remains unconditional.
    pub fn cancel_close(&mut self) {
        self.close = false;
    }
    /// Whether an uncanceled OS close request is pending.
    pub fn close_requested(&self) -> bool {
        self.close
    }
}

impl VideoMode {
    /// Physical size, refresh in millihertz and bit depth of an advertised mode.
    pub fn new(width: u32, height: u32, refresh_millihertz: u32, bit_depth: u16) -> Self {
        Self {
            width,
            height,
            refresh_millihertz,
            bit_depth,
        }
    }
}
impl WindowImage {
    /// Construct copied RGBA8 data; setters validate its length and dimensions.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Self { width, height, rgba }
    }
}
impl Default for Monitor {
    fn default() -> Self {
        Self {
            name: None,
            position: IVec2::ZERO,
            size: UVec2::ZERO,
            scale_factor: 1.0,
            refresh_millihertz: None,
            modes: Vec::new(),
        }
    }
}
impl WindowService {
    /// Current observed fullscreen mode. Headless reports windowed.
    pub fn mode(&self) -> WindowMode {
        self.state().mode
    }
    /// Index of the display the window is currently on, when available.
    pub fn current_monitor(&self) -> Option<usize> {
        self.state().monitor
    }
    /// Exclusive modes of a connected display; refresh the list after hot-plug.
    pub fn exclusive_modes(&self, monitor: usize) -> Result<Vec<VideoMode>, Error> {
        self.monitors().get(monitor).map(|m| m.modes.clone()).ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                "cannot list modes: monitor disconnected; refresh monitors and choose a connected display",
            )
        })
    }
    /// Switch presentation mode. Web supports windowed/borderless-current-display only.
    pub fn set_mode(&mut self, mode: WindowMode) -> Result<(), Error> {
        self.request(WindowRequest::Mode(mode))
    }
    /// Set the desktop title. On web winit leaves the document title unchanged.
    pub fn set_title(&mut self, title: impl Into<String>) -> Result<(), Error> {
        self.request(WindowRequest::Title(title.into()))
    }
    /// Request a positive logical client size; the OS/page may constrain it.
    pub fn set_size(&mut self, size: Vec2) -> Result<(), Error> {
        self.request(WindowRequest::Size(size))
    }
    /// Set physical desktop position. Unsupported on web.
    pub fn set_position(&mut self, position: IVec2) -> Result<(), Error> {
        self.request(WindowRequest::Position(position))
    }
    /// Set positive logical resize limits; None removes a limit. Unsupported on web.
    pub fn set_resize_limits(&mut self, min: Option<Vec2>, max: Option<Vec2>) -> Result<(), Error> {
        self.request(WindowRequest::ResizeLimits(min, max))
    }
    /// Request the topmost desktop level. Unsupported on web.
    pub fn set_always_on_top(&mut self, value: bool) -> Result<(), Error> {
        self.request(WindowRequest::AlwaysOnTop(value))
    }
    /// Set desktop decorations. Unsupported on web.
    pub fn set_decorations(&mut self, value: bool) -> Result<(), Error> {
        self.request(WindowRequest::Decorations(value))
    }
    /// Set a copied RGBA desktop icon, or restore the default. Unsupported on web.
    pub fn set_icon(&mut self, image: Option<WindowImage>) -> Result<(), Error> {
        self.request(WindowRequest::Icon(image))
    }
    /// Set a typed system cursor or custom image. Custom images apply next frame.
    pub fn set_cursor(&mut self, cursor: Cursor) -> Result<(), Error> {
        self.request(WindowRequest::Cursor(cursor))
    }
    /// Show or hide the cursor, independently of capture.
    pub fn set_cursor_visible(&mut self, visible: bool) -> Result<(), Error> {
        self.request(WindowRequest::CursorVisible(visible))
    }
    /// Confine, lock (with confinement fallback), or release. Web lock needs activation.
    pub fn set_cursor_grab(&mut self, grab: CursorGrab) -> Result<(), Error> {
        self.request(WindowRequest::CursorGrab(grab))
    }
    /// Save requested preferences as window.json in the game's player storage.
    pub fn save_settings(&self, storage: &mut Storage) -> Result<(), Error> {
        storage.write_json(SETTINGS_FILE, &self.settings)
    }
    /// Restore window.json at setup. A disconnected display or unavailable mode falls
    /// back to windowed and logs the reason. A missing file leaves defaults intact.
    ///
    /// This applies the settings to a window that is already open, so the player
    /// may see it at its default size first; [`Game::restore_window_settings`]
    /// opens the first window with them instead.
    ///
    /// [`Game::restore_window_settings`]: crate::prelude::Game::restore_window_settings
    pub fn restore_settings(&mut self, storage: &Storage) -> Result<(), Error> {
        let mut s: WindowSettings = match storage.read_json(SETTINGS_FILE) {
            Ok(s) => s,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        if let Some(reason) = unavailable_mode(&s.mode, &self.monitors()) {
            eprintln!("ponko: restoring window settings in windowed mode: {reason}");
            s.mode = WindowMode::Windowed;
        }
        if cfg!(target_arch = "wasm32") {
            drop_desktop_only(&mut s);
        }
        if let Err(e) = self.request(WindowRequest::Settings(s.clone())) {
            if !matches!(s.mode, WindowMode::Windowed)
                && matches!(e.kind(), ErrorKind::Unsupported | ErrorKind::NotFound)
            {
                eprintln!("ponko: restoring window settings in windowed mode: {e}");
                s.mode = WindowMode::Windowed;
                self.request(WindowRequest::Settings(s))
            } else {
                Err(e)
            }
        } else {
            Ok(())
        }
    }
}
/// The file `save_settings` writes in the game's player storage.
pub(crate) const SETTINGS_FILE: &str = "window.json";
/// Why a stored mode can't be used with these monitors, if it can't.
fn unavailable_mode(mode: &WindowMode, monitors: &[Monitor]) -> Option<String> {
    match mode {
        WindowMode::Exclusive(i, m) => match monitors.get(*i) {
            Some(monitor) if monitor.modes.contains(m) => None,
            Some(_) => Some("stored exclusive mode is gone; choose a newly advertised mode".to_string()),
            None => {
                Some("cannot list modes: monitor disconnected; refresh monitors and choose a connected display".into())
            }
        },
        WindowMode::Borderless(Some(i)) if monitors.get(*i).is_none() => {
            Some("stored monitor disconnected; select a connected display".into())
        }
        _ => None,
    }
}
/// A browser canvas has no desktop position, limits, level or decorations.
fn drop_desktop_only(s: &mut WindowSettings) {
    s.position = None;
    s.min_size = None;
    s.max_size = None;
    s.always_on_top = false;
    s.decorations = true;
}
/// The settings the game's first window opens with, decided before it exists:
/// `defaults` (the game's title, size and fullscreen choice) unless `saved`
/// (`window.json`, read from the player's storage) holds usable ones.
///
/// - No saved file, or a game that keeps no files: the defaults, silently.
/// - A file that can't be read or holds invalid sizes: the defaults, and the
///   reason is printed. A broken file never stops the game from starting.
/// - A saved exclusive mode or monitor that `monitors` no longer has: the saved
///   size and position, windowed, and the reason is printed.
/// - The title always stays the game's: it is set by the game, not the player.
///
/// Desktop only: a web build reads its storage after the canvas exists and
/// uses `restore_settings` (a canvas has no default size to flash).
pub(crate) fn startup_settings(
    defaults: WindowSettings,
    saved: Result<WindowSettings, Error>,
    monitors: &[Monitor],
) -> WindowSettings {
    let mut s = match saved {
        Ok(s) => s,
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::Unsupported) => return defaults,
        Err(e) => {
            eprintln!("ponko: opening the window with its default settings: {e}");
            return defaults;
        }
    };
    if let Err(e) = validate_request(&WindowRequest::Settings(s.clone())) {
        eprintln!("ponko: opening the window with its default settings: {SETTINGS_FILE}: {e}");
        return defaults;
    }
    s.title = defaults.title;
    if let Some(reason) = unavailable_mode(&s.mode, monitors) {
        eprintln!("ponko: restoring window settings in windowed mode: {reason}");
        s.mode = WindowMode::Windowed;
    }
    s
}
fn valid_size(v: Vec2) -> bool {
    v.is_finite() && v.min_element() > 0.0
}
fn validate_limits(min: Option<Vec2>, max: Option<Vec2>) -> Result<(), Error> {
    if min.is_some_and(|v| !valid_size(v))
        || max.is_some_and(|v| !valid_size(v))
        || min.zip(max).is_some_and(|(a, b)| a.cmpgt(b).any())
    {
        return Err(invalid(
            "cannot set resize limits: use positive finite sizes with minimum <= maximum",
        ));
    }
    Ok(())
}
fn validate_request(request: &WindowRequest) -> Result<(), Error> {
    match request {
        WindowRequest::Settings(s) => {
            if !valid_size(s.size) {
                return Err(invalid("cannot set window size: use a positive finite logical size"));
            }
            validate_limits(s.min_size, s.max_size)?;
        }
        WindowRequest::Size(v) if !valid_size(*v) => {
            return Err(invalid("cannot set window size: use a positive finite logical size"));
        }
        WindowRequest::ResizeLimits(a, b) => validate_limits(*a, *b)?,
        WindowRequest::Icon(Some(i)) | WindowRequest::Cursor(Cursor::Custom(i, _)) if !i.valid() => {
            return Err(invalid(
                "cannot set window image: supply width * height * 4 RGBA8 bytes and nonzero dimensions",
            ));
        }
        WindowRequest::Cursor(Cursor::Custom(i, h))
            if h.x >= i.width || h.y >= i.height || i.width > u16::MAX.into() || i.height > u16::MAX.into() =>
        {
            return Err(invalid(
                "cannot set custom cursor: keep dimensions <= 65535 and the hotspot inside the image",
            ));
        }
        _ => {}
    }
    Ok(())
}
pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Invalid, message)
}
#[cfg(any(feature = "window", test))]
pub(crate) fn unsupported(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Unsupported, message)
}
/// Validate browser restrictions without creating a browser or a window.
#[cfg(any(feature = "window", test))]
pub(crate) fn validate_web(request: &WindowRequest) -> Result<(), Error> {
    match request {
        WindowRequest::Mode(WindowMode::Exclusive(..)) => Err(unsupported(
            "cannot use exclusive fullscreen on web; choose borderless fullscreen from a user gesture",
        )),
        WindowRequest::Mode(WindowMode::Borderless(Some(_))) => {
            Err(unsupported("cannot select a monitor on web; use Borderless(None)"))
        }
        WindowRequest::Position(_)
        | WindowRequest::ResizeLimits(..)
        | WindowRequest::AlwaysOnTop(_)
        | WindowRequest::Decorations(_)
        | WindowRequest::Icon(_) => Err(unsupported(
            "cannot change desktop position, resize limits, topmost level, decorations or icon on web; configure the page/canvas instead",
        )),
        WindowRequest::Settings(s) => {
            validate_web(&WindowRequest::Mode(s.mode.clone()))?;
            if s.position.is_some() || s.min_size.is_some() || s.max_size.is_some() || s.always_on_top || !s.decorations
            {
                return Err(unsupported(
                    "cannot apply desktop-only window settings on web; restore defaults or configure the page/canvas",
                ));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// How far in from each edge of the window the platform covers it (a
/// phone's notch or camera cutout, its home bar, rounded corners), in
/// logical pixels: what [`WindowService::safe_area`] leaves out. All zero where nothing covers the window (desktops).
///
/// ```
/// use ponko::window::SafeInsets;
/// // A phone held sideways: the notch on the left, the home bar below.
/// let insets = SafeInsets::new(0.0, 0.0, 21.0, 47.0);
/// assert_eq!(insets.left, 47.0);
/// assert_eq!(SafeInsets::default(), SafeInsets::NONE);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SafeInsets {
    /// From the top edge.
    pub top: f32,
    /// From the right edge.
    pub right: f32,
    /// From the bottom edge.
    pub bottom: f32,
    /// From the left edge.
    pub left: f32,
}

impl SafeInsets {
    /// Nothing covered.
    pub const NONE: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    /// Insets in CSS order: top, right, bottom, left.
    pub fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Each inset made usable: a negative or not finite one is 0.
    pub(crate) fn sanitized(self) -> Self {
        let f = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
        Self::new(f(self.top), f(self.right), f(self.bottom), f(self.left))
    }

    /// The part of a window `size` large these insets leave, at least a
    /// point (insets wider than the window meet in it).
    pub(crate) fn area(self, size: Vec2) -> crate::camera::Rect {
        let s = self.sanitized();
        let size = size.max(Vec2::ZERO);
        let min = Vec2::new(s.left, s.top).min(size);
        let max = (size - Vec2::new(s.right, s.bottom)).max(min);
        crate::camera::Rect { min, max }
    }
}

/// `"47px"` (a computed CSS length) as 47.
#[cfg(any(test, all(target_arch = "wasm32", feature = "window")))]
fn css_pixels(value: &str) -> Option<f32> {
    value
        .trim()
        .strip_suffix("px")?
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
}

/// The browser's safe area insets (`env(safe-area-inset-*)`, non-zero only
/// on a page with `viewport-fit=cover` on a phone that has a notch or
/// rounded corners) as they reach into the game's canvas.
#[cfg(all(target_arch = "wasm32", feature = "window"))]
pub(crate) fn web_safe_insets(window: &winit::window::Window) -> SafeInsets {
    use winit::platform::web::WindowExtWebSys;
    const PROBE: &str = "ponko-safe-area";
    let (Some(page), Some(canvas)) = (web_sys::window(), window.canvas()) else {
        return SafeInsets::NONE;
    };
    let Some(document) = page.document() else {
        return SafeInsets::NONE;
    };
    // A hidden element padded by the insets: its computed padding is them.
    let probe = match document.get_element_by_id(PROBE) {
        Some(probe) => probe,
        None => {
            let (Ok(probe), Some(body)) = (document.create_element("div"), document.body()) else {
                return SafeInsets::NONE;
            };
            probe.set_id(PROBE);
            let _ = probe.set_attribute(
                "style",
                "position:fixed;left:0;top:0;width:0;height:0;visibility:hidden;pointer-events:none;\
                 padding:env(safe-area-inset-top,0px) env(safe-area-inset-right,0px) \
                 env(safe-area-inset-bottom,0px) env(safe-area-inset-left,0px)",
            );
            let _ = body.append_child(&probe);
            probe
        }
    };
    let Ok(Some(style)) = page.get_computed_style(&probe) else {
        return SafeInsets::NONE;
    };
    let inset = |side: &str| {
        style
            .get_property_value(&format!("padding-{side}"))
            .ok()
            .and_then(|v| css_pixels(&v))
            .unwrap_or(0.0)
    };
    // The insets count from the page's edges; the canvas may not reach them.
    let r = canvas.get_bounding_client_rect();
    let width = page.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(r.right());
    let height = page.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(r.bottom());
    let reach = |inset: f32, gap: f64| (f64::from(inset) - gap).max(0.0) as f32;
    SafeInsets::new(
        reach(inset("top"), r.top()),
        reach(inset("right"), width - r.right()),
        reach(inset("bottom"), height - r.bottom()),
        reach(inset("left"), r.left()),
    )
}

#[cfg(test)]
mod safe_area_tests {
    use super::*;

    #[test]
    fn safe_insets_leave_the_middle_of_the_window() {
        let area = SafeInsets::new(10.0, 20.0, 30.0, 40.0).area(Vec2::new(400.0, 300.0));
        assert_eq!((area.min, area.max), (Vec2::new(40.0, 10.0), Vec2::new(380.0, 270.0)));
        assert_eq!(
            SafeInsets::NONE.area(Vec2::new(400.0, 300.0)).max,
            Vec2::new(400.0, 300.0),
            "nothing covered: the whole window"
        );
    }

    #[test]
    fn nonsense_insets_are_made_usable() {
        let s = SafeInsets::new(-5.0, f32::NAN, f32::INFINITY, 3.0).sanitized();
        assert_eq!(s, SafeInsets::new(0.0, 0.0, 0.0, 3.0));
        // Insets wider than the window meet in a point inside it.
        let area = SafeInsets::new(0.0, 300.0, 0.0, 300.0).area(Vec2::new(400.0, 300.0));
        assert_eq!(area.size().x, 0.0);
        assert!(area.min.x <= 400.0 && area.min.x >= 0.0);
    }

    #[test]
    fn computed_css_lengths_read_as_pixels() {
        assert_eq!(css_pixels("47px"), Some(47.0));
        assert_eq!(css_pixels(" 0px "), Some(0.0));
        assert_eq!(css_pixels("34.5px"), Some(34.5));
        assert_eq!(css_pixels("auto"), None);
    }
}

#[cfg(test)]
mod web_tests {
    use super::*;
    #[test]
    fn browser_restrictions_cover_each_direct_call_without_an_event_loop() {
        for r in [
            WindowRequest::Position(IVec2::ZERO),
            WindowRequest::ResizeLimits(None, None),
            WindowRequest::AlwaysOnTop(false),
            WindowRequest::Decorations(true),
            WindowRequest::Icon(None),
            WindowRequest::Mode(WindowMode::Exclusive(0, VideoMode::new(1, 1, 60000, 32))),
            WindowRequest::Mode(WindowMode::Borderless(Some(0))),
        ] {
            assert_eq!(validate_web(&r).unwrap_err().kind(), ErrorKind::Unsupported);
        }
        for r in [
            WindowRequest::Title("title".into()),
            WindowRequest::Size(Vec2::splat(100.0)),
            WindowRequest::Mode(WindowMode::Windowed),
            WindowRequest::Mode(WindowMode::Borderless(None)),
            WindowRequest::Cursor(Cursor::System(CursorIcon::Text)),
            WindowRequest::Cursor(Cursor::Custom(WindowImage::new(1, 1, vec![0; 4]), UVec2::ZERO)),
            WindowRequest::CursorVisible(false),
            WindowRequest::CursorGrab(CursorGrab::Locked),
            WindowRequest::RefreshMonitors,
            WindowRequest::Settings(WindowSettings::default()),
        ] {
            assert!(validate_web(&r).is_ok());
        }
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    fn defaults() -> WindowSettings {
        WindowSettings {
            title: "Game".into(),
            size: Vec2::new(800.0, 600.0),
            ..Default::default()
        }
    }
    fn saved() -> WindowSettings {
        WindowSettings {
            title: "Old title".into(),
            size: Vec2::new(1280.0, 720.0),
            position: Some(IVec2::new(40, 60)),
            min_size: Some(Vec2::new(320.0, 240.0)),
            always_on_top: true,
            decorations: false,
            ..Default::default()
        }
    }
    fn display(mode: VideoMode) -> Monitor {
        Monitor {
            size: UVec2::new(2560, 1440),
            modes: vec![mode],
            ..Default::default()
        }
    }
    fn mode() -> VideoMode {
        VideoMode::new(1920, 1080, 60_000, 32)
    }

    #[test]
    fn first_window_opens_with_the_saved_size_position_mode_and_flags() {
        let mut s = saved();
        s.mode = WindowMode::Exclusive(0, mode());
        let first = startup_settings(defaults(), Ok(s.clone()), &[display(mode())]);
        assert_eq!(first.size, s.size);
        assert_eq!(first.position, s.position);
        assert_eq!(first.min_size, s.min_size);
        assert_eq!(first.mode, s.mode);
        assert!(first.always_on_top);
        assert!(!first.decorations);
    }

    #[test]
    fn first_window_keeps_the_game_title() {
        let first = startup_settings(defaults(), Ok(saved()), &[]);
        assert_eq!(first.title, "Game");
    }

    #[test]
    fn first_window_without_a_saved_file_uses_the_game_defaults() {
        let missing = Err(Error::new(ErrorKind::NotFound, "no stored file"));
        assert_eq!(startup_settings(defaults(), missing, &[]), defaults());
        let no_storage = Err(Error::new(ErrorKind::Unsupported, "this game keeps no files"));
        assert_eq!(startup_settings(defaults(), no_storage, &[]), defaults());
    }

    #[test]
    fn first_window_ignores_an_unreadable_or_invalid_settings_file() {
        let broken = Err(Error::new(ErrorKind::Json, "expected value at line 1"));
        assert_eq!(startup_settings(defaults(), broken, &[]), defaults());
        let mut s = saved();
        s.size = Vec2::new(0.0, f32::NAN);
        assert_eq!(startup_settings(defaults(), Ok(s), &[]), defaults());
        let mut s = saved();
        s.max_size = Some(Vec2::new(100.0, 100.0)); // smaller than min_size
        assert_eq!(startup_settings(defaults(), Ok(s), &[]), defaults());
    }

    #[test]
    fn first_window_opens_windowed_at_the_saved_size_when_the_saved_monitor_is_gone() {
        let mut s = saved();
        s.mode = WindowMode::Borderless(Some(1));
        let first = startup_settings(defaults(), Ok(s.clone()), &[display(mode())]);
        assert_eq!(first.mode, WindowMode::Windowed);
        assert_eq!(first.size, s.size);
        assert_eq!(first.position, s.position);
    }

    #[test]
    fn first_window_opens_windowed_when_the_saved_exclusive_mode_is_gone() {
        let mut s = saved();
        s.mode = WindowMode::Exclusive(0, VideoMode::new(800, 600, 75_000, 32));
        let first = startup_settings(defaults(), Ok(s.clone()), &[display(mode())]);
        assert_eq!(first.mode, WindowMode::Windowed);
        assert_eq!(first.size, s.size);
        // Headless (no monitors) keeps borderless on the current display.
        s.mode = WindowMode::Borderless(None);
        assert_eq!(
            startup_settings(defaults(), Ok(s), &[]).mode,
            WindowMode::Borderless(None)
        );
    }
}
