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

use super::controller::{GamepadInfo, GamepadMotion, MotorLevels};
use super::{Gamepad, GamepadAxis, GamepadButton, Input, MAX_GAMEPADS};
use crate::ctx::Ctx;

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
}

/// What a [`GamepadBackend`] reports during [`poll`](GamepadBackend::poll).
/// Events for a device that isn't connected are ignored.
pub struct GamepadEvents<'a> {
    input: &'a mut Input,
    slots: &'a mut Slots,
}

impl GamepadEvents<'_> {
    /// `device` connected: it takes a slot (its old one if it was here
    /// before) and its info is kept for [`Input::gamepad_info`]. Returns the
    /// slot, or `None` when all [`MAX_GAMEPADS`] are taken. Reporting a
    /// connected device again only updates its info.
    pub fn connected(&mut self, device: DeviceKey, info: GamepadInfo) -> Option<Gamepad> {
        let pad = match self.slots.slot_of(device) {
            Some(pad) => pad,
            None => self.slots.take(device)?,
        };
        self.input.connect_gamepad_at(pad, info);
        Some(pad)
    }

    /// `device` disconnected: its held buttons read as released this frame,
    /// and its slot waits for it to come back.
    pub fn disconnected(&mut self, device: DeviceKey) {
        if let Some(pad) = self.slots.release(device) {
            self.input.disconnect_gamepad(pad);
        }
    }

    /// A button of `device` went down (`true`) or up.
    pub fn button(&mut self, device: DeviceKey, button: GamepadButton, down: bool) {
        if let Some(pad) = self.slots.slot_of(device) {
            self.input.set_gamepad_button(pad, button, down);
        }
    }

    /// An axis of `device` moved: sticks `-1..=1` with y down, triggers `0..=1`.
    pub fn axis(&mut self, device: DeviceKey, axis: GamepadAxis, value: f32) {
        if let Some(pad) = self.slots.slot_of(device) {
            self.input.set_gamepad_axis(pad, axis, value);
        }
    }

    /// A new gyro and accelerometer reading of `device`.
    pub fn motion(&mut self, device: DeviceKey, motion: GamepadMotion) {
        if let Some(pad) = self.slots.slot_of(device) {
            self.input.set_gamepad_motion(pad, motion);
        }
    }
}

/// Which device holds each slot, and which device each empty slot waits
/// for (the one unplugged from it last).
#[derive(Debug, Default)]
struct Slots {
    held: [Option<DeviceKey>; MAX_GAMEPADS],
    waiting: [Option<DeviceKey>; MAX_GAMEPADS],
}

impl Slots {
    fn slot_of(&self, device: DeviceKey) -> Option<Gamepad> {
        let slot = self.held.iter().position(|&d| d == Some(device))?;
        Some(Gamepad(slot as u8))
    }

    /// A slot for a device connecting: the one it left, else the lowest free
    /// slot nobody waits for, else the lowest free slot.
    fn take(&mut self, device: DeviceKey) -> Option<Gamepad> {
        let free = |s: usize| self.held[s].is_none();
        let slot = (0..MAX_GAMEPADS)
            .find(|&s| free(s) && self.waiting[s] == Some(device))
            .or_else(|| (0..MAX_GAMEPADS).find(|&s| free(s) && self.waiting[s].is_none()))
            .or_else(|| (0..MAX_GAMEPADS).find(|&s| free(s)))?;
        self.held[slot] = Some(device);
        self.waiting[slot] = None;
        // A device waits for one slot only.
        for w in &mut self.waiting {
            if *w == Some(device) {
                *w = None;
            }
        }
        Some(Gamepad(slot as u8))
    }

    fn release(&mut self, device: DeviceKey) -> Option<Gamepad> {
        let pad = self.slot_of(device)?;
        self.held[pad.slot()] = None;
        self.waiting[pad.slot()] = Some(device);
        Some(pad)
    }
}

/// The installed backend, its slots and the motor levels last sent: a
/// field of `Ctx`, polled at the start of every frame.
#[derive(Default)]
pub(crate) struct GamepadDriver {
    // In a Mutex so `Ctx` stays `Sync` with a backend that is only `Send`
    // (never contended: only the game's thread touches it).
    backend: Option<Mutex<Box<dyn GamepadBackend>>>,
    slots: Slots,
    sent: [MotorLevels; MAX_GAMEPADS],
}

impl std::fmt::Debug for GamepadDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GamepadDriver")
            .field("installed", &self.backend.is_some())
            .field("slots", &self.slots)
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
                input.disconnect_gamepad(Gamepad(slot as u8));
            }
        }
        self.slots = Slots::default();
        self.sent = [MotorLevels::OFF; MAX_GAMEPADS];
    }

    /// Once per frame: counts rumble down by `dt` real seconds, sends the
    /// motor levels that changed, then applies the backend's events.
    pub(crate) fn update(&mut self, input: &mut Input, dt: f32) {
        input.controllers.tick_rumble(dt);
        if self.backend.is_none() {
            return;
        }
        self.send_rumble(input);
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

    fn poll(&mut self, input: &mut Input) {
        let Some(backend) = self.backend.as_ref() else { return };
        let mut backend = backend.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut events = GamepadEvents {
            input,
            slots: &mut self.slots,
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
    info: GamepadInfo,
    rumble: MotorLevels,
    rumble_changes: usize,
}

#[derive(Debug)]
enum FakeEvent {
    Connected(DeviceKey, GamepadInfo),
    Disconnected(DeviceKey),
    Button(DeviceKey, GamepadButton, bool),
    Axis(DeviceKey, GamepadAxis, f32),
    Motion(DeviceKey, GamepadMotion),
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
        let mut state = self.state();
        let key = DeviceKey(state.next_key);
        state.next_key += 1;
        state.devices.push(FakeDevice {
            key,
            info: info.clone(),
            rumble: MotorLevels::OFF,
            rumble_changes: 0,
        });
        state.queue.push(FakeEvent::Connected(key, info));
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
        if let Some(info) = state.devices.iter().find(|d| d.key == device).map(|d| d.info.clone()) {
            state.queue.push(FakeEvent::Connected(device, info));
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
        self.state().queue.push(FakeEvent::Disconnected(device));
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
                FakeEvent::Connected(key, info) => {
                    events.connected(key, info);
                }
                FakeEvent::Disconnected(key) => events.disconnected(key),
                FakeEvent::Button(key, button, down) => events.button(key, button, down),
                FakeEvent::Axis(key, axis, value) => events.axis(key, axis, value),
                FakeEvent::Motion(key, motion) => events.motion(key, motion),
            }
        }
    }

    fn set_rumble(&mut self, device: DeviceKey, levels: MotorLevels) {
        if let Some(d) = self.state().devices.iter_mut().find(|d| d.key == device) {
            d.rumble = levels;
            d.rumble_changes += 1;
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
}
