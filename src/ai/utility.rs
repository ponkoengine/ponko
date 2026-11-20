//! Utility scoring: turn what an agent knows into scores from 0 to 1 and
//! pick the best option. Used by the trees' `utility` nodes, and on its own
//! from plain systems.
use serde::{Deserialize, Serialize};

use super::{Blackboard, Key};

/// The shape of a response: maps an input already scaled to 0..1 onto a
/// score from 0 to 1.
///
/// In tree files: `"linear"`, `"quadratic"`, `"cubic"`, `"sqrt"`,
/// `"smooth"`, `{"power": 3}`, `{"logistic": 10}` or `{"step": 0.5}`.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Curve {
    /// `y = x`.
    #[default]
    Linear,
    /// `y = x^2`: low inputs barely count.
    Quadratic,
    /// `y = x^3`.
    Cubic,
    /// `y = sqrt(x)`: low inputs already count a lot.
    Sqrt,
    /// Smoothstep: flat at both ends, steep in the middle.
    Smooth,
    /// `y = x^k`.
    Power(f32),
    /// An S curve around 0.5 with this steepness (10 is a soft switch, 40
    /// nearly a step).
    Logistic(f32),
    /// 0 below the threshold, 1 from it on.
    Step(f32),
}

impl Curve {
    /// The score for `x` (clamped to 0..1 first).
    ///
    /// ```
    /// use ponko::ai::Curve;
    /// assert_eq!(Curve::Quadratic.eval(0.5), 0.25);
    /// assert_eq!(Curve::Step(0.3).eval(0.2), 0.0);
    /// assert_eq!(Curve::Linear.eval(7.0), 1.0);
    /// ```
    pub fn eval(self, x: f32) -> f32 {
        let x = if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) };
        let y = match self {
            Curve::Linear => x,
            Curve::Quadratic => x * x,
            Curve::Cubic => x * x * x,
            Curve::Sqrt => x.sqrt(),
            Curve::Smooth => x * x * (3.0 - 2.0 * x),
            Curve::Power(k) => x.powf(k),
            Curve::Logistic(k) => 1.0 / (1.0 + (-k * (x - 0.5)).exp()),
            Curve::Step(t) => {
                if x >= t {
                    1.0
                } else {
                    0.0
                }
            }
        };
        if y.is_nan() { 0.0 } else { y.clamp(0.0, 1.0) }
    }
}

/// `value` scaled so that `from` is 0 and `to` is 1 (clamped). `from` may
/// be above `to` for an input that matters more as it falls.
///
/// ```
/// assert_eq!(ponko::ai::normalize(15.0, 10.0, 20.0), 0.5);
/// assert_eq!(ponko::ai::normalize(2.0, 10.0, 0.0), 0.8);
/// ```
pub fn normalize(value: f32, from: f32, to: f32) -> f32 {
    if from == to {
        return if value >= to { 1.0 } else { 0.0 };
    }
    let t = (value - from) / (to - from);
    if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) }
}

/// One reason to choose an option: a blackboard number scaled from `from`
/// to `to`, through a [`Curve`], maybe inverted.
///
/// ```
/// use ponko::ai::{Blackboard, Consideration, Curve, Key};
/// const HUNGER: Key = Key::new("hunger");
/// let eat = Consideration::new(HUNGER, 0.0, 100.0).curve(Curve::Quadratic);
/// let bb = Blackboard::new().with(HUNGER, 50.0);
/// assert_eq!(eat.score(&bb), 0.25);
/// assert_eq!(eat.inverted().score(&bb), 0.75);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Consideration {
    /// The blackboard number it reads (a missing one reads as 0).
    pub key: Key,
    /// The input that scores 0 (before the curve).
    pub from: f32,
    /// The input that scores 1 (before the curve).
    pub to: f32,
    /// The response curve.
    pub curve: Curve,
    /// Whether the score is `1 - curve` (more input, less wanted).
    pub invert: bool,
}

impl Consideration {
    /// A linear consideration of `key` from `from` (0) to `to` (1).
    pub fn new(key: Key, from: f32, to: f32) -> Consideration {
        Consideration {
            key,
            from,
            to,
            curve: Curve::Linear,
            invert: false,
        }
    }

    /// This consideration with another curve.
    pub fn curve(mut self, curve: Curve) -> Consideration {
        self.curve = curve;
        self
    }

    /// This consideration inverted: `1 - score`.
    pub fn inverted(mut self) -> Consideration {
        self.invert = !self.invert;
        self
    }

    /// The score for a raw input `value`.
    pub fn score_value(&self, value: f32) -> f32 {
        let y = self.curve.eval(normalize(value, self.from, self.to));
        if self.invert { 1.0 - y } else { y }
    }

    /// The score for what `bb` holds at [`key`](Self::key).
    pub fn score(&self, bb: &Blackboard) -> f32 {
        self.score_value(bb.num(self.key))
    }
}

/// Combines the scores of an option's considerations into one: their
/// product, made up for the number of factors so that an option with five
/// considerations isn't punished against one with two (the compensation
/// factor from Dave Mark's "Infinite Axis Utility System": each factor `s`
/// becomes `s + (1 - s) * (1 - 1/n) * s`). Any 0 makes the result 0; an
/// empty list scores 1.
///
/// ```
/// use ponko::ai::combine;
/// assert_eq!(combine(&[0.5]), 0.5);
/// assert_eq!(combine(&[1.0, 0.0, 1.0]), 0.0);
/// // 0.5 * 0.5 = 0.25 as a plain product; each factor is made up to 0.625
/// // for two factors, so 0.390625.
/// assert_eq!(combine(&[0.5, 0.5]), 0.390625);
/// ```
pub fn combine(scores: &[f32]) -> f32 {
    combine_n(scores.len(), scores.iter().copied())
}

/// [`combine`] over `n` scores from an iterator (no buffer needed).
pub(crate) fn combine_n(n: usize, scores: impl Iterator<Item = f32>) -> f32 {
    let modification = if n > 1 { 1.0 - 1.0 / n as f32 } else { 0.0 };
    let mut product = 1.0f32;
    for s in scores {
        let s = if s.is_nan() { 0.0 } else { s.clamp(0.0, 1.0) };
        product *= s + (1.0 - s) * modification * s;
    }
    product
}

/// The option with the highest score above 0; on a tie, the first one, so
/// the choice is the same on every machine. `None` if none scores above 0.
/// NaN scores never win.
///
/// ```
/// use ponko::ai::best;
/// assert_eq!(best([("eat", 0.4), ("sleep", 0.7), ("work", 0.7)]), Some(("sleep", 0.7)));
/// assert_eq!(best([("idle", 0.0)]), None);
/// ```
pub fn best<T>(options: impl IntoIterator<Item = (T, f32)>) -> Option<(T, f32)> {
    let mut winner: Option<(T, f32)> = None;
    for (option, score) in options {
        if score > 0.0 && winner.as_ref().is_none_or(|(_, s)| score > *s) {
            winner = Some((option, score));
        }
    }
    winner
}
