//! The player's keyboard layout while a window is open: where the labels
//! come from on each system, and when they change.
//!
//! - macOS and Windows: the system's current layout ([`KeyLabels::system`]),
//!   read when the window opens and when it gets the focus back.
//! - Linux and the BSDs: the active layout of the xkb keymap, followed as it
//!   changes: on X11 through the X server's XKB extension (its key map,
//!   and its state for the active group), on Wayland through a keyboard
//!   object of our own on the compositor's connection, which is sent the
//!   keymap and the active group like winit's.
//! - The web: the browser's Keyboard Layout Map
//!   (`navigator.keyboard.getLayoutMap()`), asked when the page opens and
//!   when it gets the focus back, where the browser has it (not every
//!   browser does, and only in secure contexts).
//!
//! Everywhere, each character key the player presses corrects its label
//! too (in `window.rs`), which is all a browser without the map gives.

use super::KeyLabels;

/// Follows the player's layout for one window: [`poll`](Self::poll) gives
/// new labels when the layout changed.
pub(crate) struct LayoutWatch {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pending: Option<KeyLabels>,
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    xkb: Option<linux::Xkb>,
    #[cfg(target_arch = "wasm32")]
    web: web::LayoutMap,
}

impl LayoutWatch {
    /// Starts following the layout for `window`; the first labels are
    /// ready at once where the system answers at once.
    #[cfg_attr(
        not(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        )),
        allow(unused_variables)
    )]
    pub(crate) fn new(window: &winit::window::Window) -> Self {
        let mut watch = Self {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            pending: None,
            #[cfg(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd"
            ))]
            xkb: linux::Xkb::open(window),
            #[cfg(target_arch = "wasm32")]
            web: web::LayoutMap::default(),
        };
        watch.refresh();
        watch
    }

    /// The window got the focus back: the player may have switched layouts
    /// in another program.
    pub(crate) fn refresh(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.pending = KeyLabels::system();
        }
        #[cfg(target_arch = "wasm32")]
        self.web.ask();
    }

    /// New labels, when the layout changed since the last call.
    pub(crate) fn poll(&mut self) -> Option<KeyLabels> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        return self.pending.take();
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        return self.xkb.as_mut().and_then(linux::Xkb::poll);
        #[cfg(target_arch = "wasm32")]
        return self.web.take();
        #[allow(unreachable_code)]
        None
    }
}

/// The DOM `code` of each key whose label follows the layout, as a
/// Keyboard Layout Map names them.
#[cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]
pub(crate) fn dom_code(key: crate::input::Key) -> Option<&'static str> {
    use crate::input::Key::*;
    Some(match key {
        A => "KeyA",
        B => "KeyB",
        C => "KeyC",
        D => "KeyD",
        E => "KeyE",
        F => "KeyF",
        G => "KeyG",
        H => "KeyH",
        I => "KeyI",
        J => "KeyJ",
        K => "KeyK",
        L => "KeyL",
        M => "KeyM",
        N => "KeyN",
        O => "KeyO",
        P => "KeyP",
        Q => "KeyQ",
        R => "KeyR",
        S => "KeyS",
        T => "KeyT",
        U => "KeyU",
        V => "KeyV",
        W => "KeyW",
        X => "KeyX",
        Y => "KeyY",
        Z => "KeyZ",
        Num0 => "Digit0",
        Num1 => "Digit1",
        Num2 => "Digit2",
        Num3 => "Digit3",
        Num4 => "Digit4",
        Num5 => "Digit5",
        Num6 => "Digit6",
        Num7 => "Digit7",
        Num8 => "Digit8",
        Num9 => "Digit9",
        Minus => "Minus",
        Equal => "Equal",
        LeftBracket => "BracketLeft",
        RightBracket => "BracketRight",
        Backslash => "Backslash",
        Semicolon => "Semicolon",
        Apostrophe => "Quote",
        Backquote => "Backquote",
        Comma => "Comma",
        Period => "Period",
        Slash => "Slash",
        _ => return None,
    })
}

/// Labels from a Keyboard Layout Map: `get` is the map's `get(code)`,
/// what the key at that DOM `code` types (one character, or nothing).
#[cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]
pub(crate) fn labels_from_layout_map(mut get: impl FnMut(&str) -> Option<String>) -> KeyLabels {
    KeyLabels::from_layout(|key| {
        let text = get(dom_code(key)?)?;
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    })
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::{JsCast, JsValue};

    use super::KeyLabels;

    /// The browser's Keyboard Layout Map, asked for asynchronously.
    #[derive(Default)]
    pub(super) struct LayoutMap {
        ready: Rc<RefCell<Option<KeyLabels>>>,
    }

    impl LayoutMap {
        /// Asks the browser for its layout map; nothing where it has none
        /// (Firefox, Safari, a page outside a secure context) or refuses.
        pub(super) fn ask(&mut self) {
            let Some(promise) = layout_map_promise() else { return };
            let ready = self.ready.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let Ok(map) = wasm_bindgen_futures::JsFuture::from(promise).await else {
                    return;
                };
                let Ok(get) = js_sys::Reflect::get(&map, &"get".into()) else {
                    return;
                };
                let Ok(get) = get.dyn_into::<js_sys::Function>() else {
                    return;
                };
                let labels =
                    super::labels_from_layout_map(|code| get.call1(&map, &JsValue::from_str(code)).ok()?.as_string());
                *ready.borrow_mut() = Some(labels);
            });
        }

        pub(super) fn take(&mut self) -> Option<KeyLabels> {
            self.ready.borrow_mut().take()
        }
    }

    /// `navigator.keyboard.getLayoutMap()`, where the browser has it.
    fn layout_map_promise() -> Option<js_sys::Promise> {
        let navigator = web_sys::window()?.navigator();
        let keyboard = js_sys::Reflect::get(&navigator, &"keyboard".into()).ok()?;
        if keyboard.is_undefined() || keyboard.is_null() {
            return None;
        }
        let get = js_sys::Reflect::get(&keyboard, &"getLayoutMap".into()).ok()?;
        let get = get.dyn_into::<js_sys::Function>().ok()?;
        get.call0(&keyboard).ok()?.dyn_into::<js_sys::Promise>().ok()
    }
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
mod linux {
    use super::KeyLabels;
    use crate::input::key_labels::xkb::XkbKeymap;
    use winit::raw_window_handle::{HasDisplayHandle, RawDisplayHandle};

    /// The xkb keymap and active group of the window's display server.
    pub(super) enum Xkb {
        X11(x11::Watch),
        Wayland(wayland::Watch),
    }

    impl Xkb {
        pub(super) fn open(window: &winit::window::Window) -> Option<Self> {
            match window.display_handle().ok()?.as_raw() {
                RawDisplayHandle::Wayland(handle) => wayland::Watch::open(handle.display.as_ptr()).map(Self::Wayland),
                RawDisplayHandle::Xlib(_) | RawDisplayHandle::Xcb(_) => x11::Watch::open().map(Self::X11),
                _ => None,
            }
        }

        pub(super) fn poll(&mut self) -> Option<KeyLabels> {
            match self {
                Self::X11(watch) => watch.poll(),
                Self::Wayland(watch) => watch.poll(),
            }
        }
    }

    /// What both servers give: a keymap and the active group, and whether
    /// either changed since the labels were last made.
    #[derive(Default)]
    struct Layout {
        keymap: Option<XkbKeymap>,
        group: u32,
        changed: bool,
    }

    impl Layout {
        fn set_keymap(&mut self, keymap: XkbKeymap) {
            if self.keymap.as_ref() != Some(&keymap) {
                self.keymap = Some(keymap);
                self.changed = true;
            }
        }

        fn set_group(&mut self, group: u32) {
            if self.group != group {
                self.group = group;
                self.changed = true;
            }
        }

        fn take(&mut self) -> Option<KeyLabels> {
            if !std::mem::take(&mut self.changed) {
                return None;
            }
            self.keymap.as_ref().map(|k| k.labels(self.group))
        }
    }

    mod x11 {
        use x11rb::connection::Connection;
        use x11rb::protocol::Event;
        use x11rb::protocol::xkb::{self, ConnectionExt as _};
        use x11rb::rust_connection::RustConnection;

        use super::{KeyLabels, Layout, XkbKeymap};

        /// A connection of our own to the X server (`$DISPLAY`, as winit's),
        /// told about changes to the core keyboard's map and state.
        pub(in super::super) struct Watch {
            conn: RustConnection,
            layout: Layout,
        }

        const CORE: xkb::DeviceSpec = 0x100; // XkbUseCoreKbd

        impl Watch {
            pub(in super::super) fn open() -> Option<Self> {
                let (conn, _) = x11rb::connect(None).ok()?;
                if !conn.xkb_use_extension(1, 0).ok()?.reply().ok()?.supported {
                    return None;
                }
                let events =
                    xkb::EventType::NEW_KEYBOARD_NOTIFY | xkb::EventType::STATE_NOTIFY | xkb::EventType::MAP_NOTIFY;
                conn.xkb_select_events(
                    CORE,
                    xkb::EventType::from(0u16),
                    events,
                    xkb::MapPart::KEY_SYMS,
                    xkb::MapPart::KEY_SYMS,
                    &xkb::SelectEventsAux::new(),
                )
                .ok()?;
                let mut watch = Self {
                    conn,
                    layout: Layout::default(),
                };
                watch.read_map();
                let group = watch.conn.xkb_get_state(CORE).ok()?.reply().ok()?.group;
                watch.layout.set_group(u32::from(u8::from(group)));
                watch.conn.flush().ok()?;
                Some(watch)
            }

            /// Reads the core keyboard's key symbols (all of them).
            fn read_map(&mut self) {
                let reply = self
                    .conn
                    .xkb_get_map(
                        CORE,
                        xkb::MapPart::KEY_SYMS,
                        xkb::MapPart::from(0u16),
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        xkb::VMod::from(0u16),
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                    )
                    .ok()
                    .and_then(|cookie| cookie.reply().ok());
                let Some(reply) = reply else { return };
                let Some(syms) = reply.map.syms_rtrn else { return };
                let keymap = XkbKeymap::from_x11(
                    reply.first_key_sym,
                    syms.iter().map(|m| (m.group_info, m.width, m.syms.as_slice())),
                );
                self.layout.set_keymap(keymap);
            }

            pub(in super::super) fn poll(&mut self) -> Option<KeyLabels> {
                let mut reread = false;
                while let Ok(Some(event)) = self.conn.poll_for_event() {
                    match event {
                        Event::XkbStateNotify(e) => self.layout.set_group(u32::from(u8::from(e.group))),
                        Event::XkbMapNotify(_) | Event::XkbNewKeyboardNotify(_) => reread = true,
                        _ => {}
                    }
                }
                if reread {
                    self.read_map();
                }
                self.layout.take()
            }
        }
    }

    mod wayland {
        use std::ffi::c_void;
        use std::fs::File;
        use std::os::unix::fs::FileExt;

        use wayland_client::backend::Backend;
        use wayland_client::protocol::{wl_keyboard, wl_registry, wl_seat};
        use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};

        use super::{KeyLabels, Layout, XkbKeymap};

        /// A keyboard object of our own on winit's connection, on a queue
        /// of our own: the compositor sends it the keymap and the active
        /// group as it does winit's.
        pub(in super::super) struct Watch {
            queue: EventQueue<State>,
            state: State,
            conn: Connection,
        }

        #[derive(Default)]
        struct State {
            seat: Option<wl_seat::WlSeat>,
            keyboard: Option<wl_keyboard::WlKeyboard>,
            layout: Layout,
        }

        impl Watch {
            /// `display` is winit's `wl_display`, alive as long as the
            /// event loop, which outlives the window and this watch.
            pub(in super::super) fn open(display: *mut c_void) -> Option<Self> {
                // SAFETY: winit's display stays alive while its event loop
                // runs, and the watch is dropped with the app, before it.
                let backend = unsafe { Backend::from_foreign_display(display.cast()) };
                let conn = Connection::from_backend(backend);
                let mut queue = conn.new_event_queue();
                let qh = queue.handle();
                conn.display().get_registry(&qh, ());
                let mut state = State::default();
                // The globals, then the seat's capabilities, then the keymap.
                for _ in 0..3 {
                    queue.roundtrip(&mut state).ok()?;
                }
                Some(Self { queue, state, conn })
            }

            pub(in super::super) fn poll(&mut self) -> Option<KeyLabels> {
                let _ = self.conn.flush();
                // winit reads the socket; the events for our queue wait here.
                let _ = self.queue.dispatch_pending(&mut self.state);
                self.state.layout.take()
            }
        }

        impl Dispatch<wl_registry::WlRegistry, ()> for State {
            fn event(
                state: &mut Self,
                registry: &wl_registry::WlRegistry,
                event: wl_registry::Event,
                _: &(),
                _: &Connection,
                qh: &QueueHandle<Self>,
            ) {
                if let wl_registry::Event::Global {
                    name,
                    interface,
                    version,
                } = event
                    && interface == "wl_seat"
                    && state.seat.is_none()
                {
                    state.seat = Some(registry.bind(name, version.min(5), qh, ()));
                }
            }
        }

        impl Dispatch<wl_seat::WlSeat, ()> for State {
            fn event(
                state: &mut Self,
                seat: &wl_seat::WlSeat,
                event: wl_seat::Event,
                _: &(),
                _: &Connection,
                qh: &QueueHandle<Self>,
            ) {
                if let wl_seat::Event::Capabilities {
                    capabilities: WEnum::Value(caps),
                } = event
                    && caps.contains(wl_seat::Capability::Keyboard)
                    && state.keyboard.is_none()
                {
                    state.keyboard = Some(seat.get_keyboard(qh, ()));
                }
            }
        }

        impl Dispatch<wl_keyboard::WlKeyboard, ()> for State {
            fn event(
                state: &mut Self,
                _: &wl_keyboard::WlKeyboard,
                event: wl_keyboard::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                match event {
                    wl_keyboard::Event::Keymap {
                        format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
                        fd,
                        size,
                    } => {
                        // Read at offset 0 (the fd's offset may be shared
                        // with other clients); the text ends with a NUL.
                        let mut bytes = vec![0; size as usize];
                        if File::from(fd).read_exact_at(&mut bytes, 0).is_ok() {
                            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                            if let Some(keymap) = std::str::from_utf8(&bytes[..end]).ok().and_then(XkbKeymap::parse) {
                                state.layout.set_keymap(keymap);
                            }
                        }
                    }
                    wl_keyboard::Event::Modifiers { group, .. } => state.layout.set_group(group),
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Key;

    #[test]
    fn every_layout_key_has_a_dom_code_and_no_other_key_does() {
        let mut codes: Vec<_> = super::super::LAYOUT_KEYS
            .iter()
            .map(|&(key, ..)| dom_code(key).unwrap_or_else(|| panic!("{key:?} has no DOM code")))
            .collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), 47);
        assert_eq!(dom_code(Key::Space), None);
    }

    #[test]
    fn a_layout_map_labels_the_keys_as_the_browser_says() {
        // What Chromium's `getLayoutMap()` holds on a French keyboard.
        let azerty = [
            ("KeyQ", "a"),
            ("KeyW", "z"),
            ("KeyA", "q"),
            ("KeyZ", "w"),
            ("Semicolon", "m"),
            ("Digit2", "é"),
            ("BracketLeft", "^"),
            ("KeyS", "s"),
            ("Slash", "ab"), // not one character: kept as on a US keyboard
        ];
        let labels = labels_from_layout_map(|code| azerty.iter().find(|e| e.0 == code).map(|e| e.1.to_string()));
        assert_eq!(
            [Key::W, Key::A, Key::S, Key::D, Key::Num2, Key::LeftBracket, Key::Slash].map(|k| labels.label(k)),
            ["Z", "Q", "S", "D", "É", "^", "/"]
        );
        assert_eq!(labels_from_layout_map(|_| None), KeyLabels::us(), "an empty map");
    }
}
