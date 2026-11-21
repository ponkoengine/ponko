//! Adapter for an already-created winit window. Never creates an event loop.
#![deny(missing_docs)]
use crate::error::{Error, ErrorKind};
use crate::window::*;
use crate::window::{unsupported, validate_web};
use glam::{IVec2, UVec2};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use winit::{
    dpi::{LogicalSize, PhysicalPosition},
    monitor::MonitorHandle,
    window::{CursorGrabMode, Fullscreen, Window, WindowAttributes, WindowLevel},
};

pub(super) type CursorQueue = Rc<RefCell<Option<winit::window::CustomCursorSource>>>;
pub(super) struct Backend {
    pub(super) window: Arc<Window>,
    pub(super) cursor: CursorQueue,
    #[cfg(not(target_arch = "wasm32"))]
    clipboard: Option<arboard::Clipboard>,
    #[cfg(target_arch = "wasm32")]
    clipboard: Rc<RefCell<Option<Result<String, Error>>>>,
    #[cfg(target_arch = "wasm32")]
    pending: bool,
}
impl Backend {
    pub(super) fn new(window: Arc<Window>, cursor: CursorQueue) -> Self {
        Self {
            window,
            cursor,
            #[cfg(not(target_arch = "wasm32"))]
            clipboard: None,
            #[cfg(target_arch = "wasm32")]
            clipboard: Rc::default(),
            #[cfg(target_arch = "wasm32")]
            pending: false,
        }
    }
    fn fullscreen(&self, mode: &WindowMode) -> Result<Option<Fullscreen>, Error> {
        let monitors: Vec<_> = self.window.available_monitors().collect();
        fullscreen_on(&monitors, mode)
    }
}
/// The winit fullscreen state for `mode` on these monitors.
pub(super) fn fullscreen_on(monitors: &[MonitorHandle], mode: &WindowMode) -> Result<Option<Fullscreen>, Error> {
    Ok(match mode {
        WindowMode::Windowed => None,
        WindowMode::Borderless(None) => Some(Fullscreen::Borderless(None)),
        WindowMode::Borderless(Some(i)) => Some(Fullscreen::Borderless(Some(
            monitors.get(*i).ok_or_else(|| Error::new(ErrorKind::NotFound,"cannot switch fullscreen: monitor disconnected; refresh monitors and select a connected display"))?.clone(),
        ))),
        WindowMode::Exclusive(i, m) => {
            if cfg!(target_arch = "wasm32") {
                return Err(unsupported("cannot switch exclusive fullscreen on web; choose borderless from a user gesture"));
            }
            let monitor = monitors.get(*i).ok_or_else(|| Error::new(ErrorKind::NotFound,"cannot switch fullscreen: monitor disconnected; refresh monitors and select a connected display"))?;
            let found = monitor
                .video_modes()
                .find(|v| video(v) == *m)
                .ok_or_else(|| Error::new(ErrorKind::NotFound,"cannot switch exclusive mode: mode is gone; refresh the video mode list and select an advertised mode"))?;
            Some(Fullscreen::Exclusive(found))
        }
    })
}
/// A connected display as the window service reports it.
pub(super) fn monitor_info(m: &MonitorHandle) -> Monitor {
    Monitor {
        name: m.name(),
        position: IVec2::new(m.position().x, m.position().y),
        size: UVec2::new(m.size().width, m.size().height),
        scale_factor: m.scale_factor() as f32,
        refresh_millihertz: m.refresh_rate_millihertz(),
        modes: m.video_modes().map(|v| video(&v)).collect(),
    }
}
/// The attributes the first window is created with, so it opens with
/// `settings` (its title is set by the caller) instead of being changed after
/// it is shown. `fullscreen` is `settings.mode` resolved on the monitors.
pub(super) fn first_window_attributes(
    attributes: WindowAttributes,
    settings: &WindowSettings,
    fullscreen: Option<Fullscreen>,
) -> WindowAttributes {
    let logical = |v: glam::Vec2| LogicalSize::new(v.x, v.y);
    let mut attributes = attributes.with_inner_size(logical(settings.size));
    attributes.min_inner_size = settings.min_size.map(|v| logical(v).into());
    attributes.max_inner_size = settings.max_size.map(|v| logical(v).into());
    attributes.position = settings.position.map(|p| PhysicalPosition::new(p.x, p.y).into());
    attributes
        .with_fullscreen(fullscreen)
        .with_decorations(settings.decorations)
        .with_window_level(if settings.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        })
}
fn video(v: &winit::monitor::VideoModeHandle) -> VideoMode {
    VideoMode {
        width: v.size().width,
        height: v.size().height,
        refresh_millihertz: v.refresh_rate_millihertz(),
        bit_depth: v.bit_depth(),
    }
}
impl WindowBackend for Backend {
    fn request(&mut self, request: &WindowRequest) -> Result<(), Error> {
        if cfg!(target_arch = "wasm32") {
            validate_web(request)?;
        }
        let w = &self.window;
        match request {
            WindowRequest::Mode(mode) => w.set_fullscreen(self.fullscreen(mode)?),
            WindowRequest::Title(title) => w.set_title(title),
            WindowRequest::Size(v) => {
                let _ = w.request_inner_size(LogicalSize::new(v.x, v.y));
            }
            WindowRequest::Position(p) => w.set_outer_position(PhysicalPosition::new(p.x, p.y)),
            WindowRequest::ResizeLimits(min, max) => {
                w.set_min_inner_size(min.map(|v| LogicalSize::new(v.x, v.y)));
                w.set_max_inner_size(max.map(|v| LogicalSize::new(v.x, v.y)));
            }
            WindowRequest::AlwaysOnTop(v) => w.set_window_level(if *v {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            }),
            WindowRequest::Decorations(v) => w.set_decorations(*v),
            WindowRequest::Settings(s) => {
                let fullscreen = self.fullscreen(&s.mode)?;
                w.set_fullscreen(fullscreen);
                w.set_title(&s.title);
                w.set_min_inner_size(s.min_size.map(|v| LogicalSize::new(v.x, v.y)));
                w.set_max_inner_size(s.max_size.map(|v| LogicalSize::new(v.x, v.y)));
                let _ = w.request_inner_size(LogicalSize::new(s.size.x, s.size.y));
                if let Some(p) = s.position {
                    w.set_outer_position(PhysicalPosition::new(p.x, p.y));
                }
                w.set_window_level(if s.always_on_top {
                    WindowLevel::AlwaysOnTop
                } else {
                    WindowLevel::Normal
                });
                w.set_decorations(s.decorations);
            }
            WindowRequest::Icon(i) => {
                w.set_window_icon(
                    i.as_ref()
                        .map(|i| winit::window::Icon::from_rgba(i.rgba.clone(), i.width, i.height))
                        .transpose()
                        .map_err(|e| {
                            unsupported(format!(
                                "cannot set the window icon: {e}; pass width * height * 4 RGBA8 bytes, or None for the default"
                            ))
                        })?,
                );
            }
            WindowRequest::Cursor(Cursor::System(icon)) => {
                *self.cursor.borrow_mut() = None;
                w.set_cursor(icon.to_winit());
            }
            WindowRequest::Cursor(Cursor::Custom(i, h)) => {
                *self.cursor.borrow_mut() = Some(
                    winit::window::CustomCursor::from_rgba(
                        i.rgba.clone(),
                        i.width as u16,
                        i.height as u16,
                        h.x as u16,
                        h.y as u16,
                    )
                    .map_err(|e| {
                        unsupported(format!(
                            "cannot make the custom cursor: {e}; use a smaller RGBA8 image or a system CursorIcon"
                        ))
                    })?,
                );
            }
            WindowRequest::CursorVisible(v) => w.set_cursor_visible(*v),
            WindowRequest::CursorGrab(g) => {
                let mode = match g {
                    CursorGrab::None => CursorGrabMode::None,
                    CursorGrab::Confined => CursorGrabMode::Confined,
                    CursorGrab::Locked => CursorGrabMode::Locked,
                };
                w.set_cursor_grab(mode)
                    .or_else(|e| {
                        if *g == CursorGrab::Locked {
                            w.set_cursor_grab(CursorGrabMode::Confined)
                        } else {
                            Err(e)
                        }
                    })
                    .map_err(|e| {
                        unsupported(format!(
                            "cannot grab the cursor: {e}; the OS refused it (on the web, lock from a click or key press), use CursorGrab::None"
                        ))
                    })?;
            }
            WindowRequest::RefreshMonitors => { /* Queries enumerate live winit handles. */ }
        }
        Ok(())
    }
    fn state(&self) -> WindowState {
        let mode = match self.window.fullscreen() {
            None => WindowMode::Windowed,
            Some(Fullscreen::Borderless(m)) => {
                WindowMode::Borderless(m.and_then(|m| self.window.available_monitors().position(|x| x == m)))
            }
            Some(Fullscreen::Exclusive(v)) => WindowMode::Exclusive(
                self.window
                    .available_monitors()
                    .position(|m| m == v.monitor())
                    .unwrap_or(0),
                video(&v),
            ),
        };
        WindowState {
            physical_size: UVec2::new(self.window.inner_size().width, self.window.inner_size().height),
            scale_factor: self.window.scale_factor() as f32,
            focused: self.window.has_focus(),
            minimized: self.window.is_minimized().unwrap_or(false),
            mode,
            monitor: self
                .window
                .current_monitor()
                .and_then(|m| self.window.available_monitors().position(|v| v == m)),
        }
    }
    fn monitors(&self) -> Vec<Monitor> {
        self.window.available_monitors().map(|m| monitor_info(&m)).collect()
    }
    fn clipboard_read(&mut self) -> Result<Option<String>, Error> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.clipboard.is_none() {
                self.clipboard = Some(arboard::Clipboard::new().map_err(clipboard_error)?);
            }
            self.clipboard
                .as_mut()
                .unwrap()
                .get_text()
                .map(Some)
                .map_err(clipboard_error)
        }
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(result) = self.clipboard.borrow_mut().take() {
                self.pending = false;
                return result.map(Some);
            }
            if !self.pending {
                let result = self.clipboard.clone();
                let clipboard = browser_clipboard()?;
                self.pending = true;
                wasm_bindgen_futures::spawn_local(async move {
                    let value = wasm_bindgen_futures::JsFuture::from(clipboard.read_text())
                        .await
                        .map(|v| v.as_string().unwrap_or_default())
                        .map_err(|e| unsupported(format!("cannot read clipboard: permission denied or browser read failed ({e:?}); allow clipboard access on HTTPS and retry from a user gesture")));
                    *result.borrow_mut() = Some(value);
                });
            }
            Ok(None)
        }
    }
    fn clipboard_write(&mut self, text: &str) -> Result<(), Error> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.clipboard.is_none() {
                self.clipboard = Some(arboard::Clipboard::new().map_err(clipboard_error)?);
            }
            self.clipboard.as_mut().unwrap().set_text(text).map_err(clipboard_error)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let result = self.clipboard.clone();
            let clipboard = browser_clipboard()?;
            let promise = clipboard.write_text(text);
            wasm_bindgen_futures::spawn_local(async move {
                if let Err(e) = wasm_bindgen_futures::JsFuture::from(promise).await {
                    *result.borrow_mut() = Some(Err(unsupported(format!(
                        "cannot write clipboard: permission denied or browser write failed ({e:?}); allow clipboard access on HTTPS and retry from a user gesture"
                    ))));
                }
            });
            Ok(())
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn browser_clipboard() -> Result<web_sys::Clipboard, Error> {
    use wasm_bindgen::JsCast;
    let navigator = web_sys::window()
        .ok_or_else(|| unsupported("cannot access clipboard without a browser window; use an HTTPS document"))?
        .navigator();
    let value = js_sys::Reflect::get(&navigator, &"clipboard".into()).map_err(|e| {
        unsupported(format!(
            "cannot access clipboard: {e:?}; allow permission on an HTTPS document"
        ))
    })?;
    if value.is_null() || value.is_undefined() {
        return Err(unsupported(
            "cannot access clipboard in this context; use HTTPS and allow clipboard permission",
        ));
    }
    Ok(value.unchecked_into())
}

#[cfg(not(target_arch = "wasm32"))]
fn clipboard_error(e: arboard::Error) -> Error {
    unsupported(format!(
        "cannot access clipboard: {e}; ensure a desktop clipboard is available, allow access and retry"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::{Position, Size};

    #[test]
    fn first_window_is_created_with_the_restored_settings_and_shown_as_is() {
        let mut s = WindowSettings::default();
        s.size = glam::Vec2::new(1280.0, 720.0);
        s.position = Some(IVec2::new(40, -60));
        s.min_size = Some(glam::Vec2::new(320.0, 240.0));
        s.max_size = Some(glam::Vec2::new(2000.0, 1500.0));
        s.always_on_top = true;
        s.decorations = false;
        let a = first_window_attributes(Window::default_attributes(), &s, Some(Fullscreen::Borderless(None)));
        assert_eq!(a.inner_size, Some(Size::Logical(LogicalSize::new(1280.0, 720.0))));
        assert_eq!(a.min_inner_size, Some(Size::Logical(LogicalSize::new(320.0, 240.0))));
        assert_eq!(a.max_inner_size, Some(Size::Logical(LogicalSize::new(2000.0, 1500.0))));
        assert_eq!(a.position, Some(Position::Physical(PhysicalPosition::new(40, -60))));
        assert_eq!(a.fullscreen, Some(Fullscreen::Borderless(None)));
        assert_eq!(a.window_level, WindowLevel::AlwaysOnTop);
        assert!(!a.decorations);
        // Nothing is changed after the window appears, so it isn't hidden either.
        assert!(a.visible);
    }

    #[test]
    fn first_window_without_saved_settings_matches_the_game_defaults() {
        let s = WindowSettings::default();
        let a = first_window_attributes(Window::default_attributes(), &s, None);
        let plain = Window::default_attributes();
        assert_eq!(a.inner_size, Some(Size::Logical(LogicalSize::new(800.0, 600.0))));
        assert_eq!(a.position, plain.position);
        assert_eq!(a.min_inner_size, plain.min_inner_size);
        assert_eq!(a.max_inner_size, plain.max_inner_size);
        assert_eq!(a.fullscreen, None);
        assert_eq!(a.window_level, plain.window_level);
        assert_eq!(a.decorations, plain.decorations);
    }
}
