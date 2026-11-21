//! The built-in controllers of a web build with threads: the browser's
//! gamepads, read on the page's thread and passed to the engine as events.
//!
//! The ordinary web build reads them through gilrs, whose backend keeps the
//! browser's `Gamepad` objects. Once the memory is shared those can't be
//! sent between threads, and a [`GamepadBackend`] must be `Send`, so this
//! one keeps none: every poll it calls `navigator.getGamepads()` on the
//! page's thread (the game's thread there), copies what it needs into plain
//! numbers ([`PadReading`]) and compares them with the last poll's to report
//! connections, buttons and axes. The comparison is plain Rust and is tested
//! natively with fed readings; `web/smoke/threads.mjs --case gamepads` feeds
//! a page the browser's side of it.
//!
//! The pads follow the browser's "standard" layout, as gilrs does on the
//! web: buttons 0-15 are the face buttons, bumpers, triggers, Select,
//! Start, the stick buttons and the d-pad, axes 0-3 the two sticks (y down,
//! as the engine has them). Triggers report their value as an axis and,
//! when the browser says they are pressed, as a button. A pad's key is its
//! browser index. Rumble goes to the pad's `vibrationActuator` where the
//! browser has one (see [`crate::web_rumble`]); there is no motion, as with
//! gilrs on the web.

use crate::input::{
    ControllerFamily, DeviceKey, GamepadAxis, GamepadBackend, GamepadButton, GamepadEvents, GamepadInfo, MotorLevels,
};
use crate::web_rumble::{Actuators, WebRumble};

/// Buttons of the standard layout the engine reads (16 is the home button,
/// which it has no name for).
pub(crate) const BUTTONS: usize = 16;
/// Axes of the standard layout: the two sticks.
pub(crate) const AXES: usize = 4;

/// The engine's name for each standard button, by index.
const BUTTON_NAMES: [GamepadButton; BUTTONS] = [
    GamepadButton::South,
    GamepadButton::East,
    GamepadButton::West,
    GamepadButton::North,
    GamepadButton::LeftBumper,
    GamepadButton::RightBumper,
    GamepadButton::LeftTrigger,
    GamepadButton::RightTrigger,
    GamepadButton::Select,
    GamepadButton::Start,
    GamepadButton::LeftStick,
    GamepadButton::RightStick,
    GamepadButton::DPadUp,
    GamepadButton::DPadDown,
    GamepadButton::DPadLeft,
    GamepadButton::DPadRight,
];

/// The engine's name for each standard axis, by index.
const AXIS_NAMES: [GamepadAxis; AXES] = [
    GamepadAxis::LeftStickX,
    GamepadAxis::LeftStickY,
    GamepadAxis::RightStickX,
    GamepadAxis::RightStickY,
];

/// The standard buttons whose value is also an axis: the triggers.
const TRIGGERS: [(usize, GamepadAxis); 2] = [(6, GamepadAxis::LeftTrigger), (7, GamepadAxis::RightTrigger)];

/// Pads the browser lists at most (Chromium: 4; the rest are ignored).
const MAX_PADS: usize = 8;

/// One standard button: whether the browser says it is pressed, and how far (`0..=1`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct ButtonReading {
    pub(crate) pressed: bool,
    pub(crate) value: f32,
}

/// One of the browser's gamepads in one poll, as plain numbers.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PadReading {
    /// Its place in `navigator.getGamepads()`.
    pub(crate) index: u32,
    /// The browser's `id` text. Read only for a pad the backend doesn't know
    /// yet (a new string every poll would allocate), so `None` otherwise.
    pub(crate) id: Option<String>,
    /// Whether the pad has a `vibrationActuator`. Read with `id`, so only
    /// meaningful when that is `Some`.
    pub(crate) rumble: bool,
    pub(crate) buttons: [ButtonReading; BUTTONS],
    pub(crate) axes: [f32; AXES],
}

/// Where the readings come from, and where rumble goes: the browser
/// ([`BrowserPads`]), or a test.
pub(crate) trait ReadPads: Actuators + Send + 'static {
    /// Replaces `into` with the gamepads connected now. `known(index)` says
    /// whether the backend already has that pad, whose `id` it then needn't read.
    fn read(&mut self, known: &dyn Fn(u32) -> bool, into: &mut Vec<PadReading>);
}

/// A pad the backend has reported connected, and what it last reported.
#[derive(Debug, Clone)]
struct Known {
    index: u32,
    buttons: [ButtonReading; BUTTONS],
    axes: [f32; AXES],
}

/// The [`GamepadBackend`] over the browser's gamepads (or fed readings).
pub(crate) struct WebGamepads<R: ReadPads> {
    reader: R,
    known: Vec<Known>,
    /// This poll's readings, kept so polling doesn't allocate.
    readings: Vec<PadReading>,
    /// The rumble playing, replayed before the browser ends it.
    rumble: WebRumble,
}

impl<R: ReadPads> WebGamepads<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self {
            reader,
            known: Vec::with_capacity(MAX_PADS),
            readings: Vec::with_capacity(MAX_PADS),
            rumble: WebRumble::default(),
        }
    }
}

/// The name, family and ids of a pad from the browser's `id`, which carries
/// the USB ids in Chromium's (`... (STANDARD GAMEPAD Vendor: 054c Product:
/// 09cc)`) and Firefox's (`054c-09cc-Wireless Controller`) forms.
pub(crate) fn info_from_id(id: &str) -> GamepadInfo {
    let hex = |s: &str| (s.len() == 4).then(|| u16::from_str_radix(s, 16).ok()).flatten();
    let chromium = || {
        let rest = &id[id.rfind("Vendor: ")? + "Vendor: ".len()..];
        let (vendor, rest) = rest.split_once(" Product: ")?;
        Some((hex(vendor)?, hex(rest.strip_suffix(')')?)?))
    };
    let firefox = || {
        let mut parts = id.splitn(3, '-');
        let ids = (hex(parts.next()?)?, hex(parts.next()?)?);
        parts.next()?; // the name follows
        Some(ids)
    };
    let ids = chromium().or_else(firefox);
    GamepadInfo {
        name: id.to_string(),
        family: ControllerFamily::detect(ids.map(|i| i.0), ids.map(|i| i.1), id),
        vendor_id: ids.map(|i| i.0),
        product_id: ids.map(|i| i.1),
        ..GamepadInfo::default()
    }
}

impl<R: ReadPads> GamepadBackend for WebGamepads<R> {
    fn poll(&mut self, events: &mut GamepadEvents<'_>) {
        let known = &self.known;
        let rumble = &mut self.rumble;
        self.reader
            .read(&|index| known.iter().any(|k| k.index == index), &mut self.readings);
        // Gone since the last poll: disconnected.
        self.known.retain(|k| {
            let here = self.readings.iter().any(|r| r.index == k.index);
            if !here {
                let device = DeviceKey(u64::from(k.index));
                rumble.forget(device);
                events.disconnected(device);
            }
            here
        });
        for reading in &self.readings {
            let device = DeviceKey(u64::from(reading.index));
            let at = match self.known.iter().position(|k| k.index == reading.index) {
                Some(at) => at,
                None => {
                    // New: connected, then everything that isn't at rest.
                    if self.known.len() == MAX_PADS {
                        continue;
                    }
                    let mut info = info_from_id(reading.id.as_deref().unwrap_or(""));
                    info.capabilities.rumble = reading.rumble;
                    events.connected(device, info);
                    self.known.push(Known {
                        index: reading.index,
                        buttons: [ButtonReading::default(); BUTTONS],
                        axes: [0.0; AXES],
                    });
                    self.known.len() - 1
                }
            };
            let last = &mut self.known[at];
            for (i, (now, before)) in reading.buttons.iter().zip(&mut last.buttons).enumerate() {
                if now.pressed != before.pressed {
                    events.button(device, BUTTON_NAMES[i], now.pressed);
                }
                if now.value != before.value
                    && let Some(&(_, axis)) = TRIGGERS.iter().find(|(t, _)| *t == i)
                {
                    events.axis(device, axis, now.value.clamp(0.0, 1.0));
                }
                *before = *now;
            }
            for (i, (&now, before)) in reading.axes.iter().zip(&mut last.axes).enumerate() {
                if now != *before {
                    events.axis(device, AXIS_NAMES[i], now.clamp(-1.0, 1.0));
                    *before = now;
                }
            }
        }
        self.rumble.refresh(&mut self.reader);
    }

    /// Plays the body motors on the pad's `vibrationActuator`, where it has
    /// one, again before the browser ends each effect (`web_rumble`).
    fn set_rumble(&mut self, device: DeviceKey, levels: MotorLevels) {
        let Some(known) = self.known.iter().find(|k| u64::from(k.index) == device.0) else {
            return;
        };
        self.rumble.set(&mut self.reader, device, known.index, levels);
    }
}

/// The browser's gamepads: `navigator.getGamepads()`, read on the calling
/// thread. Where there is no such call (a worker), there are no pads.
#[cfg(target_arch = "wasm32")]
pub(crate) struct BrowserPads;

#[cfg(target_arch = "wasm32")]
impl Actuators for BrowserPads {
    fn play(&mut self, index: u32, strong: f32, weak: f32, duration_ms: u32) {
        crate::web_rumble::Browser.play(index, strong, weak, duration_ms);
    }

    fn stop(&mut self, index: u32) {
        crate::web_rumble::Browser.stop(index);
    }
}

#[cfg(target_arch = "wasm32")]
impl ReadPads for BrowserPads {
    fn read(&mut self, known: &dyn Fn(u32) -> bool, into: &mut Vec<PadReading>) {
        use wasm_bindgen::{JsCast, JsValue};
        into.clear();
        let get = |object: &JsValue, key: &str| js_sys::Reflect::get(object, &JsValue::from_str(key)).ok();
        let Some(navigator) = get(&js_sys::global(), "navigator") else {
            return;
        };
        let Some(call) = get(&navigator, "getGamepads").and_then(|f| f.dyn_into::<js_sys::Function>().ok()) else {
            return;
        };
        let Ok(list) = call.call0(&navigator) else { return };
        let length = get(&list, "length").and_then(|l| l.as_f64()).unwrap_or(0.0) as u32;
        for slot in 0..length.min(MAX_PADS as u32) {
            let Ok(pad) = js_sys::Reflect::get_u32(&list, slot) else {
                continue;
            };
            if pad.is_null() || pad.is_undefined() || get(&pad, "connected").is_some_and(|c| c.is_falsy()) {
                continue;
            }
            let index = get(&pad, "index").and_then(|i| i.as_f64()).map_or(slot, |i| i as u32);
            let mut reading = PadReading {
                index,
                id: (!known(index)).then(|| get(&pad, "id").and_then(|id| id.as_string()).unwrap_or_default()),
                rumble: !known(index) && crate::web_rumble::Browser::has_actuator(&pad),
                ..PadReading::default()
            };
            if let Some(buttons) = get(&pad, "buttons") {
                for (i, button) in reading.buttons.iter_mut().enumerate() {
                    let Ok(b) = js_sys::Reflect::get_u32(&buttons, i as u32) else {
                        break;
                    };
                    if b.is_undefined() || b.is_null() {
                        break;
                    }
                    button.pressed = get(&b, "pressed").is_some_and(|p| p.is_truthy());
                    button.value = get(&b, "value").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                }
            }
            if let Some(axes) = get(&pad, "axes") {
                for (i, axis) in reading.axes.iter_mut().enumerate() {
                    *axis = js_sys::Reflect::get_u32(&axes, i as u32)
                        .ok()
                        .and_then(|a| a.as_f64())
                        .unwrap_or(0.0) as f32;
                }
            }
            into.push(reading);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::*;
    use crate::ctx::Ctx;
    use crate::game::Game;
    use crate::input::{Gamepad, Rumble};

    /// Readings a test sets between frames, as a page's
    /// `navigator.getGamepads()` would return them; `ids_read` counts the
    /// `id`s the backend asked for.
    /// One call on a fake actuator: `(index, strong, weak, duration in ms)`.
    type Effect = (u32, f32, f32, u32);

    #[derive(Clone)]
    struct Fed {
        pads: Arc<Mutex<Vec<PadReading>>>,
        ids_read: Arc<Mutex<u32>>,
        /// What the browser's actuators were asked: `(index, strong, weak,
        /// duration in ms)` per `playEffect`, `(index, 0, 0, 0)` per `reset`.
        effects: Arc<Mutex<Vec<Effect>>>,
        /// How far the test has moved the clock.
        elapsed: Arc<Mutex<Duration>>,
        start: Arc<web_time::Instant>,
    }

    impl Default for Fed {
        fn default() -> Self {
            Self {
                pads: Arc::default(),
                ids_read: Arc::default(),
                effects: Arc::default(),
                elapsed: Arc::default(),
                start: Arc::new(web_time::Instant::now()),
            }
        }
    }

    impl Actuators for Fed {
        fn now(&self) -> web_time::Instant {
            *self.start + *self.elapsed.lock().unwrap()
        }
        fn play(&mut self, index: u32, strong: f32, weak: f32, duration_ms: u32) {
            self.effects.lock().unwrap().push((index, strong, weak, duration_ms));
        }
        fn stop(&mut self, index: u32) {
            self.effects.lock().unwrap().push((index, 0.0, 0.0, 0));
        }
    }

    impl Fed {
        fn set(&self, change: impl FnOnce(&mut Vec<PadReading>)) {
            change(&mut self.pads.lock().unwrap());
        }
    }

    impl ReadPads for Fed {
        fn read(&mut self, known: &dyn Fn(u32) -> bool, into: &mut Vec<PadReading>) {
            into.clear();
            for pad in self.pads.lock().unwrap().iter() {
                let mut pad = pad.clone();
                if known(pad.index) {
                    pad.id = None;
                } else {
                    *self.ids_read.lock().unwrap() += 1;
                }
                into.push(pad);
            }
        }
    }

    /// Runs `frames` headless frames over the backend reading `fed`; `each`
    /// runs in a simulation system with the frame number. What is fed
    /// during frame `n` reaches `ctx.input` in frame `n + 1`.
    fn run(fed: &Fed, frames: u64, mut each: impl FnMut(&mut Ctx, u64) + 'static) {
        let mut frame = 0;
        Game::new("web pads", 1, 1)
            .gamepad_backend(WebGamepads::new(fed.clone()))
            .sim_system(move |ctx: &mut Ctx| {
                each(ctx, frame);
                frame += 1;
            })
            .run_headless(frames, |_| {});
    }

    fn pad(index: u32, id: &str) -> PadReading {
        PadReading {
            index,
            id: Some(id.to_string()),
            ..PadReading::default()
        }
    }

    fn slot(n: usize) -> Gamepad {
        Gamepad::from_slot(n).unwrap()
    }

    fn press(reading: &mut PadReading, button: usize, value: f32) {
        reading.buttons[button] = ButtonReading {
            pressed: value > 0.5,
            value,
        };
    }

    const XBOX: &str = "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)";

    #[test]
    fn a_fed_browser_pad_connects_with_its_name_family_and_ids() {
        let fed = Fed::default();
        let feed = fed.clone();
        let frames = Arc::new(Mutex::new(0));
        let seen = frames.clone();
        run(&fed, 2, move |ctx, frame| match frame {
            0 => {
                *seen.lock().unwrap() += 1;
                assert!(ctx.input.gamepads().is_empty());
                feed.set(|pads| pads.push(pad(0, XBOX)));
            }
            _ => {
                assert_eq!(ctx.input.gamepads(), [slot(0)]);
                let info = ctx.input.gamepad_info(slot(0)).unwrap();
                assert_eq!(info.name, XBOX);
                assert_eq!(info.family, ControllerFamily::Xbox);
                assert_eq!((info.vendor_id, info.product_id), (Some(0x045e), Some(0x0b13)));
                assert!(!info.capabilities.rumble && !info.capabilities.motion);
                *seen.lock().unwrap() += 1;
            }
        });
        assert_eq!(*frames.lock().unwrap(), 2, "every frame's checks ran");
    }

    #[test]
    fn fed_buttons_sticks_and_triggers_reach_the_input() {
        let fed = Fed::default();
        fed.set(|pads| pads.push(pad(0, XBOX)));
        let feed = fed.clone();
        let p = slot(0);
        let frames = Arc::new(Mutex::new(0));
        let seen = frames.clone();
        run(&fed, 3, move |ctx, frame| {
            *seen.lock().unwrap() += 1;
            match frame {
                0 => feed.set(|pads| {
                    press(&mut pads[0], 0, 1.0); // South
                    press(&mut pads[0], 15, 1.0); // d-pad right
                    press(&mut pads[0], 6, 0.25); // left trigger, a quarter in
                    pads[0].axes = [0.5, -1.0, 0.0, 0.75];
                }),
                1 => {
                    let input = &ctx.input;
                    assert!(input.gamepad_pressed(p, GamepadButton::South));
                    assert!(input.gamepad_down(p, GamepadButton::DPadRight));
                    assert!(!input.gamepad_down(p, GamepadButton::East));
                    assert!(
                        !input.gamepad_down(p, GamepadButton::LeftTrigger),
                        "a quarter isn't pressed"
                    );
                    assert_eq!(input.gamepad_axis(p, GamepadAxis::LeftTrigger), 0.25);
                    assert_eq!(input.gamepad_axis(p, GamepadAxis::LeftStickX), 0.5);
                    assert_eq!(
                        input.gamepad_axis(p, GamepadAxis::LeftStickY),
                        -1.0,
                        "up stays negative"
                    );
                    assert_eq!(input.gamepad_axis(p, GamepadAxis::RightStickY), 0.75);
                    feed.set(|pads| {
                        press(&mut pads[0], 0, 0.0);
                        press(&mut pads[0], 6, 1.0);
                        pads[0].axes = [0.0; AXES];
                    });
                }
                _ => {
                    let input = &ctx.input;
                    assert!(input.gamepad_released(p, GamepadButton::South));
                    assert!(
                        input.gamepad_down(p, GamepadButton::DPadRight),
                        "held buttons stay down"
                    );
                    assert!(input.gamepad_pressed(p, GamepadButton::LeftTrigger));
                    assert_eq!(input.gamepad_axis(p, GamepadAxis::LeftTrigger), 1.0);
                    assert_eq!(input.gamepad_axis(p, GamepadAxis::LeftStickX), 0.0);
                }
            }
        });
        assert_eq!(*frames.lock().unwrap(), 3, "every frame's checks ran");
    }

    #[test]
    fn a_pad_that_leaves_the_browsers_list_disconnects_and_gets_its_slot_back() {
        let fed = Fed::default();
        fed.set(|pads| {
            pads.push(pad(0, XBOX));
            let mut second = pad(1, "054c-09cc-Wireless Controller");
            press(&mut second, 1, 1.0);
            pads.push(second);
        });
        let feed = fed.clone();
        let frames = Arc::new(Mutex::new(0));
        let seen = frames.clone();
        run(&fed, 3, move |ctx, frame| {
            *seen.lock().unwrap() += 1;
            match frame {
                0 => {
                    assert_eq!(ctx.input.gamepads(), [slot(0), slot(1)]);
                    let info = ctx.input.gamepad_info(slot(1)).unwrap();
                    assert_eq!(info.family, ControllerFamily::PlayStation);
                    assert!(
                        ctx.input.gamepad_down(slot(1), GamepadButton::East),
                        "held when it connected"
                    );
                    feed.set(|pads| pads.retain(|p| p.index != 0));
                }
                1 => {
                    assert_eq!(ctx.input.gamepads(), [slot(1)]);
                    feed.set(|pads| pads.insert(0, pad(0, XBOX)));
                }
                _ => {
                    assert_eq!(ctx.input.gamepads().len(), 2);
                    assert_eq!(ctx.input.gamepad_info(slot(0)).unwrap().name, XBOX, "back in its slot");
                }
            }
        });
        assert_eq!(*frames.lock().unwrap(), 3, "every frame's checks ran");
    }

    #[test]
    fn a_known_pads_id_is_read_once() {
        let fed = Fed::default();
        fed.set(|pads| pads.push(pad(2, XBOX)));
        let feed = fed.clone();
        run(&fed, 3, move |_, frame| {
            if frame == 0 {
                feed.set(|pads| press(&mut pads[0], 3, 1.0));
            }
        });
        assert_eq!(*fed.ids_read.lock().unwrap(), 1);
    }

    #[test]
    fn usb_ids_come_from_chromium_and_firefox_ids() {
        let chromium = info_from_id("DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)");
        assert_eq!((chromium.vendor_id, chromium.product_id), (Some(0x054c), Some(0x0ce6)));
        assert_eq!(chromium.family, ControllerFamily::PlayStation);
        let firefox = info_from_id("057e-2009-Pro Controller");
        assert_eq!((firefox.vendor_id, firefox.product_id), (Some(0x057e), Some(0x2009)));
        assert_eq!(firefox.family, ControllerFamily::Nintendo);
        let plain = info_from_id("Xbox 360 Controller (XInput STANDARD GAMEPAD)");
        assert_eq!((plain.vendor_id, plain.product_id), (None, None));
        assert_eq!(plain.family, ControllerFamily::Xbox, "from the name");
        assert_eq!(info_from_id("abcd-efgh").vendor_id, None);
    }

    /// A pad with a `vibrationActuator`, at `index`.
    fn rumbling_pad(index: u32) -> PadReading {
        PadReading {
            rumble: true,
            ..pad(index, XBOX)
        }
    }

    #[test]
    fn a_pad_with_a_vibration_actuator_says_it_can_rumble_and_one_without_says_it_cannot() {
        let fed = Fed::default();
        fed.set(|pads| {
            pads.push(rumbling_pad(0));
            pads.push(pad(1, XBOX));
        });
        let seen = Arc::new(Mutex::new(0));
        let count = seen.clone();
        run(&fed, 1, move |ctx, _| {
            assert!(ctx.input.gamepad_info(slot(0)).unwrap().capabilities.rumble);
            assert!(!ctx.input.gamepad_info(slot(1)).unwrap().capabilities.rumble);
            assert!(
                !ctx.input.gamepad_info(slot(0)).unwrap().capabilities.trigger_rumble,
                "body motors only"
            );
            *count.lock().unwrap() += 1;
        });
        assert_eq!(*seen.lock().unwrap(), 1, "the checks ran");
    }

    #[test]
    fn game_rumble_reaches_the_browsers_actuator_of_that_pad_and_stops_when_it_ends() {
        let fed = Fed::default();
        fed.set(|pads| {
            pads.push(rumbling_pad(0));
            pads.push(rumbling_pad(3));
        });
        run(&fed, 6, move |ctx, frame| match frame {
            0 => ctx
                .input
                .rumble(slot(1), Rumble::new(0.75, 0.25).seconds(f32::INFINITY)),
            3 => ctx.input.stop_rumble(slot(1)),
            _ => {}
        });
        let effects = fed.effects.lock().unwrap().clone();
        assert_eq!(
            effects.first(),
            Some(&(3, 0.75, 0.25, 2000)),
            "the pad's browser index, not its slot: {effects:?}"
        );
        assert!(
            effects.iter().all(|e| e.0 == 3),
            "the other pad stays quiet: {effects:?}"
        );
        assert_eq!(
            effects.last(),
            Some(&(3, 0.0, 0.0, 0)),
            "reset when the rumble ended: {effects:?}"
        );
    }

    #[test]
    fn long_rumble_is_replayed_every_second_so_the_browsers_cap_never_ends_it() {
        let fed = Fed::default();
        fed.set(|pads| pads.push(rumbling_pad(0)));
        let clock = fed.elapsed.clone();
        run(&fed, 40, move |ctx, frame| {
            if frame == 0 {
                ctx.input.rumble(slot(0), Rumble::new(1.0, 0.0).seconds(f32::INFINITY));
            }
            // A frame is a quarter of a second on the browser's clock: ten seconds in all.
            *clock.lock().unwrap() = Duration::from_millis(frame * 250);
        });
        let plays = fed.effects.lock().unwrap().iter().filter(|e| e.3 > 0).count();
        assert!(
            (9..=11).contains(&plays),
            "once, then about once a second for 10 s: {plays}"
        );
    }

    #[test]
    fn a_pad_unplugged_while_rumbling_is_forgotten_and_not_replayed() {
        let fed = Fed::default();
        fed.set(|pads| pads.push(rumbling_pad(0)));
        let feed = fed.clone();
        let clock = fed.elapsed.clone();
        run(&fed, 8, move |ctx, frame| match frame {
            0 => ctx.input.rumble(slot(0), Rumble::new(1.0, 1.0).seconds(f32::INFINITY)),
            2 => feed.set(|pads| pads.clear()),
            // The unplug is seen in frame 3's poll: the clock moves on after it.
            4.. => *clock.lock().unwrap() = Duration::from_secs(frame * 2),
            _ => {}
        });
        let effects = fed.effects.lock().unwrap().clone();
        assert!(
            effects.iter().all(|e| e.3 > 0),
            "no call for a pad that is gone: {effects:?}"
        );
        assert_eq!(effects.len(), 1, "not replayed after it left: {effects:?}");
    }
}
