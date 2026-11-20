//! Declaring what replicates: the [`Replicate`] trait (usually derived), the
//! [`Fields`] visitor its one method calls, and how each number travels
//! ([`Quantize`]). A value becomes a short list of 32-bit words, each with
//! its own width on the wire; the server compares words to find what
//! changed, and both sides turn words back into values the same way.

use std::f32::consts::TAU;

use glam::{Quat, Vec2, Vec3};

use super::bits::{bits_for, mask32};

/// How one number travels: exactly, or rounded to a range and step so it
/// takes fewer bits. Set per field with `#[replicate(..)]` on a
/// [`Replicate`] derive, or passed to the [`Fields`] calls of a hand-written
/// [`Replicate::fields`].
///
/// | Variant | For | Bits |
/// |---|---|---|
/// | `Exact` | anything | the type's own (32 for `f32`, 8 for `u8`, 1 for `bool`; 4 × 32 for a `Quat`) |
/// | `Range { min, max, step }` | floats (and each component of `Vec2`, `Vec3`) | enough for `(max - min) / step` steps |
/// | `Angle { bits }` | an angle in radians, wrapped to one turn | `bits` |
/// | `Bits(n)` | integers (the low `n` bits; signed ones keep their sign), and rotations (`Quat`: `n` bits for each of the three smallest components, 2 to 10) | `n` (a `Quat`: `2 + 3n`) |
///
/// ```
/// use ponko::net::Quantize;
/// // A position in a 200-unit arena, to the centimetre: 15 bits instead of 32.
/// let q = Quantize::range(-100.0, 100.0, 0.01);
/// assert_eq!(q.bits_per_value(), Some(15));
/// assert_eq!(Quantize::Exact.bits_per_value(), None); // depends on the type
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Quantize {
    /// The value as it is, at its type's full width.
    Exact,
    /// A float between `min` and `max`, rounded to the nearest multiple of
    /// `step` above `min`; values outside are clamped. Both sides see the
    /// rounded value.
    Range {
        /// The smallest value sent.
        min: f32,
        /// The largest value sent.
        max: f32,
        /// The precision: values are rounded to `min + k * step`.
        step: f32,
    },
    /// An angle in radians, wrapped to `0..TAU` and rounded to one of
    /// `2^bits` steps (2 to 32 bits). The other side sees the wrapped angle.
    Angle {
        /// Bits per angle.
        bits: u8,
    },
    /// The low `n` bits of an integer (1 up to the type's width); a signed
    /// integer is sign-extended back. For a `Quat`, the bits of each of the
    /// three smallest components (2 to 10).
    Bits(u8),
}

impl Quantize {
    /// [`Quantize::Range`]: `min..=max` in steps of `step`.
    ///
    /// ```
    /// let q = ponko::net::Quantize::range(0.0, 1.0, 0.25);
    /// assert_eq!(q.bits_per_value(), Some(3)); // 0, 0.25, 0.5, 0.75, 1
    /// ```
    #[must_use]
    pub const fn range(min: f32, max: f32, step: f32) -> Self {
        Self::Range { min, max, step }
    }

    /// Bits one number takes when this doesn't depend on its type: for
    /// `Range`, `Angle` and `Bits`; `None` for `Exact` (or an invalid range).
    ///
    /// ```
    /// use ponko::net::Quantize;
    /// assert_eq!(Quantize::Angle { bits: 12 }.bits_per_value(), Some(12));
    /// assert_eq!(Quantize::Bits(5).bits_per_value(), Some(5));
    /// ```
    #[must_use]
    pub fn bits_per_value(&self) -> Option<u32> {
        match *self {
            Self::Exact => None,
            Self::Range { min, max, step } => range_steps(min, max, step).map(bits_for),
            Self::Angle { bits } | Self::Bits(bits) => Some(u32::from(bits)),
        }
    }
}

/// The number of steps of a range, if it is a valid one.
fn range_steps(min: f32, max: f32, step: f32) -> Option<u32> {
    if !(min.is_finite() && max.is_finite() && step.is_finite() && step > 0.0 && max > min) {
        return None;
    }
    let steps = ((f64::from(max) - f64::from(min)) / f64::from(step)).round();
    (steps >= 1.0 && steps <= f64::from(u32::MAX)).then_some(steps as u32)
}

/// One word of a replicated value: its name (for messages and the schema
/// check between server and client) and its width on the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FieldSpec {
    pub(crate) name: String,
    pub(crate) bits: u8,
}

enum Mode<'a> {
    Describe(&'a mut Vec<FieldSpec>),
    Write { out: &'a mut [u32], at: usize },
    Read { from: &'a [u32], at: usize },
}

/// What [`Replicate::fields`] visits: one call per field, with how it
/// travels. The same calls describe the fields (once, at registration),
/// read them out for sending and write received values back, so one list
/// can't disagree with itself. Visit the same fields in the same order
/// every time, whatever the value.
///
/// ```
/// use ponko::net::{Fields, Quantize, Replicate};
/// use ponko::prelude::*;
///
/// #[derive(Clone, Default, PartialEq, Debug)]
/// struct Ship { pos: Vec2, heading: f32, shield: u8, boosting: bool }
///
/// impl Replicate for Ship {
///     fn fields(&mut self, f: &mut Fields<'_>) {
///         f.vec2("pos", &mut self.pos, Quantize::range(-500.0, 500.0, 0.05));
///         f.f32("heading", &mut self.heading, Quantize::Angle { bits: 10 });
///         f.u8("shield", &mut self.shield, Quantize::Bits(7));
///         f.bool("boosting", &mut self.boosting);
///     }
/// }
/// // 2 × 15 + 10 + 7 + 1 bits instead of 2 × 32 + 32 + 8 + 8.
/// assert_eq!(ponko::net::replicated_bits::<Ship>(), 48);
/// ```
pub struct Fields<'a> {
    mode: Mode<'a>,
}

impl<'a> Fields<'a> {
    pub(crate) fn describe(out: &'a mut Vec<FieldSpec>) -> Self {
        Self {
            mode: Mode::Describe(out),
        }
    }
    pub(crate) fn writer(out: &'a mut [u32]) -> Self {
        Self {
            mode: Mode::Write { out, at: 0 },
        }
    }
    pub(crate) fn reader(from: &'a [u32]) -> Self {
        Self {
            mode: Mode::Read { from, at: 0 },
        }
    }

    /// One word: describe it, hand out its current value, or take the new one.
    fn word(&mut self, name: impl FnOnce() -> String, bits: u32, get: impl FnOnce() -> u32) -> Option<u32> {
        match &mut self.mode {
            Mode::Describe(specs) => {
                specs.push(FieldSpec {
                    name: name(),
                    bits: bits as u8,
                });
                None
            }
            Mode::Write { out, at } => {
                if let Some(slot) = out.get_mut(*at) {
                    *slot = get() & mask32(bits);
                }
                *at += 1;
                None
            }
            Mode::Read { from, at } => {
                let word = from.get(*at).copied();
                *at += 1;
                word
            }
        }
    }

    /// A float, quantized as `quantize` says (`Exact`, `Range` or `Angle`).
    ///
    /// # Panics
    ///
    /// When the fields are described (at registration), for `Bits`, an
    /// invalid range or an angle outside 2 to 32 bits.
    ///
    /// ```
    /// use ponko::net::{Fields, Quantize, Replicate};
    /// #[derive(Clone, Default)]
    /// struct Fuel(f32);
    /// impl Replicate for Fuel {
    ///     fn fields(&mut self, f: &mut Fields<'_>) {
    ///         f.f32("fuel", &mut self.0, Quantize::range(0.0, 100.0, 0.5));
    ///     }
    /// }
    /// assert_eq!(ponko::net::replicated_bits::<Fuel>(), 8);
    /// ```
    pub fn f32(&mut self, name: &str, value: &mut f32, quantize: Quantize) {
        let bits = float_bits(name, quantize);
        let current = *value;
        if let Some(word) = self.word(|| name.to_string(), bits, || encode_f32(current, quantize)) {
            *value = decode_f32(word, quantize);
        }
    }

    /// A 2D vector: each component as [`f32`](Self::f32) with the same
    /// quantization.
    ///
    /// ```
    /// use ponko::net::{Fields, Quantize, Replicate};
    /// use ponko::prelude::*;
    /// #[derive(Clone, Default)]
    /// struct At(Vec2);
    /// impl Replicate for At {
    ///     fn fields(&mut self, f: &mut Fields<'_>) {
    ///         f.vec2("at", &mut self.0, Quantize::range(0.0, 1024.0, 0.25));
    ///     }
    /// }
    /// assert_eq!(ponko::net::replicated_bits::<At>(), 26); // 4097 steps: 13 bits each
    /// ```
    pub fn vec2(&mut self, name: &str, value: &mut Vec2, quantize: Quantize) {
        self.part(name, "x", &mut value.x, quantize);
        self.part(name, "y", &mut value.y, quantize);
    }

    /// A 3D vector: each component as [`f32`](Self::f32) with the same
    /// quantization.
    ///
    /// ```
    /// use ponko::net::{Fields, Quantize, Replicate};
    /// use ponko::prelude::*;
    /// #[derive(Clone, Default)]
    /// struct At(Vec3);
    /// impl Replicate for At {
    ///     fn fields(&mut self, f: &mut Fields<'_>) {
    ///         f.vec3("at", &mut self.0, Quantize::Exact);
    ///     }
    /// }
    /// assert_eq!(ponko::net::replicated_bits::<At>(), 96);
    /// ```
    pub fn vec3(&mut self, name: &str, value: &mut Vec3, quantize: Quantize) {
        self.part(name, "x", &mut value.x, quantize);
        self.part(name, "y", &mut value.y, quantize);
        self.part(name, "z", &mut value.z, quantize);
    }

    fn part(&mut self, name: &str, part: &str, value: &mut f32, quantize: Quantize) {
        let bits = float_bits(name, quantize);
        let current = *value;
        if let Some(word) = self.word(|| format!("{name}.{part}"), bits, || encode_f32(current, quantize)) {
            *value = decode_f32(word, quantize);
        }
    }

    /// A rotation. `Exact` sends its four components (128 bits);
    /// `Bits(n)` (2 to 10) sends the three smallest in `n` bits each plus
    /// which one was left out (`2 + 3n` bits; 10 bits is within 0.002 per
    /// component). The other side gets a normalized rotation.
    ///
    /// # Panics
    ///
    /// When described, for `Range`, `Angle` or `Bits` outside 2 to 10.
    ///
    /// ```
    /// use ponko::net::{Fields, Quantize, Replicate};
    /// use ponko::prelude::*;
    /// #[derive(Clone, Default)]
    /// struct Facing(Quat);
    /// impl Replicate for Facing {
    ///     fn fields(&mut self, f: &mut Fields<'_>) {
    ///         f.quat("facing", &mut self.0, Quantize::Bits(9));
    ///     }
    /// }
    /// assert_eq!(ponko::net::replicated_bits::<Facing>(), 29);
    /// ```
    pub fn quat(&mut self, name: &str, value: &mut Quat, quantize: Quantize) {
        match quantize {
            Quantize::Exact => {
                let mut v = value.to_array();
                for (part, c) in ["x", "y", "z", "w"].into_iter().zip(&mut v) {
                    self.part(name, part, c, Quantize::Exact);
                }
                if matches!(self.mode, Mode::Read { .. }) {
                    *value = Quat::from_array(v);
                }
            }
            Quantize::Bits(n) if (2..=10).contains(&n) => {
                let current = *value;
                let bits = 2 + 3 * u32::from(n);
                if let Some(word) = self.word(|| name.to_string(), bits, || encode_quat(current, u32::from(n))) {
                    *value = decode_quat(word, u32::from(n));
                }
            }
            other => panic!(
                "replicated field `{name}`: a rotation takes Quantize::Exact or Quantize::Bits(2..=10), not {other:?}"
            ),
        }
    }

    /// A flag: one bit.
    ///
    /// ```
    /// use ponko::net::{Fields, Replicate};
    /// #[derive(Clone, Default)]
    /// struct Lit(bool);
    /// impl Replicate for Lit {
    ///     fn fields(&mut self, f: &mut Fields<'_>) { f.bool("lit", &mut self.0); }
    /// }
    /// assert_eq!(ponko::net::replicated_bits::<Lit>(), 1);
    /// ```
    pub fn bool(&mut self, name: &str, value: &mut bool) {
        let current = *value;
        if let Some(word) = self.word(|| name.to_string(), 1, || u32::from(current)) {
            *value = word & 1 != 0;
        }
    }

    /// An unsigned integer of up to 32 bits: `Exact` (32) or `Bits(n)`, the
    /// low `n` bits.
    ///
    /// # Panics
    ///
    /// When described, for a float quantization or `Bits` outside 1 to 32.
    ///
    /// ```
    /// use ponko::net::{Fields, Quantize, Replicate};
    /// #[derive(Clone, Default)]
    /// struct Ammo(u32);
    /// impl Replicate for Ammo {
    ///     fn fields(&mut self, f: &mut Fields<'_>) { f.u32("ammo", &mut self.0, Quantize::Bits(10)); }
    /// }
    /// assert_eq!(ponko::net::replicated_bits::<Ammo>(), 10);
    /// ```
    pub fn u32(&mut self, name: &str, value: &mut u32, quantize: Quantize) {
        let bits = int_bits(name, quantize, 32);
        let current = *value;
        if let Some(word) = self.word(|| name.to_string(), bits, || current) {
            *value = word & mask32(bits);
        }
    }

    /// A signed integer of up to 32 bits: `Exact` (32) or `Bits(n)`, the
    /// low `n` bits, sign-extended back (`Bits(8)` holds -128 to 127).
    ///
    /// # Panics
    ///
    /// As [`u32`](Self::u32).
    ///
    /// ```
    /// use ponko::net::{Fields, Quantize, Replicate};
    /// #[derive(Clone, Default)]
    /// struct Lane(i32);
    /// impl Replicate for Lane {
    ///     fn fields(&mut self, f: &mut Fields<'_>) { f.i32("lane", &mut self.0, Quantize::Bits(4)); }
    /// }
    /// assert_eq!(ponko::net::replicated_bits::<Lane>(), 4);
    /// ```
    pub fn i32(&mut self, name: &str, value: &mut i32, quantize: Quantize) {
        let bits = int_bits(name, quantize, 32);
        let current = *value;
        if let Some(word) = self.word(|| name.to_string(), bits, || current as u32) {
            *value = sign_extend(word, bits);
        }
    }

    /// An unsigned integer of up to 8 bits, as [`u32`](Self::u32).
    ///
    /// # Panics
    ///
    /// As [`u32`](Self::u32), and for `Bits` above 8.
    ///
    /// ```
    /// use ponko::net::{Fields, Quantize, Replicate};
    /// #[derive(Clone, Default)]
    /// struct Team(u8);
    /// impl Replicate for Team {
    ///     fn fields(&mut self, f: &mut Fields<'_>) { f.u8("team", &mut self.0, Quantize::Bits(2)); }
    /// }
    /// assert_eq!(ponko::net::replicated_bits::<Team>(), 2);
    /// ```
    pub fn u8(&mut self, name: &str, value: &mut u8, quantize: Quantize) {
        let bits = int_bits(name, quantize, 8);
        let current = *value;
        if let Some(word) = self.word(|| name.to_string(), bits, || u32::from(current)) {
            *value = (word & mask32(bits)) as u8;
        }
    }

    /// An unsigned integer of up to 16 bits, as [`u32`](Self::u32).
    ///
    /// # Panics
    ///
    /// As [`u32`](Self::u32), and for `Bits` above 16.
    ///
    /// ```
    /// use ponko::net::{Fields, Quantize, Replicate};
    /// #[derive(Clone, Default)]
    /// struct Score(u16);
    /// impl Replicate for Score {
    ///     fn fields(&mut self, f: &mut Fields<'_>) { f.u16("score", &mut self.0, Quantize::Exact); }
    /// }
    /// assert_eq!(ponko::net::replicated_bits::<Score>(), 16);
    /// ```
    pub fn u16(&mut self, name: &str, value: &mut u16, quantize: Quantize) {
        let bits = int_bits(name, quantize, 16);
        let current = *value;
        if let Some(word) = self.word(|| name.to_string(), bits, || u32::from(current)) {
            *value = (word & mask32(bits)) as u16;
        }
    }

    /// A signed integer of up to 8 bits, as [`i32`](Self::i32).
    ///
    /// # Panics
    ///
    /// As [`u32`](Self::u32), and for `Bits` above 8.
    ///
    /// ```
    /// use ponko::net::{Fields, Quantize, Replicate};
    /// #[derive(Clone, Default)]
    /// struct Stick(i8);
    /// impl Replicate for Stick {
    ///     fn fields(&mut self, f: &mut Fields<'_>) { f.i8("x", &mut self.0, Quantize::Exact); }
    /// }
    /// assert_eq!(ponko::net::replicated_bits::<Stick>(), 8);
    /// ```
    pub fn i8(&mut self, name: &str, value: &mut i8, quantize: Quantize) {
        let bits = int_bits(name, quantize, 8);
        let current = *value;
        if let Some(word) = self.word(|| name.to_string(), bits, || current as u8 as u32) {
            *value = sign_extend(word, bits) as i8;
        }
    }

    /// A signed integer of up to 16 bits, as [`i32`](Self::i32).
    ///
    /// # Panics
    ///
    /// As [`u32`](Self::u32), and for `Bits` above 16.
    ///
    /// ```
    /// use ponko::net::{Fields, Quantize, Replicate};
    /// #[derive(Clone, Default)]
    /// struct Depth(i16);
    /// impl Replicate for Depth {
    ///     fn fields(&mut self, f: &mut Fields<'_>) { f.i16("depth", &mut self.0, Quantize::Bits(12)); }
    /// }
    /// assert_eq!(ponko::net::replicated_bits::<Depth>(), 12);
    /// ```
    pub fn i16(&mut self, name: &str, value: &mut i16, quantize: Quantize) {
        let bits = int_bits(name, quantize, 16);
        let current = *value;
        if let Some(word) = self.word(|| name.to_string(), bits, || current as u16 as u32) {
            *value = sign_extend(word, bits) as i16;
        }
    }
}

fn float_bits(name: &str, quantize: Quantize) -> u32 {
    match quantize {
        Quantize::Exact => 32,
        Quantize::Range { min, max, step } => match range_steps(min, max, step) {
            Some(steps) => bits_for(steps).max(1),
            None => panic!(
                "replicated field `{name}`: Quantize::Range needs finite min < max and a positive step that divides the range into at most 2^32 - 1 steps (got {min}..{max} by {step})"
            ),
        },
        Quantize::Angle { bits } if (2..=32).contains(&bits) => u32::from(bits),
        other => panic!(
            "replicated field `{name}`: a float takes Quantize::Exact, Range or Angle {{ bits: 2..=32 }}, not {other:?}"
        ),
    }
}

fn int_bits(name: &str, quantize: Quantize, width: u32) -> u32 {
    match quantize {
        Quantize::Exact => width,
        Quantize::Bits(n) if (1..=width).contains(&u32::from(n)) => u32::from(n),
        other => panic!(
            "replicated field `{name}`: a {width}-bit integer takes Quantize::Exact or Bits(1..={width}), not {other:?}"
        ),
    }
}

fn sign_extend(word: u32, bits: u32) -> i32 {
    if bits >= 32 {
        return word as i32;
    }
    let shift = 32 - bits;
    ((word << shift) as i32) >> shift
}

fn encode_f32(value: f32, quantize: Quantize) -> u32 {
    match quantize {
        Quantize::Range { min, max, step } => {
            let steps = range_steps(min, max, step).unwrap_or(0);
            if value.is_nan() {
                return 0;
            }
            // On the grid from `min` to `max` in `steps` equal steps (not
            // by adding up `step`, which as an f32 isn't the decimal it was
            // written as): 0 of a range around it comes back as exactly 0.
            let (lo, span, n) = (f64::from(min), f64::from(max) - f64::from(min), f64::from(steps));
            let q = ((f64::from(value) - lo) / span * n).round();
            q.clamp(0.0, n) as u32
        }
        Quantize::Angle { bits } => {
            if !value.is_finite() {
                return 0;
            }
            let turns = f64::from(value).rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU;
            let scale = (1u64 << bits) as f64;
            ((turns * scale).round() as u64 & ((1u64 << bits) - 1)) as u32
        }
        _ => value.to_bits(),
    }
}

fn decode_f32(word: u32, quantize: Quantize) -> f32 {
    match quantize {
        Quantize::Range { min, max, step } => {
            let steps = range_steps(min, max, step).unwrap_or(0);
            let (lo, span, n) = (f64::from(min), f64::from(max) - f64::from(min), f64::from(steps));
            let q = f64::from(word.min(steps));
            ((lo + span * q / n) as f32).clamp(min, max)
        }
        Quantize::Angle { bits } => {
            let scale = (1u64 << bits) as f64;
            (f64::from(word) / scale * std::f64::consts::TAU) as f32 % TAU
        }
        _ => f32::from_bits(word),
    }
}

/// Smallest three: the index of the largest component (2 bits), then the
/// other three in `n` bits each, scaled from ±1/√2.
fn encode_quat(q: Quat, n: u32) -> u32 {
    let q = if q.is_finite() && q.length_squared() > 0.0 {
        q.normalize()
    } else {
        Quat::IDENTITY
    };
    let c = q.to_array();
    let largest = (0..4).max_by(|&a, &b| c[a].abs().total_cmp(&c[b].abs())).unwrap_or(3);
    // q and -q are the same rotation: make the dropped component positive.
    let sign = if c[largest] < 0.0 { -1.0 } else { 1.0 };
    let max = mask32(n) as f32;
    let mut word = largest as u32;
    let mut shift = 2;
    for (i, v) in c.iter().enumerate() {
        if i == largest {
            continue;
        }
        let unit = (v * sign * std::f32::consts::SQRT_2 + 1.0) * 0.5; // 0..1
        let k = (unit.clamp(0.0, 1.0) * max).round() as u32;
        word |= k << shift;
        shift += n;
    }
    word
}

fn decode_quat(word: u32, n: u32) -> Quat {
    let largest = (word & 3) as usize;
    let max = mask32(n) as f32;
    let mut c = [0.0f32; 4];
    let mut shift = 2;
    let mut sum = 0.0;
    for (i, slot) in c.iter_mut().enumerate() {
        if i == largest {
            continue;
        }
        let k = (word >> shift) & mask32(n);
        let v = (k as f32 / max * 2.0 - 1.0) / std::f32::consts::SQRT_2;
        *slot = v;
        sum += v * v;
        shift += n;
    }
    c[largest] = (1.0 - sum).max(0.0).sqrt();
    Quat::from_array(c).normalize()
}

/// A type whose values replicate: a component the server sends to its
/// clients, an input a client sends to the server, or both. One method
/// lists its fields and how each travels; derive it with
/// `#[derive(Replicate)]` and per-field `#[replicate(..)]` options, or
/// write it by hand with the [`Fields`] calls.
///
/// | Field option | Means |
/// |---|---|
/// | (none) | [`Quantize::Exact`] |
/// | `#[replicate(min = -100.0, max = 100.0, step = 0.01)]` | [`Quantize::Range`] (each component of a vector) |
/// | `#[replicate(angle, bits = 12)]` | [`Quantize::Angle`] |
/// | `#[replicate(bits = 5)]` | [`Quantize::Bits`]: integers, and rotations (smallest three) |
/// | `#[replicate(skip)]` | not sent: the other side keeps its own value |
///
/// Fields of type `f32`, `bool`, `u8`, `u16`, `u32`, `i8`, `i16`, `i32`,
/// `Vec2`, `Vec3`, `Quat` and [`NetId`](crate::net::NetId) work as they
/// are; implement [`ReplicateField`] for your own (an enum, say).
///
/// ```
/// use ponko::net::Replicate;
/// use ponko::prelude::*;
///
/// #[derive(Component, Replicate, Clone, Default, PartialEq, Debug)]
/// struct Mover {
///     #[replicate(min = -64.0, max = 64.0, step = 0.01)]
///     pos: Vec2,
///     #[replicate(min = -20.0, max = 20.0, step = 0.01)]
///     vel: Vec2,
///     #[replicate(angle, bits = 10)]
///     facing: f32,
///     #[replicate(skip)]
///     trail: u32, // drawn locally, never sent
/// }
/// // 14 bits for each part of `pos`, 12 for `vel`'s, 10 for `facing`.
/// assert_eq!(ponko::net::replicated_bits::<Mover>(), 2 * 14 + 2 * 12 + 10);
/// ```
pub trait Replicate: Clone + Default + Send + Sync + 'static {
    /// Visits every replicated field, in the same order every time, each
    /// with how it travels.
    fn fields(&mut self, fields: &mut Fields<'_>);
}

/// A field type [`#[derive(Replicate)]`](Replicate) knows how to send.
/// Implement it for your own field types: an enum through
/// [`Fields::u8`], a struct field by field.
///
/// ```
/// use ponko::net::{Fields, Quantize, Replicate, ReplicateField};
///
/// #[derive(Clone, Copy, Default, PartialEq, Debug)]
/// enum Stance { #[default] Stand, Crouch, Prone }
///
/// impl ReplicateField for Stance {
///     fn replicate_field(&mut self, name: &str, _: Quantize, f: &mut Fields<'_>) {
///         let mut n = *self as u8;
///         f.u8(name, &mut n, Quantize::Bits(2));
///         *self = match n { 1 => Stance::Crouch, 2 => Stance::Prone, _ => Stance::Stand };
///     }
/// }
///
/// #[derive(Replicate, Clone, Default)]
/// struct Soldier { stance: Stance }
/// assert_eq!(ponko::net::replicated_bits::<Soldier>(), 2);
/// ```
pub trait ReplicateField {
    /// Visits this field (or its parts) under `name`, quantized as the
    /// field's `#[replicate(..)]` options say.
    fn replicate_field(&mut self, name: &str, quantize: Quantize, fields: &mut Fields<'_>);
}

macro_rules! field_impl {
    ($($ty:ty => $method:ident),* $(,)?) => {$(
        impl ReplicateField for $ty {
            fn replicate_field(&mut self, name: &str, quantize: Quantize, fields: &mut Fields<'_>) {
                fields.$method(name, self, quantize);
            }
        }
    )*};
}
field_impl!(
    f32 => f32, u8 => u8, u16 => u16, u32 => u32, i8 => i8, i16 => i16, i32 => i32,
    Vec2 => vec2, Vec3 => vec3, Quat => quat,
);
impl ReplicateField for bool {
    fn replicate_field(&mut self, name: &str, _: Quantize, fields: &mut Fields<'_>) {
        fields.bool(name, self);
    }
}

/// The 2D transform, exactly (5 × 32 bits whole; deltas send only what
/// moved). Declare your own component with ranges for fewer bits.
/// ```
/// assert_eq!(ponko::net::replicated_bits::<ponko::d2::Transform>(), 5 * 32);
/// ```
impl Replicate for crate::d2::Transform {
    fn fields(&mut self, f: &mut Fields<'_>) {
        f.vec2("position", &mut self.position, Quantize::Exact);
        f.f32("rotation", &mut self.rotation, Quantize::Exact);
        f.vec2("scale", &mut self.scale, Quantize::Exact);
    }
}

/// The 3D transform, exactly (10 × 32 bits whole; deltas send only what
/// moved).
/// ```
/// assert_eq!(ponko::net::replicated_bits::<ponko::d3::Transform>(), 10 * 32);
/// ```
#[cfg(feature = "3d")]
impl Replicate for crate::d3::Transform {
    fn fields(&mut self, f: &mut Fields<'_>) {
        f.vec3("position", &mut self.position, Quantize::Exact);
        f.quat("rotation", &mut self.rotation, Quantize::Exact);
        f.vec3("scale", &mut self.scale, Quantize::Exact);
    }
}

/// Bits one value of `T` takes on the wire when it is sent whole (before
/// delta compression, which sends only the fields that changed).
///
/// # Panics
///
/// If `T`'s fields ask for a quantization their type can't take (see
/// [`Fields`]).
///
/// ```
/// use ponko::net::Replicate;
/// #[derive(Replicate, Clone, Default)]
/// struct Health { #[replicate(bits = 7)] hp: u8, dead: bool }
/// assert_eq!(ponko::net::replicated_bits::<Health>(), 8);
/// ```
#[must_use]
pub fn replicated_bits<T: Replicate>() -> u32 {
    schema_of::<T>().iter().map(|f| u32::from(f.bits)).sum()
}

/// The words of `T`, from a default value.
pub(crate) fn schema_of<T: Replicate>() -> Vec<FieldSpec> {
    let mut specs = Vec::new();
    T::default().fields(&mut Fields::describe(&mut specs));
    specs
}

/// `value`'s words into `out` (as many as its schema has).
pub(crate) fn write_words<T: Replicate>(value: &T, out: &mut [u32]) {
    value.clone().fields(&mut Fields::writer(out));
}

/// Sets `value`'s replicated fields from `words`; the others stay.
pub(crate) fn read_words<T: Replicate>(value: &mut T, words: &[u32]) {
    value.fields(&mut Fields::reader(words));
}

/// Rounds `value` to what the other side would receive.
pub(crate) fn quantize_in_place<T: Replicate>(value: &mut T, scratch: &mut [u32]) {
    write_words(value, scratch);
    read_words(value, scratch);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default, PartialEq, Debug)]
    struct All {
        exact: f32,
        ranged: f32,
        angle: f32,
        flag: bool,
        small: u8,
        signed: i32,
        v2: Vec2,
        v3: Vec3,
        q: Quat,
        qe: Quat,
    }
    impl Replicate for All {
        fn fields(&mut self, f: &mut Fields<'_>) {
            f.f32("exact", &mut self.exact, Quantize::Exact);
            f.f32("ranged", &mut self.ranged, Quantize::range(-10.0, 10.0, 0.1));
            f.f32("angle", &mut self.angle, Quantize::Angle { bits: 12 });
            f.bool("flag", &mut self.flag);
            f.u8("small", &mut self.small, Quantize::Bits(3));
            f.i32("signed", &mut self.signed, Quantize::Bits(6));
            f.vec2("v2", &mut self.v2, Quantize::range(0.0, 1.0, 0.5));
            f.vec3("v3", &mut self.v3, Quantize::Exact);
            f.quat("q", &mut self.q, Quantize::Bits(10));
            f.quat("qe", &mut self.qe, Quantize::Exact);
        }
    }

    fn round_trip<T: Replicate>(value: &T) -> T {
        let words = schema_of::<T>().len();
        let mut buf = vec![0; words];
        write_words(value, &mut buf);
        let mut out = T::default();
        read_words(&mut out, &buf);
        out
    }

    #[test]
    fn the_schema_lists_one_word_per_number_with_its_width() {
        let specs = schema_of::<All>();
        let bits: Vec<u8> = specs.iter().map(|s| s.bits).collect();
        // 0, 0.5 and 1: three values, two bits.
        assert_eq!(bits, [32, 8, 12, 1, 3, 6, 2, 2, 32, 32, 32, 32, 32, 32, 32, 32]);
        assert_eq!(specs[6].name, "v2.x");
        assert_eq!(specs[8].name, "v3.x");
        assert_eq!(specs[11].name, "q");
        assert_eq!(specs[12].name, "qe.x");
    }

    #[test]
    fn quantized_values_round_trip_within_their_step_and_exact_ones_exactly() {
        let v = All {
            exact: 1.234_567_9,
            ranged: 3.0625,
            angle: -0.5,
            flag: true,
            small: 13, // 3 bits: 5
            signed: -7,
            v2: vec2(0.3, 2.0),
            v3: Vec3::new(1.0, -2.0, 1e-9),
            q: Quat::from_rotation_y(1.0) * Quat::from_rotation_x(0.3),
            qe: Quat::from_rotation_z(2.0),
        };
        let back = round_trip(&v);
        assert_eq!(back.exact, v.exact);
        assert!((back.ranged - 3.1).abs() < 1e-5, "{}", back.ranged);
        assert!((back.angle - (TAU - 0.5)).abs() < TAU / 4096.0);
        assert!(back.flag);
        assert_eq!(back.small, 5);
        assert_eq!(back.signed, -7);
        assert_eq!(back.v2, vec2(0.5, 1.0), "rounded, then clamped");
        assert_eq!(back.v3, v.v3);
        assert!(back.q.angle_between(v.q) < 0.005, "{}", back.q.angle_between(v.q));
        assert_eq!(back.qe, v.qe);
        // Rounding is stable: what arrives sends the same words again.
        assert_eq!(round_trip(&back), back);
    }

    #[test]
    fn the_sign_of_a_rotation_does_not_matter() {
        let q = Quat::from_rotation_y(2.5);
        let (a, b) = (encode_quat(q, 9), encode_quat(-q, 9));
        assert_eq!(a, b);
        assert!(decode_quat(a, 9).angle_between(q) < 0.01);
    }

    #[test]
    fn non_finite_floats_travel_as_the_bottom_of_their_range() {
        let mut nan = All {
            ranged: f32::NAN,
            angle: f32::INFINITY,
            ..All::default()
        };
        nan = round_trip(&nan);
        assert_eq!(nan.ranged, -10.0);
        assert_eq!(nan.angle, 0.0);
    }

    #[test]
    #[should_panic(expected = "a float takes Quantize::Exact, Range or Angle")]
    fn bits_on_a_float_is_refused_when_described() {
        #[derive(Clone, Default)]
        struct Bad(f32);
        impl Replicate for Bad {
            fn fields(&mut self, f: &mut Fields<'_>) {
                f.f32("bad", &mut self.0, Quantize::Bits(4));
            }
        }
        let _ = schema_of::<Bad>();
    }

    #[test]
    #[should_panic(expected = "Quantize::Range needs finite min < max")]
    fn an_empty_range_is_refused_when_described() {
        #[derive(Clone, Default)]
        struct Bad(f32);
        impl Replicate for Bad {
            fn fields(&mut self, f: &mut Fields<'_>) {
                f.f32("bad", &mut self.0, Quantize::range(1.0, 1.0, 0.1));
            }
        }
        let _ = schema_of::<Bad>();
    }

    #[test]
    #[should_panic(expected = "a 8-bit integer takes Quantize::Exact or Bits(1..=8)")]
    fn too_many_bits_for_an_integer_are_refused_when_described() {
        #[derive(Clone, Default)]
        struct Bad(u8);
        impl Replicate for Bad {
            fn fields(&mut self, f: &mut Fields<'_>) {
                f.u8("bad", &mut self.0, Quantize::Bits(9));
            }
        }
        let _ = schema_of::<Bad>();
    }

    #[test]
    #[should_panic(expected = "a rotation takes Quantize::Exact or Quantize::Bits(2..=10)")]
    fn a_ranged_rotation_is_refused_when_described() {
        #[derive(Clone, Default)]
        struct Bad(Quat);
        impl Replicate for Bad {
            fn fields(&mut self, f: &mut Fields<'_>) {
                f.quat("bad", &mut self.0, Quantize::range(0.0, 1.0, 0.1));
            }
        }
        let _ = schema_of::<Bad>();
    }

    fn vec2(x: f32, y: f32) -> Vec2 {
        Vec2::new(x, y)
    }
}
