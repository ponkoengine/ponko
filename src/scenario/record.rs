//! Recording live input into an [`InputScenario`] that replays it.

use super::InputScenario;
use crate::actions::InputEvent;
use crate::input::{Gamepad, GamepadInfo, ImeEvent, Input};

/// An input recording in progress: [`Input::start_recording`].
#[derive(Debug, Clone)]
pub(crate) struct Recording {
    /// The simulation tick that is the scenario's tick 0: the first tick
    /// that began after the recording started.
    start: Option<u64>,
    /// The tick running now, between its beginning and its end.
    in_tick: Option<u64>,
    /// How many ticks ended since `start`.
    ticks: u64,
    /// Events seen since the last tick boundary, in order; stamped with the
    /// next tick that begins or ends.
    pending: Vec<Recorded>,
    scenario: InputScenario,
}

#[derive(Debug, Clone)]
enum Recorded {
    Input(InputEvent),
    Ime(ImeEvent),
    /// A pad connecting, with its description.
    Connected(Gamepad, GamepadInfo),
}

impl Recording {
    fn push(&mut self, event: InputEvent) {
        // Pointer moves and stick readings between two ticks: only the last
        // one matters, so a busy mouse doesn't grow the scenario per event.
        let replaces = match (self.pending.last(), &event) {
            (Some(Recorded::Input(InputEvent::MouseMove(_))), InputEvent::MouseMove(_)) => true,
            (Some(Recorded::Input(InputEvent::GamepadAxis(p, a, _))), InputEvent::GamepadAxis(q, b, _)) => {
                p == q && a == b
            }
            (Some(Recorded::Input(InputEvent::GamepadMotion(p, _))), InputEvent::GamepadMotion(q, _)) => p == q,
            _ => false,
        };
        if replaces {
            self.pending.pop();
        }
        self.pending.push(Recorded::Input(event));
    }

    /// Stamps what is pending with simulation tick `sim_tick` (once tick 0
    /// began; before that, it waits for it).
    fn flush(&mut self, sim_tick: u64) {
        let Some(start) = self.start else {
            return;
        };
        if self.pending.is_empty() {
            return;
        }
        let tick = sim_tick.saturating_sub(start);
        let mut scenario = std::mem::take(&mut self.scenario);
        for event in self.pending.drain(..) {
            scenario = match event {
                Recorded::Input(event) => scenario.at(tick, event),
                Recorded::Ime(event) => scenario.ime(tick, event),
                Recorded::Connected(pad, info) => scenario.connect_gamepad(tick, pad, info),
            };
        }
        self.scenario = scenario;
    }
}

impl Input {
    /// Starts recording the input into an [`InputScenario`] that
    /// [`stop_recording`](Self::stop_recording) returns: keys, mouse
    /// buttons, the pointer, the wheel, gamepad buttons and axes, gamepads
    /// connecting (with their [`GamepadInfo`], so a replay shows the same
    /// prompts) and disconnecting, gamepad motion (gyro and accelerometer),
    /// focus loss and IME events, from the window, a backend or fed by
    /// code, each at the simulation tick that first sees it. Replaying it
    /// ([`Game::run_headless_scenario`](crate::game::Game::run_headless_scenario),
    /// [`Ctx::start_scenario`](crate::ctx::Ctx::start_scenario)) feeds the
    /// same events before the same ticks, so a deterministic game replays
    /// the same play; compare worlds with
    /// [`world_hash`](crate::reflect::world_hash).
    ///
    /// The recording's tick 0 is the next simulation tick that begins (from
    /// a system: the tick after this one). What is there when it starts
    /// (the pads connected, keys, buttons, the pointer, stick positions,
    /// each pad's last motion reading) is recorded at tick 0.
    /// Starting again restarts it.
    ///
    /// Not recorded: text typed without an IME (`typed_text` comes from key
    /// events on the window, which records the keys), raw mouse motion for
    /// mouse-look, and rumble (an output). Motion readings and stick
    /// positions between two ticks keep only the last. Edges (`key_pressed`) last a frame:
    /// an event in a frame that runs no simulation tick reaches the next
    /// tick as a held key, but in the replay as a fresh press.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::actions::InputEvent;
    /// use ponko::scenario::InputScenario;
    /// struct Recorded(Option<InputScenario>);
    ///
    /// let world = Game::new("t", 1, 1)
    ///     .sim_system(|ctx: &mut Ctx| match ctx.time.sim_tick() {
    ///         2 => ctx.input.feed(InputEvent::Key(Key::Space, true)), // as if live
    ///         4 => {
    ///             let scenario = ctx.input.stop_recording();
    ///             ctx.world.insert_resource(Recorded(scenario));
    ///         }
    ///         _ => {}
    ///     })
    ///     .run_headless(5, |ctx| ctx.input.start_recording());
    /// let scenario = world.resource::<Recorded>().0.clone().unwrap();
    /// assert_eq!((scenario.ticks(), scenario.step_count()), (5, 1));
    /// ```
    pub fn start_recording(&mut self) {
        let mut recording = Recording {
            start: None,
            in_tick: None,
            ticks: 0,
            pending: Vec::new(),
            scenario: InputScenario::new(),
        };
        // Pads first, so their held buttons replay onto a connected pad.
        for &pad in self.gamepads() {
            let info = self.controllers.info[pad.slot()].clone().unwrap_or_default();
            recording.pending.push(Recorded::Connected(pad, info));
        }
        for event in self.held_as_events() {
            recording.push(event);
        }
        for &pad in self.gamepads() {
            if let Some(motion) = self.controllers.motion[pad.slot()] {
                recording.push(InputEvent::GamepadMotion(pad, motion));
            }
            if let Some([first, second]) = self.gamepad_touchpad(pad) {
                for (finger, contact) in [(0, first), (1, second)] {
                    if contact != Default::default() {
                        recording.push(InputEvent::GamepadTouchpad(pad, finger, contact));
                    }
                }
            }
        }
        self.scenarios.recording = Some(Box::new(recording));
    }

    /// Stops the recording [`start_recording`](Self::start_recording) began
    /// and returns it as a scenario lasting the ticks it recorded (`None`
    /// when none was running). Name it with [`InputScenario::named`], save
    /// it with [`InputScenario::to_json`]. From a system, the tick running
    /// now is the last one, with what was fed so far.
    pub fn stop_recording(&mut self) -> Option<InputScenario> {
        let mut recording = *self.scenarios.recording.take()?;
        // Inside a tick, what was fed so far belongs to it; between ticks,
        // nothing has seen what is pending yet.
        let mut ticks = recording.ticks;
        if let Some(tick) = recording.in_tick {
            recording.flush(tick);
            ticks += 1;
        }
        Some(recording.scenario.duration(ticks))
    }

    /// Whether [`start_recording`](Self::start_recording) is recording the
    /// input.
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     assert!(!ctx.input.is_recording());
    ///     ctx.input.start_recording();
    ///     assert!(ctx.input.is_recording());
    /// });
    /// ```
    #[must_use]
    pub fn is_recording(&self) -> bool {
        self.scenarios.recording.is_some()
    }

    /// Notes a raw input event for a recording in progress (cheap when off).
    #[inline]
    pub(crate) fn record(&mut self, event: InputEvent) {
        if let Some(recording) = self.scenarios.recording.as_deref_mut() {
            recording.push(event);
        }
    }

    /// Notes a pad connecting, described by `info`, for a recording in progress.
    #[inline]
    pub(crate) fn record_connected(&mut self, pad: Gamepad, info: &GamepadInfo) {
        if let Some(recording) = self.scenarios.recording.as_deref_mut() {
            recording.pending.push(Recorded::Connected(pad, info.clone()));
        }
    }

    /// Notes an IME event for a recording in progress.
    #[inline]
    pub(crate) fn record_ime(&mut self, event: &ImeEvent) {
        if let Some(recording) = self.scenarios.recording.as_deref_mut() {
            recording.pending.push(Recorded::Ime(event.clone()));
        }
    }

    /// Before tick `sim_tick`'s systems: what arrived since the last tick is
    /// first seen by this one.
    pub(crate) fn recording_tick_begins(&mut self, sim_tick: u64) {
        if let Some(recording) = self.scenarios.recording.as_deref_mut() {
            recording.start.get_or_insert(sim_tick);
            recording.in_tick = Some(sim_tick);
            recording.flush(sim_tick);
        }
    }

    /// After tick `sim_tick`: what was fed during it (by a scenario or by
    /// systems) is this tick's.
    pub(crate) fn recording_tick_ends(&mut self, sim_tick: u64) {
        if let Some(recording) = self.scenarios.recording.as_deref_mut() {
            recording.flush(sim_tick);
            if recording.in_tick.take().is_some() {
                recording.ticks += 1;
            }
        }
    }
}
