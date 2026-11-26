//! Where gamepads come from: a [`GamepadBackend`] reports controllers and
//! their input, and plays rumble. The engine's own backend reads gilrs (the
//! `gamepad` feature, in a window; the browser's gamepads on the web); [`FakeGamepads`] is a scripted one for
//! tests; a game can choose another (a platform SDK that reads gyros) with
//! [`Game::gamepad_backend`](crate::game::Game::gamepad_backend).
//!
//! The engine gives each controller a slot ([`Gamepad`]) when it connects:
//! the slot it had before if the same device comes back, otherwise the
//! lowest slot that no recently unplugged device is waiting for.

use std::sync::{Arc, Mutex, MutexGuard};

use glam::IVec2;

use super::controller::{GamepadInfo, GamepadMotion, MotorLevels};
use super::gamepad_extras::{GamepadOutput, GamepadTouchpadFinger, TriggerEffect};
use super::joystick::{ForceFeedback, Joystick, JoystickInfo, MAX_JOYSTICKS};
use super::{Gamepad, GamepadAxis, GamepadButton, Input, MAX_GAMEPADS};
use crate::ctx::Ctx;

/// The dead zone of sticks and triggers, as a share of their travel:
/// gilrs's default, so the web's and SDL's pads read as gilrs's do.
#[cfg_attr(not(any(test, feature = "gamepad-sdl", target_arch = "wasm32")), allow(dead_code))]
pub(crate) const DEAD_ZONE: f32 = 0.1;

/// A stick's `(x, y)` with the dead zone taken out as gilrs does: nothing
/// inside a circle of [`DEAD_ZONE`], and the rest rescaled along the
/// stick's direction so the edge still reads 1.
#[cfg_attr(not(any(test, feature = "gamepad-sdl", target_arch = "wasm32")), allow(dead_code))]
pub(crate) fn stick_dead_zone(x: f32, y: f32) -> (f32, f32) {
    let magnitude = (x * x + y * y).sqrt().clamp(0.0, 1.0);
    if magnitude <= DEAD_ZONE {
        (0.0, 0.0)
    } else {
        let scale = (magnitude - DEAD_ZONE) / (1.0 - DEAD_ZONE) / magnitude;
        (x * scale, y * scale)
    }
}

/// A trigger's `0..=1` with the dead zone taken out, rescaled so full still reads 1.
#[cfg_attr(not(any(test, feature = "gamepad-sdl", target_arch = "wasm32")), allow(dead_code))]
pub(crate) fn trigger_dead_zone(value: f32) -> f32 {
    stick_dead_zone(value, 0.0).0
}

/// A backend's own number for one physical controller. Report the same
/// number when the same device comes back, and it gets its old slot (and
/// player) again.
///
/// ```
/// let a = ponko::input::DeviceKey(7);
/// assert_eq!(a, ponko::input::DeviceKey(7));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceKey(pub u64);

/// A source of gamepads: choose one with
/// [`Game::gamepad_backend`](crate::game::Game::gamepad_backend).
///
/// The engine calls [`poll`](Self::poll) once at the start of every frame,
/// before the systems, and [`set_rumble`](Self::set_rumble) when a
/// controller's motor levels change (at most once per controller per
/// frame). Both run on the game's thread.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::input::{DeviceKey, GamepadBackend, GamepadEvents, GamepadInfo, MotorLevels};
///
/// /// One pad that is always there and never presses anything.
/// struct Still { said_hello: bool }
/// impl GamepadBackend for Still {
///     fn poll(&mut self, events: &mut GamepadEvents<'_>) {
///         if !self.said_hello {
///             events.connected(DeviceKey(1), GamepadInfo::new("Still Pad"));
///             self.said_hello = true;
///         }
///     }
///     fn set_rumble(&mut self, _device: DeviceKey, _levels: MotorLevels) {}
/// }
///
/// Game::new("t", 1, 1).gamepad_backend(Still { said_hello: false }).run_headless(1, |ctx| {
///     assert_eq!(ctx.input.gamepads().len(), 1, "polled once before setup");
/// });
/// ```
pub trait GamepadBackend: Send + 'static {
    /// Reports what happened since the last call: connections,
    /// disconnections, buttons, axes, motion.
    fn poll(&mut self, events: &mut GamepadEvents<'_>);

    /// Plays these motor levels on `device` until told otherwise
    /// ([`MotorLevels::OFF`] stops). Motors the device or backend lacks are
    /// ignored.
    fn set_rumble(&mut self, device: DeviceKey, levels: MotorLevels);

    /// Plays these force-feedback effects on joystick `device` until told
    /// otherwise ([`ForceFeedback::OFF`] stops them), sent when they change
    /// (at most once per joystick per frame), already clamped. Effects the
    /// device or backend can't play are ignored; by default, all of them.
    fn set_force_feedback(&mut self, device: DeviceKey, effects: ForceFeedback) {
        let _ = (device, effects);
    }

    /// Sets the light bar's RGB channels. Black switches the light off.
    fn set_light(&mut self, device: DeviceKey, rgb: [u8; 3]) {
        let _ = (device, rgb);
    }

    /// Sets the zero-based player indicator, or switches the LEDs off.
    fn set_player_leds(&mut self, device: DeviceKey, player: Option<u8>) {
        let _ = (device, player);
    }

    /// Sets the left and right adaptive trigger effects together.
    fn set_trigger_effects(&mut self, device: DeviceKey, effects: [TriggerEffect; 2]) {
        let _ = (device, effects);
    }
}

/// What a [`GamepadBackend`] reports during [`poll`](GamepadBackend::poll).
/// Events for a device that isn't connected are ignored.
pub struct GamepadEvents<'a> {
    input: &'a mut Input,
    slots: &'a mut Slots<MAX_GAMEPADS>,
    sent_output: &'a mut [GamepadOutput; MAX_GAMEPADS],
    joystick_slots: &'a mut Slots<MAX_JOYSTICKS>,
}

impl GamepadEvents<'_> {
    /// `device` connected: it takes a slot (its old one if it was here
    /// before) and its info is kept for [`Input::gamepad_info`]. Returns the
    /// slot, or `None` when all [`MAX_GAMEPADS`] are taken. Reporting a
    /// connected device again only updates its info.
    pub fn connected(&mut self, device: DeviceKey, info: GamepadInfo) -> Option<Gamepad> {
        let slot = match self.slots.slot_of(device) {
            Some(slot) => slot,
            None => self.slots.take(device)?,
        };
        let pad = Gamepad(slot as u8);
        self.input.connect_gamepad_at(pad, info);
        Some(pad)
    }

    /// `device` disconnected: its held buttons read as released this frame,
    /// and its slot waits for it to come back.
    pub fn disconnected(&mut self, device: DeviceKey) {
        if let Some(slot) = self.slots.release(device) {
            self.sent_output[slot] = GamepadOutput::default();
            self.input.disconnect_gamepad(Gamepad(slot as u8));
        }
    }

    /// A button of `device` went down (`true`) or up.
    pub fn button(&mut self, device: DeviceKey, button: GamepadButton, down: bool) {
        if let Some(pad) = self.pad(device) {
            self.input.set_gamepad_button(pad, button, down);
        }
    }

    /// An axis of `device` moved: sticks `-1..=1` with y down, triggers `0..=1`.
    pub fn axis(&mut self, device: DeviceKey, axis: GamepadAxis, value: f32) {
        if let Some(pad) = self.pad(device) {
            self.input.set_gamepad_axis(pad, axis, value);
        }
    }

    /// A new gyro and accelerometer reading of `device`.
    pub fn motion(&mut self, device: DeviceKey, motion: GamepadMotion) {
        if let Some(pad) = self.pad(device) {
            self.input.set_gamepad_motion(pad, motion);
        }
    }

    /// A contact changed on the first touch surface of `device`.
    pub fn touchpad_finger(&mut self, device: DeviceKey, finger: u8, contact: GamepadTouchpadFinger) {
        if let Some(pad) = self.pad(device) {
            self.input.set_gamepad_touchpad_finger(pad, finger, contact);
        }
    }

    fn pad(&self, device: DeviceKey) -> Option<Gamepad> {
        self.slots.slot_of(device).map(|s| Gamepad(s as u8))
    }

    fn joystick(&self, device: DeviceKey) -> Option<Joystick> {
        self.joystick_slots.slot_of(device).map(|s| Joystick(s as u8))
    }

    /// Joystick `device` connected (a wheel, a flight stick, anything read
    /// by axis and button number): it takes a joystick slot, its old one if
    /// it was here before. Returns the slot, or `None` when all
    /// [`MAX_JOYSTICKS`] are taken. Reporting it again only updates its info.
    /// Joysticks have their own slots and keys, apart from gamepads'.
    pub fn joystick_connected(&mut self, device: DeviceKey, info: JoystickInfo) -> Option<Joystick> {
        let slot = match self.joystick_slots.slot_of(device) {
            Some(slot) => slot,
            None => self.joystick_slots.take(device)?,
        };
        let joystick = Joystick(slot as u8);
        self.input.connect_joystick_at(joystick, info);
        Some(joystick)
    }

    /// Joystick `device` disconnected: its held buttons read as released
    /// this frame, and its slot waits for it to come back.
    pub fn joystick_disconnected(&mut self, device: DeviceKey) {
        if let Some(slot) = self.joystick_slots.release(device) {
            self.input.disconnect_joystick(Joystick(slot as u8));
        }
    }

    /// Axis number `axis` of joystick `device` moved to `value`, `-1..=1`.
    pub fn joystick_axis(&mut self, device: DeviceKey, axis: usize, value: f32) {
        if let Some(j) = self.joystick(device) {
            self.input.set_joystick_axis(j, axis, value);
        }
    }

    /// Button number `button` of joystick `device` went down (`true`) or up.
    pub fn joystick_button(&mut self, device: DeviceKey, button: usize, down: bool) {
        if let Some(j) = self.joystick(device) {
            self.input.set_joystick_button(j, button, down);
        }
    }

    /// Hat number `hat` of joystick `device` moved: `x` left -1 to right 1,
    /// `y` up -1 to down 1 ([`hat_from_bits`](crate::input::hat_from_bits)
    /// reads the usual bit form).
    pub fn joystick_hat(&mut self, device: DeviceKey, hat: usize, value: IVec2) {
        if let Some(j) = self.joystick(device) {
            self.input.set_joystick_hat(j, hat, value);
        }
    }
}

/// Which device holds each slot, and which device each empty slot waits
/// for (the one unplugged from it last).
#[derive(Debug)]
struct Slots<const N: usize> {
    held: [Option<DeviceKey>; N],
    waiting: [Option<DeviceKey>; N],
}

impl<const N: usize> Default for Slots<N> {
    fn default() -> Self {
        Self {
            held: [None; N],
            waiting: [None; N],
        }
    }
}

impl<const N: usize> Slots<N> {
    fn slot_of(&self, device: DeviceKey) -> Option<usize> {
        self.held.iter().position(|&d| d == Some(device))
    }

    /// A slot for a device connecting: the one it left, else the lowest free
    /// slot nobody waits for, else the lowest free slot.
    fn take(&mut self, device: DeviceKey) -> Option<usize> {
        let free = |s: usize| self.held[s].is_none();
        let slot = (0..N)
            .find(|&s| free(s) && self.waiting[s] == Some(device))
            .or_else(|| (0..N).find(|&s| free(s) && self.waiting[s].is_none()))
            .or_else(|| (0..N).find(|&s| free(s)))?;
        self.held[slot] = Some(device);
        self.waiting[slot] = None;
        // A device waits for one slot only.
        for w in &mut self.waiting {
            if *w == Some(device) {
                *w = None;
            }
        }
        Some(slot)
    }

    fn release(&mut self, device: DeviceKey) -> Option<usize> {
        let slot = self.slot_of(device)?;
        self.held[slot] = None;
        self.waiting[slot] = Some(device);
        Some(slot)
    }
}

/// The installed backend, its slots and the motor levels last sent: a
/// field of `Ctx`, polled at the start of every frame.
#[derive(Default)]
pub(crate) struct GamepadDriver {
    // In a Mutex so `Ctx` stays `Sync` with a backend that is only `Send`
    // (never contended: only the game's thread touches it).
    backend: Option<Mutex<Box<dyn GamepadBackend>>>,
    slots: Slots<MAX_GAMEPADS>,
    sent: [MotorLevels; MAX_GAMEPADS],
    sent_output: [GamepadOutput; MAX_GAMEPADS],
    joystick_slots: Slots<MAX_JOYSTICKS>,
    /// The force feedback last sent to each joystick.
    sent_force_feedback: [ForceFeedback; MAX_JOYSTICKS],
}

impl std::fmt::Debug for GamepadDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GamepadDriver")
            .field("installed", &self.backend.is_some())
            .field("slots", &self.slots)
            .field("joystick_slots", &self.joystick_slots)
            .finish()
    }
}

impl GamepadDriver {
    pub(crate) fn installed(&self) -> bool {
        self.backend.is_some()
    }

    /// Swaps in `backend`: the old one's pads disconnect (and stop
    /// rumbling), then the new one is polled once.
    pub(crate) fn install(&mut self, input: &mut Input, backend: Box<dyn GamepadBackend>) {
        self.uninstall(input);
        self.backend = Some(Mutex::new(backend));
        self.poll(input);
    }

    fn uninstall(&mut self, input: &mut Input) {
        let Some(old) = self.backend.take() else { return };
        let mut old = old.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
        for slot in 0..MAX_GAMEPADS {
            if let Some(device) = self.slots.held[slot] {
                if !self.sent[slot].is_off() {
                    old.set_rumble(device, MotorLevels::OFF);
                }
                self.sent_output[slot].reset(&mut *old, device);
                input.disconnect_gamepad(Gamepad(slot as u8));
            }
        }
        for slot in 0..MAX_JOYSTICKS {
            if let Some(device) = self.joystick_slots.held[slot] {
                if !self.sent_force_feedback[slot].is_off() {
                    old.set_force_feedback(device, ForceFeedback::OFF);
                }
                input.disconnect_joystick(Joystick(slot as u8));
            }
        }
        self.slots = Slots::default();
        self.sent = [MotorLevels::OFF; MAX_GAMEPADS];
        self.sent_output = Default::default();
        self.joystick_slots = Slots::default();
        self.sent_force_feedback = [ForceFeedback::OFF; MAX_JOYSTICKS];
    }

    /// Once per frame: counts rumble down by `dt` real seconds, sends the
    /// motor levels that changed, then applies the backend's events.
    pub(crate) fn update(&mut self, input: &mut Input, dt: f32) {
        input.controllers.tick_rumble(dt);
        if self.backend.is_none() {
            return;
        }
        self.send_rumble(input);
        self.send_gamepad_output(input);
        self.send_force_feedback(input);
        self.poll(input);
    }

    fn backend(&mut self) -> Option<MutexGuard<'_, Box<dyn GamepadBackend>>> {
        let backend = self.backend.as_ref()?;
        Some(backend.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }

    fn send_rumble(&mut self, input: &Input) {
        for slot in 0..MAX_GAMEPADS {
            let Some(device) = self.slots.held[slot] else {
                self.sent[slot] = MotorLevels::OFF;
                continue;
            };
            let want = input.controllers.rumble[slot].levels;
            if want != self.sent[slot] {
                self.sent[slot] = want;
                if let Some(mut backend) = self.backend() {
                    backend.set_rumble(device, want);
                }
            }
        }
    }

    fn send_gamepad_output(&mut self, input: &Input) {
        for slot in 0..MAX_GAMEPADS {
            let want = input.controllers.output[slot];
            let previous = std::mem::replace(&mut self.sent_output[slot], want);
            if want != previous
                && let Some(device) = self.slots.held[slot]
                && let Some(mut backend) = self.backend()
            {
                want.send_changes(previous, &mut **backend, device);
            }
        }
    }

    fn send_force_feedback(&mut self, input: &Input) {
        for slot in 0..MAX_JOYSTICKS {
            let Some(device) = self.joystick_slots.held[slot] else {
                self.sent_force_feedback[slot] = ForceFeedback::OFF;
                continue;
            };
            let want = input.joysticks.force_feedback[slot];
            if want != self.sent_force_feedback[slot] {
                self.sent_force_feedback[slot] = want;
                if let Some(mut backend) = self.backend() {
                    backend.set_force_feedback(device, want);
                }
            }
        }
    }

    fn poll(&mut self, input: &mut Input) {
        let Some(backend) = self.backend.as_ref() else { return };
        let mut backend = backend.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut events = GamepadEvents {
            input,
            slots: &mut self.slots,
            sent_output: &mut self.sent_output,
            joystick_slots: &mut self.joystick_slots,
        };
        backend.poll(&mut events);
    }
}

impl Ctx {
    /// Installs where gamepads come from, replacing the current backend: its
    /// pads disconnect first, and the new one is polled right away. Games
    /// choose one with [`Game::gamepad_backend`](crate::game::Game::gamepad_backend);
    /// the window installs gilrs (the browser's gamepads on the web) when they didn't.
    #[cfg_attr(not(all(feature = "gamepad", feature = "window")), allow(dead_code))]
    pub(crate) fn set_gamepad_backend(&mut self, backend: impl GamepadBackend) {
        self.gamepad_driver.install(&mut self.input, Box::new(backend));
    }

    /// Whether a gamepad backend is installed.
    #[cfg_attr(not(all(feature = "gamepad", feature = "window")), allow(dead_code))]
    pub(crate) fn has_gamepad_backend(&self) -> bool {
        self.gamepad_driver.installed()
    }

    /// Polls the backend for this frame (called from `begin_frame`).
    pub(crate) fn poll_gamepads(&mut self, dt: f32) {
        self.gamepad_driver.update(&mut self.input, dt);
    }
}

/// A scripted [`GamepadBackend`] for tests: connect pads, press buttons,
/// move sticks and send motion from test code, and read back the rumble the
/// game asked for. A handle: clones share the same pads, so keep one and
/// give the game another with
/// [`Game::gamepad_backend`](crate::game::Game::gamepad_backend).
///
/// What you do to it reaches `ctx.input` at the start of the next frame,
/// the way a real backend's events do.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::input::{FakeGamepads, GamepadButton, GamepadInfo};
/// let fake = FakeGamepads::new();
/// let device = fake.connect(GamepadInfo::new("Pad"));
/// #[derive(Component)]
/// struct Jumped;
/// let world = Game::new("t", 1, 1)
///     .gamepad_backend(fake.clone())
///     .sim_system(move |ctx: &mut Ctx| {
///         if ctx.time.sim_tick() == 0 {
///             fake.press(device, GamepadButton::South);
///         }
///         let pad = ctx.input.gamepads()[0];
///         if ctx.input.gamepad_pressed(pad, GamepadButton::South) {
///             ctx.world.spawn((Jumped,));
///         }
///     })
///     .run_headless(3, |_| {});
/// assert_eq!(world.len(), 1, "pressed once, on the frame after the script");
/// ```
#[derive(Debug, Clone, Default)]
pub struct FakeGamepads {
    state: Arc<Mutex<FakeState>>,
}

#[derive(Debug, Default)]
struct FakeState {
    next_key: u64,
    queue: Vec<FakeEvent>,
    devices: Vec<FakeDevice>,
}

#[derive(Debug)]
struct FakeDevice {
    key: DeviceKey,
    kind: FakeKind,
    rumble: MotorLevels,
    rumble_changes: usize,
    force_feedback: ForceFeedback,
    force_feedback_changes: usize,
}

#[derive(Debug, Clone)]
enum FakeKind {
    Pad(GamepadInfo),
    Joystick(JoystickInfo),
}

#[derive(Debug)]
enum FakeEvent {
    Connected(DeviceKey, FakeKind),
    /// The device, and whether it is a joystick.
    Disconnected(DeviceKey, bool),
    Button(DeviceKey, GamepadButton, bool),
    Axis(DeviceKey, GamepadAxis, f32),
    Motion(DeviceKey, GamepadMotion),
    JoystickAxis(DeviceKey, usize, f32),
    JoystickButton(DeviceKey, usize, bool),
    JoystickHat(DeviceKey, usize, IVec2),
}

impl FakeGamepads {
    /// No pads yet.
    ///
    /// ```
    /// let fake = ponko::input::FakeGamepads::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Plugs in a new pad described by `info`; returns its key.
    ///
    /// ```
    /// let fake = ponko::input::FakeGamepads::new();
    /// let a = fake.connect(ponko::input::GamepadInfo::new("A"));
    /// let b = fake.connect(ponko::input::GamepadInfo::new("B"));
    /// assert_ne!(a, b);
    /// ```
    pub fn connect(&self, info: GamepadInfo) -> DeviceKey {
        self.plug(FakeKind::Pad(info))
    }

    /// Plugs in a new joystick (a wheel, a flight stick) described by
    /// `info`; returns its key. [`disconnect`](Self::disconnect) and
    /// [`reconnect`](Self::reconnect) work on it as on a pad.
    ///
    /// ```
    /// use ponko::input::{FakeGamepads, JoystickInfo, JoystickKind};
    /// let fake = FakeGamepads::new();
    /// let wheel = fake.connect_joystick(JoystickInfo::new("Wheel").kind(JoystickKind::Wheel));
    /// ```
    pub fn connect_joystick(&self, info: JoystickInfo) -> DeviceKey {
        self.plug(FakeKind::Joystick(info))
    }

    fn plug(&self, kind: FakeKind) -> DeviceKey {
        let mut state = self.state();
        let key = DeviceKey(state.next_key);
        state.next_key += 1;
        state.devices.push(FakeDevice {
            key,
            kind: kind.clone(),
            rumble: MotorLevels::OFF,
            rumble_changes: 0,
            force_feedback: ForceFeedback::OFF,
            force_feedback_changes: 0,
        });
        state.queue.push(FakeEvent::Connected(key, kind));
        key
    }

    /// Plugs `device` back in after [`disconnect`](Self::disconnect), with
    /// the info it had.
    ///
    /// ```
    /// let fake = ponko::input::FakeGamepads::new();
    /// let pad = fake.connect(ponko::input::GamepadInfo::new("A"));
    /// fake.disconnect(pad);
    /// fake.reconnect(pad);
    /// ```
    pub fn reconnect(&self, device: DeviceKey) {
        let mut state = self.state();
        if let Some(kind) = state.devices.iter().find(|d| d.key == device).map(|d| d.kind.clone()) {
            state.queue.push(FakeEvent::Connected(device, kind));
        }
    }

    /// Unplugs `device`.
    ///
    /// ```
    /// let fake = ponko::input::FakeGamepads::new();
    /// let pad = fake.connect(ponko::input::GamepadInfo::new("A"));
    /// fake.disconnect(pad);
    /// ```
    pub fn disconnect(&self, device: DeviceKey) {
        let mut state = self.state();
        let joystick = state
            .devices
            .iter()
            .any(|d| d.key == device && matches!(d.kind, FakeKind::Joystick(_)));
        state.queue.push(FakeEvent::Disconnected(device, joystick));
    }

    /// Presses `button` on `device` (held until [`release`](Self::release)).
    ///
    /// ```
    /// # let fake = ponko::input::FakeGamepads::new();
    /// # let pad = fake.connect(ponko::input::GamepadInfo::new("A"));
    /// fake.press(pad, ponko::input::GamepadButton::Start);
    /// ```
    pub fn press(&self, device: DeviceKey, button: GamepadButton) {
        self.state().queue.push(FakeEvent::Button(device, button, true));
    }

    /// Releases `button` on `device`.
    ///
    /// ```
    /// # let fake = ponko::input::FakeGamepads::new();
    /// # let pad = fake.connect(ponko::input::GamepadInfo::new("A"));
    /// fake.release(pad, ponko::input::GamepadButton::Start);
    /// ```
    pub fn release(&self, device: DeviceKey, button: GamepadButton) {
        self.state().queue.push(FakeEvent::Button(device, button, false));
    }

    /// Moves an axis of `device` to `value`.
    ///
    /// ```
    /// # let fake = ponko::input::FakeGamepads::new();
    /// # let pad = fake.connect(ponko::input::GamepadInfo::new("A"));
    /// fake.set_axis(pad, ponko::input::GamepadAxis::LeftStickX, -1.0);
    /// ```
    pub fn set_axis(&self, device: DeviceKey, axis: GamepadAxis, value: f32) {
        self.state().queue.push(FakeEvent::Axis(device, axis, value));
    }

    /// Sends a gyro and accelerometer reading from `device`.
    ///
    /// ```
    /// # let fake = ponko::input::FakeGamepads::new();
    /// # let pad = fake.connect(ponko::input::GamepadInfo::new("A"));
    /// fake.set_motion(pad, ponko::input::GamepadMotion::default());
    /// ```
    pub fn set_motion(&self, device: DeviceKey, motion: GamepadMotion) {
        self.state().queue.push(FakeEvent::Motion(device, motion));
    }

    /// Moves axis number `axis` of joystick `device` to `value`.
    ///
    /// ```
    /// # let fake = ponko::input::FakeGamepads::new();
    /// # let wheel = fake.connect_joystick(ponko::input::JoystickInfo::new("W"));
    /// fake.set_joystick_axis(wheel, 0, 0.5); // steered half right
    /// ```
    pub fn set_joystick_axis(&self, device: DeviceKey, axis: usize, value: f32) {
        self.state().queue.push(FakeEvent::JoystickAxis(device, axis, value));
    }

    /// Presses (`true`) or releases button number `button` of joystick `device`.
    ///
    /// ```
    /// # let fake = ponko::input::FakeGamepads::new();
    /// # let wheel = fake.connect_joystick(ponko::input::JoystickInfo::new("W"));
    /// fake.set_joystick_button(wheel, 4, true);
    /// ```
    pub fn set_joystick_button(&self, device: DeviceKey, button: usize, down: bool) {
        self.state().queue.push(FakeEvent::JoystickButton(device, button, down));
    }

    /// Moves hat number `hat` of joystick `device` (`x` left -1 to right 1, `y` up -1 to down 1).
    ///
    /// ```
    /// # let fake = ponko::input::FakeGamepads::new();
    /// # let stick = fake.connect_joystick(ponko::input::JoystickInfo::new("S"));
    /// fake.set_joystick_hat(stick, 0, glam::IVec2::new(0, -1)); // up
    /// ```
    pub fn set_joystick_hat(&self, device: DeviceKey, hat: usize, value: IVec2) {
        self.state().queue.push(FakeEvent::JoystickHat(device, hat, value));
    }

    /// The force feedback the engine last told joystick `device` to play
    /// ([`ForceFeedback::OFF`] for an unknown device).
    ///
    /// ```
    /// let fake = ponko::input::FakeGamepads::new();
    /// let wheel = fake.connect_joystick(ponko::input::JoystickInfo::new("W"));
    /// assert!(fake.force_feedback(wheel).is_off());
    /// ```
    #[must_use]
    pub fn force_feedback(&self, device: DeviceKey) -> ForceFeedback {
        self.state()
            .devices
            .iter()
            .find(|d| d.key == device)
            .map_or(ForceFeedback::OFF, |d| d.force_feedback)
    }

    /// How many times the engine changed joystick `device`'s force feedback.
    ///
    /// ```
    /// let fake = ponko::input::FakeGamepads::new();
    /// let wheel = fake.connect_joystick(ponko::input::JoystickInfo::new("W"));
    /// assert_eq!(fake.force_feedback_changes(wheel), 0);
    /// ```
    #[must_use]
    pub fn force_feedback_changes(&self, device: DeviceKey) -> usize {
        self.state()
            .devices
            .iter()
            .find(|d| d.key == device)
            .map_or(0, |d| d.force_feedback_changes)
    }

    /// The motor levels the engine last told `device` to play
    /// ([`MotorLevels::OFF`] for an unknown device).
    ///
    /// ```
    /// let fake = ponko::input::FakeGamepads::new();
    /// let pad = fake.connect(ponko::input::GamepadInfo::new("A"));
    /// assert!(fake.rumble(pad).is_off());
    /// ```
    #[must_use]
    pub fn rumble(&self, device: DeviceKey) -> MotorLevels {
        self.state()
            .devices
            .iter()
            .find(|d| d.key == device)
            .map_or(MotorLevels::OFF, |d| d.rumble)
    }

    /// How many times the engine changed `device`'s motor levels: it sends
    /// a change, not the same levels every frame.
    ///
    /// ```
    /// let fake = ponko::input::FakeGamepads::new();
    /// let pad = fake.connect(ponko::input::GamepadInfo::new("A"));
    /// assert_eq!(fake.rumble_changes(pad), 0);
    /// ```
    #[must_use]
    pub fn rumble_changes(&self, device: DeviceKey) -> usize {
        self.state()
            .devices
            .iter()
            .find(|d| d.key == device)
            .map_or(0, |d| d.rumble_changes)
    }
}

impl GamepadBackend for FakeGamepads {
    fn poll(&mut self, events: &mut GamepadEvents<'_>) {
        let queue = std::mem::take(&mut self.state().queue);
        for event in queue {
            match event {
                FakeEvent::Connected(key, FakeKind::Pad(info)) => {
                    events.connected(key, info);
                }
                FakeEvent::Connected(key, FakeKind::Joystick(info)) => {
                    events.joystick_connected(key, info);
                }
                FakeEvent::Disconnected(key, false) => events.disconnected(key),
                FakeEvent::Disconnected(key, true) => events.joystick_disconnected(key),
                FakeEvent::Button(key, button, down) => events.button(key, button, down),
                FakeEvent::Axis(key, axis, value) => events.axis(key, axis, value),
                FakeEvent::Motion(key, motion) => events.motion(key, motion),
                FakeEvent::JoystickAxis(key, axis, value) => events.joystick_axis(key, axis, value),
                FakeEvent::JoystickButton(key, button, down) => events.joystick_button(key, button, down),
                FakeEvent::JoystickHat(key, hat, value) => events.joystick_hat(key, hat, value),
            }
        }
    }

    fn set_rumble(&mut self, device: DeviceKey, levels: MotorLevels) {
        if let Some(d) = self.state().devices.iter_mut().find(|d| d.key == device) {
            d.rumble = levels;
            d.rumble_changes += 1;
        }
    }

    fn set_force_feedback(&mut self, device: DeviceKey, effects: ForceFeedback) {
        if let Some(d) = self.state().devices.iter_mut().find(|d| d.key == device) {
            d.force_feedback = effects;
            d.force_feedback_changes += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{GamepadCapabilities, Rumble};

    #[test]
    fn replacing_the_backend_disconnects_its_pads_and_stops_their_rumble() {
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        let old = FakeGamepads::new();
        let device = old.connect(GamepadInfo::new("Old").capabilities(GamepadCapabilities {
            rumble: true,
            trigger_rumble: false,
            motion: false,
            ..Default::default()
        }));
        ctx.set_gamepad_backend(old.clone());
        let pad = ctx.input.gamepads()[0];
        ctx.input.rumble(pad, Rumble::new(1.0, 1.0).seconds(60.0));
        ctx.poll_gamepads(1.0 / 60.0);
        assert!(!old.rumble(device).is_off());
        let new = FakeGamepads::new();
        new.connect(GamepadInfo::new("New"));
        ctx.set_gamepad_backend(new);
        assert!(old.rumble(device).is_off(), "the old pad stops");
        assert_eq!(ctx.input.gamepads(), [pad]);
        assert_eq!(ctx.input.gamepad_info(pad).unwrap().name, "New");
    }

    #[test]
    fn a_wheel_keeps_its_slot_across_a_reconnect_and_its_inputs_arrive() {
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        let fake = FakeGamepads::new();
        let pad = fake.connect(GamepadInfo::new("Pad"));
        let stick = fake.connect_joystick(JoystickInfo::new("Stick"));
        let wheel = fake.connect_joystick(JoystickInfo::new("Wheel").layout(3, 20, 1));
        ctx.set_gamepad_backend(fake.clone());
        assert_eq!(ctx.input.gamepads().len(), 1, "joysticks take no gamepad slot");
        assert_eq!(ctx.input.joysticks(), [Joystick(0), Joystick(1)]);
        let w = Joystick(1);
        fake.set_joystick_axis(wheel, 0, -0.5);
        fake.set_joystick_axis(wheel, 2, 1.0);
        fake.set_joystick_button(wheel, 19, true);
        fake.set_joystick_hat(wheel, 0, IVec2::new(-1, 1));
        fake.press(pad, GamepadButton::South);
        ctx.poll_gamepads(1.0 / 60.0);
        assert_eq!(ctx.input.joystick_axis(w, 0), -0.5);
        assert_eq!(ctx.input.joystick_axis(w, 2), 1.0);
        assert!(ctx.input.joystick_pressed(w, 19));
        assert_eq!(ctx.input.joystick_hat(w, 0), IVec2::new(-1, 1));
        assert!(!ctx.input.joystick_down(Joystick(0), 19), "the other joystick");

        fake.disconnect(stick);
        fake.disconnect(wheel);
        ctx.poll_gamepads(1.0 / 60.0);
        assert!(ctx.input.joysticks().is_empty());
        assert_eq!(ctx.input.gamepads().len(), 1, "the pad stays");
        let other = fake.connect_joystick(JoystickInfo::new("Other"));
        fake.reconnect(wheel);
        ctx.poll_gamepads(1.0 / 60.0);
        assert_eq!(ctx.input.joystick_info(w).unwrap().name, "Wheel", "back in its slot");
        fake.set_joystick_button(other, 0, true);
        ctx.poll_gamepads(1.0 / 60.0);
        assert!(
            ctx.input.joystick_down(Joystick(2), 0),
            "a new device takes a slot nobody waits for"
        );
    }

    #[test]
    fn force_feedback_is_sent_on_change_and_stopped_when_the_backend_is_replaced() {
        let mut ctx = Ctx::new(1.0 / 60.0, glam::Vec2::ONE);
        let old = FakeGamepads::new();
        let wheel = old.connect_joystick(JoystickInfo::new("Wheel"));
        ctx.set_gamepad_backend(old.clone());
        let w = ctx.input.joysticks()[0];
        let ff = ForceFeedback {
            constant: -0.25,
            spring: 0.75,
            ..ForceFeedback::OFF
        };
        ctx.input.set_force_feedback(w, ff);
        ctx.poll_gamepads(1.0 / 60.0);
        ctx.poll_gamepads(1.0 / 60.0);
        assert_eq!(old.force_feedback(wheel), ff);
        assert_eq!(old.force_feedback_changes(wheel), 1);
        ctx.input.set_force_feedback(w, ForceFeedback::OFF);
        ctx.poll_gamepads(1.0 / 60.0);
        assert!(old.force_feedback(wheel).is_off());
        assert_eq!(old.force_feedback_changes(wheel), 2);

        ctx.input.set_force_feedback(w, ff);
        ctx.poll_gamepads(1.0 / 60.0);
        ctx.set_gamepad_backend(FakeGamepads::new());
        assert!(old.force_feedback(wheel).is_off(), "the old wheel stops");
        assert!(ctx.input.joysticks().is_empty());
    }
}
