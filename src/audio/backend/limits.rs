//! Voice limits and virtual voices.
//!
//! Every frame the voices are ranked: priority first, then how loud they
//! are heard (volume, distance and buses), then age. The first
//! `voice_limit` (and each bus's own limit) are real, Kira voices; the rest
//! are virtual: silent, costing only a little bookkeeping, their place in
//! the sound followed on the game side so they come back in time when a
//! slot frees up. A voice that can't be heard (beyond its maximum
//! distance, on a muted bus, at volume zero) is always virtual.

use super::{Audio, PlaybackState, StartTime, Tween, wrap};

/// The default number of voices heard at once.
pub(crate) const DEFAULT_VOICE_LIMIT: u32 = 64;
/// The most voices heard at once that a game can ask for.
pub(crate) const MAX_VOICE_LIMIT: u32 = 256;
/// Below this a voice can't be heard (-80 dB) and goes virtual.
const SILENT: f32 = 1e-4;
/// A real voice keeps its place until a virtual one is this much louder,
/// so two voices of about the same loudness don't take turns.
const HOLD: f32 = 1.25;

/// A value moved linearly toward a target, a frame at a time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fade {
    pub value: f32,
    target: f32,
    /// Change per second.
    rate: f32,
}
impl Fade {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            target: value,
            rate: 0.0,
        }
    }
    /// Moves to `target` over `seconds` (at once for 0 or less).
    pub fn to(&mut self, target: f32, seconds: f32) {
        self.target = target;
        if seconds > 0.0 && seconds.is_finite() {
            self.rate = (target - self.value).abs() / seconds;
        } else {
            self.value = target;
        }
    }
    /// One frame of `dt` seconds along. Whether the value changed.
    pub fn advance(&mut self, dt: f32) -> bool {
        if self.value == self.target {
            return false;
        }
        let step = self.rate * dt.max(0.0);
        self.value = if self.value < self.target {
            (self.value + step).min(self.target)
        } else {
            (self.value - step).max(self.target)
        };
        true
    }
}

/// One voice in the frame's ranking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Candidate {
    pub id: u64,
    pub priority: u8,
    /// How loud it is heard, linear.
    pub audibility: f32,
    /// Real now.
    pub real: bool,
    /// On a paused bus: ranked last, and never virtual for being silent.
    pub paused: bool,
    pub bus: usize,
    /// The ranking's answer: real from now on.
    pub keep: bool,
}

/// Ranks `voices` and marks the ones to be heard: at most `limit`, at most
/// `bus_limit(bus)` per bus, none that can't be heard. Sorts `voices` in
/// place, most important first; allocates only when `counts` must grow.
pub(crate) fn choose(
    voices: &mut [Candidate],
    limit: usize,
    bus_limit: impl Fn(usize) -> Option<u32>,
    counts: &mut Vec<u32>,
) {
    let score = |c: &Candidate| if c.real { c.audibility * HOLD } else { c.audibility };
    voices.sort_unstable_by(|a, b| {
        a.paused
            .cmp(&b.paused)
            .then(b.priority.cmp(&a.priority))
            .then(score(b).total_cmp(&score(a)))
            .then(a.id.cmp(&b.id))
    });
    counts.iter_mut().for_each(|c| *c = 0);
    let mut used = 0;
    for c in voices.iter_mut() {
        c.keep = false;
        if (!c.paused && c.audibility <= SILENT) || used >= limit {
            continue;
        }
        if counts.len() <= c.bus {
            counts.resize(c.bus + 1, 0);
        }
        if bus_limit(c.bus).is_some_and(|l| counts[c.bus] >= l) {
            continue;
        }
        counts[c.bus] += 1;
        used += 1;
        c.keep = true;
    }
}

/// The limits and the frame's reused buffers.
pub(crate) struct Limits {
    pub voice_limit: u32,
    scratch: Vec<Candidate>,
    counts: Vec<u32>,
    ended: Vec<u64>,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            voice_limit: DEFAULT_VOICE_LIMIT,
            scratch: Vec::new(),
            counts: Vec::new(),
            ended: Vec::new(),
        }
    }
}

impl Audio {
    /// A voice just asked for: heard at once if there is room for it, or
    /// if it outranks the weakest voice heard (which goes virtual).
    /// Otherwise it starts virtual and the next frame's ranking decides.
    pub(super) fn admit(&mut self, id: u64) {
        let Some(new) = self.candidate(id) else {
            return;
        };
        let exempt = self.playing.get(&id).is_some_and(|v| v.exempt);
        if exempt || self.manager().is_none() {
            self.realize(id, StartTime::Immediate, None);
            return;
        }
        if new.audibility <= SILENT && !new.paused {
            return;
        }
        // The real voices: how many, how many on its bus, and the weakest.
        let (mut real, mut on_bus) = (0u32, 0u32);
        let mut weakest: Option<Candidate> = None;
        for (&other, v) in &self.playing {
            if other == id || v.exempt || v.real.is_none() {
                continue;
            }
            real += 1;
            if v.bus.0 == new.bus {
                on_bus += 1;
            }
            let Some(c) = self.candidate(other) else {
                continue;
            };
            if weakest.is_none_or(|w| outranks(&w, &c)) {
                weakest = Some(c);
            }
        }
        let bus_full = self
            .buses
            .get(super::AudioBus(new.bus))
            .and_then(|s| s.voice_limit)
            .is_some_and(|l| on_bus >= l);
        if real < self.limits.voice_limit && !bus_full {
            self.realize(id, StartTime::Immediate, None);
        } else if !bus_full
            && let Some(w) = weakest
            && outranks(&new, &w)
        {
            self.virtualize(w.id);
            self.realize(id, StartTime::Immediate, None);
        }
    }

    /// Voice `id` as the ranking sees it.
    fn candidate(&self, id: u64) -> Option<Candidate> {
        let v = self.playing.get(&id)?;
        let paused = self.buses.paused(v.bus);
        Some(Candidate {
            id,
            priority: v.priority,
            audibility: v.heard().0 * self.buses.chain_gain(v.bus),
            real: v.real.is_some(),
            paused,
            bus: v.bus.0,
            keep: false,
        })
    }

    /// The frame's voice work, after the mixer moved: fades, where virtual
    /// voices are, which voices are heard, the music clock and the output
    /// device. Allocates nothing once its buffers have grown.
    pub fn update(&mut self, dt: f32) {
        if !matches!(self.manager, Some(Some(_))) {
            self.poll_output(dt);
            return;
        }
        self.poll_output(dt);
        let dt = dt.max(0.0);
        // Ended voices go; the others move along.
        let mut ended = std::mem::take(&mut self.limits.ended);
        ended.clear();
        for (&id, v) in &mut self.playing {
            if let Some(real) = &mut v.real
                && real.handle.state() == PlaybackState::Stopped
            {
                if let Some(error) = real.handle.take_error() {
                    eprintln!("ponko: a streamed sound stopped: {error}");
                }
                ended.push(id);
                continue;
            }
            if v.stopping && v.real.is_none() {
                ended.push(id);
                continue;
            }
            let moved = v.gain.advance(dt) | v.rate.advance(dt);
            let (volume, pan, speed) = v.heard();
            if moved && let Some(real) = &mut v.real {
                real.handle.apply(volume, pan, speed);
            }
            // A fade-out reached silence: stop it for good.
            if v.stopping
                && v.gain.value <= 0.0
                && let Some(real) = &mut v.real
                && real.handle.state() == PlaybackState::Playing
            {
                real.handle.stop(Tween::default());
            }
            if self.buses.paused(v.bus) {
                continue;
            }
            let meta = self
                .ids
                .slot(v.sound)
                .and_then(|s| self.meta.get(s))
                .copied()
                .unwrap_or_default();
            let next = v.position + f64::from(dt * speed);
            if v.real.is_none() && !v.params.looping && next >= meta.seconds {
                ended.push(id);
                continue;
            }
            v.position = wrap(next, meta, v.params.looping);
        }
        for id in &ended {
            self.playing.remove(id);
        }
        self.limits.ended = ended;
        self.rank();
        self.update_music();
    }

    /// Ranks every voice and moves them between real and virtual.
    pub(super) fn rank(&mut self) {
        let mut scratch = std::mem::take(&mut self.limits.scratch);
        scratch.clear();
        let mut exempt_virtual = false;
        for (&id, v) in &self.playing {
            if v.exempt {
                exempt_virtual |= v.real.is_none();
                continue;
            }
            if v.stopping {
                continue;
            }
            if let Some(c) = self.candidate(id) {
                scratch.push(c);
            }
        }
        if exempt_virtual {
            // Music voices that lost their output start again together.
            let mut music: Vec<u64> = self
                .playing
                .iter()
                .filter(|(_, v)| v.exempt && v.real.is_none())
                .map(|(&id, _)| id)
                .collect();
            music.sort_unstable();
            for id in music {
                self.realize(id, StartTime::Immediate, None);
            }
        }
        let mut counts = std::mem::take(&mut self.limits.counts);
        let buses = &self.buses;
        choose(
            &mut scratch,
            self.limits.voice_limit as usize,
            |bus| buses.entries.get(bus).and_then(|s| s.voice_limit),
            &mut counts,
        );
        self.limits.counts = counts;
        // Out first, so the new ones have the room.
        for c in scratch.iter().filter(|c| c.real && !c.keep) {
            self.virtualize(c.id);
        }
        for c in scratch.iter().filter(|c| !c.real && c.keep) {
            self.realize(c.id, StartTime::Immediate, None);
        }
        self.limits.scratch = scratch;
    }

    /// Fades voice `id`'s volume (1: as played) over `seconds`.
    pub fn set_volume(&mut self, id: u64, volume: f32, seconds: f32) {
        if let Some(v) = self.playing.get_mut(&id) {
            v.gain.to(volume.max(0.0), seconds);
            if seconds <= 0.0 {
                let (volume, pan, speed) = v.heard();
                if let Some(real) = &mut v.real {
                    real.handle.apply(volume, pan, speed);
                }
            }
        }
    }
    /// Moves voice `id`'s speed (and pitch; 1: as played) over `seconds`.
    pub fn set_speed(&mut self, id: u64, speed: f32, seconds: f32) {
        if let Some(v) = self.playing.get_mut(&id) {
            v.rate.to(speed.max(0.01), seconds);
            if seconds <= 0.0 {
                let (volume, pan, speed) = v.heard();
                if let Some(real) = &mut v.real {
                    real.handle.apply(volume, pan, speed);
                }
            }
        }
    }
    /// Fades voice `id` out over `seconds`, then stops it.
    pub fn fade_out(&mut self, id: u64, seconds: f32) {
        if self.gate.cancel(id) {
            return;
        }
        let Some(v) = self.playing.get_mut(&id) else {
            return;
        };
        match &mut v.real {
            // Linear in amplitude like `set_volume`, so a crossfade's two
            // halves add up; the frame it reaches 0 the voice stops.
            Some(real) => {
                v.stopping = true;
                v.gain.to(0.0, seconds);
                if seconds <= 0.0 {
                    real.handle.stop(Tween::default());
                }
            }
            None => {
                self.playing.remove(&id);
            }
        }
    }
    pub fn set_priority(&mut self, id: u64, priority: u8) {
        if let Some(v) = self.playing.get_mut(&id) {
            v.priority = priority;
        }
    }
    /// `Some(true)` if voice `id` is virtual, `None` if it isn't playing.
    pub fn is_virtual(&self, id: u64) -> Option<bool> {
        if self.gate.is_waiting(id) {
            return Some(true);
        }
        self.playing.get(&id).map(|v| v.real.is_none())
    }
    /// Real and virtual voices (music not counted).
    pub fn voice_count(&self) -> (u32, u32) {
        self.playing
            .values()
            .filter(|v| !v.exempt && !v.stopping)
            .fold(
                (0, 0),
                |(r, n), v| if v.real.is_some() { (r + 1, n) } else { (r, n + 1) },
            )
    }
    pub fn set_voice_limit(&mut self, limit: u32) {
        self.limits.voice_limit = limit.min(MAX_VOICE_LIMIT);
    }
    pub fn voice_limit(&self) -> u32 {
        self.limits.voice_limit
    }
}

/// Whether `a` should be heard before `b`.
fn outranks(a: &Candidate, b: &Candidate) -> bool {
    let score = |c: &Candidate| if c.real { c.audibility * HOLD } else { c.audibility };
    (a.paused, std::cmp::Reverse(a.priority)) < (b.paused, std::cmp::Reverse(b.priority))
        || ((a.paused, a.priority) == (b.paused, b.priority) && score(a) > score(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(id: u64, priority: u8, audibility: f32, bus: usize) -> Candidate {
        Candidate {
            id,
            priority,
            audibility,
            real: false,
            paused: false,
            bus,
            keep: false,
        }
    }
    fn kept(voices: &[Candidate]) -> Vec<u64> {
        let mut ids: Vec<u64> = voices.iter().filter(|c| c.keep).map(|c| c.id).collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn the_loudest_voices_are_heard_up_to_the_limit() {
        let mut v = [voice(1, 128, 0.2, 0), voice(2, 128, 0.9, 0), voice(3, 128, 0.5, 0)];
        choose(&mut v, 2, |_| None, &mut Vec::new());
        assert_eq!(kept(&v), [2, 3]);
    }

    #[test]
    fn priority_beats_loudness() {
        let mut v = [voice(1, 200, 0.01, 0), voice(2, 128, 0.9, 0), voice(3, 128, 0.5, 0)];
        choose(&mut v, 2, |_| None, &mut Vec::new());
        assert_eq!(kept(&v), [1, 2]);
    }

    #[test]
    fn voices_nobody_can_hear_are_always_virtual() {
        let mut v = [voice(1, 255, 0.0, 0), voice(2, 128, 0.3, 0)];
        choose(&mut v, 8, |_| None, &mut Vec::new());
        assert_eq!(kept(&v), [2]);
    }

    #[test]
    fn a_bus_limit_caps_its_own_voices_only() {
        let mut v = [
            voice(1, 128, 0.9, 1),
            voice(2, 128, 0.8, 1),
            voice(3, 128, 0.7, 1),
            voice(4, 128, 0.1, 0),
        ];
        choose(&mut v, 8, |bus| (bus == 1).then_some(2), &mut Vec::new());
        assert_eq!(kept(&v), [1, 2, 4]);
    }

    #[test]
    fn a_real_voice_keeps_its_place_against_a_slightly_louder_one() {
        let mut v = [
            Candidate {
                real: true,
                ..voice(1, 128, 0.5, 0)
            },
            voice(2, 128, 0.55, 0),
        ];
        choose(&mut v, 1, |_| None, &mut Vec::new());
        assert_eq!(kept(&v), [1]);
        let mut v = [
            Candidate {
                real: true,
                ..voice(1, 128, 0.5, 0)
            },
            voice(2, 128, 0.9, 0),
        ];
        choose(&mut v, 1, |_| None, &mut Vec::new());
        assert_eq!(kept(&v), [2], "a clearly louder one takes over");
    }

    #[test]
    fn equal_voices_keep_the_oldest() {
        let mut v = [voice(9, 128, 0.5, 0), voice(3, 128, 0.5, 0), voice(5, 128, 0.5, 0)];
        choose(&mut v, 2, |_| None, &mut Vec::new());
        assert_eq!(kept(&v), [3, 5]);
    }

    #[test]
    fn paused_voices_come_last_but_keep_their_slot_when_there_is_room() {
        let mut v = [
            Candidate {
                paused: true,
                real: true,
                ..voice(1, 255, 0.0, 0)
            },
            voice(2, 128, 0.5, 0),
        ];
        choose(&mut v, 1, |_| None, &mut Vec::new());
        assert_eq!(kept(&v), [2]);
        choose(&mut v, 2, |_| None, &mut Vec::new());
        assert_eq!(kept(&v), [1, 2]);
    }

    #[test]
    fn fades_reach_their_target_in_time_and_stop_there() {
        let mut f = Fade::new(1.0);
        f.to(0.0, 0.5);
        assert!(f.advance(0.25));
        assert!((f.value - 0.5).abs() < 1e-6);
        assert!(f.advance(0.5));
        assert_eq!(f.value, 0.0);
        assert!(!f.advance(0.1));
        f.to(0.3, 0.0);
        assert_eq!(f.value, 0.3, "no fade: at once");
    }
}
