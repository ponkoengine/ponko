//! Sound before the player's first click on the web.
//!
//! Browsers only let a page make sound after the player has clicked,
//! tapped or pressed a key, and an audio context opened before that stays
//! suspended. So on the web the engine opens its output only at the first
//! such gesture ([`Gate::open`]). What the game asked for before it waits
//! here: looping sounds (music, ambience) start once sound is allowed, and
//! one-shots are dropped, since a click or footstep heard seconds late
//! would be out of place. On the desktop the gate starts open and never
//! holds anything.

use super::{AudioBus, AudioEffects, SoundParams};

/// A voice asked for before sound was allowed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Waiting {
    /// The id its `Playing` handle carries, kept when it starts.
    pub id: u64,
    pub sound: u32,
    pub params: SoundParams,
    pub bus: AudioBus,
    pub effects: AudioEffects,
    /// The latest volume, panning and speed the 3D update gave it.
    #[cfg(feature = "3d")]
    pub spatial: Option<[f32; 3]>,
}

impl Waiting {
    pub fn new(id: u64, sound: u32, params: SoundParams, bus: AudioBus, effects: AudioEffects) -> Self {
        Self {
            id,
            sound,
            params,
            bus,
            effects,
            #[cfg(feature = "3d")]
            spatial: None,
        }
    }
}

/// Whether sound is allowed yet, and the voices waiting for it.
#[derive(Debug)]
pub(crate) struct Gate {
    locked: bool,
    waiting: Vec<Waiting>,
}

impl Default for Gate {
    /// Locked on the web until the first gesture; open on the desktop.
    fn default() -> Self {
        Self {
            locked: cfg!(target_arch = "wasm32"),
            waiting: Vec::new(),
        }
    }
}

impl Gate {
    /// A gate that waits for a gesture, as on the web.
    #[cfg(test)]
    pub fn locked() -> Self {
        Self {
            locked: true,
            waiting: Vec::new(),
        }
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// A voice asked for while locked: kept if it loops, dropped if not.
    /// Returns whether it waits.
    pub fn hold(&mut self, voice: Waiting) -> bool {
        if voice.params.looping {
            self.waiting.push(voice);
        }
        voice.params.looping
    }

    /// Stops a waiting voice before it started. Returns whether it was waiting.
    pub fn cancel(&mut self, id: u64) -> bool {
        let before = self.waiting.len();
        self.waiting.retain(|voice| voice.id != id);
        self.waiting.len() != before
    }

    /// Whether voice `id` waits for the gesture.
    pub fn is_waiting(&self, id: u64) -> bool {
        self.waiting.iter().any(|voice| voice.id == id)
    }

    /// New effects for a waiting voice. Returns whether it was waiting.
    pub fn set_effects(&mut self, id: u64, effects: AudioEffects) -> bool {
        self.find(id).map(|voice| voice.effects = effects).is_some()
    }

    /// The 3D update's volume, panning and speed for a waiting voice, to
    /// start it with. Returns whether it was waiting.
    #[cfg(feature = "3d")]
    pub fn spatialize(&mut self, id: u64, volume: f32, panning: f32, speed: f32) -> bool {
        self.find(id)
            .map(|voice| voice.spatial = Some([volume, panning, speed]))
            .is_some()
    }

    /// Sound `sound` was freed: its waiting voices go too.
    pub fn forget_sound(&mut self, sound: u32) {
        self.waiting.retain(|voice| voice.sound != sound);
    }

    /// The first gesture: sound is allowed from now on. Returns the voices
    /// that waited, in the order they were asked for, to start now; later
    /// calls return none.
    pub fn open(&mut self) -> Vec<Waiting> {
        self.locked = false;
        std::mem::take(&mut self.waiting)
    }

    fn find(&mut self, id: u64) -> Option<&mut Waiting> {
        self.waiting.iter_mut().find(|voice| voice.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(id: u64, looping: bool) -> Waiting {
        Waiting::new(
            id,
            id as u32 * 10,
            SoundParams {
                looping,
                ..Default::default()
            },
            AudioBus::default(),
            AudioEffects::default(),
        )
    }

    #[test]
    fn desktop_gate_starts_open_and_web_gate_locked() {
        assert_eq!(Gate::default().is_locked(), cfg!(target_arch = "wasm32"));
        assert!(Gate::locked().is_locked());
    }

    #[test]
    fn looping_voices_wait_for_the_gesture_and_one_shots_are_dropped() {
        let mut gate = Gate::locked();
        assert!(gate.hold(voice(1, true)));
        assert!(!gate.hold(voice(2, false)));
        assert!(gate.hold(voice(3, true)));
        let started = gate.open();
        assert_eq!(started.iter().map(|v| v.id).collect::<Vec<_>>(), [1, 3]);
        assert!(!gate.is_locked());
        assert!(gate.open().is_empty(), "only the first gesture starts them");
    }

    #[test]
    fn stopping_a_waiting_voice_keeps_it_from_starting() {
        let mut gate = Gate::locked();
        gate.hold(voice(1, true));
        gate.hold(voice(2, true));
        assert!(gate.cancel(1));
        assert!(!gate.cancel(1));
        assert!(!gate.cancel(7));
        assert_eq!(gate.open().iter().map(|v| v.id).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn waiting_voices_start_with_their_latest_effects() {
        let mut gate = Gate::locked();
        gate.hold(voice(1, true));
        let muffled = AudioEffects {
            low_pass_hz: 500.0,
            reverb: 0.3,
        };
        assert!(gate.set_effects(1, muffled));
        assert!(!gate.set_effects(2, muffled));
        assert_eq!(gate.open()[0].effects, muffled);
    }

    #[cfg(feature = "3d")]
    #[test]
    fn waiting_voices_start_where_the_3d_update_last_put_them() {
        let mut gate = Gate::locked();
        gate.hold(voice(1, true));
        assert!(gate.spatialize(1, 0.5, -0.25, 1.0));
        assert!(gate.spatialize(1, 0.25, 0.5, 0.9));
        assert_eq!(gate.open()[0].spatial, Some([0.25, 0.5, 0.9]));
    }

    #[test]
    fn freeing_a_sound_drops_its_waiting_voices() {
        let mut gate = Gate::locked();
        gate.hold(voice(1, true));
        gate.hold(voice(2, true));
        gate.forget_sound(10);
        assert_eq!(gate.open().iter().map(|v| v.id).collect::<Vec<_>>(), [2]);
    }
}
