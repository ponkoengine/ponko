//! Clips: keyed bone, slot and draw-order tracks, and events.

use std::sync::Arc;

use glam::Vec2;

use super::data::{Bone, BonePose, SkeletonError, err};
use crate::anim::AnimationEvent;
use crate::color::Color;

/// How a key's value goes to the next key's.
///
/// ```
/// use ponko::skeleton2d::{Bezier, Curve};
/// // Ease in and out: slow at both keys, fast in between.
/// let ease = Curve::Bezier(Bezier::new(0.42, 0.0, 0.58, 1.0));
/// assert_eq!(ease.ease(0.0, 0), 0.0);
/// assert!((ease.ease(0.5, 0) - 0.5).abs() < 1e-5);
/// assert!(ease.ease(0.1, 0) < 0.1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub enum Curve {
    /// A straight line.
    #[default]
    Linear,
    /// Holds the value until the next key.
    Stepped,
    /// A cubic bezier, the same for every channel of the value.
    Bezier(Bezier),
    /// One cubic bezier per channel: x then y for 2D values; red, green,
    /// blue, alpha for colors; only the first for numbers. Imported curves
    /// that differ per channel use it.
    Channels([Bezier; 4]),
}

impl Curve {
    /// How far (0 at this key, 1 at the next) channel `channel`'s value
    /// has gone at fraction `t` of the time between the keys. Stepped
    /// stays at 0 until the next key. A bezier may leave 0..1 to overshoot.
    #[must_use]
    pub fn ease(&self, t: f32, channel: usize) -> f32 {
        match self {
            Curve::Linear => t,
            Curve::Stepped => 0.0,
            Curve::Bezier(b) => b.ease(t),
            Curve::Channels(bs) => bs[channel.min(3)].ease(t),
        }
    }

    /// Whether every bezier's time handles are inside the segment, so
    /// time only goes forward.
    fn is_valid(&self) -> bool {
        match self {
            Curve::Linear | Curve::Stepped => true,
            Curve::Bezier(b) => b.is_valid(),
            Curve::Channels(bs) => bs.iter().all(Bezier::is_valid),
        }
    }
}

/// A cubic bezier from (0, 0) to (1, 1) through two handles, as CSS's
/// `cubic-bezier()`: x is the fraction of the time between two keys
/// (0..=1, so time only goes forward), y the fraction of the change in
/// value (outside 0..1 overshoots).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bezier {
    /// The first handle's time fraction, 0..=1.
    pub x1: f32,
    /// The first handle's value fraction.
    pub y1: f32,
    /// The second handle's time fraction, 0..=1.
    pub x2: f32,
    /// The second handle's value fraction.
    pub y2: f32,
}

impl Bezier {
    /// A straight line, as a bezier.
    pub const LINEAR: Self = Self::new(1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);
    /// CSS's `ease`: a quick start that settles slowly.
    /// ```
    /// use ponko::skeleton2d::Bezier;
    /// assert!(Bezier::EASE.ease(0.5) > 0.75);
    /// ```
    pub const EASE: Self = Self::new(0.25, 0.1, 0.25, 1.0);
    /// CSS's `ease-in`: starts slowly, ends at full speed (things leaving).
    /// ```
    /// use ponko::skeleton2d::Bezier;
    /// assert!(Bezier::EASE_IN.ease(0.5) < 0.5);
    /// ```
    pub const EASE_IN: Self = Self::new(0.42, 0.0, 1.0, 1.0);
    /// CSS's `ease-out`: starts at full speed, settles slowly (things
    /// arriving, and most UI changes).
    /// ```
    /// use ponko::skeleton2d::Bezier;
    /// assert!(Bezier::EASE_OUT.ease(0.5) > 0.5);
    /// ```
    pub const EASE_OUT: Self = Self::new(0.0, 0.0, 0.58, 1.0);
    /// CSS's `ease-in-out`: slow at both ends.
    /// ```
    /// use ponko::skeleton2d::Bezier;
    /// assert!((Bezier::EASE_IN_OUT.ease(0.5) - 0.5).abs() < 1e-5);
    /// ```
    pub const EASE_IN_OUT: Self = Self::new(0.42, 0.0, 0.58, 1.0);
    /// Overshoots a little past the end, then settles back: a pop.
    /// ```
    /// use ponko::skeleton2d::Bezier;
    /// let most = (1..100).map(|i| Bezier::BACK_OUT.ease(i as f32 / 100.0)).fold(0.0, f32::max);
    /// assert!(most > 1.05 && Bezier::BACK_OUT.ease(1.0) == 1.0);
    /// ```
    pub const BACK_OUT: Self = Self::new(0.34, 1.56, 0.64, 1.0);

    /// A curve through handles (`x1`, `y1`) and (`x2`, `y2`).
    #[must_use]
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    /// The value fraction at time fraction `t` (clamped to 0..=1): the
    /// curve's parameter is found where its time is `t`, exactly to float
    /// precision, then its value read there.
    #[must_use]
    pub fn ease(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let s = self.parameter_at(t);
        cubic(self.y1, self.y2, s)
    }

    /// The curve parameter where its time fraction is `t`: Newton's method
    /// from `t`, falling back to bisection (the time is monotonic).
    pub(crate) fn parameter_at(&self, t: f32) -> f32 {
        let (x1, x2) = (f64::from(self.x1), f64::from(self.x2));
        let t = f64::from(t);
        let x = |s: f64| 3.0 * (1.0 - s) * (1.0 - s) * s * x1 + 3.0 * (1.0 - s) * s * s * x2 + s * s * s;
        let dx = |s: f64| 3.0 * (1.0 - s) * (1.0 - s) * x1 + 6.0 * (1.0 - s) * s * (x2 - x1) + 3.0 * s * s * (1.0 - x2);
        let mut s = t;
        for _ in 0..8 {
            let error = x(s) - t;
            if error.abs() < 1e-12 {
                return s as f32;
            }
            let slope = dx(s);
            if slope.abs() < 1e-9 {
                break;
            }
            s = (s - error / slope).clamp(0.0, 1.0);
        }
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if x(mid) < t {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (0.5 * (lo + hi)) as f32
    }

    fn is_valid(&self) -> bool {
        (0.0..=1.0).contains(&self.x1) && (0.0..=1.0).contains(&self.x2) && self.y1.is_finite() && self.y2.is_finite()
    }
}

/// A cubic from 0 to 1 with inner points `a` and `b`, at parameter `s`.
fn cubic(a: f32, b: f32, s: f32) -> f32 {
    let (a, b, s) = (f64::from(a), f64::from(b), f64::from(s));
    (3.0 * (1.0 - s) * (1.0 - s) * s * a + 3.0 * (1.0 - s) * s * s * b + s * s * s) as f32
}

/// Values a clip interpolates, channel by channel.
pub(crate) trait Channels: Clone {
    /// `a` toward `b` by `t[channel]` in each channel.
    fn mix(a: &Self, b: &Self, t: [f32; 4]) -> Self;
}

impl Channels for f32 {
    fn mix(a: &Self, b: &Self, t: [f32; 4]) -> Self {
        a + (b - a) * t[0]
    }
}

impl Channels for Vec2 {
    fn mix(a: &Self, b: &Self, t: [f32; 4]) -> Self {
        Vec2::new(a.x + (b.x - a.x) * t[0], a.y + (b.y - a.y) * t[1])
    }
}

impl Channels for Color {
    fn mix(a: &Self, b: &Self, t: [f32; 4]) -> Self {
        Color::rgba(
            a.r + (b.r - a.r) * t[0],
            a.g + (b.g - a.g) * t[1],
            a.b + (b.b - a.b) * t[2],
            a.a + (b.a - a.a) * t[3],
        )
    }
}

/// A value at a time, in seconds from the clip's start.
///
/// ```
/// use ponko::skeleton2d::{Curve, Key};
/// let up = Key::new(0.5, 30f32.to_radians());
/// let snap = Key::new(1.0, 0.0).with_curve(Curve::Stepped);
/// assert_eq!(up.curve, Curve::Linear);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Key<T> {
    /// Seconds from the clip's start.
    pub time: f32,
    /// The value.
    pub value: T,
    /// How it goes to the next key's value.
    pub curve: Curve,
}

impl<T> Key<T> {
    /// `value` at `time` seconds, going linearly to the next key.
    #[must_use]
    pub fn new(time: f32, value: T) -> Self {
        Self {
            time,
            value,
            curve: Curve::Linear,
        }
    }

    /// The same key with curve `curve`.
    #[must_use]
    pub fn with_curve(self, curve: Curve) -> Self {
        Self { curve, ..self }
    }
}

/// A bone's keys. Values are relative to the bone's rest pose: translation
/// is added to it, rotation (radians) added, scale multiplied, shear added.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct BoneTrack {
    /// The bone.
    pub bone: usize,
    /// Added to the rest translation.
    pub translate: Vec<Key<Vec2>>,
    /// Added to the rest rotation, in radians. Values go straight from one
    /// key to the next (from 0 to 2π is a full turn), not the shorter way.
    pub rotate: Vec<Key<f32>>,
    /// Multiplies the rest scale.
    pub scale: Vec<Key<Vec2>>,
    /// Added to the rest shear, in radians (see
    /// [`BonePose::shear`](super::BonePose::shear)).
    pub shear: Vec<Key<Vec2>>,
    /// One axis of the translation, scale or shear keyed on its own, with
    /// its own key times (Spine's `translatex`, `scaley`... timelines).
    /// Applied after the tracks above: a channel replaces that axis of
    /// theirs.
    pub channels: Vec<ChannelTrack>,
}

/// Which axis of a bone's pose a [`ChannelTrack`] keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BoneChannel {
    /// Added to the rest translation's x.
    TranslateX,
    /// Added to the rest translation's y.
    TranslateY,
    /// Multiplies the rest scale's x.
    ScaleX,
    /// Multiplies the rest scale's y.
    ScaleY,
    /// Added to the rest shear's x (radians).
    ShearX,
    /// Added to the rest shear's y (radians).
    ShearY,
}

/// Keys of one axis of a bone's pose.
///
/// ```
/// use ponko::skeleton2d::{BoneChannel, BoneTrack, ChannelTrack, Key};
/// let mut hop = BoneTrack::new(1);
/// // Up and down twice while x is keyed once: y alone, its own times.
/// hop.channels.push(ChannelTrack::new(
///     BoneChannel::TranslateY,
///     vec![Key::new(0.0, 0.0), Key::new(0.25, -8.0), Key::new(0.5, 0.0), Key::new(0.75, -8.0), Key::new(1.0, 0.0)],
/// ));
/// hop.channels.push(ChannelTrack::new(BoneChannel::TranslateX, vec![Key::new(0.0, 0.0), Key::new(1.0, 40.0)]));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ChannelTrack {
    /// The axis it keys.
    pub channel: BoneChannel,
    /// Its keys.
    pub keys: Vec<Key<f32>>,
}

impl ChannelTrack {
    /// Keys `keys` of axis `channel`.
    #[must_use]
    pub fn new(channel: BoneChannel, keys: Vec<Key<f32>>) -> Self {
        Self { channel, keys }
    }
}

/// Keys that move a [`SkinnedMesh`](super::SkinnedMesh)'s vertices, or a
/// [`ClippingMask`](super::ClippingMask)'s corners (Spine's deform
/// timelines): while slot `slot` shows mesh `attachment` of skin `skin`,
/// each key's offsets are added to the mesh's bound vertices.
///
/// A key holds one offset per vertex and influence, four per vertex in the
/// order of [`MeshVertex::bones`](super::MeshVertex::bones): offset
/// `4 * v + k` moves vertex `v` where influence `k`'s bone sees it, in
/// that bone's space at rest (unused influences take any value). For a
/// vertex on one bone only offset `4 * v` counts. Keys blend linearly
/// (through their curve) from one to the next.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DeformTrack {
    /// The slot.
    pub slot: usize,
    /// The skin the mesh is in (0 is `"default"`).
    pub skin: usize,
    /// The mesh's name in the slot.
    pub attachment: Arc<str>,
    /// The offsets from each key on.
    pub keys: Vec<Key<Arc<[Vec2]>>>,
}

impl DeformTrack {
    /// A track for mesh `attachment` of skin `skin` in slot `slot`, with
    /// no keys.
    #[must_use]
    pub fn new(slot: usize, skin: usize, attachment: &str) -> Self {
        Self {
            slot,
            skin,
            attachment: attachment.into(),
            keys: Vec::new(),
        }
    }
}

impl BoneTrack {
    /// A track for bone `bone` with no keys.
    #[must_use]
    pub fn new(bone: usize) -> Self {
        Self {
            bone,
            ..Default::default()
        }
    }
}

/// A slot's keys: which attachment it shows (sprite swaps), whether it
/// shows anything, and its tint.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct SlotTrack {
    /// The slot.
    pub slot: usize,
    /// The attachment it shows from each key on (`None`: nothing).
    pub attachment: Vec<Key<Option<Arc<str>>>>,
    /// Whether it is drawn, from each key on.
    pub visible: Vec<Key<bool>>,
    /// Its tint, replacing the slot's rest color.
    pub color: Vec<Key<Color>>,
}

impl SlotTrack {
    /// A track for slot `slot` with no keys.
    #[must_use]
    pub fn new(slot: usize) -> Self {
        Self {
            slot,
            ..Default::default()
        }
    }
}

/// An animation of a [`SkeletonData`](super::SkeletonData): bone and slot
/// tracks, draw-order keys and events.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::skeleton2d::{BoneTrack, Clip, Key};
/// let mut wave = BoneTrack::new(1);
/// wave.rotate = vec![Key::new(0.0, 0.0), Key::new(0.5, 1.0), Key::new(1.0, 0.0)];
/// let clip = Clip::new("wave", 1.0).with_bone_track(wave).with_event(0.5, "peak");
/// assert_eq!(clip.events[0].time, 0.5); // normalized: half way
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Clip {
    /// Its name, unique in the skeleton.
    pub name: Arc<str>,
    /// Its length in seconds.
    pub duration: f32,
    /// Bone tracks, at most one per bone.
    pub bones: Vec<BoneTrack>,
    /// Slot tracks, at most one per slot.
    pub slots: Vec<SlotTrack>,
    /// Draw orders: from each key on, the slots drawn bottom to top (every
    /// slot once); `None` goes back to the rest order.
    pub draw_order: Vec<Key<Option<Arc<[u16]>>>>,
    /// Mesh deforms, at most one per mesh.
    pub deform: Vec<DeformTrack>,
    /// Events, on the clip's normalized clock (0..1), in time order.
    pub events: Vec<AnimationEvent>,
}

impl Clip {
    /// An empty clip `duration` seconds long.
    #[must_use]
    pub fn new(name: &str, duration: f32) -> Self {
        Self {
            name: name.into(),
            duration,
            bones: Vec::new(),
            slots: Vec::new(),
            draw_order: Vec::new(),
            deform: Vec::new(),
            events: Vec::new(),
        }
    }

    /// The same clip with deform track `track`.
    #[must_use]
    pub fn with_deform_track(mut self, track: DeformTrack) -> Self {
        self.deform.push(track);
        self
    }

    /// The same clip with bone track `track`.
    #[must_use]
    pub fn with_bone_track(mut self, track: BoneTrack) -> Self {
        self.bones.push(track);
        self
    }

    /// The same clip with slot track `track`.
    #[must_use]
    pub fn with_slot_track(mut self, track: SlotTrack) -> Self {
        self.slots.push(track);
        self
    }

    /// The same clip with event `name` at `seconds` from its start.
    #[must_use]
    pub fn with_event(mut self, seconds: f32, name: &str) -> Self {
        let time = if self.duration > 0.0 {
            (seconds / self.duration).clamp(0.0, 0.999_999)
        } else {
            0.0
        };
        let at = self.events.partition_point(|e| e.time <= time);
        self.events.insert(at, AnimationEvent::new(time, name));
        self
    }

    /// `Ok` if every index is below the skeleton's counts and every time
    /// and value is a number.
    pub(crate) fn check(&self, bones: usize, slots: usize) -> Result<(), SkeletonError> {
        let name = &self.name;
        if !(self.duration.is_finite() && self.duration >= 0.0) {
            return Err(err(format!("clip \"{name}\"'s duration must be 0 or more")));
        }
        for t in &self.bones {
            if t.bone >= bones {
                return Err(err(format!(
                    "clip \"{name}\" animates bone {}, which isn't there",
                    t.bone
                )));
            }
            let channels = t.channels.iter().flat_map(|c| &c.keys);
            let curves = t
                .translate
                .iter()
                .map(|k| &k.curve)
                .chain(t.rotate.iter().map(|k| &k.curve))
                .chain(t.scale.iter().map(|k| &k.curve))
                .chain(t.shear.iter().map(|k| &k.curve))
                .chain(channels.clone().map(|k| &k.curve))
                .all(Curve::is_valid);
            if !curves {
                return Err(err(format!(
                    "clip \"{name}\": bone {}'s bezier handles must be numbers with times inside their keys (0..=1)",
                    t.bone
                )));
            }
            let finite = t.translate.iter().all(|k| k.value.is_finite())
                && t.rotate.iter().all(|k| k.value.is_finite())
                && t.scale.iter().all(|k| k.value.is_finite())
                && t.shear.iter().all(|k| k.value.is_finite())
                && channels.clone().all(|k| k.value.is_finite());
            if !finite
                || !in_order(t.translate.iter().map(|k| k.time))
                || !in_order(t.rotate.iter().map(|k| k.time))
                || !in_order(t.scale.iter().map(|k| k.time))
                || !in_order(t.shear.iter().map(|k| k.time))
                || !t.channels.iter().all(|c| in_order(c.keys.iter().map(|k| k.time)))
            {
                return Err(err(format!(
                    "clip \"{name}\": bone {}'s keys need numbers, in time order",
                    t.bone
                )));
            }
        }
        for t in &self.slots {
            if t.slot >= slots {
                return Err(err(format!(
                    "clip \"{name}\" animates slot {}, which isn't there",
                    t.slot
                )));
            }
            if !t.color.iter().all(|k| k.curve.is_valid()) {
                return Err(err(format!(
                    "clip \"{name}\": slot {}'s bezier handles must be numbers with times inside their keys (0..=1)",
                    t.slot
                )));
            }
            if !in_order(t.attachment.iter().map(|k| k.time))
                || !in_order(t.visible.iter().map(|k| k.time))
                || !in_order(t.color.iter().map(|k| k.time))
            {
                return Err(err(format!(
                    "clip \"{name}\": slot {}'s keys need to be in time order",
                    t.slot
                )));
            }
        }
        for t in &self.deform {
            if t.slot >= slots {
                return Err(err(format!(
                    "clip \"{name}\" deforms a mesh in slot {}, which isn't there",
                    t.slot
                )));
            }
            let finite = t.keys.iter().all(|k| k.value.iter().all(|v| v.is_finite()));
            if !finite || !in_order(t.keys.iter().map(|k| k.time)) || !t.keys.iter().all(|k| k.curve.is_valid()) {
                return Err(err(format!(
                    "clip \"{name}\": the deform keys of \"{}\" need numbers, valid curves and time order",
                    t.attachment
                )));
            }
        }
        if !in_order(self.draw_order.iter().map(|k| k.time)) {
            return Err(err(format!(
                "clip \"{name}\"'s draw order keys need to be in time order"
            )));
        }
        for key in &self.draw_order {
            if let Some(order) = &key.value {
                let mut seen = vec![false; slots];
                let complete = order.len() == slots
                    && order
                        .iter()
                        .all(|&s| usize::from(s) < slots && !std::mem::replace(&mut seen[usize::from(s)], true));
                if !complete {
                    return Err(err(format!("clip \"{name}\": a draw order must name every slot once")));
                }
            }
        }
        if !self.events.iter().all(|e| (0.0..1.0).contains(&e.time)) {
            return Err(err(format!("clip \"{name}\"'s events must be inside it")));
        }
        Ok(())
    }

    /// Writes the clip's pose at `seconds` over `pose` for the bones it
    /// animates (rest pose plus the tracks' values).
    pub(crate) fn sample_bones(&self, seconds: f32, bones: &[Bone], pose: &mut [BonePose]) {
        for track in &self.bones {
            let rest = bones[track.bone].rest;
            let mut p = rest;
            if let Some(v) = sample(&track.translate, seconds) {
                p.translation = rest.translation + v;
            }
            if let Some(v) = sample(&track.rotate, seconds) {
                p.rotation = rest.rotation + v;
            }
            if let Some(v) = sample(&track.scale, seconds) {
                p.scale = rest.scale * v;
            }
            if let Some(v) = sample(&track.shear, seconds) {
                p.shear = rest.shear + v;
            }
            for c in &track.channels {
                let Some(v) = sample(&c.keys, seconds) else {
                    continue;
                };
                match c.channel {
                    BoneChannel::TranslateX => p.translation.x = rest.translation.x + v,
                    BoneChannel::TranslateY => p.translation.y = rest.translation.y + v,
                    BoneChannel::ScaleX => p.scale.x = rest.scale.x * v,
                    BoneChannel::ScaleY => p.scale.y = rest.scale.y * v,
                    BoneChannel::ShearX => p.shear.x = rest.shear.x + v,
                    BoneChannel::ShearY => p.shear.y = rest.shear.y + v,
                }
            }
            pose[track.bone] = p;
        }
    }
}

/// Whether `times` are numbers in increasing order (equal ones allowed).
fn in_order(times: impl Iterator<Item = f32>) -> bool {
    let mut last = f32::MIN;
    for t in times {
        if !(t.is_finite() && t >= last) {
            return false;
        }
        last = t;
    }
    true
}

/// The value of `keys` at `time`: the first key's before it, the last's
/// after it, between two keys their curve's. `None` without keys.
pub(crate) fn sample<T: Channels>(keys: &[Key<T>], time: f32) -> Option<T> {
    let next = keys.partition_point(|k| k.time <= time);
    if next == 0 {
        return keys.first().map(|k| k.value.clone());
    }
    let a = &keys[next - 1];
    let Some(b) = keys.get(next) else {
        return Some(a.value.clone());
    };
    if a.curve == Curve::Stepped {
        return Some(a.value.clone());
    }
    let span = b.time - a.time;
    let t = if span > 0.0 { (time - a.time) / span } else { 1.0 };
    let eased = [0, 1, 2, 3].map(|c| a.curve.ease(t, c));
    Some(T::mix(&a.value, &b.value, eased))
}

/// Writes the offsets of deform `keys` at `time` into `out` (resized to
/// them). `false` without keys.
pub(crate) fn sample_deform(keys: &[Key<Arc<[Vec2]>>], time: f32, out: &mut Vec<Vec2>) -> bool {
    if keys.is_empty() {
        return false;
    }
    let next = keys.partition_point(|k| k.time <= time);
    let a = &keys[next.saturating_sub(1)];
    out.clear();
    match keys.get(next) {
        Some(b) if next > 0 && a.curve != Curve::Stepped => {
            let span = b.time - a.time;
            let t = a.curve.ease(if span > 0.0 { (time - a.time) / span } else { 1.0 }, 0);
            out.extend(a.value.iter().zip(b.value.iter()).map(|(x, y)| *x + (*y - *x) * t));
        }
        _ => out.extend_from_slice(&a.value),
    }
    true
}

/// The key of `keys` in effect at `time` (the last at or before it).
pub(crate) fn step<T>(keys: &[Key<T>], time: f32) -> Option<&T> {
    let next = keys.partition_point(|k| k.time <= time);
    next.checked_sub(1).map(|i| &keys[i].value)
}
