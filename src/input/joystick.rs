//! Steering wheels, pedals, flight sticks and other joysticks: devices read
//! by axis, button and hat number rather than as a standard gamepad, and the
//! force feedback they can play.
//!
//! A backend reports them through
//! [`GamepadEvents::joystick_connected`](crate::input::GamepadEvents::joystick_connected)
//! and the other `joystick_*` events; the SDL backend (feature
//! `gamepad-sdl`) does for wheels, flight sticks, throttles and anything SDL
//! can't read as a gamepad. Each gets a [`Joystick`] slot, kept for the same
//! device when it comes back, like a gamepad's.

use glam::IVec2;

use super::ButtonStates;
use super::Input;

/// How many joysticks can be connected at once.
pub const MAX_JOYSTICKS: usize = 8;
/// The axes read on each joystick; ones past it are ignored.
pub const MAX_JOYSTICK_AXES: usize = 16;
/// The buttons read on each joystick; ones past it are ignored.
pub const MAX_JOYSTICK_BUTTONS: usize = 64;
/// The hats (eight-way switches) read on each joystick; ones past it are ignored.
pub const MAX_JOYSTICK_HATS: usize = 4;

/// One connected joystick, by slot: [`Input::joysticks`].
///
/// ```
/// let j = ponko::input::Joystick::from_slot(0).unwrap();
/// assert_eq!(j.slot(), 0);
/// assert_eq!(ponko::input::Joystick::from_slot(ponko::input::MAX_JOYSTICKS), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Joystick(pub(crate) u8);

impl Joystick {
    /// Its slot, `0..MAX_JOYSTICKS`.
    #[must_use]
    pub fn slot(self) -> usize {
        usize::from(self.0)
    }

    /// The joystick in `slot`, or `None` past [`MAX_JOYSTICKS`].
    #[must_use]
    pub fn from_slot(slot: usize) -> Option<Joystick> {
        (slot < MAX_JOYSTICKS).then_some(Joystick(slot as u8))
    }
}

/// What kind of device a joystick is, where its backend can tell, so a
/// game can choose a layout: steering on axis 0 for a wheel, say.
///
/// ```
/// let info = ponko::input::JoystickInfo::new("Racing Wheel").kind(ponko::input::JoystickKind::Wheel);
/// assert_eq!(info.kind, ponko::input::JoystickKind::Wheel);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum JoystickKind {
    /// The backend can't tell.
    #[default]
    Unknown,
    /// A steering wheel, often with pedals and a shifter.
    Wheel,
    /// A flight stick.
    FlightStick,
    /// A throttle quadrant.
    Throttle,
    /// An arcade stick.
    ArcadeStick,
    /// An arcade button pad.
    ArcadePad,
    /// A dance mat.
    DancePad,
    /// A guitar controller.
    Guitar,
    /// A drum kit.
    DrumKit,
    /// A gamepad read as a joystick (one the backend has no gamepad layout for).
    Gamepad,
}

/// Which force-feedback effects a joystick can play, as its backend
/// reports them. Effects it can't play are ignored.
///
/// ```
/// let s = ponko::input::ForceFeedbackSupport { spring: true, ..Default::default() };
/// assert!(s.any());
/// assert!(!ponko::input::ForceFeedbackSupport::default().any());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ForceFeedbackSupport {
    /// [`ForceFeedback::constant`].
    pub constant: bool,
    /// [`ForceFeedback::spring`].
    pub spring: bool,
    /// [`ForceFeedback::damper`].
    pub damper: bool,
    /// [`ForceFeedback::friction`].
    pub friction: bool,
    /// [`ForceFeedback::vibration`].
    pub vibration: bool,
    /// [`ForceFeedback::autocenter`].
    pub autocenter: bool,
}

impl ForceFeedbackSupport {
    /// Whether it can play any effect.
    #[must_use]
    pub fn any(&self) -> bool {
        self.constant || self.spring || self.damper || self.friction || self.vibration || self.autocenter
    }
}

/// One connected joystick, as its backend described it when it connected:
/// [`Input::joystick_info`].
///
/// ```
/// use ponko::input::{JoystickInfo, JoystickKind};
/// let info = JoystickInfo::new("Racing Wheel").kind(JoystickKind::Wheel).ids(0x046d, 0xc24f).layout(4, 25, 1);
/// assert_eq!((info.axes, info.buttons, info.hats), (4, 25, 1));
/// assert_eq!(info.vendor_id, Some(0x046d));
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct JoystickInfo {
    /// The name the system gives it. Can be empty.
    pub name: String,
    /// What kind of device it is.
    pub kind: JoystickKind,
    /// USB vendor id, where the backend reports one.
    pub vendor_id: Option<u16>,
    /// USB product id, where the backend reports one.
    pub product_id: Option<u16>,
    /// How many axes it has (only the first [`MAX_JOYSTICK_AXES`] are read).
    pub axes: usize,
    /// How many buttons it has (only the first [`MAX_JOYSTICK_BUTTONS`] are read).
    pub buttons: usize,
    /// How many hats it has (only the first [`MAX_JOYSTICK_HATS`] are read).
    pub hats: usize,
    /// The force-feedback effects it can play.
    pub force_feedback: ForceFeedbackSupport,
}

impl JoystickInfo {
    /// A joystick called `name`, of unknown kind, with no ids, inputs or effects.
    ///
    /// ```
    /// let info = ponko::input::JoystickInfo::new("Stick");
    /// assert_eq!(info.name, "Stick");
    /// ```
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Sets its kind.
    ///
    /// ```
    /// use ponko::input::{JoystickInfo, JoystickKind};
    /// assert_eq!(JoystickInfo::new("T").kind(JoystickKind::Throttle).kind, JoystickKind::Throttle);
    /// ```
    #[must_use]
    pub fn kind(mut self, kind: JoystickKind) -> Self {
        self.kind = kind;
        self
    }

    /// Sets the USB vendor and product ids.
    ///
    /// ```
    /// let info = ponko::input::JoystickInfo::new("W").ids(1, 2);
    /// assert_eq!((info.vendor_id, info.product_id), (Some(1), Some(2)));
    /// ```
    #[must_use]
    pub fn ids(mut self, vendor_id: u16, product_id: u16) -> Self {
        self.vendor_id = Some(vendor_id);
        self.product_id = Some(product_id);
        self
    }

    /// Sets how many axes, buttons and hats it has.
    ///
    /// ```
    /// let info = ponko::input::JoystickInfo::new("W").layout(3, 12, 1);
    /// assert_eq!(info.buttons, 12);
    /// ```
    #[must_use]
    pub fn layout(mut self, axes: usize, buttons: usize, hats: usize) -> Self {
        self.axes = axes;
        self.buttons = buttons;
        self.hats = hats;
        self
    }

    /// Sets the force-feedback effects it can play.
    ///
    /// ```
    /// use ponko::input::{ForceFeedbackSupport, JoystickInfo};
    /// let info = JoystickInfo::new("W").force_feedback(ForceFeedbackSupport { constant: true, ..Default::default() });
    /// assert!(info.force_feedback.constant);
    /// ```
    #[must_use]
    pub fn force_feedback(mut self, support: ForceFeedbackSupport) -> Self {
        self.force_feedback = support;
        self
    }
}

/// A vibration through the force-feedback motor: a rough road, an engine.
///
/// ```
/// let v = ponko::input::Vibration { strength: 0.3, hz: 40.0 };
/// assert!(!v.is_off());
/// assert!(ponko::input::Vibration::default().is_off());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vibration {
    /// How strong, `0..=1`.
    pub strength: f32,
    /// How fast, in shakes per second.
    pub hz: f32,
}

impl Vibration {
    /// Whether it plays nothing.
    #[must_use]
    pub fn is_off(&self) -> bool {
        !(self.strength > 0.0 && self.hz > 0.0)
    }
}

/// The force-feedback effects a joystick plays, all at once, until changed:
/// set with [`Input::set_force_feedback`]. On a wheel, positive is right.
/// Strengths are `0..=1` (clamped), and an effect the device can't play
/// ([`JoystickInfo::force_feedback`]) is left out.
///
/// ```
/// use ponko::input::ForceFeedback;
/// // A car at speed: the wheel pulls back to the middle, feels heavy, and
/// // the road shakes it a little.
/// let ff = ForceFeedback { spring: 0.6, damper: 0.2, ..ForceFeedback::OFF }
///     .vibration(0.1, 30.0);
/// assert!(!ff.is_off());
/// assert!(ForceFeedback::OFF.is_off());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ForceFeedback {
    /// A steady push, `-1..=1`: left (or forward) to right (or back). A car
    /// sliding, the wind.
    pub constant: f32,
    /// A spring pulling toward [`spring_center`](Self::spring_center): how
    /// strongly.
    pub spring: f32,
    /// Where the spring pulls to, `-1..=1` (0 is the middle).
    pub spring_center: f32,
    /// Resistance that grows with how fast it turns: weight.
    pub damper: f32,
    /// Resistance to any turning: grip.
    pub friction: f32,
    /// A vibration.
    pub vibration: Vibration,
    /// The device's own centring spring, which most wheels have built in:
    /// how strongly. Off while the game plays its own `spring`.
    pub autocenter: f32,
}

impl ForceFeedback {
    /// Nothing plays.
    pub const OFF: ForceFeedback = ForceFeedback {
        constant: 0.0,
        spring: 0.0,
        spring_center: 0.0,
        damper: 0.0,
        friction: 0.0,
        vibration: Vibration { strength: 0.0, hz: 0.0 },
        autocenter: 0.0,
    };

    /// Whether nothing plays.
    #[must_use]
    pub fn is_off(&self) -> bool {
        self.clamped() == Self::OFF
    }

    /// With a vibration of `strength` (`0..=1`) at `hz` shakes a second.
    ///
    /// ```
    /// let ff = ponko::input::ForceFeedback::OFF.vibration(0.5, 20.0);
    /// assert_eq!(ff.vibration.hz, 20.0);
    /// ```
    #[must_use]
    pub fn vibration(mut self, strength: f32, hz: f32) -> Self {
        self.vibration = Vibration { strength, hz };
        self
    }

    /// Every field in its range, NaN as 0, and a vibration that plays
    /// nothing as none.
    ///
    /// ```
    /// let ff = ponko::input::ForceFeedback { constant: 3.0, spring: -1.0, ..Default::default() }.clamped();
    /// assert_eq!((ff.constant, ff.spring), (1.0, 0.0));
    /// ```
    #[must_use]
    pub fn clamped(&self) -> Self {
        let unit = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
        let signed = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(-1.0, 1.0) };
        let vibration = Vibration {
            strength: unit(self.vibration.strength),
            hz: if self.vibration.hz.is_nan() {
                0.0
            } else {
                self.vibration.hz.max(0.0)
            },
        };
        Self {
            constant: signed(self.constant),
            spring: unit(self.spring),
            spring_center: signed(self.spring_center),
            damper: unit(self.damper),
            friction: unit(self.friction),
            vibration: if vibration.is_off() {
                Vibration::default()
            } else {
                vibration
            },
            autocenter: unit(self.autocenter),
        }
    }
}

/// One joystick's buttons, axes and hats.
#[derive(Debug, Clone)]
pub(crate) struct JoystickState {
    buttons: ButtonStates<MAX_JOYSTICK_BUTTONS>,
    axes: [f32; MAX_JOYSTICK_AXES],
    hats: [IVec2; MAX_JOYSTICK_HATS],
}

impl JoystickState {
    const fn new() -> Self {
        Self {
            buttons: ButtonStates::new(),
            axes: [0.0; MAX_JOYSTICK_AXES],
            hats: [IVec2::ZERO; MAX_JOYSTICK_HATS],
        }
    }
}

/// Every joystick's state, what each is, and the force feedback the game asked for.
#[derive(Debug, Clone)]
pub(crate) struct Joysticks {
    states: [JoystickState; MAX_JOYSTICKS],
    infos: [Option<JoystickInfo>; MAX_JOYSTICKS],
    /// Connected joysticks, sorted by slot.
    connected: Vec<Joystick>,
    pub(crate) force_feedback: [ForceFeedback; MAX_JOYSTICKS],
}

impl Default for Joysticks {
    fn default() -> Self {
        Self {
            states: [const { JoystickState::new() }; MAX_JOYSTICKS],
            infos: [const { None }; MAX_JOYSTICKS],
            connected: Vec::with_capacity(MAX_JOYSTICKS),
            force_feedback: [ForceFeedback::OFF; MAX_JOYSTICKS],
        }
    }
}

impl Joysticks {
    pub(crate) fn end_frame(&mut self) {
        for state in &mut self.states {
            state.buttons.end_frame();
        }
    }
}

/// A hat's position as SDL and most systems report it (bits: up 1, right
/// 2, down 4, left 8) as `x` left -1 to right 1 and `y` up -1 to down 1.
///
/// ```
/// use glam::IVec2;
/// assert_eq!(ponko::input::hat_from_bits(0b0011), IVec2::new(1, -1)); // up and right
/// assert_eq!(ponko::input::hat_from_bits(0), IVec2::ZERO);
/// ```
#[must_use]
pub fn hat_from_bits(bits: u8) -> IVec2 {
    let axis = |neg: u8, pos: u8| i32::from(bits & pos != 0) - i32::from(bits & neg != 0);
    IVec2::new(axis(8, 2), axis(1, 4))
}

impl Input {
    pub(crate) fn connect_joystick_at(&mut self, joystick: Joystick, info: JoystickInfo) {
        let j = &mut self.joysticks;
        j.infos[joystick.slot()] = Some(info);
        if let Err(at) = j.connected.binary_search(&joystick) {
            j.connected.insert(at, joystick);
        }
    }

    /// Unplugged: its held buttons read as released this frame, its axes
    /// and hats rest, and its force feedback is forgotten.
    pub(crate) fn disconnect_joystick(&mut self, joystick: Joystick) {
        let j = &mut self.joysticks;
        j.connected.retain(|&c| c != joystick);
        let state = &mut j.states[joystick.slot()];
        state.buttons.release_all();
        state.axes = [0.0; MAX_JOYSTICK_AXES];
        state.hats = [IVec2::ZERO; MAX_JOYSTICK_HATS];
        j.force_feedback[joystick.slot()] = ForceFeedback::OFF;
    }

    pub(crate) fn set_joystick_axis(&mut self, joystick: Joystick, axis: usize, value: f32) {
        if let Some(a) = self.joysticks.states[joystick.slot()].axes.get_mut(axis) {
            *a = if value.is_nan() { 0.0 } else { value.clamp(-1.0, 1.0) };
        }
    }

    pub(crate) fn set_joystick_button(&mut self, joystick: Joystick, button: usize, down: bool) {
        if button < MAX_JOYSTICK_BUTTONS {
            self.joysticks.states[joystick.slot()].buttons.set(button, down);
        }
    }

    pub(crate) fn set_joystick_hat(&mut self, joystick: Joystick, hat: usize, value: IVec2) {
        if let Some(h) = self.joysticks.states[joystick.slot()].hats.get_mut(hat) {
            *h = value.clamp(IVec2::NEG_ONE, IVec2::ONE);
        }
    }

    fn joystick_state(&self, joystick: Joystick) -> Option<&JoystickState> {
        self.joysticks
            .connected
            .contains(&joystick)
            .then(|| &self.joysticks.states[joystick.slot()])
    }

    /// The joysticks connected now, by slot: wheels, flight sticks and other
    /// devices that aren't read as gamepads.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::input::{FakeGamepads, JoystickInfo, JoystickKind};
    /// let fake = FakeGamepads::new();
    /// fake.connect_joystick(JoystickInfo::new("Wheel").kind(JoystickKind::Wheel).layout(3, 12, 1));
    /// Game::new("t", 1, 1).gamepad_backend(fake).run_headless(1, |ctx| {
    ///     let wheel = ctx.input.joysticks()[0];
    ///     assert_eq!(ctx.input.joystick_info(wheel).unwrap().kind, JoystickKind::Wheel);
    /// });
    /// ```
    #[must_use]
    pub fn joysticks(&self) -> &[Joystick] {
        &self.joysticks.connected
    }

    /// What `joystick` is, while it is connected.
    #[must_use]
    pub fn joystick_info(&self, joystick: Joystick) -> Option<&JoystickInfo> {
        self.joystick_state(joystick)?;
        self.joysticks.infos[joystick.slot()].as_ref()
    }

    /// Axis number `axis` of `joystick`, `-1..=1` as the device reports it
    /// (a pedal may rest at -1 and read 1 pressed). 0 for an axis it
    /// doesn't have, or when it isn't connected. No dead zone is taken out.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::input::{FakeGamepads, JoystickInfo};
    /// let fake = FakeGamepads::new();
    /// let device = fake.connect_joystick(JoystickInfo::new("Wheel").layout(3, 0, 0));
    /// fake.set_joystick_axis(device, 0, -0.25);
    /// Game::new("t", 1, 1).gamepad_backend(fake).run_headless(1, |ctx| {
    ///     let wheel = ctx.input.joysticks()[0];
    ///     assert_eq!(ctx.input.joystick_axis(wheel, 0), -0.25);
    ///     assert_eq!(ctx.input.joystick_axis(wheel, 99), 0.0);
    /// });
    /// ```
    #[must_use]
    pub fn joystick_axis(&self, joystick: Joystick, axis: usize) -> f32 {
        self.joystick_state(joystick)
            .and_then(|s| s.axes.get(axis).copied())
            .unwrap_or(0.0)
    }

    /// Whether button number `button` of `joystick` is held.
    #[must_use]
    pub fn joystick_down(&self, joystick: Joystick, button: usize) -> bool {
        self.joystick_state(joystick)
            .is_some_and(|s| button < MAX_JOYSTICK_BUTTONS && s.buttons.down[button])
    }

    /// Whether button number `button` of `joystick` went down this frame.
    #[must_use]
    pub fn joystick_pressed(&self, joystick: Joystick, button: usize) -> bool {
        self.joystick_state(joystick)
            .is_some_and(|s| button < MAX_JOYSTICK_BUTTONS && s.buttons.pressed[button])
    }

    /// Whether button number `button` of `joystick` came up this frame.
    #[must_use]
    pub fn joystick_released(&self, joystick: Joystick, button: usize) -> bool {
        self.joystick_state(joystick)
            .is_some_and(|s| button < MAX_JOYSTICK_BUTTONS && s.buttons.released[button])
    }

    /// Hat number `hat` of `joystick`: `x` left -1 to right 1, `y` up -1 to
    /// down 1 (a diagonal sets both). Zero when centred, missing or not connected.
    #[must_use]
    pub fn joystick_hat(&self, joystick: Joystick, hat: usize) -> IVec2 {
        self.joystick_state(joystick)
            .and_then(|s| s.hats.get(hat).copied())
            .unwrap_or(IVec2::ZERO)
    }

    /// Plays `effects` on `joystick` until changed ([`ForceFeedback::OFF`]
    /// stops them): the backend gets them at the start of the next frame,
    /// only when they changed. Ignored for a joystick that isn't connected;
    /// forgotten when it disconnects.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::input::{FakeGamepads, ForceFeedback, ForceFeedbackSupport, JoystickInfo};
    /// let fake = FakeGamepads::new();
    /// let support = ForceFeedbackSupport { spring: true, ..Default::default() };
    /// let device = fake.connect_joystick(JoystickInfo::new("Wheel").force_feedback(support));
    /// let played = fake.clone();
    /// Game::new("t", 1, 1)
    ///     .gamepad_backend(fake)
    ///     .sim_system(|ctx: &mut Ctx| {
    ///         let wheel = ctx.input.joysticks()[0];
    ///         ctx.input.set_force_feedback(wheel, ForceFeedback { spring: 0.5, ..ForceFeedback::OFF });
    ///     })
    ///     .run_headless(2, |_| {});
    /// assert_eq!(played.force_feedback(device).spring, 0.5);
    /// assert_eq!(played.force_feedback_changes(device), 1, "sent once, not every frame");
    /// ```
    pub fn set_force_feedback(&mut self, joystick: Joystick, effects: ForceFeedback) {
        if self.joystick_state(joystick).is_some() {
            self.joysticks.force_feedback[joystick.slot()] = effects.clamped();
        }
    }

    /// The force feedback the game asked `joystick` to play (clamped).
    #[must_use]
    pub fn force_feedback(&self, joystick: Joystick) -> ForceFeedback {
        self.joysticks.force_feedback[joystick.slot()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hats_read_every_direction_and_opposites_cancel() {
        assert_eq!(hat_from_bits(1), IVec2::new(0, -1));
        assert_eq!(hat_from_bits(2), IVec2::new(1, 0));
        assert_eq!(hat_from_bits(4), IVec2::new(0, 1));
        assert_eq!(hat_from_bits(8), IVec2::new(-1, 0));
        assert_eq!(hat_from_bits(4 | 8), IVec2::new(-1, 1));
        assert_eq!(hat_from_bits(1 | 4), IVec2::ZERO, "a broken hat reporting both");
    }

    #[test]
    fn force_feedback_is_clamped_and_a_silent_vibration_is_none() {
        let ff = ForceFeedback {
            constant: f32::NAN,
            spring_center: -2.0,
            damper: 7.0,
            vibration: Vibration { strength: 1.0, hz: 0.0 },
            ..ForceFeedback::OFF
        }
        .clamped();
        assert_eq!(ff.constant, 0.0);
        assert_eq!(ff.spring_center, -1.0);
        assert_eq!(ff.damper, 1.0);
        assert_eq!(ff.vibration, Vibration::default());
        assert!(ForceFeedback::OFF.vibration(0.0, 50.0).is_off());
    }

    #[test]
    fn a_disconnected_joystick_reads_at_rest_and_forgets_its_effects() {
        let mut input = Input::new();
        let j = Joystick(2);
        input.connect_joystick_at(j, JoystickInfo::new("W").layout(2, 4, 1));
        input.set_joystick_axis(j, 1, 0.5);
        input.set_joystick_button(j, 3, true);
        input.set_joystick_hat(j, 0, IVec2::new(1, 0));
        input.set_force_feedback(
            j,
            ForceFeedback {
                damper: 0.5,
                ..ForceFeedback::OFF
            },
        );
        assert!(input.joystick_pressed(j, 3) && input.joystick_down(j, 3));
        input.end_frame();
        assert!(!input.joystick_pressed(j, 3) && input.joystick_down(j, 3));
        input.disconnect_joystick(j);
        assert!(input.joysticks().is_empty());
        assert_eq!(input.joystick_axis(j, 1), 0.0);
        assert!(!input.joystick_down(j, 3));
        assert_eq!(input.joystick_hat(j, 0), IVec2::ZERO);
        assert!(input.force_feedback(j).is_off());
        assert!(input.joystick_info(j).is_none());
        input.set_force_feedback(
            j,
            ForceFeedback {
                damper: 0.5,
                ..ForceFeedback::OFF
            },
        );
        assert!(input.force_feedback(j).is_off(), "ignored while unplugged");
    }

    #[test]
    fn inputs_past_the_limits_are_ignored() {
        let mut input = Input::new();
        let j = Joystick(0);
        input.connect_joystick_at(j, JoystickInfo::new("Big"));
        input.set_joystick_axis(j, MAX_JOYSTICK_AXES, 1.0);
        input.set_joystick_button(j, MAX_JOYSTICK_BUTTONS, true);
        input.set_joystick_hat(j, MAX_JOYSTICK_HATS, IVec2::ONE);
        assert_eq!(input.joystick_axis(j, MAX_JOYSTICK_AXES), 0.0);
        assert!(!input.joystick_down(j, MAX_JOYSTICK_BUTTONS));
        assert_eq!(input.joystick_hat(j, MAX_JOYSTICK_HATS), IVec2::ZERO);
        input.set_joystick_axis(j, 0, 5.0);
        assert_eq!(input.joystick_axis(j, 0), 1.0, "clamped");
    }
}
