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
//! playing past SDL's 65-second cap) over any [`PadSystem`]; `gamepad_sdl_sys::Sdl`
//! is the `PadSystem` that calls SDL. Tests drive the first with a scripted
//! system, and the second with SDL's own virtual gamepads.

use crate::input::backend::{stick_dead_zone, trigger_dead_zone};
use crate::input::{
    ControllerFamily, DeviceKey, ForceFeedback, ForceFeedbackSupport, GamepadAxis, GamepadBackend, GamepadButton,
    GamepadCapabilities, GamepadEvents, GamepadInfo, GamepadMotion, JoystickInfo, JoystickKind, MotorLevels,
    hat_from_bits,
};

/// SDL's numbers for gamepad buttons, axes, types and sensors
/// (`SDL_gamepad.h`, `SDL_sensor.h`); a test checks them against `sdl3-sys`.
/// Axes are read by their number in order; their names are for tests.
#[allow(dead_code)] // the complete list, checked whole against sdl3-sys
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

    // Joystick types (`SDL_joystick.h`).
    pub(crate) const JOYSTICK_UNKNOWN: i32 = 0;
    pub(crate) const JOYSTICK_GAMEPAD: i32 = 1;
    pub(crate) const JOYSTICK_WHEEL: i32 = 2;
    pub(crate) const JOYSTICK_ARCADE_STICK: i32 = 3;
    pub(crate) const JOYSTICK_FLIGHT_STICK: i32 = 4;
    pub(crate) const JOYSTICK_DANCE_PAD: i32 = 5;
    pub(crate) const JOYSTICK_GUITAR: i32 = 6;
    pub(crate) const JOYSTICK_DRUM_KIT: i32 = 7;
    pub(crate) const JOYSTICK_ARCADE_PAD: i32 = 8;
    pub(crate) const JOYSTICK_THROTTLE: i32 = 9;

    // What `SDL_GetHapticFeatures` sets (`SDL_haptic.h`). The gain and
    // autocenter bits don't fit sdl3-sys's 16-bit effect type, so they are
    // spelled here.
    pub(crate) const HAPTIC_CONSTANT: u32 = 1 << 0;
    pub(crate) const HAPTIC_SINE: u32 = 1 << 1;
    pub(crate) const HAPTIC_SPRING: u32 = 1 << 7;
    pub(crate) const HAPTIC_DAMPER: u32 = 1 << 8;
    pub(crate) const HAPTIC_FRICTION: u32 = 1 << 10;
    pub(crate) const HAPTIC_GAIN: u32 = 1 << 16;
    pub(crate) const HAPTIC_AUTOCENTER: u32 = 1 << 17;
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
    /// A joystick (any device, gamepads too) was plugged in or out.
    JoyAdded(u32),
    JoyRemoved(u32),
    /// A joystick's axis, `-32768..=32767`.
    JoyAxis(u32, i32, i16),
    JoyButton(u32, i32, bool),
    /// A joystick's hat, as SDL's bits (up 1, right 2, down 4, left 8).
    JoyHat(u32, i32, u8),
}

/// What SDL says about a joystick when it is opened.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct OpenedJoystick {
    pub(crate) name: String,
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    /// SDL's joystick type (`raw::JOYSTICK_*`).
    pub(crate) kind: i32,
    /// The same for the same device when it comes back.
    pub(crate) identity: String,
    pub(crate) axes: usize,
    pub(crate) buttons: usize,
    pub(crate) hats: usize,
    /// Its force-feedback effects (`raw::HAPTIC_*` bits), 0 for none.
    pub(crate) haptic: u32,
}

/// One force-feedback effect as plain numbers, SDL's ranges: what the
/// device layer turns into an `SDL_HapticEffect`, all of them along the
/// steering axis and playing until stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Effect {
    /// A steady push, `-32767..=32767` (positive right).
    Constant {
        level: i16,
    },
    /// A spring toward `center`, both `-32767..=32767` (`coeff` `0..`).
    Spring {
        coeff: i16,
        center: i16,
    },
    Damper {
        coeff: i16,
    },
    Friction {
        coeff: i16,
    },
    /// A sine wave of `magnitude` (`0..=32767`) every `period_ms`.
    Sine {
        magnitude: i16,
        period_ms: u16,
    },
}

/// The effects a joystick plays at once, one per slot:
/// constant, spring, damper, friction, vibration.
pub(crate) const EFFECT_SLOTS: usize = 5;

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

/// What the backend asks of SDL: `gamepad_sdl_sys::Sdl`, or a script in tests.
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

    /// The joysticks already there when the backend starts (gamepads too).
    fn present_joysticks(&mut self, into: &mut Vec<u32>);
    /// Whether SDL reads joystick `id` as a gamepad.
    fn is_gamepad(&mut self, id: u32) -> bool;
    /// SDL's type for joystick `id` (`raw::JOYSTICK_*`).
    fn joystick_type(&mut self, id: u32) -> i32;
    /// Opens a joystick for reading and force feedback; `None` when it can't be.
    fn open_joystick(&mut self, id: u32) -> Option<OpenedJoystick>;
    /// Closes it, stopping its effects.
    fn close_joystick(&mut self, id: u32);
    /// Plays `effect` in slot `slot` (`0..EFFECT_SLOTS`) of joystick `id`
    /// until changed, or stops that slot (`None`).
    fn haptic_effect(&mut self, id: u32, slot: usize, effect: Option<Effect>);
    /// Sets the device's own centring spring, `0..=100`.
    fn haptic_autocenter(&mut self, id: u32, level: u8);
}

/// Whether a device of SDL joystick type `kind` is read as a joystick even
/// when SDL has a gamepad layout for it: its other axes and its force
/// feedback matter more than the layout.
pub(crate) fn joystick_first(kind: i32) -> bool {
    matches!(
        kind,
        raw::JOYSTICK_WHEEL | raw::JOYSTICK_FLIGHT_STICK | raw::JOYSTICK_THROTTLE
    )
}

/// The engine's name for SDL joystick type `kind`.
pub(crate) fn joystick_kind(kind: i32) -> JoystickKind {
    match kind {
        raw::JOYSTICK_GAMEPAD => JoystickKind::Gamepad,
        raw::JOYSTICK_WHEEL => JoystickKind::Wheel,
        raw::JOYSTICK_ARCADE_STICK => JoystickKind::ArcadeStick,
        raw::JOYSTICK_FLIGHT_STICK => JoystickKind::FlightStick,
        raw::JOYSTICK_DANCE_PAD => JoystickKind::DancePad,
        raw::JOYSTICK_GUITAR => JoystickKind::Guitar,
        raw::JOYSTICK_DRUM_KIT => JoystickKind::DrumKit,
        raw::JOYSTICK_ARCADE_PAD => JoystickKind::ArcadePad,
        raw::JOYSTICK_THROTTLE => JoystickKind::Throttle,
        _ => JoystickKind::Unknown,
    }
}

/// The effects SDL's feature bits `haptic` say a joystick can play.
pub(crate) fn support_of(haptic: u32) -> ForceFeedbackSupport {
    let has = |bit: u32| haptic & bit != 0;
    ForceFeedbackSupport {
        constant: has(raw::HAPTIC_CONSTANT),
        spring: has(raw::HAPTIC_SPRING),
        damper: has(raw::HAPTIC_DAMPER),
        friction: has(raw::HAPTIC_FRICTION),
        vibration: has(raw::HAPTIC_SINE),
        autocenter: has(raw::HAPTIC_AUTOCENTER),
    }
}

/// The SDL effects `ff` (clamped) plays in each slot on a joystick that
/// supports `support`: `None` for an effect that is off or unsupported.
pub(crate) fn effects_of(ff: &ForceFeedback, support: ForceFeedbackSupport) -> [Option<Effect>; EFFECT_SLOTS] {
    let level = |v: f32| (v * 32767.0).round() as i16;
    let on = |supported: bool, v: f32| supported && v != 0.0;
    [
        on(support.constant, ff.constant).then(|| Effect::Constant {
            level: level(ff.constant),
        }),
        on(support.spring, ff.spring).then(|| Effect::Spring {
            coeff: level(ff.spring),
            center: level(ff.spring_center),
        }),
        on(support.damper, ff.damper).then(|| Effect::Damper {
            coeff: level(ff.damper),
        }),
        on(support.friction, ff.friction).then(|| Effect::Friction {
            coeff: level(ff.friction),
        }),
        (support.vibration && !ff.vibration.is_off()).then(|| Effect::Sine {
            magnitude: level(ff.vibration.strength),
            period_ms: (1000.0 / ff.vibration.hz).round().clamp(1.0, 65535.0) as u16,
        }),
    ]
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

/// A joystick the backend has reported connected.
#[derive(Debug)]
struct Joy {
    id: u32,
    key: DeviceKey,
    support: ForceFeedbackSupport,
    /// The effect last sent to each slot.
    sent: [Option<Effect>; EFFECT_SLOTS],
    /// The autocenter level last sent, if any.
    autocenter: Option<u8>,
}

/// The [`GamepadBackend`] over SDL's gamepads and joysticks (or a scripted
/// [`PadSystem`]). Wheels, flight sticks and throttles are joysticks even
/// when SDL has a gamepad layout for them; other devices are gamepads when
/// SDL can read them as one, joysticks otherwise.
pub(crate) struct SdlGamepads<S: PadSystem> {
    system: S,
    started: bool,
    pads: Vec<Pad>,
    joysticks: Vec<Joy>,
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
            joysticks: Vec::new(),
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
        let kind = self.system.joystick_type(id);
        if joystick_first(kind) {
            return; // read as a joystick (`joystick_added`)
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

    fn joystick_added(&mut self, id: u32, events: &mut GamepadEvents<'_>) {
        if self.joysticks.iter().any(|j| j.id == id) {
            return;
        }
        let kind = self.system.joystick_type(id);
        if !joystick_first(kind) && self.system.is_gamepad(id) {
            return; // read as a gamepad (`added`)
        }
        let Some(opened) = self.system.open_joystick(id) else {
            return;
        };
        let support = support_of(opened.haptic);
        let mut info = JoystickInfo::new(opened.name)
            .kind(joystick_kind(opened.kind))
            .layout(opened.axes, opened.buttons, opened.hats)
            .force_feedback(support);
        info.vendor_id = (opened.vendor_id != 0).then_some(opened.vendor_id);
        info.product_id = (opened.product_id != 0).then_some(opened.product_id);
        // Apart from the gamepads' identities: a device is one or the other.
        let key = self.key_for(format!("joystick/{}", opened.identity));
        if events.joystick_connected(key, info).is_none() {
            self.system.close_joystick(id);
            if let Some(entry) = self.known.iter_mut().find(|(_, k, _)| *k == key) {
                entry.2 = false;
            }
            return;
        }
        self.joysticks.push(Joy {
            id,
            key,
            support,
            sent: [None; EFFECT_SLOTS],
            autocenter: None,
        });
    }

    fn joystick_removed(&mut self, id: u32, events: &mut GamepadEvents<'_>) {
        let Some(at) = self.joysticks.iter().position(|j| j.id == id) else {
            return;
        };
        let joy = self.joysticks.swap_remove(at);
        self.system.close_joystick(id);
        if let Some(entry) = self.known.iter_mut().find(|(_, k, _)| *k == joy.key) {
            entry.2 = false;
        }
        events.joystick_disconnected(joy.key);
    }

    fn joystick_key(&self, id: u32) -> Option<DeviceKey> {
        self.joysticks.iter().find(|j| j.id == id).map(|j| j.key)
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
            self.system.present_joysticks(&mut present);
            for &id in &present {
                self.joystick_added(id, events);
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
                PadEvent::JoyAdded(id) => self.joystick_added(id, events),
                PadEvent::JoyRemoved(id) => self.joystick_removed(id, events),
                PadEvent::JoyAxis(id, axis, value) => {
                    if let (Some(key), Ok(axis)) = (self.joystick_key(id), usize::try_from(axis)) {
                        events.joystick_axis(key, axis, (f32::from(value) / 32767.0).clamp(-1.0, 1.0));
                    }
                }
                PadEvent::JoyButton(id, button, down) => {
                    if let (Some(key), Ok(button)) = (self.joystick_key(id), usize::try_from(button)) {
                        events.joystick_button(key, button, down);
                    }
                }
                PadEvent::JoyHat(id, hat, bits) => {
                    if let (Some(key), Ok(hat)) = (self.joystick_key(id), usize::try_from(hat)) {
                        events.joystick_hat(key, hat, hat_from_bits(bits));
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

    fn set_force_feedback(&mut self, device: DeviceKey, effects: ForceFeedback) {
        let Some(joy) = self.joysticks.iter_mut().find(|j| j.key == device) else {
            return;
        };
        let want = effects_of(&effects, joy.support);
        for (slot, effect) in want.into_iter().enumerate() {
            if effect != joy.sent[slot] {
                joy.sent[slot] = effect;
                self.system.haptic_effect(joy.id, slot, effect);
            }
        }
        // The device's own spring: left alone until the game sets any
        // effect, and off while the game's spring plays.
        if joy.support.autocenter && (joy.autocenter.is_some() || !effects.is_off()) {
            let level = if effects.spring > 0.0 {
                0
            } else {
                (effects.autocenter * 100.0).round() as u8
            };
            if joy.autocenter != Some(level) {
                joy.autocenter = Some(level);
                self.system.haptic_autocenter(joy.id, level);
            }
        }
    }
}

impl<S: PadSystem> Drop for SdlGamepads<S> {
    /// Closing a pad stops its rumble, and a joystick its effects.
    fn drop(&mut self) {
        for pad in self.pads.drain(..) {
            self.system.close(pad.id);
        }
        for joy in self.joysticks.drain(..) {
            self.system.close_joystick(joy.id);
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
        /// Joysticks: their types, which SDL reads as gamepads, what opening says.
        types: Vec<(u32, i32)>,
        gamepads: Vec<u32>,
        joysticks: Vec<(u32, OpenedJoystick)>,
        present_joysticks: Vec<u32>,
        open_joysticks: Vec<u32>,
        effects: Vec<(u32, usize, Option<Effect>)>,
        autocenter: Vec<(u32, u8)>,
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
        fn present_joysticks(&mut self, into: &mut Vec<u32>) {
            self.with(|s| into.extend(&s.present_joysticks));
        }
        fn is_gamepad(&mut self, id: u32) -> bool {
            self.with(|s| s.gamepads.contains(&id) || s.pads.iter().any(|(i, _)| *i == id))
        }
        fn joystick_type(&mut self, id: u32) -> i32 {
            self.with(|s| {
                s.types
                    .iter()
                    .find(|(i, _)| *i == id)
                    .map_or(raw::JOYSTICK_UNKNOWN, |(_, t)| *t)
            })
        }
        fn open_joystick(&mut self, id: u32) -> Option<OpenedJoystick> {
            self.with(|s| {
                let opened = s.joysticks.iter().find(|(i, _)| *i == id)?.1.clone();
                s.open_joysticks.push(id);
                Some(opened)
            })
        }
        fn close_joystick(&mut self, id: u32) {
            self.with(|s| s.open_joysticks.retain(|&i| i != id));
        }
        fn haptic_effect(&mut self, id: u32, slot: usize, effect: Option<Effect>) {
            self.with(|s| s.effects.push((id, slot, effect)));
        }
        fn haptic_autocenter(&mut self, id: u32, level: u8) {
            self.with(|s| s.autocenter.push((id, level)));
        }
    }

    /// A wheel with three pedals, a hat, 20 buttons and every effect.
    fn wheel() -> OpenedJoystick {
        OpenedJoystick {
            name: "Racing Wheel".into(),
            vendor_id: 0x046d,
            product_id: 0xc24f,
            kind: raw::JOYSTICK_WHEEL,
            identity: "wheel-guid".into(),
            axes: 4,
            buttons: 20,
            hats: 1,
            haptic: raw::HAPTIC_CONSTANT
                | raw::HAPTIC_SPRING
                | raw::HAPTIC_DAMPER
                | raw::HAPTIC_FRICTION
                | raw::HAPTIC_SINE
                | raw::HAPTIC_GAIN
                | raw::HAPTIC_AUTOCENTER,
        }
    }

    #[test]
    fn a_wheel_is_a_joystick_even_with_a_gamepad_layout_and_its_inputs_arrive() {
        let fake = Fake::default();
        fake.with(|s| {
            // SDL has a gamepad layout for the wheel too: it stays a joystick.
            s.types = vec![(3, raw::JOYSTICK_WHEEL), (7, raw::JOYSTICK_GAMEPAD)];
            s.gamepads = vec![3];
            s.joysticks = vec![(3, wheel()), (7, OpenedJoystick::default())];
            s.pads = vec![(3, xbox()), (7, dualsense())];
            s.present = vec![3, 7];
            s.present_joysticks = vec![3, 7];
        });
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        ctx.set_gamepad_backend(SdlGamepads::new(fake.clone()));
        assert_eq!(ctx.input.gamepads().len(), 1, "the DualSense, not the wheel");
        assert_eq!(ctx.input.joysticks().len(), 1, "the wheel, not the DualSense");
        let w = ctx.input.joysticks()[0];
        let info = ctx.input.joystick_info(w).unwrap().clone();
        assert_eq!(info.name, "Racing Wheel");
        assert_eq!(info.kind, JoystickKind::Wheel);
        assert_eq!((info.axes, info.buttons, info.hats), (4, 20, 1));
        assert_eq!(info.vendor_id, Some(0x046d));
        assert!(info.force_feedback.constant && info.force_feedback.autocenter);
        assert_eq!(fake.with(|s| s.open_joysticks.clone()), [3]);
        assert_eq!(fake.with(|s| s.open.clone()), [7]);

        fake.send(PadEvent::JoyAxis(3, 0, -16384));
        fake.send(PadEvent::JoyAxis(3, 2, i16::MAX));
        fake.send(PadEvent::JoyAxis(3, 3, i16::MIN));
        fake.send(PadEvent::JoyButton(3, 12, true));
        fake.send(PadEvent::JoyHat(3, 0, 0b0110)); // right and down
        fake.send(PadEvent::JoyButton(7, 1, true)); // the DualSense's joystick: not ours
        frame(&mut ctx);
        assert!((ctx.input.joystick_axis(w, 0) + 0.5).abs() < 1e-4);
        assert_eq!(ctx.input.joystick_axis(w, 2), 1.0);
        assert_eq!(ctx.input.joystick_axis(w, 3), -1.0, "clamped");
        assert!(ctx.input.joystick_pressed(w, 12));
        assert_eq!(ctx.input.joystick_hat(w, 0), glam::IVec2::new(1, 1));

        fake.send(PadEvent::JoyRemoved(3));
        frame(&mut ctx);
        assert!(ctx.input.joysticks().is_empty());
        assert!(fake.with(|s| s.open_joysticks.is_empty()), "closed");
        fake.send(PadEvent::JoyAdded(3));
        frame(&mut ctx);
        assert_eq!(ctx.input.joysticks(), [w], "back in its slot");
    }

    #[test]
    fn a_device_sdl_cant_read_as_a_gamepad_is_a_joystick() {
        let fake = Fake::default();
        fake.with(|s| {
            s.joysticks = vec![(
                4,
                OpenedJoystick {
                    name: "Button Box".into(),
                    buttons: 32,
                    ..OpenedJoystick::default()
                },
            )];
        });
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        ctx.set_gamepad_backend(SdlGamepads::new(fake.clone()));
        fake.send(PadEvent::JoyAdded(4));
        fake.send(PadEvent::JoyButton(4, 31, true));
        frame(&mut ctx);
        let j = ctx.input.joysticks()[0];
        assert_eq!(ctx.input.joystick_info(j).unwrap().kind, JoystickKind::Unknown);
        assert!(ctx.input.joystick_down(j, 31));
    }

    #[test]
    fn force_feedback_becomes_sdl_effects_sent_only_when_they_change() {
        let fake = Fake::default();
        fake.with(|s| {
            s.types = vec![(3, raw::JOYSTICK_WHEEL)];
            s.joysticks = vec![(3, wheel())];
            s.present_joysticks = vec![3];
        });
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        ctx.set_gamepad_backend(SdlGamepads::new(fake.clone()));
        let w = ctx.input.joysticks()[0];
        let road = ForceFeedback {
            constant: 0.5,
            damper: 1.0,
            ..ForceFeedback::OFF
        }
        .vibration(0.25, 40.0);
        ctx.input.set_force_feedback(w, road);
        frame(&mut ctx);
        assert_eq!(
            fake.with(|s| std::mem::take(&mut s.effects)),
            [
                (3, 0, Some(Effect::Constant { level: 16384 })),
                (3, 2, Some(Effect::Damper { coeff: 32767 })),
                (
                    3,
                    4,
                    Some(Effect::Sine {
                        magnitude: 8192,
                        period_ms: 25
                    })
                ),
            ]
        );
        assert_eq!(fake.with(|s| std::mem::take(&mut s.autocenter)), [(3, 0)]);
        frame(&mut ctx);
        assert!(fake.with(|s| s.effects.is_empty()), "nothing changed");

        // The game's spring instead of the push: one slot stops, one starts.
        ctx.input.set_force_feedback(
            w,
            ForceFeedback {
                spring: 0.5,
                spring_center: -0.5,
                damper: 1.0,
                ..ForceFeedback::OFF
            }
            .vibration(0.25, 40.0),
        );
        frame(&mut ctx);
        assert_eq!(
            fake.with(|s| std::mem::take(&mut s.effects)),
            [
                (3, 0, None),
                (
                    3,
                    1,
                    Some(Effect::Spring {
                        coeff: 16384,
                        center: -16384
                    })
                ),
            ]
        );
        ctx.input.set_force_feedback(
            w,
            ForceFeedback {
                autocenter: 0.4,
                ..ForceFeedback::OFF
            },
        );
        frame(&mut ctx);
        assert_eq!(fake.with(|s| s.effects.len()), 3, "spring, damper and vibration stop");
        assert_eq!(fake.with(|s| std::mem::take(&mut s.autocenter)), [(3, 40)]);
    }

    #[test]
    fn effects_a_joystick_lacks_are_left_out() {
        let support = ForceFeedbackSupport {
            constant: true,
            ..Default::default()
        };
        let ff = ForceFeedback {
            constant: -1.0,
            spring: 1.0,
            ..ForceFeedback::OFF
        };
        assert_eq!(
            effects_of(&ff, support),
            [Some(Effect::Constant { level: -32767 }), None, None, None, None]
        );
        assert_eq!(effects_of(&ForceFeedback::OFF, support), [None; EFFECT_SLOTS]);
        let slow = effects_of(&ForceFeedback::OFF.vibration(1.0, 0.001), support_of(raw::HAPTIC_SINE));
        assert_eq!(
            slow[4],
            Some(Effect::Sine {
                magnitude: 32767,
                period_ms: 65535
            }),
            "the longest period SDL takes"
        );
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
        assert_eq!(rumble.iter().rfind(|r| r.0 == 1), Some(&(1, 0, 0)));
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
