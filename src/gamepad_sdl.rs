//! The second desktop gamepad backend: SDL 3's gamepad API (feature
//! `gamepad-sdl`), installed by the window instead of gilrs.
//!
//! What it adds over gilrs (the guide's controllers chapter has the table
//! and the reasons for the library): gyro and accelerometer readings on pads
//! that have them (DualShock 4, DualSense, Switch Pro, Joy-Cons), trigger
//! rumble (Xbox One and Series pads: Windows.Gaming.Input on Windows, the
//! GameController framework on macOS, SDL's own USB driver on Linux), and
//! body rumble on macOS. What SDL reports a pad can do is what
//! `GamepadInfo::capabilities` says.
//!
//! Two layers, so everything but the calls into SDL is tested without a pad:
//! [`SdlGamepads`] turns plain-number events ([`PadEvent`]) into the
//! engine's (slots by device, dead zones as gilrs has them, triggers as
//! buttons, gyro and accelerometer joined into one reading, rumble kept
//! playing past SDL's 65-second cap) over any [`PadSystem`]; [`sdl::Sdl`] is
//! the `PadSystem` that calls SDL. Tests drive the first with a scripted
//! system, and the second with SDL's own virtual gamepads.

use crate::input::backend::{stick_dead_zone, trigger_dead_zone};
use crate::input::{
    ControllerFamily, DeviceKey, GamepadAxis, GamepadBackend, GamepadButton, GamepadCapabilities, GamepadEvents,
    GamepadInfo, GamepadMotion, MotorLevels,
};

/// SDL's numbers for gamepad buttons, axes, types and sensors
/// (`SDL_gamepad.h`, `SDL_sensor.h`); a test checks them against `sdl3-sys`.
/// Axes are read by their number in order; their names are for tests.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod raw {
    pub(crate) const BUTTON_SOUTH: i32 = 0;
    pub(crate) const BUTTON_EAST: i32 = 1;
    pub(crate) const BUTTON_WEST: i32 = 2;
    pub(crate) const BUTTON_NORTH: i32 = 3;
    pub(crate) const BUTTON_BACK: i32 = 4;
    pub(crate) const BUTTON_GUIDE: i32 = 5;
    pub(crate) const BUTTON_START: i32 = 6;
    pub(crate) const BUTTON_LEFT_STICK: i32 = 7;
    pub(crate) const BUTTON_RIGHT_STICK: i32 = 8;
    pub(crate) const BUTTON_LEFT_SHOULDER: i32 = 9;
    pub(crate) const BUTTON_RIGHT_SHOULDER: i32 = 10;
    pub(crate) const BUTTON_DPAD_UP: i32 = 11;
    pub(crate) const BUTTON_DPAD_DOWN: i32 = 12;
    pub(crate) const BUTTON_DPAD_LEFT: i32 = 13;
    pub(crate) const BUTTON_DPAD_RIGHT: i32 = 14;

    pub(crate) const AXIS_LEFTX: i32 = 0;
    pub(crate) const AXIS_LEFTY: i32 = 1;
    pub(crate) const AXIS_RIGHTX: i32 = 2;
    pub(crate) const AXIS_RIGHTY: i32 = 3;
    pub(crate) const AXIS_LEFT_TRIGGER: i32 = 4;
    pub(crate) const AXIS_RIGHT_TRIGGER: i32 = 5;

    pub(crate) const TYPE_STANDARD: i32 = 1;
    pub(crate) const TYPE_XBOX360: i32 = 2;
    pub(crate) const TYPE_XBOXONE: i32 = 3;
    pub(crate) const TYPE_PS3: i32 = 4;
    pub(crate) const TYPE_PS4: i32 = 5;
    pub(crate) const TYPE_PS5: i32 = 6;
    pub(crate) const TYPE_SWITCH_PRO: i32 = 7;
    pub(crate) const TYPE_JOYCON_LEFT: i32 = 8;
    pub(crate) const TYPE_JOYCON_RIGHT: i32 = 9;
    pub(crate) const TYPE_JOYCON_PAIR: i32 = 10;
    pub(crate) const TYPE_GAMECUBE: i32 = 11;

    pub(crate) const SENSOR_ACCEL: i32 = 1;
    pub(crate) const SENSOR_GYRO: i32 = 2;
}

/// One event from SDL's queue, as plain numbers: the pad's SDL instance id
/// first, then SDL's button, axis or sensor number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PadEvent {
    Added(u32),
    Removed(u32),
    Button(u32, i32, bool),
    /// Sticks `-32768..=32767` (y down), triggers `0..=32767`.
    Axis(u32, i32, i16),
    /// Gyro in rad/s or accelerometer in m/s², on the pad's axes (x right,
    /// y up, z toward the player): the engine's own convention.
    Sensor(u32, i32, [f32; 3]),
}

/// What SDL says about a pad when it is opened.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Opened {
    pub(crate) name: String,
    /// USB ids, 0 when SDL doesn't know them.
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    /// SDL's gamepad type (`raw::TYPE_*`).
    pub(crate) kind: i32,
    /// The same for the same device when it comes back (its model's GUID
    /// and, where it has one, its serial number).
    pub(crate) identity: String,
    pub(crate) rumble: bool,
    pub(crate) trigger_rumble: bool,
    pub(crate) gyro: bool,
    pub(crate) accel: bool,
}

/// What the backend asks of SDL: [`sdl::Sdl`], or a script in tests.
pub(crate) trait PadSystem: Send + 'static {
    /// The next queued event, pumping SDL's when the queue is empty.
    fn next_event(&mut self) -> Option<PadEvent>;
    /// The pads already there when the backend starts.
    fn present(&mut self, into: &mut Vec<u32>);
    /// Opens a pad for reading and rumble; `None` when it can't be.
    fn open(&mut self, id: u32) -> Option<Opened>;
    fn close(&mut self, id: u32);
    /// Turns the gyro and accelerometer readings on; whether the gyro's is.
    fn enable_motion(&mut self, id: u32, gyro: bool, accel: bool) -> bool;
    /// Plays the body motors (`0..=65535`) for `ms` milliseconds.
    fn rumble(&mut self, id: u32, low: u16, high: u16, ms: u32);
    /// Plays the trigger motors (`0..=65535`) for `ms` milliseconds.
    fn rumble_triggers(&mut self, id: u32, left: u16, right: u16, ms: u32);
    /// Milliseconds since some fixed start.
    fn now_ms(&self) -> u64;
}

/// The longest effect SDL plays (`SDL_MAX_RUMBLE_DURATION_MS`).
pub(crate) const RUMBLE_MS: u32 = 0xFFFF;
/// How often a rumble still playing is sent again, well before SDL ends it.
pub(crate) const RUMBLE_REFRESH_MS: u64 = 30_000;
/// A trigger reads as pressed past this much of its travel, and as released
/// under [`TRIGGER_RELEASE`]: gilrs's thresholds.
pub(crate) const TRIGGER_PRESS: f32 = 0.75;
pub(crate) const TRIGGER_RELEASE: f32 = 0.65;
/// Events read in one poll at most, so a flood can't hold a frame up.
const MAX_EVENTS_PER_POLL: usize = 4096;

/// SDL's button number to the engine's (the guide button, paddles and the
/// touchpad have no engine name).
pub(crate) fn button(b: i32) -> Option<GamepadButton> {
    use GamepadButton as G;
    Some(match b {
        raw::BUTTON_SOUTH => G::South,
        raw::BUTTON_EAST => G::East,
        raw::BUTTON_WEST => G::West,
        raw::BUTTON_NORTH => G::North,
        raw::BUTTON_BACK => G::Select,
        raw::BUTTON_START => G::Start,
        raw::BUTTON_LEFT_STICK => G::LeftStick,
        raw::BUTTON_RIGHT_STICK => G::RightStick,
        raw::BUTTON_LEFT_SHOULDER => G::LeftBumper,
        raw::BUTTON_RIGHT_SHOULDER => G::RightBumper,
        raw::BUTTON_DPAD_UP => G::DPadUp,
        raw::BUTTON_DPAD_DOWN => G::DPadDown,
        raw::BUTTON_DPAD_LEFT => G::DPadLeft,
        raw::BUTTON_DPAD_RIGHT => G::DPadRight,
        _ => return None,
    })
}

/// SDL's axes in order, as the engine names them.
const AXES: [GamepadAxis; 6] = [
    GamepadAxis::LeftStickX,
    GamepadAxis::LeftStickY,
    GamepadAxis::RightStickX,
    GamepadAxis::RightStickY,
    GamepadAxis::LeftTrigger,
    GamepadAxis::RightTrigger,
];

/// The button family SDL's type names; `None` for a pad SDL only knows as
/// a standard one (its family is guessed from its ids and name).
pub(crate) fn family_of_type(kind: i32) -> Option<ControllerFamily> {
    match kind {
        raw::TYPE_XBOX360 | raw::TYPE_XBOXONE => Some(ControllerFamily::Xbox),
        raw::TYPE_PS3 | raw::TYPE_PS4 | raw::TYPE_PS5 => Some(ControllerFamily::PlayStation),
        raw::TYPE_SWITCH_PRO
        | raw::TYPE_JOYCON_LEFT
        | raw::TYPE_JOYCON_RIGHT
        | raw::TYPE_JOYCON_PAIR
        | raw::TYPE_GAMECUBE => Some(ControllerFamily::Nintendo),
        _ => None,
    }
}

/// A `0..=1` motor level as SDL's `0..=65535`.
fn motor(level: f32) -> u16 {
    (level.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16
}

/// A pad the backend has reported connected.
#[derive(Debug)]
struct Pad {
    id: u32,
    key: DeviceKey,
    trigger_rumble: bool,
    /// SDL's axes in the last event, as `-1..=1` (triggers `0..=1`).
    axes: [f32; 6],
    /// The axes last reported, dead zone taken out.
    sent: [f32; 6],
    /// Whether each trigger reads as a pressed button.
    trigger_down: [bool; 2],
    motion: GamepadMotion,
    motion_new: bool,
    /// The motor levels the game asked for, and when they were last sent.
    levels: MotorLevels,
    sent_at: u64,
}

/// The [`GamepadBackend`] over SDL's gamepads (or a scripted [`PadSystem`]).
pub(crate) struct SdlGamepads<S: PadSystem> {
    system: S,
    started: bool,
    pads: Vec<Pad>,
    /// Every device seen: its identity, its key, and whether it is here now.
    /// A device that comes back gets its key (and so its slot) again.
    known: Vec<(String, DeviceKey, bool)>,
    next_key: u64,
    /// Kept so polling doesn't allocate.
    scratch: Vec<u32>,
}

impl<S: PadSystem> SdlGamepads<S> {
    pub(crate) fn new(system: S) -> Self {
        Self {
            system,
            started: false,
            pads: Vec::new(),
            known: Vec::new(),
            next_key: 0,
            scratch: Vec::new(),
        }
    }

    /// The scripted system, for tests.
    #[cfg(test)]
    pub(crate) fn system(&mut self) -> &mut S {
        &mut self.system
    }

    fn key_for(&mut self, identity: String) -> DeviceKey {
        if let Some(entry) = self.known.iter_mut().find(|(i, _, here)| !*here && *i == identity) {
            entry.2 = true;
            return entry.1;
        }
        let key = DeviceKey(self.next_key);
        self.next_key += 1;
        self.known.push((identity, key, true));
        key
    }

    fn added(&mut self, id: u32, events: &mut GamepadEvents<'_>) {
        if self.pads.iter().any(|p| p.id == id) {
            return;
        }
        let Some(opened) = self.system.open(id) else { return };
        let motion = (opened.gyro || opened.accel) && self.system.enable_motion(id, opened.gyro, opened.accel);
        let vendor = (opened.vendor_id != 0).then_some(opened.vendor_id);
        let product = (opened.product_id != 0).then_some(opened.product_id);
        let info = GamepadInfo {
            family: family_of_type(opened.kind)
                .unwrap_or_else(|| ControllerFamily::detect(vendor, product, &opened.name)),
            name: opened.name,
            vendor_id: vendor,
            product_id: product,
            capabilities: GamepadCapabilities {
                rumble: opened.rumble,
                trigger_rumble: opened.trigger_rumble,
                motion,
            },
        };
        let key = self.key_for(opened.identity);
        if events.connected(key, info).is_none() {
            // Every slot is taken: leave it closed, to be found when one frees.
            self.system.close(id);
            if let Some(entry) = self.known.iter_mut().find(|(_, k, _)| *k == key) {
                entry.2 = false;
            }
            return;
        }
        self.pads.push(Pad {
            id,
            key,
            trigger_rumble: opened.trigger_rumble,
            axes: [0.0; 6],
            sent: [0.0; 6],
            trigger_down: [false; 2],
            motion: GamepadMotion::default(),
            motion_new: false,
            levels: MotorLevels::OFF,
            sent_at: 0,
        });
    }

    fn removed(&mut self, id: u32, events: &mut GamepadEvents<'_>) {
        let Some(at) = self.pads.iter().position(|p| p.id == id) else {
            return;
        };
        let pad = self.pads.swap_remove(at);
        self.system.close(id);
        if let Some(entry) = self.known.iter_mut().find(|(_, k, _)| *k == pad.key) {
            entry.2 = false;
        }
        events.disconnected(pad.key);
    }

    fn axis(pad: &mut Pad, axis: i32, value: i16, events: &mut GamepadEvents<'_>) {
        let Ok(i) = usize::try_from(axis) else { return };
        if i >= AXES.len() {
            return;
        }
        let device = pad.key;
        if i < 4 {
            pad.axes[i] = (f32::from(value) / 32767.0).clamp(-1.0, 1.0);
            // A stick's two axes share its dead zone.
            let x = i & !1;
            let (dx, dy) = stick_dead_zone(pad.axes[x], pad.axes[x + 1]);
            for (j, v) in [(x, dx), (x + 1, dy)] {
                if v != pad.sent[j] {
                    pad.sent[j] = v;
                    events.axis(device, AXES[j], v);
                }
            }
            return;
        }
        let raw = (f32::from(value) / 32767.0).clamp(0.0, 1.0);
        pad.axes[i] = raw;
        let v = trigger_dead_zone(raw);
        if v != pad.sent[i] {
            pad.sent[i] = v;
            events.axis(device, AXES[i], v);
        }
        let t = i - 4;
        let button = [GamepadButton::LeftTrigger, GamepadButton::RightTrigger][t];
        if !pad.trigger_down[t] && raw >= TRIGGER_PRESS {
            pad.trigger_down[t] = true;
            events.button(device, button, true);
        } else if pad.trigger_down[t] && raw <= TRIGGER_RELEASE {
            pad.trigger_down[t] = false;
            events.button(device, button, false);
        }
    }

    /// Sends `pad`'s levels to SDL: the body motors always (zeros stop
    /// them), the trigger motors where the pad has them.
    fn play(system: &mut S, pad: &mut Pad) {
        let l = pad.levels;
        system.rumble(pad.id, motor(l.strong), motor(l.weak), RUMBLE_MS);
        if pad.trigger_rumble {
            system.rumble_triggers(pad.id, motor(l.left_trigger), motor(l.right_trigger), RUMBLE_MS);
        }
        pad.sent_at = system.now_ms();
    }
}

impl<S: PadSystem> GamepadBackend for SdlGamepads<S> {
    fn poll(&mut self, events: &mut GamepadEvents<'_>) {
        if !self.started {
            self.started = true;
            let mut present = std::mem::take(&mut self.scratch);
            self.system.present(&mut present);
            for &id in &present {
                self.added(id, events);
            }
            present.clear();
            self.scratch = present;
        }
        for _ in 0..MAX_EVENTS_PER_POLL {
            let Some(event) = self.system.next_event() else { break };
            match event {
                PadEvent::Added(id) => self.added(id, events),
                PadEvent::Removed(id) => self.removed(id, events),
                PadEvent::Button(id, b, down) => {
                    if let (Some(pad), Some(b)) = (self.pads.iter().find(|p| p.id == id), button(b)) {
                        events.button(pad.key, b, down);
                    }
                }
                PadEvent::Axis(id, axis, value) => {
                    if let Some(pad) = self.pads.iter_mut().find(|p| p.id == id) {
                        Self::axis(pad, axis, value, events);
                    }
                }
                PadEvent::Sensor(id, sensor, data) => {
                    if let Some(pad) = self.pads.iter_mut().find(|p| p.id == id) {
                        let v = glam::Vec3::from(data);
                        match sensor {
                            raw::SENSOR_GYRO => pad.motion.gyro = v,
                            raw::SENSOR_ACCEL => pad.motion.accel = v,
                            _ => continue,
                        }
                        pad.motion_new = true;
                    }
                }
            }
        }
        // One reading a frame, the gyro's and accelerometer's latest together.
        for pad in &mut self.pads {
            if pad.motion_new {
                pad.motion_new = false;
                events.motion(pad.key, pad.motion);
            }
        }
        // SDL ends an effect after 65 seconds: one still playing is sent again.
        let now = self.system.now_ms();
        for pad in &mut self.pads {
            if !pad.levels.is_off() && now.saturating_sub(pad.sent_at) >= RUMBLE_REFRESH_MS {
                Self::play(&mut self.system, pad);
            }
        }
    }

    fn set_rumble(&mut self, device: DeviceKey, levels: MotorLevels) {
        let Some(pad) = self.pads.iter_mut().find(|p| p.key == device) else {
            return;
        };
        pad.levels = levels;
        Self::play(&mut self.system, pad);
    }
}

impl<S: PadSystem> Drop for SdlGamepads<S> {
    /// Closing a pad stops its rumble.
    fn drop(&mut self) {
        for pad in self.pads.drain(..) {
            self.system.close(pad.id);
        }
    }
}

/// The [`PadSystem`] that calls SDL 3.
#[cfg(all(feature = "gamepad-sdl", not(target_arch = "wasm32")))]
pub(crate) mod sdl {
    use std::ffi::CStr;
    use std::ptr::NonNull;

    use sdl3_sys::events::{SDL_Event, SDL_EventType, SDL_PollEvent};
    use sdl3_sys::gamepad::{
        SDL_CloseGamepad, SDL_Gamepad, SDL_GamepadHasSensor, SDL_GetGamepadGUIDForID, SDL_GetGamepadName,
        SDL_GetGamepadProduct, SDL_GetGamepadProperties, SDL_GetGamepadSerial, SDL_GetGamepadType,
        SDL_GetGamepadVendor, SDL_GetGamepads, SDL_OpenGamepad, SDL_PROP_GAMEPAD_CAP_RUMBLE_BOOLEAN,
        SDL_PROP_GAMEPAD_CAP_TRIGGER_RUMBLE_BOOLEAN, SDL_RumbleGamepad, SDL_RumbleGamepadTriggers,
        SDL_SetGamepadSensorEnabled,
    };
    use sdl3_sys::init::{SDL_INIT_GAMEPAD, SDL_InitSubSystem, SDL_QuitSubSystem};
    use sdl3_sys::properties::SDL_GetBooleanProperty;
    use sdl3_sys::sensor::{SDL_SENSOR_ACCEL, SDL_SENSOR_GYRO};

    use super::{Opened, PadEvent, PadSystem};

    /// SDL's gamepad subsystem, and the pads opened through it.
    pub(crate) struct Sdl {
        open: Vec<(u32, NonNull<SDL_Gamepad>)>,
    }

    // SAFETY: SDL's gamepad functions lock SDL's joystick lock themselves, so
    // they may be called from any thread; `Sdl` is used by one thread at a
    // time (the engine's backend sits behind a mutex), and the pad pointers
    // it holds are owned by it alone.
    unsafe impl Send for Sdl {}

    /// SDL's last error message.
    fn error() -> String {
        // SAFETY: SDL_GetError returns a valid, NUL-terminated string (empty
        // when there is no error) that stays valid until the next SDL call.
        unsafe { CStr::from_ptr(sdl3_sys::error::SDL_GetError()) }
            .to_string_lossy()
            .into_owned()
    }

    /// A string SDL returned, or "" for none.
    fn text(s: *const std::ffi::c_char) -> String {
        if s.is_null() {
            return String::new();
        }
        // SAFETY: not null, and SDL's strings are NUL-terminated and live
        // while the pad they describe is open.
        unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()
    }

    impl Sdl {
        /// Starts SDL's gamepad subsystem (and its event queue). SDL's
        /// handlers for Ctrl-C are left off: the game's own stay in charge.
        pub(crate) fn start() -> Result<Self, String> {
            // SAFETY: the hint name and value are NUL-terminated constants;
            // initialising a subsystem is allowed from any thread.
            let ok = unsafe {
                sdl3_sys::hints::SDL_SetHint(sdl3_sys::hints::SDL_HINT_NO_SIGNAL_HANDLERS, c"1".as_ptr());
                SDL_InitSubSystem(SDL_INIT_GAMEPAD)
            };
            if ok {
                Ok(Self { open: Vec::new() })
            } else {
                Err(error())
            }
        }

        fn pad(&self, id: u32) -> Option<*mut SDL_Gamepad> {
            self.open.iter().find(|(i, _)| *i == id).map(|(_, p)| p.as_ptr())
        }
    }

    impl Drop for Sdl {
        fn drop(&mut self) {
            for (_, pad) in self.open.drain(..) {
                // SAFETY: opened by `open` and not closed since.
                unsafe { SDL_CloseGamepad(pad.as_ptr()) };
            }
            // SAFETY: balances the `SDL_InitSubSystem` of `start`.
            unsafe { SDL_QuitSubSystem(SDL_INIT_GAMEPAD) };
        }
    }

    impl PadSystem for Sdl {
        fn next_event(&mut self) -> Option<PadEvent> {
            loop {
                let mut event = std::mem::MaybeUninit::<SDL_Event>::uninit();
                // SAFETY: SDL writes a whole event when it returns true.
                if !unsafe { SDL_PollEvent(event.as_mut_ptr()) } {
                    return None;
                }
                // SAFETY: written by SDL_PollEvent above.
                let event = unsafe { event.assume_init() };
                // SAFETY: every event starts with its type, and the variant
                // read below is the one that type names.
                let kind = SDL_EventType(unsafe { event.r#type });
                let read = unsafe {
                    match kind {
                        SDL_EventType::GAMEPAD_ADDED => PadEvent::Added(event.gdevice.which.0),
                        SDL_EventType::GAMEPAD_REMOVED => PadEvent::Removed(event.gdevice.which.0),
                        SDL_EventType::GAMEPAD_BUTTON_DOWN | SDL_EventType::GAMEPAD_BUTTON_UP => PadEvent::Button(
                            event.gbutton.which.0,
                            i32::from(event.gbutton.button),
                            event.gbutton.down,
                        ),
                        SDL_EventType::GAMEPAD_AXIS_MOTION => {
                            PadEvent::Axis(event.gaxis.which.0, i32::from(event.gaxis.axis), event.gaxis.value)
                        }
                        SDL_EventType::GAMEPAD_SENSOR_UPDATE => {
                            PadEvent::Sensor(event.gsensor.which.0, event.gsensor.sensor, event.gsensor.data)
                        }
                        // Joystick events (the same pads, raw) and the rest.
                        _ => continue,
                    }
                };
                return Some(read);
            }
        }

        fn present(&mut self, into: &mut Vec<u32>) {
            let mut count = 0;
            // SAFETY: SDL returns an array of `count` ids (or null), which
            // the caller frees with SDL_free.
            unsafe {
                let ids = SDL_GetGamepads(&mut count);
                if ids.is_null() {
                    return;
                }
                let n = usize::try_from(count).unwrap_or(0);
                into.extend(std::slice::from_raw_parts(ids, n).iter().map(|id| id.0));
                sdl3_sys::stdinc::SDL_free(ids.cast());
            }
        }

        fn open(&mut self, id: u32) -> Option<Opened> {
            let instance = sdl3_sys::joystick::SDL_JoystickID(id);
            // SAFETY: opening by an id SDL reported; a null result is handled.
            let pad = NonNull::new(unsafe { SDL_OpenGamepad(instance) })?;
            let p = pad.as_ptr();
            // SAFETY: `p` is an open gamepad; the property names are SDL's
            // NUL-terminated constants.
            let opened = unsafe {
                let guid = SDL_GetGamepadGUIDForID(instance);
                let mut identity: String = guid.data.iter().map(|b| format!("{b:02x}")).collect();
                let serial = text(SDL_GetGamepadSerial(p));
                if !serial.is_empty() {
                    identity.push('/');
                    identity.push_str(&serial);
                }
                let props = SDL_GetGamepadProperties(p);
                Opened {
                    name: text(SDL_GetGamepadName(p)),
                    vendor_id: SDL_GetGamepadVendor(p),
                    product_id: SDL_GetGamepadProduct(p),
                    kind: SDL_GetGamepadType(p).0,
                    identity,
                    rumble: SDL_GetBooleanProperty(props, SDL_PROP_GAMEPAD_CAP_RUMBLE_BOOLEAN, false),
                    trigger_rumble: SDL_GetBooleanProperty(props, SDL_PROP_GAMEPAD_CAP_TRIGGER_RUMBLE_BOOLEAN, false),
                    gyro: SDL_GamepadHasSensor(p, SDL_SENSOR_GYRO),
                    accel: SDL_GamepadHasSensor(p, SDL_SENSOR_ACCEL),
                }
            };
            self.open.push((id, pad));
            Some(opened)
        }

        fn close(&mut self, id: u32) {
            if let Some(at) = self.open.iter().position(|(i, _)| *i == id) {
                let (_, pad) = self.open.swap_remove(at);
                // SAFETY: opened by `open` and not closed since (removed above).
                unsafe { SDL_CloseGamepad(pad.as_ptr()) };
            }
        }

        fn enable_motion(&mut self, id: u32, gyro: bool, accel: bool) -> bool {
            let Some(p) = self.pad(id) else { return false };
            // SAFETY: `p` is an open gamepad.
            unsafe {
                if accel {
                    SDL_SetGamepadSensorEnabled(p, SDL_SENSOR_ACCEL, true);
                }
                gyro && SDL_SetGamepadSensorEnabled(p, SDL_SENSOR_GYRO, true)
            }
        }

        fn rumble(&mut self, id: u32, low: u16, high: u16, ms: u32) {
            if let Some(p) = self.pad(id) {
                // SAFETY: `p` is an open gamepad. A pad without motors says no.
                unsafe { SDL_RumbleGamepad(p, low, high, ms) };
            }
        }

        fn rumble_triggers(&mut self, id: u32, left: u16, right: u16, ms: u32) {
            if let Some(p) = self.pad(id) {
                // SAFETY: `p` is an open gamepad. A pad without trigger motors says no.
                unsafe { SDL_RumbleGamepadTriggers(p, left, right, ms) };
            }
        }

        fn now_ms(&self) -> u64 {
            // SAFETY: a plain clock read, valid once SDL is initialised.
            unsafe { sdl3_sys::timer::SDL_GetTicks() }
        }
    }

    #[cfg(test)]
    mod tests {
        //! The calls into SDL, against SDL's own virtual gamepads: no device,
        //! no window. SDL is one process-wide library, so it is one test.
        use std::sync::atomic::{AtomicU32, Ordering};

        use sdl3_sys::gamepad::*;
        use sdl3_sys::hints::{SDL_HINT_JOYSTICK_HIDAPI, SDL_SetHint};
        use sdl3_sys::joystick::*;
        use sdl3_sys::sensor::*;

        use super::super::{SdlGamepads, raw};
        use super::Sdl;
        use crate::ctx::Ctx;
        use crate::input::{ControllerFamily, GamepadAxis, GamepadButton, Rumble};

        static LOW: AtomicU32 = AtomicU32::new(0);
        static RIGHT_TRIGGER: AtomicU32 = AtomicU32::new(0);

        unsafe extern "C" fn rumble(_: *mut std::ffi::c_void, low: u16, _high: u16) -> bool {
            LOW.store(u32::from(low), Ordering::SeqCst);
            true
        }

        unsafe extern "C" fn rumble_triggers(_: *mut std::ffi::c_void, _left: u16, right: u16) -> bool {
            RIGHT_TRIGGER.store(u32::from(right), Ordering::SeqCst);
            true
        }

        #[test]
        fn sdls_numbers_are_the_ones_the_backend_uses() {
            assert_eq!(SDL_GAMEPAD_BUTTON_SOUTH.0, raw::BUTTON_SOUTH);
            assert_eq!(SDL_GAMEPAD_BUTTON_WEST.0, raw::BUTTON_WEST);
            assert_eq!(SDL_GAMEPAD_BUTTON_BACK.0, raw::BUTTON_BACK);
            assert_eq!(SDL_GAMEPAD_BUTTON_GUIDE.0, raw::BUTTON_GUIDE);
            assert_eq!(SDL_GAMEPAD_BUTTON_START.0, raw::BUTTON_START);
            assert_eq!(SDL_GAMEPAD_BUTTON_LEFT_SHOULDER.0, raw::BUTTON_LEFT_SHOULDER);
            assert_eq!(SDL_GAMEPAD_BUTTON_DPAD_RIGHT.0, raw::BUTTON_DPAD_RIGHT);
            assert_eq!(SDL_GAMEPAD_AXIS_RIGHTY.0, raw::AXIS_RIGHTY);
            assert_eq!(SDL_GAMEPAD_AXIS_RIGHT_TRIGGER.0, raw::AXIS_RIGHT_TRIGGER);
            assert_eq!(SDL_GAMEPAD_TYPE_STANDARD.0, raw::TYPE_STANDARD);
            assert_eq!(SDL_GAMEPAD_TYPE_XBOXONE.0, raw::TYPE_XBOXONE);
            assert_eq!(SDL_GAMEPAD_TYPE_PS5.0, raw::TYPE_PS5);
            assert_eq!(SDL_GAMEPAD_TYPE_NINTENDO_SWITCH_PRO.0, raw::TYPE_SWITCH_PRO);
            assert_eq!(SDL_GAMEPAD_TYPE_GAMECUBE.0, raw::TYPE_GAMECUBE);
            assert_eq!(SDL_SENSOR_ACCEL.0, raw::SENSOR_ACCEL);
            assert_eq!(SDL_SENSOR_GYRO.0, raw::SENSOR_GYRO);
        }

        /// A virtual Xbox-style pad with a gyro, an accelerometer, body and
        /// trigger motors goes through SDL into the engine: connection with
        /// its name, family and capabilities, a button, a trigger, motion,
        /// rumble on both kinds of motor, and its removal.
        #[test]
        fn a_virtual_sdl_pad_reaches_the_engine_with_motion_and_both_kinds_of_rumble() {
            // SAFETY (whole test): SDL calls with valid arguments; the
            // descriptor and the strings it points to outlive the attach call.
            unsafe {
                // Only virtual pads: SDL opens none of the machine's devices.
                SDL_SetHint(SDL_HINT_JOYSTICK_HIDAPI, c"0".as_ptr());
                SDL_SetHint(c"SDL_JOYSTICK_MFI".as_ptr(), c"0".as_ptr());
                SDL_SetHint(c"SDL_JOYSTICK_IOKIT".as_ptr(), c"0".as_ptr());
                SDL_SetHint(c"SDL_JOYSTICK_RAWINPUT".as_ptr(), c"0".as_ptr());
                SDL_SetHint(c"SDL_JOYSTICK_WGI".as_ptr(), c"0".as_ptr());
                SDL_SetHint(c"SDL_JOYSTICK_DIRECTINPUT".as_ptr(), c"0".as_ptr());
                SDL_SetHint(c"SDL_JOYSTICK_XINPUT".as_ptr(), c"0".as_ptr());
                SDL_SetHint(c"SDL_JOYSTICK_LINUX_CLASSIC".as_ptr(), c"0".as_ptr());
                let system = Sdl::start().expect("SDL's gamepad subsystem starts");
                let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
                ctx.set_gamepad_backend(SdlGamepads::new(system));
                assert!(ctx.input.gamepads().is_empty(), "no virtual pad yet");

                let sensors = [
                    SDL_VirtualJoystickSensorDesc {
                        r#type: SDL_SENSOR_GYRO,
                        rate: 0.0,
                    },
                    SDL_VirtualJoystickSensorDesc {
                        r#type: SDL_SENSOR_ACCEL,
                        rate: 0.0,
                    },
                ];
                let mut desc = SDL_VirtualJoystickDesc::default(); // SDL_INIT_INTERFACE done
                desc.r#type = SDL_JOYSTICK_TYPE_GAMEPAD.0 as u16;
                desc.vendor_id = 0x045e;
                desc.product_id = 0x0b13;
                desc.naxes = SDL_GAMEPAD_AXIS_COUNT.0 as u16;
                desc.nbuttons = SDL_GAMEPAD_BUTTON_COUNT.0 as u16;
                desc.nsensors = 2;
                desc.sensors = sensors.as_ptr();
                desc.name = c"Virtual Test Pad".as_ptr();
                desc.Rumble = Some(rumble);
                desc.RumbleTriggers = Some(rumble_triggers);
                let id = SDL_AttachVirtualJoystick(&desc);
                assert_ne!(id.0, 0, "attached");
                let joystick = SDL_OpenJoystick(id);
                assert!(!joystick.is_null());

                ctx.poll_gamepads(1.0 / 60.0);
                let pads = ctx.input.gamepads().to_vec();
                assert_eq!(pads.len(), 1, "connected");
                let pad = pads[0];
                let info = ctx.input.gamepad_info(pad).unwrap().clone();
                assert_eq!(info.name, "Virtual Test Pad");
                assert_eq!(info.vendor_id, Some(0x045e));
                assert_eq!(info.family, ControllerFamily::Xbox);
                assert!(info.capabilities.rumble && info.capabilities.trigger_rumble && info.capabilities.motion);

                SDL_SetJoystickVirtualButton(joystick, SDL_GAMEPAD_BUTTON_SOUTH.0, true);
                SDL_SetJoystickVirtualAxis(joystick, SDL_GAMEPAD_AXIS_RIGHT_TRIGGER.0, i16::MAX);
                let gyro = [0.5f32, -1.0, 2.0];
                let accel = [0.0f32, 9.81, 0.0];
                SDL_SendJoystickVirtualSensorData(joystick, SDL_SENSOR_GYRO, 1, gyro.as_ptr(), 3);
                SDL_SendJoystickVirtualSensorData(joystick, SDL_SENSOR_ACCEL, 1, accel.as_ptr(), 3);
                ctx.input.begin_frame(0.0);
                ctx.poll_gamepads(1.0 / 60.0);
                assert!(ctx.input.gamepad_down(pad, GamepadButton::South));
                assert!(ctx.input.gamepad_down(pad, GamepadButton::RightTrigger));
                assert_eq!(ctx.input.gamepad_axis(pad, GamepadAxis::RightTrigger), 1.0);
                let motion = ctx.input.gamepad_motion(pad).expect("motion arrived");
                assert_eq!(motion.gyro, glam::Vec3::from(gyro));
                assert_eq!(motion.accel, glam::Vec3::from(accel));

                ctx.input
                    .rumble(pad, Rumble::new(1.0, 0.0).triggers(0.0, 0.5).seconds(10.0));
                ctx.poll_gamepads(1.0 / 60.0);
                // SDL sends rumble to a virtual pad when it updates it.
                SDL_UpdateJoysticks();
                assert_eq!(LOW.load(Ordering::SeqCst), 65535, "the body motor");
                assert_eq!(RIGHT_TRIGGER.load(Ordering::SeqCst), 32768, "the trigger motor");

                SDL_CloseJoystick(joystick);
                SDL_DetachVirtualJoystick(id);
                ctx.poll_gamepads(1.0 / 60.0);
                assert!(ctx.input.gamepads().is_empty(), "removed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::ctx::Ctx;
    use crate::input::{Gamepad, Rumble};

    /// The device layer's side, scripted: queued events, pads that can be
    /// opened, and what the backend sent.
    #[derive(Debug, Default)]
    struct Script {
        queue: Vec<PadEvent>,
        present: Vec<u32>,
        pads: Vec<(u32, Opened)>,
        open: Vec<u32>,
        motion_on: Vec<u32>,
        rumble: Vec<(u32, u16, u16)>,
        triggers: Vec<(u32, u16, u16)>,
        now: u64,
    }

    #[derive(Debug, Clone, Default)]
    struct Fake(Arc<Mutex<Script>>);

    impl Fake {
        fn with<R>(&self, f: impl FnOnce(&mut Script) -> R) -> R {
            f(&mut self.0.lock().unwrap())
        }
        fn send(&self, event: PadEvent) {
            self.with(|s| s.queue.push(event));
        }
    }

    impl PadSystem for Fake {
        fn next_event(&mut self) -> Option<PadEvent> {
            self.with(|s| (!s.queue.is_empty()).then(|| s.queue.remove(0)))
        }
        fn present(&mut self, into: &mut Vec<u32>) {
            self.with(|s| into.extend(&s.present));
        }
        fn open(&mut self, id: u32) -> Option<Opened> {
            self.with(|s| {
                let opened = s.pads.iter().find(|(i, _)| *i == id)?.1.clone();
                s.open.push(id);
                Some(opened)
            })
        }
        fn close(&mut self, id: u32) {
            self.with(|s| s.open.retain(|&i| i != id));
        }
        fn enable_motion(&mut self, id: u32, gyro: bool, _accel: bool) -> bool {
            self.with(|s| s.motion_on.push(id));
            gyro
        }
        fn rumble(&mut self, id: u32, low: u16, high: u16, _ms: u32) {
            self.with(|s| s.rumble.push((id, low, high)));
        }
        fn rumble_triggers(&mut self, id: u32, left: u16, right: u16, _ms: u32) {
            self.with(|s| s.triggers.push((id, left, right)));
        }
        fn now_ms(&self) -> u64 {
            self.0.lock().unwrap().now
        }
    }

    fn dualsense() -> Opened {
        Opened {
            name: "DualSense Wireless Controller".into(),
            vendor_id: 0x054c,
            product_id: 0x0ce6,
            kind: raw::TYPE_PS5,
            identity: "ps5-guid/serial-1".into(),
            rumble: true,
            trigger_rumble: false,
            gyro: true,
            accel: true,
        }
    }

    fn xbox() -> Opened {
        Opened {
            name: "Xbox Wireless Controller".into(),
            vendor_id: 0x045e,
            product_id: 0x0b13,
            kind: raw::TYPE_XBOXONE,
            identity: "xbox-guid".into(),
            rumble: true,
            trigger_rumble: true,
            gyro: false,
            accel: false,
        }
    }

    fn setup(pads: &[(u32, Opened)]) -> (Ctx, Fake) {
        let fake = Fake::default();
        fake.with(|s| s.pads = pads.to_vec());
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        ctx.set_gamepad_backend(SdlGamepads::new(fake.clone()));
        (ctx, fake)
    }

    fn frame(ctx: &mut Ctx) {
        ctx.input.begin_frame(0.0);
        ctx.poll_gamepads(1.0 / 60.0);
    }

    #[test]
    fn pads_present_at_start_connect_on_the_first_poll_with_sdls_description() {
        let fake = Fake::default();
        fake.with(|s| {
            s.pads = vec![(7, dualsense())];
            s.present = vec![7];
        });
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        ctx.set_gamepad_backend(SdlGamepads::new(fake.clone()));
        let pad = ctx.input.gamepads()[0];
        let info = ctx.input.gamepad_info(pad).unwrap();
        assert_eq!(info.name, "DualSense Wireless Controller");
        assert_eq!(info.family, ControllerFamily::PlayStation);
        assert_eq!((info.vendor_id, info.product_id), (Some(0x054c), Some(0x0ce6)));
        assert_eq!(
            info.capabilities,
            GamepadCapabilities {
                rumble: true,
                trigger_rumble: false,
                motion: true
            }
        );
        assert_eq!(fake.with(|s| s.motion_on.clone()), [7], "its sensors were turned on");
        // SDL's own added event for the same pad changes nothing.
        fake.send(PadEvent::Added(7));
        frame(&mut ctx);
        assert_eq!(ctx.input.gamepads().len(), 1);
    }

    #[test]
    fn sdls_type_names_the_family_before_ids_and_name_are_guessed() {
        let mut odd = xbox();
        odd.name = "Wireless Gamepad".into();
        odd.vendor_id = 0x2dc8; // a third-party maker
        odd.kind = raw::TYPE_SWITCH_PRO;
        let mut plain = odd.clone();
        plain.kind = raw::TYPE_STANDARD;
        plain.name = "Some Xbox Pad".into();
        plain.identity = "other".into();
        let (mut ctx, fake) = setup(&[(1, odd), (2, plain)]);
        fake.send(PadEvent::Added(1));
        fake.send(PadEvent::Added(2));
        frame(&mut ctx);
        let family = |s: u8| ctx.input.gamepad_info(Gamepad(s)).unwrap().family;
        assert_eq!(family(0), ControllerFamily::Nintendo, "from SDL's type");
        assert_eq!(family(1), ControllerFamily::Xbox, "guessed from the name");
    }

    #[test]
    fn a_pad_without_sensors_says_it_has_no_motion_and_none_are_turned_on() {
        let (mut ctx, fake) = setup(&[(1, xbox())]);
        fake.send(PadEvent::Added(1));
        frame(&mut ctx);
        let pad = ctx.input.gamepads()[0];
        assert!(!ctx.input.gamepad_info(pad).unwrap().capabilities.motion);
        assert!(fake.with(|s| s.motion_on.is_empty()));
    }

    #[test]
    fn buttons_map_by_position_and_the_guide_button_is_ignored() {
        let (mut ctx, fake) = setup(&[(1, xbox())]);
        fake.send(PadEvent::Added(1));
        frame(&mut ctx);
        let pad = ctx.input.gamepads()[0];
        fake.send(PadEvent::Button(1, raw::BUTTON_WEST, true));
        fake.send(PadEvent::Button(1, raw::BUTTON_BACK, true));
        fake.send(PadEvent::Button(1, raw::BUTTON_GUIDE, true));
        fake.send(PadEvent::Button(1, 42, true));
        frame(&mut ctx);
        assert!(ctx.input.gamepad_pressed(pad, GamepadButton::West));
        assert!(ctx.input.gamepad_pressed(pad, GamepadButton::Select));
        let held = GamepadButton::ALL
            .iter()
            .filter(|&&b| ctx.input.gamepad_down(pad, b))
            .count();
        assert_eq!(held, 2);
        let mapped: std::collections::HashSet<_> = (0..32).filter_map(button).collect();
        assert_eq!(
            mapped.len() + 2,
            GamepadButton::COUNT,
            "every button but the triggers, which are axes in SDL"
        );
    }

    #[test]
    fn sticks_point_y_down_and_get_gilrss_dead_zone() {
        let (mut ctx, fake) = setup(&[(1, xbox())]);
        fake.send(PadEvent::Added(1));
        fake.send(PadEvent::Axis(1, raw::AXIS_LEFTY, i16::MIN));
        fake.send(PadEvent::Axis(1, raw::AXIS_RIGHTX, 2000)); // drift: 6 % of the travel
        frame(&mut ctx);
        let pad = ctx.input.gamepads()[0];
        assert_eq!(ctx.input.gamepad_axis(pad, GamepadAxis::LeftStickY), -1.0, "pushed up");
        assert_eq!(
            ctx.input.gamepad_axis(pad, GamepadAxis::RightStickX),
            0.0,
            "inside the dead zone"
        );
        fake.send(PadEvent::Axis(1, raw::AXIS_RIGHTX, 32767 / 2));
        frame(&mut ctx);
        let x = ctx.input.gamepad_axis(pad, GamepadAxis::RightStickX);
        assert!((x - (0.5 - 0.1) / 0.9).abs() < 1e-3, "rescaled past the dead zone: {x}");
    }

    #[test]
    fn a_trigger_is_an_axis_and_a_button_pressed_at_three_quarters_released_under_two_thirds() {
        let (mut ctx, fake) = setup(&[(1, xbox())]);
        fake.send(PadEvent::Added(1));
        frame(&mut ctx);
        let pad = ctx.input.gamepads()[0];
        let pull = |ctx: &mut Ctx, share: f32| {
            fake.send(PadEvent::Axis(1, raw::AXIS_LEFT_TRIGGER, (share * 32767.0) as i16));
            frame(ctx);
            ctx.input.gamepad_down(pad, GamepadButton::LeftTrigger)
        };
        assert!(!pull(&mut ctx, 0.7));
        assert!((ctx.input.gamepad_axis(pad, GamepadAxis::LeftTrigger) - (0.7 - 0.1) / 0.9).abs() < 1e-3);
        assert!(pull(&mut ctx, 0.8), "past 0.75");
        assert!(pull(&mut ctx, 0.7), "held above 0.65");
        assert!(!pull(&mut ctx, 0.6), "released under 0.65");
        fake.send(PadEvent::Axis(1, raw::AXIS_LEFT_TRIGGER, -5)); // never below rest
        frame(&mut ctx);
        assert_eq!(ctx.input.gamepad_axis(pad, GamepadAxis::LeftTrigger), 0.0);
    }

    #[test]
    fn gyro_and_accelerometer_readings_join_into_one_motion_reading_a_frame() {
        let (mut ctx, fake) = setup(&[(1, dualsense())]);
        fake.send(PadEvent::Added(1));
        frame(&mut ctx);
        let pad = ctx.input.gamepads()[0];
        assert_eq!(ctx.input.gamepad_motion(pad), None, "nothing read yet");
        fake.send(PadEvent::Sensor(1, raw::SENSOR_GYRO, [0.1, 0.2, 0.3]));
        fake.send(PadEvent::Sensor(1, raw::SENSOR_GYRO, [1.0, 2.0, 3.0]));
        fake.send(PadEvent::Sensor(1, raw::SENSOR_ACCEL, [0.0, 9.81, 0.0]));
        fake.send(PadEvent::Sensor(1, 99, [5.0, 5.0, 5.0])); // another sensor
        frame(&mut ctx);
        let motion = ctx.input.gamepad_motion(pad).unwrap();
        assert_eq!(motion.gyro, glam::vec3(1.0, 2.0, 3.0), "the latest gyro reading");
        assert_eq!(motion.accel, glam::vec3(0.0, 9.81, 0.0));
        fake.send(PadEvent::Sensor(1, raw::SENSOR_ACCEL, [0.0, 0.0, 9.81]));
        frame(&mut ctx);
        let motion = ctx.input.gamepad_motion(pad).unwrap();
        assert_eq!(motion.gyro, glam::vec3(1.0, 2.0, 3.0), "the gyro's last reading stays");
        assert_eq!(motion.accel, glam::vec3(0.0, 0.0, 9.81));
    }

    #[test]
    fn rumble_plays_the_body_motors_and_the_trigger_motors_only_where_the_pad_has_them() {
        let (mut ctx, fake) = setup(&[(1, xbox()), (2, dualsense())]);
        fake.send(PadEvent::Added(1));
        fake.send(PadEvent::Added(2));
        frame(&mut ctx);
        let [xbox_pad, ps_pad] = [Gamepad(0), Gamepad(1)];
        let kick = Rumble::new(1.0, 0.5).triggers(0.25, 1.0).seconds(1.0);
        ctx.input.rumble(xbox_pad, kick);
        ctx.input.rumble(ps_pad, kick);
        frame(&mut ctx);
        let (rumble, triggers) = fake.with(|s| (s.rumble.clone(), s.triggers.clone()));
        assert!(rumble.contains(&(1, 65535, 32768)) && rumble.contains(&(2, 65535, 32768)));
        assert_eq!(triggers, [(1, 16384, 65535)], "only the pad with trigger motors");
        // Ends after its second: zeros stop both kinds of motor.
        for _ in 0..70 {
            frame(&mut ctx);
        }
        let (rumble, triggers) = fake.with(|s| (s.rumble.clone(), s.triggers.clone()));
        assert_eq!(rumble.iter().filter(|r| r.0 == 1).last(), Some(&(1, 0, 0)));
        assert_eq!(triggers.last(), Some(&(1, 0, 0)));
    }

    #[test]
    fn a_long_rumble_is_sent_again_before_sdls_cap_ends_it_and_a_stopped_one_is_not() {
        let (mut ctx, fake) = setup(&[(1, xbox())]);
        fake.send(PadEvent::Added(1));
        frame(&mut ctx);
        let pad = ctx.input.gamepads()[0];
        ctx.input.rumble(pad, Rumble::new(0.5, 0.5).seconds(f32::INFINITY));
        frame(&mut ctx);
        let sent = || fake.with(|s| s.rumble.len());
        assert_eq!(sent(), 1);
        fake.with(|s| s.now = RUMBLE_REFRESH_MS - 1);
        frame(&mut ctx);
        assert_eq!(sent(), 1, "not yet");
        fake.with(|s| s.now = RUMBLE_REFRESH_MS);
        frame(&mut ctx);
        assert_eq!(sent(), 2, "sent again, before SDL's 65 s");
        assert!(u64::from(RUMBLE_MS) > RUMBLE_REFRESH_MS);
        ctx.input.stop_rumble(pad);
        frame(&mut ctx);
        fake.with(|s| s.now = 10 * RUMBLE_REFRESH_MS);
        frame(&mut ctx);
        assert_eq!(sent(), 3, "the stop, and nothing after it");
    }

    #[test]
    fn a_pad_that_comes_back_gets_its_slot_again_and_a_new_one_the_next_free_slot() {
        let mut second = dualsense();
        second.identity = "ps5-guid/serial-2".into();
        let (mut ctx, fake) = setup(&[(1, xbox()), (2, dualsense()), (3, dualsense()), (4, second)]);
        fake.send(PadEvent::Added(1));
        fake.send(PadEvent::Added(2));
        frame(&mut ctx);
        fake.send(PadEvent::Removed(1));
        frame(&mut ctx);
        assert_eq!(ctx.input.gamepads(), [Gamepad(1)]);
        assert!(fake.with(|s| s.open == [2]), "the removed pad was closed");
        // SDL gives a returning device a new instance id; its identity is the same.
        fake.with(|s| s.pads.push((5, xbox())));
        fake.send(PadEvent::Removed(2));
        fake.send(PadEvent::Added(4)); // a different DualSense: not slot 1's
        fake.send(PadEvent::Added(5));
        frame(&mut ctx);
        let name = |s: u8| ctx.input.gamepad_info(Gamepad(s)).map(|i| i.name.clone());
        assert_eq!(name(0).as_deref(), Some("Xbox Wireless Controller"), "back in its slot");
        assert_eq!(
            ctx.input.gamepads(),
            [Gamepad(0), Gamepad(2)],
            "slot 1 waits for its pad"
        );
        fake.send(PadEvent::Added(3)); // the first DualSense, again
        frame(&mut ctx);
        assert_eq!(ctx.input.gamepads(), [Gamepad(0), Gamepad(1), Gamepad(2)]);
    }

    #[test]
    fn events_of_pads_it_never_opened_are_ignored() {
        let (mut ctx, fake) = setup(&[]);
        fake.send(PadEvent::Added(9)); // SDL can't open it
        fake.send(PadEvent::Button(9, raw::BUTTON_SOUTH, true));
        fake.send(PadEvent::Axis(9, raw::AXIS_LEFTX, 100));
        fake.send(PadEvent::Sensor(9, raw::SENSOR_GYRO, [1.0; 3]));
        fake.send(PadEvent::Removed(9));
        frame(&mut ctx);
        assert!(ctx.input.gamepads().is_empty());
    }

    #[test]
    fn dropping_the_backend_closes_every_pad() {
        let fake = Fake::default();
        fake.with(|s| s.pads = vec![(1, xbox())]);
        let mut backend = SdlGamepads::new(fake.clone());
        backend.system().send(PadEvent::Added(1));
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        ctx.set_gamepad_backend(backend);
        assert_eq!(fake.with(|s| s.open.clone()), [1]);
        ctx.set_gamepad_backend(crate::input::FakeGamepads::new());
        assert!(fake.with(|s| s.open.is_empty()));
    }
}
