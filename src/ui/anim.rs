//! Animation of the retained UI: state transitions (a control's fill and
//! size easing between idle, hovered, focused, pressed and disabled), screens
//! coming on and going off (fade, slide, scale), and tweens and timelines a
//! game plays on any node.
//!
//! Every animation runs on the UI clock, [`UiInput::dt`](super::UiInput::dt):
//! the engine feeds it the frame time, which keeps going while the
//! simulation is paused (a pause menu still animates) and stands still at
//! zero. Values are a function of the time since an animation started, not
//! of the number of frames, so they are the same at 30, 60 or 144 frames a
//! second. Nothing here allocates per frame: tracks live in one list that
//! keeps its capacity, and the per-node state is plain data on the node.
//!
//! Offset, scale and opacity are drawn, not laid out: input and layout use
//! a node's resting rectangle, and its children move, scale and fade with
//! it. Width and height are layout and move the nodes around them.

use super::retained::{Ui, UiEvent, UiId};
use crate::{camera::Rect, color::Color, skeleton2d::Bezier};
use glam::Vec2;

/// How long a change takes and how it eases: the theme's
/// [`transition`](super::UiTheme::transition) for state changes, and the
/// timing of screen transitions and tweens. The easing is a CSS-style cubic
/// bezier ([`Bezier`], with `EASE`, `EASE_OUT`, `BACK_OUT`, ...).
/// ```
/// use ponko::skeleton2d::Bezier;
/// use ponko::ui::UiTransition;
/// let quick = UiTransition::new(0.2).with_ease(Bezier::EASE_IN_OUT);
/// assert_eq!(quick.progress(0.0), 0.0);
/// assert!((quick.progress(0.1) - 0.5).abs() < 1e-4);
/// assert_eq!(quick.progress(0.2), 1.0);
/// assert_eq!(UiTransition::NONE.progress(0.0), 1.0, "no transition: at once");
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiTransition {
    /// How long it takes; zero (or less) is at once.
    pub seconds: f32,
    /// How the value goes from start to end over that time.
    pub ease: Bezier,
}

impl Default for UiTransition {
    /// [`UiTransition::NONE`].
    fn default() -> Self {
        Self::NONE
    }
}

impl UiTransition {
    /// No transition: changes show at once.
    pub const NONE: Self = Self {
        seconds: 0.0,
        ease: Bezier::LINEAR,
    };

    /// Takes `seconds`, eased out ([`Bezier::EASE_OUT`]): fast at first,
    /// settling at the end.
    /// ```
    /// let t = ponko::ui::UiTransition::new(0.15);
    /// assert_eq!(t.ease, ponko::skeleton2d::Bezier::EASE_OUT);
    /// ```
    pub const fn new(seconds: f32) -> Self {
        Self {
            seconds,
            ease: Bezier::EASE_OUT,
        }
    }

    /// The same timing with another easing.
    /// ```
    /// use ponko::skeleton2d::Bezier;
    /// use ponko::ui::UiTransition;
    /// assert_eq!(UiTransition::new(0.3).with_ease(Bezier::EASE).ease, Bezier::EASE);
    /// ```
    #[must_use]
    pub const fn with_ease(mut self, ease: Bezier) -> Self {
        self.ease = ease;
        self
    }

    /// Whether it takes no time.
    /// ```
    /// assert!(ponko::ui::UiTransition::NONE.is_instant());
    /// assert!(!ponko::ui::UiTransition::new(0.1).is_instant());
    /// ```
    pub fn is_instant(&self) -> bool {
        !(self.seconds > 0.0 && self.seconds.is_finite())
    }

    /// How far it has gone `elapsed` seconds after it started: 0 before
    /// it starts, exactly 1 from its end on, eased in between (an
    /// overshooting curve leaves 0..1 on the way). Use it to animate what
    /// you draw yourself on the same timing as the UI.
    /// ```
    /// use ponko::ui::UiTransition;
    /// let fade = UiTransition::new(0.4);
    /// assert_eq!(fade.progress(-1.0), 0.0);
    /// assert!(fade.progress(0.1) > 0.25, "eased out: ahead early on");
    /// assert_eq!(fade.progress(9.0), 1.0);
    /// ```
    pub fn progress(&self, elapsed: f32) -> f32 {
        if self.is_instant() || elapsed >= self.seconds {
            1.0
        } else if elapsed > 0.0 {
            self.ease.ease(elapsed / self.seconds)
        } else {
            0.0
        }
    }
}

/// How a screen comes on ([`Ui::set_screen_transition`]'s `enter`) or goes
/// off (`exit`): the pose it is in while away (see-through, moved, scaled)
/// and the timing between that pose and its place. Combine the parts:
/// `UiScreenTransition::fade(0.25).with_offset(vec2(0.0, 40.0))` fades and
/// slides.
/// ```
/// use ponko::prelude::*;
/// use ponko::ui::UiScreenTransition;
/// let t = UiScreenTransition::slide(vec2(-80.0, 0.0), 0.3).with_fade();
/// assert_eq!((t.opacity, t.offset, t.scale), (0.0, vec2(-80.0, 0.0), 1.0));
/// assert!(UiScreenTransition::NONE.transition.is_instant());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiScreenTransition {
    /// The timing.
    pub transition: UiTransition,
    /// Opacity while away: 0 fades the screen, 1 leaves it opaque.
    pub opacity: f32,
    /// Where it is while away, from its place, in UI units.
    pub offset: Vec2,
    /// Its size while away, around its centre: below 1 grows it in, above
    /// 1 shrinks it in; 1 leaves the size alone.
    pub scale: f32,
}

impl Default for UiScreenTransition {
    /// [`UiScreenTransition::NONE`].
    fn default() -> Self {
        Self::NONE
    }
}

impl UiScreenTransition {
    /// None: the screen shows and goes at once.
    pub const NONE: Self = Self {
        transition: UiTransition::NONE,
        opacity: 1.0,
        offset: Vec2::ZERO,
        scale: 1.0,
    };

    /// A fade over `seconds`.
    /// ```
    /// assert_eq!(ponko::ui::UiScreenTransition::fade(0.2).opacity, 0.0);
    /// ```
    pub const fn fade(seconds: f32) -> Self {
        Self {
            transition: UiTransition::new(seconds),
            opacity: 0.0,
            ..Self::NONE
        }
    }

    /// A slide from (or to) `offset` UI units away, over `seconds`.
    /// ```
    /// use ponko::prelude::*;
    /// assert_eq!(ponko::ui::UiScreenTransition::slide(vec2(0.0, 60.0), 0.3).offset.y, 60.0);
    /// ```
    pub const fn slide(offset: Vec2, seconds: f32) -> Self {
        Self {
            transition: UiTransition::new(seconds),
            offset,
            ..Self::NONE
        }
    }

    /// A zoom from (or to) `scale` times its size, over `seconds`.
    /// ```
    /// assert_eq!(ponko::ui::UiScreenTransition::zoom(0.9, 0.2).scale, 0.9);
    /// ```
    pub const fn zoom(scale: f32, seconds: f32) -> Self {
        Self {
            transition: UiTransition::new(seconds),
            scale,
            ..Self::NONE
        }
    }

    /// The same, faded too.
    /// ```
    /// assert_eq!(ponko::ui::UiScreenTransition::zoom(0.9, 0.2).with_fade().opacity, 0.0);
    /// ```
    #[must_use]
    pub const fn with_fade(mut self) -> Self {
        self.opacity = 0.0;
        self
    }

    /// The same, moved `offset` UI units away too.
    /// ```
    /// use ponko::prelude::*;
    /// let t = ponko::ui::UiScreenTransition::fade(0.2).with_offset(vec2(0.0, 30.0));
    /// assert_eq!(t.offset, vec2(0.0, 30.0));
    /// ```
    #[must_use]
    pub const fn with_offset(mut self, offset: Vec2) -> Self {
        self.offset = offset;
        self
    }

    /// The same, at `scale` times its size while away too.
    /// ```
    /// assert_eq!(ponko::ui::UiScreenTransition::fade(0.2).with_scale(1.1).scale, 1.1);
    /// ```
    #[must_use]
    pub const fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// The same, with another easing.
    /// ```
    /// use ponko::skeleton2d::Bezier;
    /// use ponko::ui::UiScreenTransition;
    /// let t = UiScreenTransition::fade(0.2).with_ease(Bezier::EASE_IN);
    /// assert_eq!(t.transition.ease, Bezier::EASE_IN);
    /// ```
    #[must_use]
    pub const fn with_ease(mut self, ease: Bezier) -> Self {
        self.transition.ease = ease;
        self
    }

    fn is_none(&self) -> bool {
        self.transition.is_instant()
    }
}

/// A property of a node a [`UiTween`] changes. Opacity, scale and offset
/// are drawn (they move, scale and fade the node's children with it, and
/// input still uses its place); background and text color are its paint;
/// width and height are its layout, so the nodes around it make room.
/// ```
/// use ponko::ui::{UiProperty, UiTween};
/// assert_eq!(UiTween::opacity(0.0).property(), UiProperty::Opacity);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UiProperty {
    /// How opaque the node and its children are drawn, 0 to 1 (1 at rest).
    /// Each piece is faded on its own, so a faded child lets a little of
    /// its parent show through.
    Opacity,
    /// Its size, and its children's, around its centre (1 at rest).
    Scale,
    /// How far it and its children are drawn from their place, in UI
    /// units (zero at rest).
    Offset,
    /// Its paint's [`background`](super::UiPaint::background).
    Background,
    /// Its paint's [`color`](super::UiPaint::color): text and image tint.
    TextColor,
    /// Its layout width, in UI units.
    Width,
    /// Its layout height, in UI units.
    Height,
}

/// A value of a [`UiProperty`]: a number (opacity, scale, width, height), a
/// point (offset) or a color (background, text color). `f32`, `Vec2` and
/// `Color` turn into one with `into()`. A value of another kind than its
/// property's is read channel by channel (a number fills every channel).
/// ```
/// use ponko::prelude::*;
/// use ponko::ui::UiValue;
/// assert_eq!(UiValue::from(0.5), UiValue::Number(0.5));
/// assert_eq!(UiValue::from(Color::RED), UiValue::Color(Color::RED));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum UiValue {
    /// Opacity, scale, width or height.
    Number(f32),
    /// An offset.
    Point(Vec2),
    /// A background or text color.
    Color(Color),
}

impl From<f32> for UiValue {
    fn from(v: f32) -> Self {
        Self::Number(v)
    }
}

impl From<Vec2> for UiValue {
    fn from(v: Vec2) -> Self {
        Self::Point(v)
    }
}

impl From<Color> for UiValue {
    fn from(v: Color) -> Self {
        Self::Color(v)
    }
}

impl UiValue {
    fn channels(self) -> [f32; 4] {
        match self {
            Self::Number(n) => [n; 4],
            Self::Point(p) => [p.x, p.y, 0.0, 0.0],
            Self::Color(c) => [c.r, c.g, c.b, c.a],
        }
    }
}

/// One property of a node going to a value: from where it is when the
/// tween starts (interrupting whatever moved it before, without a jump), or
/// from a value given with [`from`](Self::from). Play it with
/// [`Ui::animate`], or place several on a [`UiTimeline`]. The default
/// timing is a quarter second, eased out.
/// ```
/// use ponko::prelude::*;
/// use ponko::skeleton2d::Bezier;
/// use ponko::ui::UiTween;
/// let rise = UiTween::offset(Vec2::ZERO).from(vec2(0.0, 24.0)).seconds(0.3).with_ease(Bezier::BACK_OUT);
/// assert_eq!(rise.transition().seconds, 0.3);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiTween {
    property: UiProperty,
    from: Option<[f32; 4]>,
    to: [f32; 4],
    transition: UiTransition,
}

impl UiTween {
    fn new(property: UiProperty, to: UiValue) -> Self {
        Self {
            property,
            from: None,
            to: to.channels(),
            transition: UiTransition::new(0.25),
        }
    }

    /// Opacity to `to` (0 to 1).
    /// ```
    /// let fade_out = ponko::ui::UiTween::opacity(0.0);
    /// # let _ = fade_out;
    /// ```
    pub fn opacity(to: f32) -> Self {
        Self::new(UiProperty::Opacity, to.into())
    }

    /// Scale to `to` (1 is its size).
    /// ```
    /// let grow = ponko::ui::UiTween::scale(1.1);
    /// # let _ = grow;
    /// ```
    pub fn scale(to: f32) -> Self {
        Self::new(UiProperty::Scale, to.into())
    }

    /// Offset to `to` UI units from its place.
    /// ```
    /// use ponko::prelude::*;
    /// let home = ponko::ui::UiTween::offset(Vec2::ZERO);
    /// # let _ = home;
    /// ```
    pub fn offset(to: Vec2) -> Self {
        Self::new(UiProperty::Offset, to.into())
    }

    /// Background color to `to`.
    /// ```
    /// use ponko::prelude::*;
    /// let flash = ponko::ui::UiTween::background(Color::RED);
    /// # let _ = flash;
    /// ```
    pub fn background(to: Color) -> Self {
        Self::new(UiProperty::Background, to.into())
    }

    /// Text (and image tint) color to `to`.
    /// ```
    /// use ponko::prelude::*;
    /// let warn = ponko::ui::UiTween::text_color(Color::YELLOW);
    /// # let _ = warn;
    /// ```
    pub fn text_color(to: Color) -> Self {
        Self::new(UiProperty::TextColor, to.into())
    }

    /// Layout width to `to` UI units.
    /// ```
    /// let open = ponko::ui::UiTween::width(240.0);
    /// # let _ = open;
    /// ```
    pub fn width(to: f32) -> Self {
        Self::new(UiProperty::Width, to.into())
    }

    /// Layout height to `to` UI units.
    /// ```
    /// let open = ponko::ui::UiTween::height(120.0);
    /// # let _ = open;
    /// ```
    pub fn height(to: f32) -> Self {
        Self::new(UiProperty::Height, to.into())
    }

    /// Starts from `from` instead of where the property is. A tween with a
    /// start value that waits on a timeline holds it until it starts, so a
    /// staggered fade-in starts see-through.
    /// ```
    /// let fade_in = ponko::ui::UiTween::opacity(1.0).from(0.0);
    /// # let _ = fade_in;
    /// ```
    #[must_use]
    pub fn from(mut self, from: impl Into<UiValue>) -> Self {
        self.from = Some(from.into().channels());
        self
    }

    /// Takes `seconds` (zero: at once).
    /// ```
    /// assert_eq!(ponko::ui::UiTween::scale(2.0).seconds(1.5).transition().seconds, 1.5);
    /// ```
    #[must_use]
    pub fn seconds(mut self, seconds: f32) -> Self {
        self.transition.seconds = seconds;
        self
    }

    /// Eases with `ease`.
    /// ```
    /// use ponko::skeleton2d::Bezier;
    /// use ponko::ui::UiTween;
    /// assert_eq!(UiTween::scale(2.0).with_ease(Bezier::EASE).transition().ease, Bezier::EASE);
    /// ```
    #[must_use]
    pub fn with_ease(mut self, ease: Bezier) -> Self {
        self.transition.ease = ease;
        self
    }

    /// The property it changes.
    /// ```
    /// assert_eq!(ponko::ui::UiTween::width(9.0).property(), ponko::ui::UiProperty::Width);
    /// ```
    pub fn property(&self) -> UiProperty {
        self.property
    }

    /// Its timing.
    /// ```
    /// assert_eq!(ponko::ui::UiTween::width(9.0).transition().seconds, 0.25);
    /// ```
    pub fn transition(&self) -> UiTransition {
        self.transition
    }
}

/// How often a [`UiTimeline`] plays.
/// ```
/// use ponko::ui::{UiRepeat, UiTimeline, UiTween};
/// let pulse = UiTimeline::new()
///     .then(UiTween::scale(1.08).seconds(0.4))
///     .repeat(UiRepeat::PingPong);
/// assert_eq!(pulse.seconds(), 0.4);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum UiRepeat {
    /// Once (the default).
    #[default]
    Once,
    /// This many times from the start (0 and 1 are once).
    Times(u32),
    /// From the start again, until stopped.
    Loop,
    /// Forward, then backward, and so on, until stopped.
    PingPong,
}

/// Tweens placed in time, played on one node with [`Ui::play`]: the
/// explicit timeline. [`then`](Self::then) starts a tween when everything
/// before it has ended, [`with`](Self::with) together with the one added
/// last, [`at`](Self::at) at a time of its own. A property's later tween
/// takes over from its earlier one. Build it once (it allocates) and play
/// it as often as you like: playing copies it into the UI's own list.
/// ```
/// use ponko::prelude::*;
/// use ponko::ui::{UiTimeline, UiTween};
/// let bounce_in = UiTimeline::new()
///     .then(UiTween::opacity(1.0).from(0.0).seconds(0.2))
///     .with(UiTween::scale(1.1).from(0.6).seconds(0.2))
///     .then(UiTween::scale(1.0).seconds(0.1));
/// assert!((bounce_in.seconds() - 0.3).abs() < 1e-6);
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiTimeline {
    steps: Vec<(f32, UiTween)>,
    /// Where the tween added last starts.
    last: f32,
    repeat: UiRepeat,
}

impl UiTimeline {
    /// An empty timeline.
    /// ```
    /// assert_eq!(ponko::ui::UiTimeline::new().seconds(), 0.0);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `tween` starting when everything so far has ended.
    /// ```
    /// use ponko::ui::{UiTimeline, UiTween};
    /// let t = UiTimeline::new().then(UiTween::opacity(0.0).seconds(0.5)).then(UiTween::opacity(1.0).seconds(0.5));
    /// assert_eq!(t.seconds(), 1.0);
    /// ```
    #[must_use]
    pub fn then(self, tween: UiTween) -> Self {
        let start = self.seconds();
        self.at(start, tween)
    }

    /// Adds `tween` starting with the tween added last.
    /// ```
    /// use ponko::ui::{UiTimeline, UiTween};
    /// let t = UiTimeline::new().then(UiTween::opacity(0.0).seconds(0.5)).with(UiTween::scale(0.5).seconds(0.2));
    /// assert_eq!(t.seconds(), 0.5);
    /// ```
    #[must_use]
    pub fn with(self, tween: UiTween) -> Self {
        let start = self.last;
        self.at(start, tween)
    }

    /// Adds `tween` starting `seconds` into the timeline.
    /// ```
    /// use ponko::ui::{UiTimeline, UiTween};
    /// let t = UiTimeline::new().at(1.0, UiTween::opacity(1.0).seconds(0.5));
    /// assert_eq!(t.seconds(), 1.5);
    /// ```
    #[must_use]
    pub fn at(mut self, seconds: f32, tween: UiTween) -> Self {
        let start = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
        // Kept in start order (equal starts in the order added), so a
        // property's later tween is applied after its earlier one.
        let index = self.steps.partition_point(|(s, _)| *s <= start);
        self.steps.insert(index, (start, tween));
        self.last = start;
        self
    }

    /// Plays `repeat` times.
    /// ```
    /// use ponko::ui::{UiRepeat, UiTimeline};
    /// let t = UiTimeline::new().repeat(UiRepeat::Loop);
    /// assert_eq!(t.repeats(), UiRepeat::Loop);
    /// ```
    #[must_use]
    pub fn repeat(mut self, repeat: UiRepeat) -> Self {
        self.repeat = repeat;
        self
    }

    /// How often it plays.
    /// ```
    /// assert_eq!(ponko::ui::UiTimeline::new().repeats(), ponko::ui::UiRepeat::Once);
    /// ```
    pub fn repeats(&self) -> UiRepeat {
        self.repeat
    }

    /// How long one pass takes: where its last tween ends.
    /// ```
    /// assert_eq!(ponko::ui::UiTimeline::new().seconds(), 0.0);
    /// ```
    pub fn seconds(&self) -> f32 {
        self.steps
            .iter()
            .map(|(start, t)| start + t.transition.seconds.max(0.0))
            .fold(0.0, f32::max)
    }

    /// Its tweens with their start times, in start order.
    /// ```
    /// use ponko::ui::{UiTimeline, UiTween};
    /// let t = UiTimeline::new().at(0.5, UiTween::scale(2.0)).at(0.1, UiTween::opacity(0.0));
    /// assert_eq!(t.steps()[0].0, 0.1);
    /// ```
    pub fn steps(&self) -> &[(f32, UiTween)] {
        &self.steps
    }
}

/// How node `id` is drawn now ([`Ui::drawn`]): its rectangle after its
/// own and its ancestors' offset and scale, how opaque, and the fill its
/// state transition shows. Input and layout use [`Ui::rect`], its place.
/// ```
/// use ponko::prelude::*;
/// let mut ui = ponko::ui::Ui::default();
/// let root = ui.root().id();
/// ui.push_modeless(root);
/// ui.layout(vec2(320.0, 240.0));
/// let drawn = ui.drawn(root).unwrap();
/// assert_eq!((drawn.rect, drawn.opacity, drawn.scale), (ui.rect(root).unwrap(), 1.0, 1.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiDrawn {
    /// Where it is drawn, in window units.
    pub rect: Rect,
    /// How opaque, 0 to 1, its ancestors' opacity included.
    pub opacity: f32,
    /// How much bigger than its place it is drawn.
    pub scale: f32,
    /// The background it is drawn with now: between its states' fills
    /// while a state transition runs.
    pub fill: Color,
}

// ---- engine side ------------------------------------------------------------------

/// A map from a node's laid-out place to where it is drawn: `p * scale +
/// shift`, and how opaque. Ancestors' maps are composed in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Xf {
    pub scale: f32,
    pub shift: Vec2,
    pub opacity: f32,
}

impl Xf {
    pub const IDENTITY: Self = Self {
        scale: 1.0,
        shift: Vec2::ZERO,
        opacity: 1.0,
    };

    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    pub fn map(&self, p: Vec2) -> Vec2 {
        p * self.scale + self.shift
    }

    pub fn map_rect(&self, r: Rect) -> Rect {
        Rect {
            min: self.map(r.min),
            max: self.map(r.max),
        }
    }

    /// This map after a node's own `look` around `pivot`, `units` window
    /// pixels per UI unit. A node at rest keeps its parent's map exactly.
    fn then(&self, look: &Look, state_scale: f32, pivot: Vec2, units: f32) -> Self {
        let s = look.scale.max(0.0) * state_scale.max(0.0);
        let local_shift = pivot - pivot * s + look.offset * units;
        Self {
            scale: self.scale * s,
            shift: local_shift * self.scale + self.shift,
            opacity: self.opacity * look.opacity.clamp(0.0, 1.0),
        }
    }
}

/// A node's drawn-only properties, set by tweens and screen transitions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Look {
    pub opacity: f32,
    pub scale: f32,
    pub offset: Vec2,
}

impl Look {
    const REST: Self = Self {
        opacity: 1.0,
        scale: 1.0,
        offset: Vec2::ZERO,
    };
}

/// What a node's state (idle, hovered, ...) gives it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StateLook {
    fill: Color,
    scale: f32,
}

impl StateLook {
    fn mix(a: Self, b: Self, k: f32) -> Self {
        if k >= 1.0 {
            return b;
        }
        if k <= 0.0 {
            return a;
        }
        let m = |x: f32, y: f32| x + (y - x) * k;
        Self {
            fill: Color::rgba(
                m(a.fill.r, b.fill.r),
                m(a.fill.g, b.fill.g),
                m(a.fill.b, b.fill.b),
                m(a.fill.a, b.fill.a),
            ),
            scale: m(a.scale, b.scale),
        }
    }
}

/// A state transition in flight: from what was shown to the state's look.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Fade {
    from: StateLook,
    to: StateLook,
    start: f64,
    /// `from` and `to` are set: the node was seen with transitions on.
    set: bool,
}

/// Per node: its drawn properties, its state transition, its screen
/// transitions and where the last layout puts it on screen.
#[derive(Debug, Clone, Copy)]
pub(super) struct NodeMotion {
    pub look: Look,
    fade: Fade,
    pub enter: UiScreenTransition,
    pub exit: UiScreenTransition,
    /// Its map to the screen, ancestors included, as of the last layout.
    pub xf: Xf,
    /// Its clip on screen (its ancestors' drawn rectangles).
    pub shown_clip: Rect,
}

impl Default for NodeMotion {
    fn default() -> Self {
        let clear = StateLook {
            fill: Color::rgba(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
        };
        Self {
            look: Look::REST,
            fade: Fade {
                from: clear,
                to: clear,
                start: 0.0,
                set: false,
            },
            enter: UiScreenTransition::NONE,
            exit: UiScreenTransition::NONE,
            xf: Xf::IDENTITY,
            shown_clip: Rect::default(),
        }
    }
}

/// Where a track came from: what replacing or finishing it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// [`Ui::animate`] or [`Ui::play`].
    Game,
    /// A screen coming on.
    Enter,
    /// A screen going off: when it ends, the screen is gone.
    Exit,
}

/// One tween of a played timeline on one node.
#[derive(Debug, Clone, Copy)]
struct Track {
    node: UiId,
    group: u32,
    source: Source,
    property: UiProperty,
    from: [f32; 4],
    /// `from` is known (given, or read when the tween started).
    from_set: bool,
    to: [f32; 4],
    /// Where it starts in a pass of its timeline.
    start: f32,
    transition: UiTransition,
    /// When its timeline was played, on the UI clock.
    played: f64,
    /// One pass of its timeline, and how often it plays.
    pass: f32,
    repeat: UiRepeat,
    /// The first of its timeline's tweens on this property: a given start
    /// value holds until it starts.
    leads: bool,
    /// Sends `AnimationDone` when its timeline ends (one track per played
    /// timeline that nothing interrupted).
    report: bool,
}

/// A screen popped with an exit transition, still drawn while it goes.
#[derive(Debug, Clone, Copy)]
pub(super) struct Exiting {
    pub root: UiId,
    pub modal: bool,
    /// Where it was on the screen stack: it is drawn there, under the
    /// screens pushed since (a menu swapped for another fades out below it).
    pub depth: usize,
}

/// The UI's animations: its clock and its playing tracks.
#[derive(Debug, Default)]
pub(crate) struct Motion {
    /// Seconds of `UiInput::dt` so far.
    time: f64,
    tracks: Vec<Track>,
    pub(super) exiting: Vec<Exiting>,
    next_group: u32,
    pub(super) reduced: bool,
}

/// Where in one pass of a timeline `elapsed` seconds after it was played
/// falls, and whether it has ended.
fn pass_time(elapsed: f32, pass: f32, repeat: UiRepeat) -> (f32, bool) {
    let times = match repeat {
        _ if pass.is_nan() || pass <= 0.0 => Some(1),
        UiRepeat::Once => Some(1),
        UiRepeat::Times(n) => Some(n.max(1)),
        UiRepeat::Loop | UiRepeat::PingPong => None,
    };
    if let Some(n) = times
        && elapsed >= pass * n as f32
    {
        // Past the end, a ping-pong would be at its start; it never ends.
        return (pass.max(0.0), true);
    }
    if elapsed <= 0.0 || pass.is_nan() || pass <= 0.0 {
        return (elapsed.min(0.0), false);
    }
    let cycle = (elapsed / pass).floor();
    let within = elapsed - cycle * pass;
    if repeat == UiRepeat::PingPong && cycle as u64 % 2 == 1 {
        (pass - within, false)
    } else {
        (within, false)
    }
}

fn mix(a: [f32; 4], b: [f32; 4], k: f32) -> [f32; 4] {
    if k >= 1.0 {
        return b;
    }
    if k <= 0.0 {
        return a;
    }
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * k)
}

impl Ui {
    /// The UI clock moves `dt` seconds: every track writes its value.
    pub(super) fn advance_motion(&mut self, dt: f32) {
        self.motion.time += f64::from(dt);
        self.apply_tracks();
    }

    /// Writes every track's value at the UI clock's time, drops the ended
    /// ones and finishes the screens whose exit ended.
    fn apply_tracks(&mut self) {
        if self.motion.tracks.is_empty() && self.motion.exiting.is_empty() {
            return;
        }
        let now = self.motion.time;
        let mut i = 0;
        let mut any_ended = false;
        while i < self.motion.tracks.len() {
            let t = self.motion.tracks[i];
            if self.node(t.node).is_none() {
                self.motion.tracks.swap_remove(i);
                continue;
            }
            let elapsed = (now - t.played) as f32;
            let (at, ended) = pass_time(elapsed, t.pass, t.repeat);
            let local = at - t.start;
            if local >= 0.0 || (t.leads && t.from_set) {
                let from = if t.from_set {
                    t.from
                } else {
                    let from = self.read_property(t.node, t.property);
                    let track = &mut self.motion.tracks[i];
                    track.from = from;
                    track.from_set = true;
                    from
                };
                let value = mix(from, t.to, t.transition.progress(local));
                self.write_property(t.node, t.property, value);
            }
            if ended {
                self.motion.tracks[i].pass = f32::NAN;
                any_ended = true;
                if t.report {
                    self.events.push_back(UiEvent::AnimationDone(t.node));
                }
            }
            i += 1;
        }
        if any_ended {
            // NaN marks the ended ones; `retain` keeps the order the rest
            // are applied in.
            self.motion.tracks.retain(|t| !t.pass.is_nan());
        }
        // A screen whose exit tracks are all gone is gone.
        let mut e = 0;
        while e < self.motion.exiting.len() {
            let root = self.motion.exiting[e].root;
            let going = self
                .motion
                .tracks
                .iter()
                .any(|t| t.node == root && t.source == Source::Exit);
            if going && self.node(root).is_some() {
                e += 1;
            } else {
                self.motion.exiting.remove(e);
                if let Some(n) = self.node_mut(root) {
                    n.motion.look = Look::REST;
                }
                self.rest_xf(root);
            }
        }
    }

    /// A subtree no longer drawn is drawn at rest when it shows again.
    fn rest_xf(&mut self, id: UiId) {
        let Some(n) = self.node_mut(id) else { return };
        n.motion.xf = Xf::IDENTITY;
        n.motion.shown_clip = n.clip;
        for i in 0..n.children.len() {
            let child = self.node(id).unwrap().children[i];
            self.rest_xf(child);
        }
    }

    fn read_property(&self, id: UiId, property: UiProperty) -> [f32; 4] {
        let Some(n) = self.node(id) else { return [0.0; 4] };
        let look = &n.motion.look;
        match property {
            UiProperty::Opacity => [look.opacity; 4],
            UiProperty::Scale => [look.scale; 4],
            UiProperty::Offset => UiValue::Point(look.offset).channels(),
            UiProperty::Background => UiValue::Color(n.paint.background).channels(),
            UiProperty::TextColor => UiValue::Color(n.paint.color).channels(),
            UiProperty::Width => [n.rect.size().x / self.scale; 4],
            UiProperty::Height => [n.rect.size().y / self.scale; 4],
        }
    }

    fn write_property(&mut self, id: UiId, property: UiProperty, v: [f32; 4]) {
        let color = Color::rgba(v[0], v[1], v[2], v[3]);
        let Some(n) = self.node_mut(id) else { return };
        let look = &mut n.motion.look;
        match property {
            UiProperty::Opacity => look.opacity = v[0],
            UiProperty::Scale => look.scale = v[0],
            UiProperty::Offset => look.offset = Vec2::new(v[0], v[1]),
            UiProperty::Background => n.paint.background = color,
            UiProperty::TextColor => n.paint.color = color,
            UiProperty::Width | UiProperty::Height => {
                let layout = n.layout;
                let Ok(style) = self.tree.style(layout) else { return };
                let length = taffy::Dimension::length(v[0].max(0.0));
                let size = &style.size;
                let same = if property == UiProperty::Width {
                    size.width == length
                } else {
                    size.height == length
                };
                if !same {
                    // Taffy has no in-place style change: a copy (which
                    // allocates only for a grid's track lists).
                    let mut style = style.clone();
                    if property == UiProperty::Width {
                        style.size.width = length;
                    } else {
                        style.size.height = length;
                    }
                    let _ = self.tree.set_style(layout, style);
                }
            }
        }
    }

    /// Adds `steps` (start, tween) as one played timeline on `id`, after
    /// taking the tracks off the properties it touches.
    fn add_tracks(&mut self, id: UiId, steps: &[(f32, UiTween)], repeat: UiRepeat, source: Source) {
        if self.node(id).is_none() || steps.is_empty() {
            return;
        }
        // The properties it touches lose their tracks; a timeline that
        // loses one was interrupted and won't report its end.
        let mut i = 0;
        while i < self.motion.tracks.len() {
            let t = self.motion.tracks[i];
            if t.node == id && steps.iter().any(|(_, s)| s.property == t.property) {
                self.motion.tracks.remove(i);
                for other in &mut self.motion.tracks {
                    if other.group == t.group {
                        other.report = false;
                    }
                }
            } else {
                i += 1;
            }
        }
        let group = self.motion.next_group;
        self.motion.next_group = self.motion.next_group.wrapping_add(1);
        let pass = steps
            .iter()
            .map(|(start, t)| start + t.transition.seconds.max(0.0))
            .fold(0.0, f32::max);
        let first = self.motion.tracks.len();
        for (k, &(start, tween)) in steps.iter().enumerate() {
            let leads = !steps[..k].iter().any(|(_, s)| s.property == tween.property);
            self.motion.tracks.push(Track {
                node: id,
                group,
                source,
                property: tween.property,
                from: tween.from.unwrap_or([0.0; 4]),
                from_set: tween.from.is_some(),
                to: tween.to,
                start,
                transition: tween.transition,
                played: self.motion.time,
                pass,
                repeat,
                leads,
                report: source == Source::Game && k == 0,
            });
        }
        // Start values show at once, not a frame late.
        for k in first..self.motion.tracks.len() {
            let t = self.motion.tracks[k];
            if t.leads && t.from_set {
                self.write_property(id, t.property, t.from);
            }
        }
        // A timeline with nothing to wait for ends now.
        if pass.is_nan() || pass <= 0.0 {
            self.apply_tracks();
        }
    }

    /// Plays `tween` on node `id`: its property goes from where it is now
    /// (or the tween's [`from`](UiTween::from)) to the tween's value,
    /// replacing any tween already moving that property, so changing your
    /// mind half way is smooth. The value stays when it ends, and a
    /// [`UiEvent::AnimationDone`] for `id` is sent then. Runs on the UI
    /// clock (see [`Ui::play`]).
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{UiInput, UiTween};
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// let badge = ui.label(root, "New!").id();
    /// ui.push_modeless(root);
    /// ui.animate(badge, UiTween::opacity(0.0).seconds(0.5));
    /// for _ in 0..30 {
    ///     ui.frame(vec2(320.0, 240.0), &UiInput { dt: 1.0 / 60.0, ..Default::default() });
    /// }
    /// assert_eq!(ui.drawn(badge).unwrap().opacity, 0.0);
    /// ```
    pub fn animate(&mut self, id: UiId, tween: UiTween) {
        self.add_tracks(id, &[(0.0, tween)], UiRepeat::Once, Source::Game);
    }

    /// Plays `timeline` on node `id`, from now: each tween replaces the
    /// tweens moving its property on that node. Every animation runs on
    /// the UI clock ([`UiInput::dt`](super::UiInput::dt), the frame time):
    /// it keeps going while the simulation is paused and holds while `dt`
    /// is zero, and its values depend on the time, not on the frame rate.
    /// When a timeline that plays a set number of times ends, its values
    /// stay and a [`UiEvent::AnimationDone`] for `id` is sent, unless
    /// another tween took over one of its properties first. Playing copies
    /// the timeline into a list the UI reuses, so playing again costs no
    /// allocation once the list has grown.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{UiEvent, UiInput, UiTimeline, UiTween};
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// let toast = ui.label(root, "Saved").id();
    /// ui.push_modeless(root);
    /// let show = UiTimeline::new()
    ///     .then(UiTween::opacity(1.0).from(0.0).seconds(0.2))
    ///     .then(UiTween::opacity(0.0).seconds(0.2));
    /// ui.play(toast, &show);
    /// let mut done = false;
    /// for _ in 0..30 {
    ///     ui.frame(vec2(320.0, 240.0), &UiInput { dt: 1.0 / 60.0, ..Default::default() });
    ///     done |= ui.events().any(|e| e == UiEvent::AnimationDone(toast));
    /// }
    /// assert!(done && !ui.is_animating(toast));
    /// ```
    pub fn play(&mut self, id: UiId, timeline: &UiTimeline) {
        self.add_tracks(id, &timeline.steps, timeline.repeat, Source::Game);
    }

    /// Stops the tweens and timelines playing on node `id`: their
    /// properties stay where they are now, and no
    /// [`UiEvent::AnimationDone`] is sent. A screen's transition is left
    /// running.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{UiInput, UiTween};
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// ui.push_modeless(root);
    /// ui.animate(root, UiTween::scale(2.0).seconds(1.0));
    /// ui.frame(vec2(320.0, 240.0), &UiInput { dt: 0.5, ..Default::default() });
    /// ui.stop_animations(root);
    /// let held = ui.drawn(root).unwrap().scale;
    /// ui.frame(vec2(320.0, 240.0), &UiInput { dt: 0.5, ..Default::default() });
    /// assert!(held > 1.0 && held < 2.0);
    /// assert_eq!(ui.drawn(root).unwrap().scale, held);
    /// ```
    pub fn stop_animations(&mut self, id: UiId) {
        self.motion
            .tracks
            .retain(|t| !(t.node == id && t.source == Source::Game));
    }

    /// Whether a tween, timeline or screen transition is moving node `id`.
    /// ```
    /// use ponko::ui::UiTween;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// assert!(!ui.is_animating(root));
    /// ui.animate(root, UiTween::opacity(0.5));
    /// assert!(ui.is_animating(root));
    /// ```
    pub fn is_animating(&self, id: UiId) -> bool {
        self.motion.tracks.iter().any(|t| t.node == id)
    }

    /// Sets how screen `root` comes on when it is pushed (`enter`) and goes
    /// off when it is popped (`exit`), Escape and a closing button
    /// included. While it goes it is drawn where it was on the stack (over
    /// the screens below it, under any pushed since), takes no input and is
    /// not [shown](Ui::is_shown); pushing it again turns it
    /// round from where it is. Both are [`UiScreenTransition::NONE`] until
    /// set; with [reduced motion](Ui::set_reduced_motion) screens show and
    /// go at once. The transition drives the root's opacity, offset and
    /// scale; input uses its place throughout.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{UiInput, UiScreenTransition};
    /// let mut ui = ponko::ui::Ui::default();
    /// let menu = ui.root().id();
    /// ui.set_screen_transition(menu, UiScreenTransition::fade(0.2), UiScreenTransition::fade(0.2));
    /// ui.push_modeless(menu);
    /// ui.frame(vec2(320.0, 240.0), &UiInput { dt: 0.1, ..Default::default() });
    /// let half = ui.drawn(menu).unwrap().opacity;
    /// assert!(half > 0.0 && half < 1.0);
    /// ```
    pub fn set_screen_transition(&mut self, root: UiId, enter: UiScreenTransition, exit: UiScreenTransition) {
        if let Some(n) = self.node_mut(root) {
            n.motion.enter = enter;
            n.motion.exit = exit;
        }
    }

    /// The transitions screen `root` comes on and goes off with
    /// ([`set_screen_transition`](Self::set_screen_transition)), or `None`
    /// for a removed node.
    /// ```
    /// use ponko::ui::UiScreenTransition;
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// assert_eq!(ui.screen_transition(root), Some((UiScreenTransition::NONE, UiScreenTransition::NONE)));
    /// ```
    pub fn screen_transition(&self, root: UiId) -> Option<(UiScreenTransition, UiScreenTransition)> {
        let n = self.node(root)?;
        Some((n.motion.enter, n.motion.exit))
    }

    /// Seconds on the UI clock: every frame's
    /// [`UiInput::dt`](super::UiInput::dt) added up. The engine feeds it the
    /// frame time, so it runs while the simulation is paused or slowed.
    /// Time what you draw yourself next to the UI with it, with a
    /// [`UiTransition`]'s [`progress`](UiTransition::progress).
    /// ```
    /// use ponko::prelude::*;
    /// let mut ui = ponko::ui::Ui::default();
    /// ui.frame(vec2(320.0, 240.0), &ponko::ui::UiInput { dt: 0.25, ..Default::default() });
    /// assert_eq!(ui.clock(), 0.25);
    /// ```
    pub fn clock(&self) -> f64 {
        self.motion.time
    }

    /// Turns reduced motion on or off, for players who ask for less
    /// movement: state transitions and screen transitions happen at once.
    /// Tweens and timelines the game plays still run; check
    /// [`reduced_motion`](Self::reduced_motion) before playing decoration.
    /// ```
    /// let mut ui = ponko::ui::Ui::default();
    /// ui.set_reduced_motion(true);
    /// assert!(ui.reduced_motion());
    /// ```
    pub fn set_reduced_motion(&mut self, on: bool) {
        self.motion.reduced = on;
    }

    /// Whether reduced motion is on ([`set_reduced_motion`](Self::set_reduced_motion)).
    /// ```
    /// assert!(!ponko::ui::Ui::default().reduced_motion());
    /// ```
    pub fn reduced_motion(&self) -> bool {
        self.motion.reduced
    }

    /// How node `id` is drawn as of the last layout ([`Ui::frame`], run by
    /// the engine before the frame's systems, or [`Ui::layout`]): its
    /// rectangle after the offsets and scales of it and its ancestors, its
    /// opacity, and its fill. `None` for an id not in the tree.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::{UiInput, UiTween};
    /// let mut ui = ponko::ui::Ui::default();
    /// let root = ui.root().id();
    /// let card = ui.panel(root).layout(ponko::ui::UiLayout::fixed(100.0, 50.0)).id();
    /// ui.push_modeless(root);
    /// ui.animate(card, UiTween::offset(vec2(10.0, 0.0)).seconds(0.0));
    /// ui.frame(vec2(320.0, 240.0), &UiInput::default());
    /// assert_eq!(ui.drawn(card).unwrap().rect.min.x, ui.rect(card).unwrap().min.x + 10.0);
    /// ```
    pub fn drawn(&self, id: UiId) -> Option<UiDrawn> {
        let n = self.node(id)?;
        let xf = n.motion.xf;
        Some(UiDrawn {
            rect: xf.map_rect(n.rect),
            opacity: xf.opacity,
            scale: xf.scale,
            fill: self.shown_fill(id)?,
        })
    }

    // ---- screens ----------------------------------------------------------------

    /// After `root` was pushed: its enter transition, from where it is if
    /// it was still going off.
    pub(super) fn screen_entered(&mut self, root: UiId) {
        let was_exiting = self.motion.exiting.iter().any(|e| e.root == root);
        self.motion.exiting.retain(|e| e.root != root);
        let Some(n) = self.node(root) else { return };
        let (enter, look) = (n.motion.enter, n.motion.look);
        self.motion
            .tracks
            .retain(|t| !(t.node == root && t.source != Source::Game));
        if enter.is_none() || self.motion.reduced {
            if was_exiting && let Some(n) = self.node_mut(root) {
                n.motion.look = Look::REST;
            }
            return;
        }
        let at_rest = look == Look::REST;
        let from = |v: UiValue| if at_rest { Some(v.channels()) } else { None };
        let tween = |property, away: UiValue, rest: UiValue| UiTween {
            property,
            from: from(away),
            to: rest.channels(),
            transition: enter.transition,
        };
        let steps = [
            (0.0, tween(UiProperty::Opacity, enter.opacity.into(), 1.0.into())),
            (0.0, tween(UiProperty::Offset, enter.offset.into(), Vec2::ZERO.into())),
            (0.0, tween(UiProperty::Scale, enter.scale.into(), 1.0.into())),
        ];
        self.add_tracks(root, &steps, UiRepeat::Once, Source::Enter);
    }

    /// `root` was popped: it goes off with its exit transition (drawn,
    /// without input, until it ends) or at once.
    pub(super) fn screen_left(&mut self, root: UiId, modal: bool) {
        let Some(n) = self.node(root) else { return };
        let exit = n.motion.exit;
        self.motion
            .tracks
            .retain(|t| !(t.node == root && t.source != Source::Game));
        if exit.is_none() || self.motion.reduced {
            if let Some(n) = self.node_mut(root) {
                n.motion.look = Look::REST;
            }
            return;
        }
        let tween = |property, away: UiValue| UiTween {
            property,
            from: None,
            to: away.channels(),
            transition: exit.transition,
        };
        let steps = [
            (0.0, tween(UiProperty::Opacity, exit.opacity.into())),
            (0.0, tween(UiProperty::Offset, exit.offset.into())),
            (0.0, tween(UiProperty::Scale, exit.scale.into())),
        ];
        let depth = self.screens.len();
        self.motion.exiting.push(Exiting { root, modal, depth });
        self.add_tracks(root, &steps, UiRepeat::Once, Source::Exit);
    }

    /// `root` is shown as a HUD: if it was going off as a screen, it stops
    /// at rest.
    pub(super) fn screen_shown_as_hud(&mut self, root: UiId) {
        if self.motion.exiting.iter().any(|e| e.root == root) {
            self.motion.exiting.retain(|e| e.root != root);
            self.motion
                .tracks
                .retain(|t| !(t.node == root && t.source != Source::Game));
            if let Some(n) = self.node_mut(root) {
                n.motion.look = Look::REST;
            }
        }
    }

    /// Node `id` was removed: its tracks and its exit go too.
    pub(super) fn motion_removed(&mut self, id: UiId) {
        self.motion.tracks.retain(|t| t.node != id);
        self.motion.exiting.retain(|e| e.root != id);
    }

    /// Whether `id` is the root of a screen going off as a modal.
    pub(super) fn exiting_modal(&self, id: UiId) -> bool {
        self.motion.exiting.iter().any(|e| e.root == id && e.modal)
    }

    // ---- states -----------------------------------------------------------------

    /// Whether state changes animate now.
    fn states_animate(&self) -> bool {
        !self.theme.transition.is_instant() && !self.motion.reduced
    }

    /// What node `id`'s state gives it with no transition.
    fn state_target(&self, id: UiId) -> Option<StateLook> {
        let state = self.paint_state(id);
        Some(StateLook {
            fill: self.fill_in(id, state)?,
            scale: if state == super::widgets_draw::PaintState::Pressed {
                self.theme.press_scale
            } else {
                1.0
            },
        })
    }

    /// Before a layout: every node whose state changed starts its
    /// transition from what it shows now.
    pub(super) fn update_states(&mut self) {
        if !self.states_animate() {
            return;
        }
        let now = self.motion.time;
        let transition = self.theme.transition;
        for slot in 0..self.nodes.len() {
            let Some(id) = self.nodes[slot].as_ref().map(|n| n.id) else {
                continue;
            };
            let Some(target) = self.state_target(id) else { continue };
            let n = self.nodes[slot].as_mut().unwrap();
            let fade = &mut n.motion.fade;
            if !fade.set {
                *fade = Fade {
                    from: target,
                    to: target,
                    start: now,
                    set: true,
                };
            } else if fade.to != target {
                let shown = StateLook::mix(fade.from, fade.to, transition.progress((now - fade.start) as f32));
                *fade = Fade {
                    from: shown,
                    to: target,
                    start: now,
                    set: true,
                };
            }
        }
    }

    /// What node `id`'s state shows now, its transition included.
    fn shown_state(&self, id: UiId) -> Option<StateLook> {
        if !self.states_animate() {
            return self.state_target(id);
        }
        let fade = &self.node(id)?.motion.fade;
        if !fade.set {
            return self.state_target(id);
        }
        let k = self.theme.transition.progress((self.motion.time - fade.start) as f32);
        Some(StateLook::mix(fade.from, fade.to, k))
    }

    /// The fill node `id` is drawn with now.
    pub(crate) fn shown_fill(&self, id: UiId) -> Option<Color> {
        Some(self.shown_state(id)?.fill)
    }

    /// Node `id`'s map to the screen under `parent`'s, for its rectangle
    /// `rect` (window units).
    pub(super) fn node_xf(&self, id: UiId, parent: Xf, rect: Rect) -> Xf {
        let Some(n) = self.node(id) else { return parent };
        let look = n.motion.look;
        let state_scale = if self.theme.press_scale == 1.0 {
            1.0
        } else {
            self.shown_state(id).map_or(1.0, |s| s.scale)
        };
        if look == Look::REST && state_scale == 1.0 {
            return parent;
        }
        parent.then(&look, state_scale, rect.center(), self.scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_time_loops_ping_pongs_and_ends() {
        assert_eq!(pass_time(0.5, 1.0, UiRepeat::Once), (0.5, false));
        assert_eq!(pass_time(1.0, 1.0, UiRepeat::Once), (1.0, true));
        assert_eq!(pass_time(2.5, 1.0, UiRepeat::Times(3)), (0.5, false));
        assert_eq!(pass_time(3.0, 1.0, UiRepeat::Times(3)), (1.0, true));
        assert_eq!(pass_time(1.25, 1.0, UiRepeat::Loop), (0.25, false));
        assert_eq!(pass_time(1.25, 1.0, UiRepeat::PingPong), (0.75, false));
        assert_eq!(pass_time(99.0, 0.0, UiRepeat::Loop), (0.0, true), "an empty loop ends");
        assert_eq!(pass_time(-0.5, 1.0, UiRepeat::Once), (-0.5, false));
    }

    #[test]
    fn a_node_at_rest_keeps_its_parents_map_bit_for_bit() {
        let parent = Xf {
            scale: 1.5,
            shift: Vec2::new(3.0, -2.0),
            opacity: 0.5,
        };
        let same = parent.then(&Look::REST, 1.0, Vec2::new(123.456, 78.9), 1.25);
        assert_eq!(same, parent);
        let rect = Rect::new(10.1, 20.2, 30.3, 40.4);
        assert_eq!(Xf::IDENTITY.map_rect(rect), rect);
    }

    #[test]
    fn a_screen_swapped_for_another_goes_off_under_it() {
        use crate::ui::UiLayout;
        let mut ui = Ui::default();
        let a = ui.root().layout(UiLayout::centered(200.0, 100.0)).id();
        let b = ui.root().layout(UiLayout::centered(200.0, 100.0)).id();
        let fade = UiScreenTransition::fade(0.2);
        ui.set_screen_transition(a, fade, fade);
        ui.push_modal(a);
        ui.pop_screen();
        ui.push_modal(b);
        ui.frame(
            Vec2::new(640.0, 480.0),
            &super::super::UiInput {
                dt: 0.1,
                ..Default::default()
            },
        );
        let at = |id| ui.visible.iter().position(|&v| v == id).unwrap();
        assert!(at(a) < at(b), "the old menu is drawn under the new one");
        assert!(ui.exiting_modal(a), "its overlay fades with it");
    }

    /// The engine's frame loop with the simulation paused: the frame clock
    /// goes on, no ticks run, and the UI's animations play on.
    #[test]
    fn ui_animations_play_on_while_the_simulation_is_paused() {
        let mut ctx = crate::ctx::Ctx::new(1.0 / 60.0, Vec2::new(640.0, 480.0));
        let root = ctx.ui.root().id();
        ctx.ui.push_modeless(root);
        ctx.ui
            .animate(root, UiTween::opacity(0.0).seconds(0.5).with_ease(Bezier::LINEAR));
        ctx.time.pause();
        let mut clock = crate::time::Clock::new(60);
        for _ in 0..15 {
            let timing = clock.advance(1.0 / 60.0, ctx.time.effective_sim_speed());
            assert_eq!(timing.fixed_steps, 0, "paused: no ticks");
            ctx.time.begin_frame(timing);
            ctx.update_retained_ui();
        }
        assert_eq!(ctx.time.sim_tick(), 0);
        let opacity = ctx.ui.drawn(root).unwrap().opacity;
        assert!((opacity - 0.5).abs() < 1e-3, "a quarter second of frames: {opacity}");
        // A slowed simulation doesn't slow the UI either.
        ctx.time.resume();
        ctx.time.set_speed(0.1);
        for _ in 0..15 {
            let timing = clock.advance(1.0 / 60.0, ctx.time.effective_sim_speed());
            ctx.time.begin_frame(timing);
            ctx.update_retained_ui();
        }
        assert_eq!(ctx.ui.drawn(root).unwrap().opacity, 0.0);
        assert!((ctx.ui.clock() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn scaling_keeps_the_pivot_in_place() {
        let look = Look {
            scale: 2.0,
            ..Look::REST
        };
        let xf = Xf::IDENTITY.then(&look, 1.0, Vec2::new(50.0, 50.0), 1.0);
        assert_eq!(xf.map(Vec2::new(50.0, 50.0)), Vec2::new(50.0, 50.0));
        assert_eq!(xf.map(Vec2::new(60.0, 50.0)), Vec2::new(70.0, 50.0));
    }
}
