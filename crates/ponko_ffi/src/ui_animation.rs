//! The retained UI's animation from C (Rust `UiTransition`,
//! `UiScreenTransition`, `UiTween`, `UiTimeline`, `Ui::animate`, `Ui::play`,
//! `Ui::set_screen_transition`, `Ui::drawn`, ...): state transitions come
//! with the theme (`PonkoUiThemeEx::transition`), screens fade, slide and
//! scale on and off, and tweens and timelines move any node. Everything runs
//! on the UI clock: the frame time, which goes on while the simulation is
//! paused.
use ponko::prelude::{Color, Vec2};
use ponko::skeleton2d::Bezier;
use ponko::ui::{Ui, UiRepeat, UiScreenTransition, UiTimeline, UiTransition, UiTween, UiValue};

use crate::ctx::out;
use crate::types::{PonkoColor, PonkoRect, PonkoVec2, to_c};
use crate::ui_retained::{existing, with_ui};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, invalid};

/// An easing curve (Rust `Bezier`): a CSS-style cubic bezier from (0, 0) to
/// (1, 1) through (`x1`, `y1`) and (`x2`, `y2`). `x1` and `x2` are 0 to 1;
/// `y` outside 0 to 1 overshoots. `ponko_ui_ease` gives the named ones.
/// ```
/// let e = ponko_ffi::ponko_ui_ease(ponko_ffi::PonkoUiEaseName::PONKO_UI_EASE_IN_OUT as u32);
/// assert_eq!((e.x1, e.y1, e.x2, e.y2), (0.42, 0.0, 0.58, 1.0));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoUiEase {
    /// The first handle's time, 0 to 1.
    pub x1: f32,
    /// The first handle's value.
    pub y1: f32,
    /// The second handle's time, 0 to 1.
    pub x2: f32,
    /// The second handle's value.
    pub y2: f32,
}

/// The named easing curves (`ponko_ui_ease`).
/// ```
/// assert_eq!(ponko_ffi::PonkoUiEaseName::PONKO_UI_EASE_BACK_OUT as u32, 5);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoUiEaseName {
    /// A straight line.
    #[default]
    PONKO_UI_EASE_LINEAR,
    /// CSS `ease`: a quick start that settles slowly.
    PONKO_UI_EASE_EASE,
    /// CSS `ease-in`: slow start (things leaving).
    PONKO_UI_EASE_IN,
    /// CSS `ease-out`: slow end (things arriving, most UI changes).
    PONKO_UI_EASE_OUT,
    /// CSS `ease-in-out`: slow at both ends.
    PONKO_UI_EASE_IN_OUT,
    /// Overshoots a little, then settles: a pop.
    PONKO_UI_EASE_BACK_OUT,
}

/// How long a change takes and how it eases (Rust `UiTransition`). Zero
/// seconds: at once.
/// ```
/// let t = ponko_ffi::ponko_ui_transition(0.2);
/// assert_eq!(t.seconds, 0.2);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoUiTransition {
    /// How long it takes; zero is at once.
    pub seconds: f32,
    /// How the value goes from start to end.
    pub ease: PonkoUiEase,
}

/// How a screen comes on or goes off (Rust `UiScreenTransition`): the pose
/// it has while away and the timing between that pose and its place. Start
/// from `ponko_ui_screen_fade`, `_slide` or `_zoom` and combine the fields.
/// ```
/// let t = ponko_ffi::ponko_ui_screen_fade(0.25);
/// assert_eq!((t.opacity, t.scale), (0.0, 1.0));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoUiScreenTransition {
    /// The timing; zero seconds: none.
    pub transition: PonkoUiTransition,
    /// Opacity while away: 0 fades, 1 doesn't.
    pub opacity: f32,
    /// Where it is while away, from its place, in UI units.
    pub offset: PonkoVec2,
    /// Its size while away, around its centre; 1 leaves the size alone.
    pub scale: f32,
}

/// What a `PonkoUiTween` changes (Rust `UiProperty`). Opacity, scale and
/// offset are drawn (input keeps the node's place); background and text
/// color are its paint; width and height its layout.
/// ```
/// assert_eq!(ponko_ffi::PonkoUiProperty::PONKO_UI_PROPERTY_HEIGHT as u32, 6);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoUiProperty {
    /// Opacity of the node and its children, 0 to 1 (`number`).
    #[default]
    PONKO_UI_PROPERTY_OPACITY,
    /// Size around its centre, 1 at rest (`number`).
    PONKO_UI_PROPERTY_SCALE,
    /// Drawn offset from its place, UI units (`point`).
    PONKO_UI_PROPERTY_OFFSET,
    /// Its paint's background (`color`).
    PONKO_UI_PROPERTY_BACKGROUND,
    /// Its paint's text and image tint (`color`).
    PONKO_UI_PROPERTY_TEXT_COLOR,
    /// Its layout width, UI units (`number`).
    PONKO_UI_PROPERTY_WIDTH,
    /// Its layout height, UI units (`number`).
    PONKO_UI_PROPERTY_HEIGHT,
}

/// A value of a property: the property reads `number`, `point` or `color`.
/// ```
/// let v = ponko_ffi::PonkoUiValue { number: 0.5, ..Default::default() };
/// assert_eq!(v.number, 0.5);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoUiValue {
    /// Opacity, scale, width or height.
    pub number: f32,
    /// An offset.
    pub point: PonkoVec2,
    /// A background or text color.
    pub color: PonkoColor,
}

/// One property of a node going to `to` (Rust `UiTween`), from where it is
/// when it starts or, with `has_from`, from `from`. `start` is when it
/// starts: seconds into its timeline (`ponko_ctx_ui_play`), or a delay
/// (`ponko_ctx_ui_animate`). Fill one with `ponko_ui_tween_defaults`.
/// ```
/// let t = ponko_ffi::ponko_ui_tween_defaults(ponko_ffi::PonkoUiProperty::PONKO_UI_PROPERTY_SCALE as u32);
/// assert_eq!((t.seconds, t.has_from), (0.25, false));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoUiTween {
    /// A `PonkoUiProperty`.
    pub property: u32,
    /// When it starts, in seconds (see above).
    pub start: f32,
    /// How long it takes; zero is at once.
    pub seconds: f32,
    /// How it eases.
    pub ease: PonkoUiEase,
    /// Whether it starts from `from` rather than from where the property is.
    pub has_from: bool,
    /// The start value, with `has_from`.
    pub from: PonkoUiValue,
    /// The end value.
    pub to: PonkoUiValue,
}

/// How often a timeline plays (Rust `UiRepeat`).
/// ```
/// assert_eq!(ponko_ffi::PonkoUiRepeat::PONKO_UI_REPEAT_PING_PONG as u32, 3);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoUiRepeat {
    /// Once.
    #[default]
    PONKO_UI_REPEAT_ONCE,
    /// `times` times from the start.
    PONKO_UI_REPEAT_TIMES,
    /// From the start again until stopped.
    PONKO_UI_REPEAT_LOOP,
    /// Forward, backward, forward, ... until stopped.
    PONKO_UI_REPEAT_PING_PONG,
}

/// How a node is drawn now (Rust `UiDrawn`): its rectangle after its own
/// and its ancestors' offset and scale, how opaque, how much bigger than its
/// place, and its fill (between two states' fills while a state transition
/// runs).
/// ```
/// let d = ponko_ffi::PonkoUiDrawn::default();
/// assert_eq!(d.opacity, 0.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoUiDrawn {
    /// Where it is drawn, window units.
    pub rect: PonkoRect,
    /// How opaque, 0 to 1.
    pub opacity: f32,
    /// How much bigger than its place.
    pub scale: f32,
    /// Its fill now.
    pub fill: PonkoColor,
}

/// Transparent black.
const CLEAR: PonkoColor = PonkoColor {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

/// All zero, the color transparent.
impl Default for PonkoUiValue {
    fn default() -> Self {
        Self {
            number: 0.0,
            point: PonkoVec2::default(),
            color: CLEAR,
        }
    }
}

/// All zero: nothing drawn.
impl Default for PonkoUiDrawn {
    fn default() -> Self {
        Self {
            rect: PonkoRect::default(),
            opacity: 0.0,
            scale: 0.0,
            fill: CLEAR,
        }
    }
}

fn ease_to_c(b: Bezier) -> PonkoUiEase {
    PonkoUiEase {
        x1: b.x1,
        y1: b.y1,
        x2: b.x2,
        y2: b.y2,
    }
}

fn ease_from_c(e: &PonkoUiEase) -> Result<Bezier, Failure> {
    let ok = (0.0..=1.0).contains(&e.x1) && (0.0..=1.0).contains(&e.x2) && e.y1.is_finite() && e.y2.is_finite();
    if !ok {
        return Err(invalid(
            "an easing curve's x1 and x2 must be 0 to 1, its y1 and y2 finite",
        ));
    }
    Ok(Bezier::new(e.x1, e.y1, e.x2, e.y2))
}

pub(crate) fn transition_to_c(t: UiTransition) -> PonkoUiTransition {
    PonkoUiTransition {
        seconds: t.seconds,
        ease: ease_to_c(t.ease),
    }
}

pub(crate) fn transition_from_c(t: &PonkoUiTransition) -> Result<UiTransition, Failure> {
    if !t.seconds.is_finite() || t.seconds < 0.0 {
        return Err(invalid("a transition's seconds must be finite and not negative"));
    }
    Ok(UiTransition::new(t.seconds).with_ease(ease_from_c(&t.ease)?))
}

fn screen_to_c(t: UiScreenTransition) -> PonkoUiScreenTransition {
    PonkoUiScreenTransition {
        transition: transition_to_c(t.transition),
        opacity: t.opacity,
        offset: to_c(t.offset),
        scale: t.scale,
    }
}

fn screen_from_c(t: *const PonkoUiScreenTransition) -> Result<UiScreenTransition, Failure> {
    // SAFETY: the caller passes null or a readable struct.
    let Some(t) = (unsafe { t.as_ref() }) else {
        return Ok(UiScreenTransition::NONE);
    };
    let offset: Vec2 = to_c(t.offset);
    if !t.opacity.is_finite() || !t.scale.is_finite() || t.scale < 0.0 || !offset.is_finite() {
        return Err(invalid(
            "a screen transition's opacity, offset and scale must be finite, its scale not negative",
        ));
    }
    Ok(UiScreenTransition {
        transition: transition_from_c(&t.transition)?,
        opacity: t.opacity,
        offset,
        scale: t.scale,
    })
}

fn tween_from_c(t: &PonkoUiTween) -> Result<(f32, UiTween), Failure> {
    let finite = [t.start, t.seconds].iter().all(|v| v.is_finite()) && t.seconds >= 0.0 && t.start >= 0.0;
    if !finite {
        return Err(invalid("a tween's start and seconds must be finite and not negative"));
    }
    let color = |v: &PonkoUiValue| -> Color { to_c(v.color) };
    let point = |v: &PonkoUiValue| -> Vec2 { to_c(v.point) };
    let (tween, from): (UiTween, UiValue) = match t.property {
        0 => (UiTween::opacity(t.to.number), t.from.number.into()),
        1 => (UiTween::scale(t.to.number), t.from.number.into()),
        2 => (UiTween::offset(point(&t.to)), point(&t.from).into()),
        3 => (UiTween::background(color(&t.to)), color(&t.from).into()),
        4 => (UiTween::text_color(color(&t.to)), color(&t.from).into()),
        5 => (UiTween::width(t.to.number), t.from.number.into()),
        6 => (UiTween::height(t.to.number), t.from.number.into()),
        _ => return Err(invalid("unknown UI property")),
    };
    let tween = tween.seconds(t.seconds).with_ease(ease_from_c(&t.ease)?);
    Ok((t.start, if t.has_from { tween.from(from) } else { tween }))
}

/// A named easing curve, `name` a `PonkoUiEaseName` (Rust
/// `Bezier::EASE_OUT`, ...); an unknown name is linear.
/// ```
/// let e = ponko_ffi::ponko_ui_ease(ponko_ffi::PonkoUiEaseName::PONKO_UI_EASE_OUT as u32);
/// assert_eq!(e.x2, 0.58);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_ui_ease(name: u32) -> PonkoUiEase {
    ease_to_c(match name {
        1 => Bezier::EASE,
        2 => Bezier::EASE_IN,
        3 => Bezier::EASE_OUT,
        4 => Bezier::EASE_IN_OUT,
        5 => Bezier::BACK_OUT,
        _ => Bezier::LINEAR,
    })
}

/// A transition of `seconds`, eased out (Rust `UiTransition::new`).
/// ```
/// assert_eq!(ponko_ffi::ponko_ui_transition(0.1).ease.x2, 0.58);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_ui_transition(seconds: f32) -> PonkoUiTransition {
    transition_to_c(UiTransition::new(seconds))
}

/// How far `transition` has gone `elapsed` seconds after it started (Rust
/// `UiTransition::progress`): 0 before, exactly 1 from its end on, eased in
/// between. Time what you draw yourself with it and `ponko_ctx_ui_clock`.
/// ```
/// let t = ponko_ffi::ponko_ui_transition(0.5);
/// let mut k = 0.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ui_transition_progress(&t, 1.0, &mut k) }, ponko_ffi::PonkoStatus::PONKO_OK);
/// assert_eq!(k, 1.0);
/// ```
/// # Safety
/// `transition` null or readable; `out_progress` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ui_transition_progress(
    transition: *const PonkoUiTransition,
    elapsed: f32,
    out_progress: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let Some(t) = transition.as_ref() else {
            return Err(invalid("transition is null"));
        };
        out(out_progress, transition_from_c(t)?.progress(elapsed))
    })
}

/// A screen fading on or off over `seconds` (Rust `UiScreenTransition::fade`).
/// ```
/// assert_eq!(ponko_ffi::ponko_ui_screen_fade(0.2).transition.seconds, 0.2);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_ui_screen_fade(seconds: f32) -> PonkoUiScreenTransition {
    screen_to_c(UiScreenTransition::fade(seconds))
}

/// A screen sliding from or to `offset` UI units away over `seconds`
/// (Rust `UiScreenTransition::slide`).
/// ```
/// let t = ponko_ffi::ponko_ui_screen_slide(ponko_ffi::PonkoVec2 { x: 0.0, y: 40.0 }, 0.3);
/// assert_eq!((t.offset.y, t.opacity), (40.0, 1.0));
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_ui_screen_slide(offset: PonkoVec2, seconds: f32) -> PonkoUiScreenTransition {
    screen_to_c(UiScreenTransition::slide(to_c(offset), seconds))
}

/// A screen zooming from or to `scale` times its size over `seconds` (Rust
/// `UiScreenTransition::zoom`).
/// ```
/// assert_eq!(ponko_ffi::ponko_ui_screen_zoom(0.9, 0.2).scale, 0.9);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_ui_screen_zoom(scale: f32, seconds: f32) -> PonkoUiScreenTransition {
    screen_to_c(UiScreenTransition::zoom(scale, seconds))
}

/// A tween of `property` (a `PonkoUiProperty`) with the default timing: a
/// quarter second, eased out, starting at once from where the property is.
/// Set its `to` (and `from`, `start`, ...) after.
/// ```
/// let t = ponko_ffi::ponko_ui_tween_defaults(0);
/// assert_eq!(t.ease.x2, 0.58);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_ui_tween_defaults(property: u32) -> PonkoUiTween {
    PonkoUiTween {
        property,
        start: 0.0,
        seconds: 0.25,
        ease: ease_to_c(Bezier::EASE_OUT),
        has_from: false,
        from: PonkoUiValue::default(),
        to: PonkoUiValue::default(),
    }
}

/// Plays `tween` on node `id` (Rust `Ui::animate`), after `tween.start`
/// seconds: its property goes to `tween.to`, replacing whatever moved that
/// property, and stays there; a `PONKO_UI_EVENT_ANIMATION_DONE` for `id`
/// follows (`ponko_ctx_ui_next_event_ex`). `PONKO_NOT_FOUND` for a node
/// that doesn't exist.
/// ```
/// let t = ponko_ffi::ponko_ui_tween_defaults(0);
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_animate(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &t) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `tween` null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_animate(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    tween: *const PonkoUiTween,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let Some(tween) = tween.as_ref() else {
            return Err(invalid("tween is null"));
        };
        let (start, tween) = tween_from_c(tween)?;
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            if start > 0.0 {
                ui.play(id, &UiTimeline::new().at(start, tween));
            } else {
                ui.animate(id, tween);
            }
            Ok(())
        })
    })
}

/// Plays the timeline of `count` tweens on node `id` (Rust `Ui::play`):
/// each starts `start` seconds in and replaces whatever moved its property;
/// a property's later tween takes over from its earlier one. `repeat` (a
/// `PonkoUiRepeat`) says how often it plays, `times` how many times for
/// `PONKO_UI_REPEAT_TIMES`. A timeline that ends sends one
/// `PONKO_UI_EVENT_ANIMATION_DONE` for `id`. `PONKO_NOT_FOUND` for a node
/// that doesn't exist.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_play(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null(), 0, 0, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `tweens` null (with `count` 0) or `count` readable
/// tweens.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_play(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    tweens: *const PonkoUiTween,
    count: usize,
    repeat: u32,
    times: u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if tweens.is_null() && count > 0 {
            return Err(invalid("tweens is null"));
        }
        let repeat = match repeat {
            0 => UiRepeat::Once,
            1 => UiRepeat::Times(times),
            2 => UiRepeat::Loop,
            3 => UiRepeat::PingPong,
            _ => return Err(invalid("unknown PonkoUiRepeat")),
        };
        let given = if count == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(tweens, count)
        };
        let mut timeline = UiTimeline::new().repeat(repeat);
        for t in given {
            let (start, tween) = tween_from_c(t)?;
            timeline = timeline.at(start, tween);
        }
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.play(id, &timeline);
            Ok(())
        })
    })
}

/// Stops the tweens and timelines on node `id` where they are (Rust
/// `Ui::stop_animations`); no event follows.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_stop_animations(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_stop_animations(ctx: *mut PonkoCtx, target: u64, id: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.stop_animations(id);
            Ok(())
        })
    })
}

/// Whether a tween, timeline or screen transition moves node `id` (Rust
/// `Ui::is_animating`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_is_animating(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut false) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_animating` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_is_animating(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    out_animating: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            out(out_animating, ui.is_animating(id))
        })
    })
}

/// Sets how screen `root` comes on when pushed (`enter`) and goes off when
/// popped (`exit`); null is none (Rust `Ui::set_screen_transition`). While
/// it goes it is drawn and takes no input.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_screen_transition(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `enter` and `exit` null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_screen_transition(
    ctx: *mut PonkoCtx,
    target: u64,
    root: u64,
    enter: *const PonkoUiScreenTransition,
    exit: *const PonkoUiScreenTransition,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let (enter, exit) = (screen_from_c(enter)?, screen_from_c(exit)?);
        with_ui(ctx, target, |ui| {
            let root = existing(ui, root)?;
            ui.set_screen_transition(root, enter, exit);
            Ok(())
        })
    })
}

/// Writes the transitions screen `root` comes on and goes off with (Rust
/// `Ui::screen_transition`); either output may be null.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_screen_transition(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_enter` and `out_exit` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_screen_transition(
    ctx: *mut PonkoCtx,
    target: u64,
    root: u64,
    out_enter: *mut PonkoUiScreenTransition,
    out_exit: *mut PonkoUiScreenTransition,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let root = existing(ui, root)?;
            let (enter, exit) = ui
                .screen_transition(root)
                .unwrap_or((UiScreenTransition::NONE, UiScreenTransition::NONE));
            if let Some(o) = out_enter.as_mut() {
                *o = screen_to_c(enter);
            }
            if let Some(o) = out_exit.as_mut() {
                *o = screen_to_c(exit);
            }
            Ok(())
        })
    })
}

/// Writes how node `id` is drawn as of the last layout (Rust `Ui::drawn`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_drawn(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_drawn` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_drawn(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    out_drawn: *mut PonkoUiDrawn,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            let drawn = drawn(ui, id);
            out(out_drawn, drawn)
        })
    })
}

fn drawn(ui: &Ui, id: ponko::ui::UiId) -> PonkoUiDrawn {
    ui.drawn(id).map_or(PonkoUiDrawn::default(), |d| PonkoUiDrawn {
        rect: to_c(d.rect),
        opacity: d.opacity,
        scale: d.scale,
        fill: to_c(d.fill),
    })
}

/// Writes the UI clock (Rust `Ui::clock`): seconds of frame time the UI has
/// run, going on while the simulation is paused.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_clock(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, &mut 0.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_seconds` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_clock(ctx: *mut PonkoCtx, target: u64, out_seconds: *mut f64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { with_ui(ctx, target, |ui| out(out_seconds, ui.clock())) })
}

/// Turns reduced motion on or off (Rust `Ui::set_reduced_motion`): state
/// and screen transitions happen at once.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_reduced_motion(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_reduced_motion(ctx: *mut PonkoCtx, target: u64, on: bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            ui.set_reduced_motion(on);
            Ok(())
        })
    })
}

/// Whether reduced motion is on (Rust `Ui::reduced_motion`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_reduced_motion(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, &mut false) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_on` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_reduced_motion(
    ctx: *mut PonkoCtx,
    target: u64,
    out_on: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { with_ui(ctx, target, |ui| out(out_on, ui.reduced_motion())) })
}
