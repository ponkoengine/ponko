//! Pause groups: a paused bus holds every sound on it and on the buses
//! under it where they are, and they carry on from there when it resumes.
//! Sounds started on a paused bus wait for it too. Put gameplay sound on a
//! bus of its own and the menu's on another, and a pause menu pauses one
//! and plays the other; [`Game::pause_audio_in`] ties that to a game state.

use super::{Audio, AudioBus};
use crate::ctx::Ctx;
use crate::game::Game;
use crate::state::States;

impl Audio {
    /// Pauses `bus` (and the buses under it), or resumes it. A bus also
    /// stays paused while a game state that pauses it is on the stack
    /// ([`Game::pause_audio_in`]).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let (world, sfx) = (ctx.audio.bus("world"), ctx.audio.bus("sfx"));
    ///     ctx.audio.set_bus_parent(sfx, Some(world)).unwrap();
    ///     ctx.audio.pause_bus(world, true);
    ///     assert!(ctx.audio.bus_paused(sfx), "children pause with their parent");
    ///     ctx.audio.pause_bus(world, false);
    ///     assert!(!ctx.audio.bus_paused(sfx));
    /// });
    /// ```
    pub fn pause_bus(&mut self, bus: AudioBus, paused: bool) {
        self.backend.buses.ensure_default();
        if let Some(state) = self.backend.buses.entries.get_mut(bus.0)
            && state.paused != paused
        {
            state.paused = paused;
            self.backend.sync_buses();
        }
    }

    /// Whether `bus` is paused: by [`pause_bus`](Self::pause_bus), by a game
    /// state, or because a bus above it is.
    #[must_use]
    pub fn bus_paused(&self, bus: AudioBus) -> bool {
        self.backend.buses.paused(bus)
    }

    /// A game state that pauses `bus` was entered (`true`) or left.
    fn state_pause(&mut self, bus: AudioBus, entered: bool) {
        self.backend.buses.ensure_default();
        if let Some(state) = self.backend.buses.entries.get_mut(bus.0) {
            state.state_pauses = if entered {
                state.state_pauses + 1
            } else {
                state.state_pauses.saturating_sub(1)
            };
            self.backend.sync_buses();
        }
    }
}

impl Game {
    /// Pauses the buses named in `buses` (made if missing) while `state` is
    /// on the state stack: from entering it to leaving it, including while
    /// other states are pushed over it. A pause menu pushed over play
    /// pauses the world's sound and leaves its own bus playing:
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use serde::{Deserialize, Serialize};
    ///
    /// #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
    /// enum Screen { Playing, Paused }
    ///
    /// Game::new("t", 1, 1)
    ///     .state(Screen::Playing)
    ///     .pause_audio_in(Screen::Paused, &["world", "music"])
    ///     .add(Phase::AfterPhysics, |ctx: &mut Ctx| {
    ///         if ctx.time.sim_tick() == 2 {
    ///             ctx.push_state(Screen::Paused);
    ///         }
    ///         let world = ctx.audio.bus("world");
    ///         let paused = ctx.audio.bus_paused(world);
    ///         assert_eq!(paused, ctx.time.sim_tick() > 2);
    ///     })
    ///     .run_headless(5, |_| {});
    /// ```
    pub fn pause_audio_in<S: States>(self, state: S, buses: &[&str]) -> Self {
        let names: Vec<String> = buses.iter().map(|&b| b.to_owned()).collect();
        let exit_names = names.clone();
        self.on_enter(state.clone(), move |ctx: &mut Ctx| {
            for name in &names {
                let bus = ctx.audio.bus(name);
                ctx.audio.state_pause(bus, true);
            }
        })
        .on_exit(state, move |ctx: &mut Ctx| {
            for name in &exit_names {
                let bus = ctx.audio.bus(name);
                ctx.audio.state_pause(bus, false);
            }
        })
    }
}
