//! What the engine knows about each controller beyond its buttons: its
//! family (for button prompts), what it can do (rumble, motion), rumble
//! requests, motion readings, which device was used last, and which player
//! holds which device.
//!
//! The state lives in [`Input`] (one fixed-size slot per gamepad, nothing
//! allocated per frame); the queries are `Input` methods, documented there.

use glam::{Vec2, Vec3};

use super::gamepad_extras::{GamepadOutput, GamepadTouchpadFinger};
use super::{Gamepad, GamepadAxis, GamepadButton, Input, MAX_GAMEPADS};
use crate::actions::PRESS_THRESHOLD;

/// The button-label family of a controller, for prompts: which name and
/// shape [`GamepadButton::South`] has on it.
///
/// Detected from the controller's USB vendor id and name when it connects
/// ([`ControllerFamily::detect`]); a game can override it per pad with
/// [`Input::set_gamepad_family`] (a "button style" option).
///
/// ```
/// use ponko::input::ControllerFamily;
/// assert_eq!(ControllerFamily::detect(Some(0x054c), None, "Wireless Controller"), ControllerFamily::PlayStation);
/// assert_eq!(ControllerFamily::detect(None, None, "Xbox Controller"), ControllerFamily::Xbox);
/// assert_eq!(ControllerFamily::detect(None, None, "USB Gamepad"), ControllerFamily::Generic);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum ControllerFamily {
    /// Unknown make: positional prompts (which face button, by its place).
    #[default]
    Generic,
    /// Xbox-style labels: A, B, X, Y; LB, RB, LT, RT; View, Menu.
    Xbox,
    /// PlayStation-style labels: cross, circle, triangle, square; L1, R1,
    /// L2, R2; Share, Options.
    PlayStation,
    /// Nintendo-style labels: B, A, X, Y (south, east, north, west); L, R,
    /// ZL, ZR; minus, plus.
    Nintendo,
}

impl ControllerFamily {
    /// Every family, in declaration order.
    pub const ALL: [ControllerFamily; 4] = [
        ControllerFamily::Generic,
        ControllerFamily::Xbox,
        ControllerFamily::PlayStation,
        ControllerFamily::Nintendo,
    ];

    /// Guesses the family from what a backend reports: the USB vendor id
    /// first (Microsoft, Sony, Nintendo), then words in the name. Anything
    /// else is [`Generic`](Self::Generic).
    ///
    /// ```
    /// use ponko::input::ControllerFamily;
    /// assert_eq!(ControllerFamily::detect(Some(0x057e), Some(0x2009), "Pro Controller"), ControllerFamily::Nintendo);
    /// assert_eq!(ControllerFamily::detect(None, None, "DualSense Wireless Controller"), ControllerFamily::PlayStation);
    /// ```
    #[must_use]
    pub fn detect(vendor_id: Option<u16>, _product_id: Option<u16>, name: &str) -> ControllerFamily {
        match vendor_id {
            Some(0x045e) => return ControllerFamily::Xbox,
            Some(0x054c) => return ControllerFamily::PlayStation,
            Some(0x057e) => return ControllerFamily::Nintendo,
            _ => {}
        }
        let name = name.to_ascii_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
        if has(&["xbox", "xinput", "x-box"]) {
            ControllerFamily::Xbox
        } else if has(&["playstation", "dualshock", "dualsense", "ps3", "ps4", "ps5", "sony"]) {
            ControllerFamily::PlayStation
        } else if has(&["nintendo", "switch", "joy-con", "joycon", "pro controller"]) {
            ControllerFamily::Nintendo
        } else {
            ControllerFamily::Generic
        }
    }

    /// A short lowercase name: `generic`, `xbox`, `playstation`, `nintendo`.
    ///
    /// ```
    /// assert_eq!(ponko::input::ControllerFamily::Xbox.name(), "xbox");
    /// ```
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ControllerFamily::Generic => "generic",
            ControllerFamily::Xbox => "xbox",
            ControllerFamily::PlayStation => "playstation",
            ControllerFamily::Nintendo => "nintendo",
        }
    }
}

/// What a connected controller can do, as its backend reports it. A
/// feature the backend can't drive reads `false` even if the hardware has
/// it (see the guide's "What each platform supports").
///
/// ```
/// use ponko::input::GamepadCapabilities;
/// let caps = GamepadCapabilities { rumble: true, ..Default::default() };
/// assert!(caps.rumble && !caps.trigger_rumble && !caps.motion);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GamepadCapabilities {
    /// The two body motors ([`Rumble`]'s `strong` and `weak`) can be driven.
    pub rumble: bool,
    /// The trigger motors ([`Rumble::triggers`]) can be driven.
    pub trigger_rumble: bool,
    /// Gyro and accelerometer readings arrive
    /// ([`Input::gamepad_motion`]).
    pub motion: bool,
    /// The RGB light bar can be set.
    pub light: bool,
    /// The player indicator LEDs can be set.
    pub player_leds: bool,
    /// The controller reports touchpad contacts.
    pub touchpad: bool,
    /// The controller accepts adaptive trigger effects.
    pub adaptive_triggers: bool,
}

/// One connected controller, as its backend described it when it connected:
/// [`Input::gamepad_info`].
///
/// ```
/// use ponko::input::{ControllerFamily, GamepadInfo};
/// let info = GamepadInfo::new("Wireless Controller").ids(0x054c, 0x0ce6);
/// assert_eq!(info.family, ControllerFamily::PlayStation);
/// assert_eq!(info.vendor_id, Some(0x054c));
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GamepadInfo {
    /// The name the system gives it ("Xbox Wireless Controller"). Can be empty.
    pub name: String,
    /// Its button-label family, for prompts.
    pub family: ControllerFamily,
    /// USB vendor id, where the backend reports one.
    pub vendor_id: Option<u16>,
    /// USB product id, where the backend reports one.
    pub product_id: Option<u16>,
    /// What it can do.
    pub capabilities: GamepadCapabilities,
}

impl GamepadInfo {
    /// A controller called `name`, its family guessed from the name, with no
    /// ids and no capabilities.
    ///
    /// ```
    /// let info = ponko::input::GamepadInfo::new("Xbox Controller");
    /// assert_eq!(info.family, ponko::input::ControllerFamily::Xbox);
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            family: ControllerFamily::detect(None, None, &name),
            name,
            ..Self::default()
        }
    }

    /// Sets the USB vendor and product ids and guesses the family again from
    /// them and the name.
    ///
    /// ```
    /// let info = ponko::input::GamepadInfo::new("Gamepad").ids(0x045e, 0x0b13);
    /// assert_eq!(info.family, ponko::input::ControllerFamily::Xbox);
    /// ```
    #[must_use]
    pub fn ids(mut self, vendor_id: u16, product_id: u16) -> Self {
        self.vendor_id = Some(vendor_id);
        self.product_id = Some(product_id);
        self.family = ControllerFamily::detect(self.vendor_id, self.product_id, &self.name);
        self
    }

    /// Sets the family, overriding the guess.
    ///
    /// ```
    /// use ponko::input::{ControllerFamily, GamepadInfo};
    /// let info = GamepadInfo::new("Arcade Stick").family(ControllerFamily::Nintendo);
    /// assert_eq!(info.family, ControllerFamily::Nintendo);
    /// ```
    #[must_use]
    pub fn family(mut self, family: ControllerFamily) -> Self {
        self.family = family;
        self
    }

    /// Sets what it can do.
    ///
    /// ```
    /// use ponko::input::{GamepadCapabilities, GamepadInfo};
    /// let info = GamepadInfo::new("Pad").capabilities(GamepadCapabilities { rumble: true, ..Default::default() });
    /// assert!(info.capabilities.rumble);
    /// ```
    #[must_use]
    pub fn capabilities(mut self, capabilities: GamepadCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}

/// A rumble request for [`Input::rumble`]: how hard each motor runs, from
/// `0.0` (off) to `1.0` (full), and for how long.
///
/// `strong` is the low-frequency motor (usually in the left grip), `weak`
/// the high-frequency one (right grip). Trigger motors exist on some
/// controllers only; see [`GamepadCapabilities`]. Values outside `0..=1`
/// (and NaN) are clamped to that range.
///
/// ```
/// use ponko::input::Rumble;
/// let hit = Rumble::new(0.8, 0.3).seconds(0.25);
/// assert_eq!(hit.strong, 0.8);
/// assert_eq!(hit.seconds, 0.25);
/// let shot = Rumble::new(0.0, 0.2).triggers(0.0, 0.9).seconds(0.1);
/// assert_eq!(shot.right_trigger, 0.9);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rumble {
    /// The low-frequency body motor, `0..=1`.
    pub strong: f32,
    /// The high-frequency body motor, `0..=1`.
    pub weak: f32,
    /// The left trigger's motor, `0..=1`, where there is one.
    pub left_trigger: f32,
    /// The right trigger's motor, `0..=1`, where there is one.
    pub right_trigger: f32,
    /// How long it plays, in real seconds (not slowed by the time scale or
    /// pause). [`f32::INFINITY`] plays until [`Input::stop_rumble`].
    pub seconds: f32,
}

impl Rumble {
    /// The two body motors at these strengths, for 0.2 seconds.
    ///
    /// ```
    /// let r = ponko::input::Rumble::new(1.0, 0.5);
    /// assert_eq!((r.strong, r.weak, r.seconds), (1.0, 0.5, 0.2));
    /// ```
    #[must_use]
    pub const fn new(strong: f32, weak: f32) -> Self {
        Self {
            strong,
            weak,
            left_trigger: 0.0,
            right_trigger: 0.0,
            seconds: 0.2,
        }
    }

    /// Sets the trigger motors.
    ///
    /// ```
    /// let r = ponko::input::Rumble::new(0.0, 0.0).triggers(0.5, 1.0);
    /// assert_eq!((r.left_trigger, r.right_trigger), (0.5, 1.0));
    /// ```
    #[must_use]
    pub const fn triggers(mut self, left: f32, right: f32) -> Self {
        self.left_trigger = left;
        self.right_trigger = right;
        self
    }

    /// Sets how long it plays, in real seconds.
    ///
    /// ```
    /// let r = ponko::input::Rumble::new(1.0, 1.0).seconds(1.5);
    /// assert_eq!(r.seconds, 1.5);
    /// ```
    #[must_use]
    pub const fn seconds(mut self, seconds: f32) -> Self {
        self.seconds = seconds;
        self
    }

    /// The motor levels it asks for, clamped to `0..=1`.
    fn levels(self) -> MotorLevels {
        let c = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
        MotorLevels {
            strong: c(self.strong),
            weak: c(self.weak),
            left_trigger: c(self.left_trigger),
            right_trigger: c(self.right_trigger),
        }
    }
}

/// How hard each motor of one controller runs right now, `0..=1`: what
/// [`Input::rumble_levels`] reads and what a
/// [`GamepadBackend`](crate::input::GamepadBackend) is told to play.
///
/// ```
/// let off = ponko::input::MotorLevels::OFF;
/// assert!(off.is_off());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MotorLevels {
    /// The low-frequency body motor.
    pub strong: f32,
    /// The high-frequency body motor.
    pub weak: f32,
    /// The left trigger's motor.
    pub left_trigger: f32,
    /// The right trigger's motor.
    pub right_trigger: f32,
}

impl MotorLevels {
    /// Every motor off.
    pub const OFF: MotorLevels = MotorLevels {
        strong: 0.0,
        weak: 0.0,
        left_trigger: 0.0,
        right_trigger: 0.0,
    };

    /// Whether every motor is off.
    ///
    /// ```
    /// let on = ponko::input::MotorLevels { strong: 0.5, ..Default::default() };
    /// assert!(!on.is_off());
    /// ```
    #[must_use]
    pub fn is_off(&self) -> bool {
        *self == Self::OFF
    }
}

/// One motion reading of a controller with a gyro and accelerometer:
/// [`Input::gamepad_motion`].
///
/// Axes are the controller's, held flat in front of the player: `x` to the
/// right, `y` up, `z` toward the player. `gyro` is the turn rate around each
/// axis in radians per second (positive is counterclockwise looking down
/// the axis); `accel` is the acceleration in m/s², gravity included (a pad
/// lying still reads about `(0, 9.81, 0)`).
///
/// ```
/// use ponko::input::GamepadMotion;
/// let still = GamepadMotion { gyro: glam::Vec3::ZERO, accel: glam::vec3(0.0, 9.81, 0.0) };
/// assert_eq!(still.aim_delta(1.0 / 60.0), glam::Vec2::ZERO);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GamepadMotion {
    /// Turn rate around x, y, z, in radians per second.
    pub gyro: Vec3,
    /// Acceleration along x, y, z, in m/s², gravity included.
    pub accel: Vec3,
}

impl GamepadMotion {
    /// How far to turn an aim this frame, in radians: yaw (turning the pad
    /// left or right, around `y`) as `x`, pitch (tilting it up or down,
    /// around `x`) as `y`, both signed for screen coordinates (right and
    /// down positive). Multiply by a sensitivity.
    ///
    /// ```
    /// use ponko::input::GamepadMotion;
    /// let turning_right = GamepadMotion { gyro: glam::vec3(0.0, -1.0, 0.0), accel: glam::Vec3::ZERO };
    /// assert!(turning_right.aim_delta(0.5).x > 0.0);
    /// ```
    #[must_use]
    pub fn aim_delta(&self, dt: f32) -> Vec2 {
        Vec2::new(-self.gyro.y, -self.gyro.x) * dt
    }
}

/// The device a person played with: [`Input::last_device`], and what a
/// player is assigned ([`Input::assign_player`]).
///
/// More kinds (touch) may be added, so a `match` needs a `_` arm.
///
/// ```
/// use ponko::input::{Gamepad, InputDevice};
/// let pad = InputDevice::Gamepad(Gamepad::from_slot(1).unwrap());
/// assert_eq!(pad.gamepad().map(|p| p.slot()), Some(1));
/// assert_eq!(InputDevice::KeyboardMouse.gamepad(), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum InputDevice {
    /// The keyboard and mouse, together.
    #[default]
    KeyboardMouse,
    /// One gamepad, by slot.
    Gamepad(Gamepad),
}

impl InputDevice {
    /// The gamepad, for [`InputDevice::Gamepad`].
    ///
    /// ```
    /// assert!(ponko::input::InputDevice::KeyboardMouse.gamepad().is_none());
    /// ```
    #[must_use]
    pub fn gamepad(self) -> Option<Gamepad> {
        match self {
            InputDevice::Gamepad(pad) => Some(pad),
            _ => None,
        }
    }
}

/// How many local players [`Input::assign_player`] tracks.
pub const MAX_PLAYERS: usize = MAX_GAMEPADS;

/// A rumble playing on one slot: its levels and the real seconds left.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ActiveRumble {
    pub(crate) levels: MotorLevels,
    pub(crate) left: f32,
}

/// The per-controller state beyond buttons and axes. Fixed-size: nothing
/// here allocates per frame (a connect stores the pad's name once).
#[derive(Debug, Clone, Default)]
pub(crate) struct Controllers {
    pub(crate) info: [Option<GamepadInfo>; MAX_GAMEPADS],
    pub(crate) motion: [Option<GamepadMotion>; MAX_GAMEPADS],
    pub(crate) rumble: [ActiveRumble; MAX_GAMEPADS],
    pub(crate) output: [GamepadOutput; MAX_GAMEPADS],
    pub(crate) touchpad: [[GamepadTouchpadFinger; 2]; MAX_GAMEPADS],
    pub(crate) touchpad_events: Vec<(Gamepad, u8, GamepadTouchpadFinger)>,
    players: [Option<InputDevice>; MAX_PLAYERS],
    last_device: InputDevice,
    last_device_changed: bool,
    connected_now: [bool; MAX_GAMEPADS],
    disconnected_now: [bool; MAX_GAMEPADS],
}

impl Controllers {
    pub(super) fn end_frame(&mut self) {
        self.last_device_changed = false;
        self.touchpad_events.clear();
        self.connected_now = [false; MAX_GAMEPADS];
        self.disconnected_now = [false; MAX_GAMEPADS];
    }

    pub(super) fn used(&mut self, device: InputDevice) {
        if self.last_device != device {
            self.last_device = device;
            self.last_device_changed = true;
        }
    }

    pub(super) fn connected(&mut self, pad: Gamepad, info: GamepadInfo) {
        let slot = pad.slot();
        self.info[slot] = Some(info);
        self.motion[slot] = None;
        self.rumble[slot] = ActiveRumble::default();
        self.output[slot] = GamepadOutput::default();
        self.touchpad[slot] = Default::default();
        self.connected_now[slot] = true;
    }

    pub(super) fn disconnected(&mut self, pad: Gamepad) {
        let slot = pad.slot();
        self.info[slot] = None;
        self.motion[slot] = None;
        self.rumble[slot] = ActiveRumble::default();
        self.output[slot] = GamepadOutput::default();
        self.touchpad[slot] = Default::default();
        self.disconnected_now[slot] = true;
    }

    /// Counts the rumbles down by `dt` real seconds; those that end turn off.
    pub(crate) fn tick_rumble(&mut self, dt: f32) {
        for r in &mut self.rumble {
            if r.levels.is_off() {
                continue;
            }
            r.left -= dt;
            if r.left <= 0.0 {
                *r = ActiveRumble::default();
            }
        }
    }

    pub(super) fn stop_all_rumble(&mut self) {
        self.rumble = [ActiveRumble::default(); MAX_GAMEPADS];
    }
}

/// Whether an axis moved from below the press threshold to past it.
pub(super) fn axis_crossed(before: f32, now: f32) -> bool {
    before.abs() < PRESS_THRESHOLD && now.abs() >= PRESS_THRESHOLD
}

/// Controllers: identity, rumble, motion, the last used device, players.
impl Input {
    /// What the backend reported about `pad` when it connected: name, family,
    /// ids, capabilities. `None` for a slot with no pad, and for a pad only
    /// [`feed`](Self::feed)-ed (no backend described it).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::input::{ControllerFamily, FakeGamepads, GamepadInfo};
    /// let fake = FakeGamepads::new();
    /// fake.connect(GamepadInfo::new("Wireless Controller").ids(0x054c, 0x09cc));
    /// Game::new("t", 1, 1).gamepad_backend(fake.clone()).run_headless(0, |ctx| {
    ///     let pad = ctx.input.gamepads()[0];
    ///     assert_eq!(ctx.input.gamepad_info(pad).unwrap().family, ControllerFamily::PlayStation);
    /// });
    /// ```
    #[must_use]
    pub fn gamepad_info(&self, pad: Gamepad) -> Option<&GamepadInfo> {
        self.controllers.info[pad.slot()].as_ref()
    }

    /// The prompt family of `pad`: its detected (or overridden) family, or
    /// [`ControllerFamily::Generic`] when nothing described it.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx, pad: ponko::input::Gamepad) {
    /// let family = ctx.input.gamepad_family(pad);
    /// # let _ = family; }
    /// ```
    #[must_use]
    pub fn gamepad_family(&self, pad: Gamepad) -> ControllerFamily {
        self.gamepad_info(pad).map_or(ControllerFamily::Generic, |i| i.family)
    }

    /// Overrides the prompt family of a connected `pad` (a "button style"
    /// option for a controller detected wrongly). Lasts until it disconnects;
    /// does nothing for an empty slot.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, pad: ponko::input::Gamepad) {
    /// ctx.input.set_gamepad_family(pad, ponko::input::ControllerFamily::Nintendo);
    /// # }
    /// ```
    pub fn set_gamepad_family(&mut self, pad: Gamepad, family: ControllerFamily) {
        if let Some(info) = &mut self.controllers.info[pad.slot()] {
            info.family = family;
        }
    }

    /// The pads that connected this frame (or reconnected), by slot. With
    /// [`gamepads_disconnected`](Self::gamepads_disconnected), what a game
    /// needs for "controller 2 disconnected" and join screens.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx) {
    /// for pad in ctx.input.gamepads_connected() {
    ///     println!("pad in slot {} connected", pad.slot());
    /// }
    /// # }
    /// ```
    pub fn gamepads_connected(&self) -> impl Iterator<Item = Gamepad> + '_ {
        slots_set(&self.controllers.connected_now)
    }

    /// The pads that disconnected this frame, by slot. Their held buttons
    /// read as released this frame; a player assigned to one stays assigned
    /// ([`player_connected`](Self::player_connected) turns false) until the
    /// pad comes back or the game reassigns the player.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// if ctx.input.gamepads_disconnected().next().is_some() {
    ///     ctx.time.pause();
    /// }
    /// # }
    /// ```
    pub fn gamepads_disconnected(&self) -> impl Iterator<Item = Gamepad> + '_ {
        slots_set(&self.controllers.disconnected_now)
    }

    /// Starts `rumble` on `pad`, replacing what it was playing. It plays for
    /// `rumble.seconds` of real time, then stops. Does nothing for an empty
    /// slot; a pad whose backend can't rumble
    /// ([`GamepadCapabilities::rumble`]) stays still. Rumble stops when the
    /// window loses focus.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::input::{FakeGamepads, GamepadInfo, Rumble};
    /// let fake = FakeGamepads::new();
    /// let device = fake.connect(GamepadInfo::new("Pad"));
    /// Game::new("t", 1, 1).gamepad_backend(fake.clone()).run_headless(2, |ctx| {
    ///     let pad = ctx.input.gamepads()[0];
    ///     ctx.input.rumble(pad, Rumble::new(1.0, 0.5).seconds(10.0));
    /// });
    /// assert_eq!(fake.rumble(device).strong, 1.0);
    /// ```
    pub fn rumble(&mut self, pad: Gamepad, rumble: Rumble) {
        let slot = pad.slot();
        if !self.connected.contains(&pad) {
            return;
        }
        let seconds = if rumble.seconds.is_nan() { 0.0 } else { rumble.seconds };
        let levels = rumble.levels();
        self.controllers.rumble[slot] = if seconds > 0.0 && !levels.is_off() {
            ActiveRumble { levels, left: seconds }
        } else {
            ActiveRumble::default()
        };
    }

    /// Stops `pad`'s rumble now.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, pad: ponko::input::Gamepad) {
    /// ctx.input.stop_rumble(pad);
    /// # }
    /// ```
    pub fn stop_rumble(&mut self, pad: Gamepad) {
        self.controllers.rumble[pad.slot()] = ActiveRumble::default();
    }

    /// The motor levels `pad` is asked to play right now (all zero when
    /// nothing plays). What was asked for, not what the hardware does: see
    /// [`GamepadCapabilities`].
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx, pad: ponko::input::Gamepad) {
    /// let shaking = !ctx.input.rumble_levels(pad).is_off();
    /// # let _ = shaking; }
    /// ```
    #[must_use]
    pub fn rumble_levels(&self, pad: Gamepad) -> MotorLevels {
        self.controllers.rumble[pad.slot()].levels
    }

    /// The latest gyro and accelerometer reading of `pad`, or `None` when it
    /// has sent none: no motion sensors, or a backend that can't read them
    /// (the built-in one can't; see the guide's platform table).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx, pad: ponko::input::Gamepad, aim: &mut Vec2) {
    /// if let Some(motion) = ctx.input.gamepad_motion(pad) {
    ///     *aim += motion.aim_delta(ctx.time.dt()) * 1.5;
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn gamepad_motion(&self, pad: Gamepad) -> Option<GamepadMotion> {
        self.controllers.motion[pad.slot()]
    }

    /// The device that was used last: a key or mouse button pressed, the
    /// mouse moved or scrolled, a gamepad button pressed or a stick or
    /// trigger pushed past halfway. Button prompts follow it
    /// ([`action_glyphs`](Self::action_glyphs)). Starts as
    /// [`InputDevice::KeyboardMouse`].
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// use ponko::input::{Gamepad, GamepadButton, InputDevice};
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let pad = Gamepad::from_slot(0).unwrap();
    ///     ctx.input.feed(InputEvent::GamepadButton(pad, GamepadButton::South, true));
    ///     assert_eq!(ctx.input.last_device(), InputDevice::Gamepad(pad));
    ///     ctx.input.feed(InputEvent::Key(Key::Space, true));
    ///     assert_eq!(ctx.input.last_device(), InputDevice::KeyboardMouse);
    /// });
    /// ```
    #[must_use]
    pub fn last_device(&self) -> InputDevice {
        self.controllers.last_device
    }

    /// Whether [`last_device`](Self::last_device) changed this frame: the
    /// moment to redraw prompts.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx) {
    /// if ctx.input.last_device_changed() { /* rebuild the prompt row */ }
    /// # }
    /// ```
    #[must_use]
    pub fn last_device_changed(&self) -> bool {
        self.controllers.last_device_changed
    }

    /// Gives `player` (`0..MAX_PLAYERS`) the `device`, taking it from any
    /// other player that had it. Returns `false` (and changes nothing) for a
    /// player number out of range. A pad keeps its player while unplugged
    /// and when it reconnects into its old slot.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::input::{Gamepad, InputDevice};
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let pad = InputDevice::Gamepad(Gamepad::from_slot(0).unwrap());
    ///     assert!(ctx.input.assign_player(1, pad));
    ///     assert_eq!(ctx.input.player_device(1), Some(pad));
    ///     assert_eq!(ctx.input.player_of(pad), Some(1));
    ///     assert!(ctx.input.assign_player(0, pad), "moves the pad to player 0");
    ///     assert_eq!(ctx.input.player_device(1), None);
    /// });
    /// ```
    pub fn assign_player(&mut self, player: usize, device: InputDevice) -> bool {
        if player >= MAX_PLAYERS {
            return false;
        }
        for p in &mut self.controllers.players {
            if *p == Some(device) {
                *p = None;
            }
        }
        self.controllers.players[player] = Some(device);
        true
    }

    /// Takes `player`'s device away (the player left). Out of range does
    /// nothing.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.input.unassign_player(1);
    /// assert_eq!(ctx.input.player_device(1), None);
    /// # }
    /// ```
    pub fn unassign_player(&mut self, player: usize) {
        if let Some(p) = self.controllers.players.get_mut(player) {
            *p = None;
        }
    }

    /// The device `player` holds, if any.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx) {
    /// assert_eq!(ctx.input.player_device(ponko::input::MAX_PLAYERS), None);
    /// # }
    /// ```
    #[must_use]
    pub fn player_device(&self, player: usize) -> Option<InputDevice> {
        self.controllers.players.get(player).copied().flatten()
    }

    /// The player that holds `device`, if any.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx) {
    /// let who = ctx.input.player_of(ponko::input::InputDevice::KeyboardMouse);
    /// # let _ = who; }
    /// ```
    #[must_use]
    pub fn player_of(&self, device: InputDevice) -> Option<usize> {
        self.controllers.players.iter().position(|&p| p == Some(device))
    }

    /// Whether `player` holds a device that is there now: the keyboard and
    /// mouse always are; a pad while connected. `false` for a player with no
    /// device.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx) {
    /// if !ctx.input.player_connected(1) { /* "Player 2: reconnect your controller" */ }
    /// # }
    /// ```
    #[must_use]
    pub fn player_connected(&self, player: usize) -> bool {
        match self.player_device(player) {
            Some(InputDevice::Gamepad(pad)) => self.connected.contains(&pad),
            Some(_) => true,
            None => false,
        }
    }

    /// The lowest player number with no device, if any: where a joining
    /// device goes.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx) {
    /// let next = ctx.input.free_player();
    /// # let _ = next; }
    /// ```
    #[must_use]
    pub fn free_player(&self) -> Option<usize> {
        self.controllers.players.iter().position(Option::is_none)
    }

    /// A connected pad that no player holds and that pressed `button` this
    /// frame (the lowest slot if several did): the "press Start to join"
    /// check. Assign it with [`assign_player`](Self::assign_player).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::input::{GamepadButton, InputDevice};
    /// fn join(ctx: &mut Ctx) {
    ///     if let (Some(pad), Some(player)) = (ctx.input.join_pressed(GamepadButton::Start), ctx.input.free_player()) {
    ///         ctx.input.assign_player(player, InputDevice::Gamepad(pad));
    ///     }
    /// }
    /// # Game::new("t", 1, 1).sim_system(join).run_headless(1, |_| {});
    /// ```
    #[must_use]
    pub fn join_pressed(&self, button: GamepadButton) -> Option<Gamepad> {
        self.connected
            .iter()
            .copied()
            .find(|&pad| self.gamepad_pressed(pad, button) && self.player_of(InputDevice::Gamepad(pad)).is_none())
    }

    /// A pad connects into `pad`'s slot, described by `info`. The backend
    /// driver chooses the slot; [`feed`](Self::feed) passes generic info.
    pub(crate) fn connect_gamepad_at(&mut self, pad: Gamepad, info: GamepadInfo) {
        self.record_connected(pad, &info);
        if self.connected.contains(&pad) {
            self.controllers.info[pad.slot()] = Some(info);
            return;
        }
        let slot = pad.slot();
        self.pads[slot] = super::PadState::new();
        let at = self.connected.partition_point(|p| p.slot() < slot);
        self.connected.insert(at, pad);
        self.controllers.connected(pad, info);
        self.refresh_actions();
    }

    /// A scenario's pad connecting with its description: like
    /// [`feed`](Self::feed)ing `InputEvent::GamepadConnected`, with `info`.
    pub(crate) fn feed_gamepad_connected(&mut self, pad: Gamepad, info: GamepadInfo) {
        self.fed_after_ui();
        self.connect_gamepad_at(pad, info);
    }

    pub(crate) fn set_gamepad_motion(&mut self, pad: Gamepad, motion: GamepadMotion) {
        self.record(crate::actions::InputEvent::GamepadMotion(pad, motion));
        self.controllers.motion[pad.slot()] = Some(motion);
    }

    /// The axis value `pad` had when this frame started, for "pushed past
    /// halfway" device tracking.
    pub(super) fn axis_before(&self, pad: Gamepad, axis: GamepadAxis) -> f32 {
        self.pads[pad.slot()].axes[axis as usize]
    }
}

fn slots_set(set: &[bool; MAX_GAMEPADS]) -> impl Iterator<Item = Gamepad> + '_ {
    set.iter()
        .enumerate()
        .filter(|&(_, &on)| on)
        .map(|(slot, _)| Gamepad(slot as u8))
}
