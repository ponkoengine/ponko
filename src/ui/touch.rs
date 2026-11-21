//! On-screen controls for touch screens: a stick and buttons drawn over the
//! game, each bound to an action of the action map, as the keys and the
//! gamepad are. Shown only on touch devices by default.
//!
//! ```
//! use ponko::prelude::*;
//! use ponko::ui::touch::TouchControls;
//!
//! Game::new("touch", 960, 540).run_headless(1, |ctx| {
//!     let mut map = ActionMap::default();
//!     map.set("move", Action::Axis2(vec![Axis2Binding::Keys { left: Key::A, right: Key::D, up: Key::W, down: Key::S }]));
//!     map.set("jump", Action::Button(vec![ButtonBinding::Key(Key::Space)]));
//!     ctx.input.set_actions(map);
//!     // The stick drives "move", the button "jump", alongside their keys.
//!     ctx.input.set_touch_controls(TouchControls::new().stick("move").button("jump", "Jump"));
//!     assert!(!ctx.input.touch_controls_shown(), "not a touch device");
//! });
//! ```
//!
//! The controls are retained UI nodes in a HUD of their own ([`Ui::show_hud`](crate::ui::Ui::show_hud)),
//! laid out from the window's size and the UI scale every frame: the
//! sticks at the bottom corners, the buttons at the bottom right, the corner
//! buttons (pause, menu) at the top right. The buttons have the button role
//! and the stick is a named group, for assistive technology. A finger that
//! goes down on one belongs to it until it lifts, makes no gestures, and is
//! read with every other finger at once: run with the stick while pressing
//! jump. The mouse never presses them; on a desktop they stay hidden unless
//! a touch arrives or they are [`TouchShow::Always`] shown.
//!
//! A game restyles them ([`TouchStyle`]: colors, images, size) and places
//! them ([`TouchPlace`]); the player moves and resizes them
//! ([`Input::edit_touch_controls`](crate::input::Input::edit_touch_controls),
//! [`TouchLayout`]), and the arrangement is saved with the settings.
//! [`Ctx::arrange_touch_controls`] opens a ready-made screen for that.
#![deny(missing_docs)]

mod arrange;
mod layout;

pub use layout::{TouchAdjust, TouchLayout, TouchPlace, TouchStyle};

use glam::{Vec2, vec2};

use crate::color::Color;
use crate::ctx::Ctx;
pub use crate::input::touch::TouchShow;
use crate::input::touch::{Zone, ZoneKind};
use crate::texture::Texture;
use crate::ui::{UiId, UiLayout, UiPaint, UiTextAlign, UiWidget};

/// Side of the texture the controls are drawn with, in pixels.
pub(crate) const DISC: u32 = 64;

/// A control of a [`TouchControls`] set.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Spec {
    /// A stick in a bottom corner (`right`: the right one).
    Stick { action: String, right: bool },
    /// A round button at the bottom right.
    Button { action: String, label: String },
    /// A small button at the top right.
    Corner { action: String, label: String },
}

impl Spec {
    fn action(&self) -> &str {
        match self {
            Spec::Stick { action, .. } | Spec::Button { action, .. } | Spec::Corner { action, .. } => action,
        }
    }
}

/// A set of on-screen controls, each bound to an action by name, for
/// [`Input::set_touch_controls`](crate::input::Input::set_touch_controls). Build it once in setup; the order buttons
/// are added in is the order they are placed in, from the bottom-right
/// corner out (put the most used first).
///
/// - A **stick** ([`stick`](Self::stick), [`right_stick`](Self::right_stick))
///   drives an `Axis2` action with its direction and how far it is pushed
///   (length up to 1, y down, a small deadzone), like a gamepad stick; bound
///   to an `Axis` action it gives its x.
/// - A **button** ([`button`](Self::button), [`corner_button`](Self::corner_button))
///   holds a `Button` action down while a finger is on it (an `Axis`
///   action reads 1).
///
/// An action the map doesn't have yet is picked up when it is set.
///
/// ```
/// use ponko::ui::touch::{TouchControls, TouchShow};
/// let controls = TouchControls::new()
///     .stick("move")
///     .button("jump", "A")
///     .button("attack", "B")
///     .corner_button("pause", "II")
///     .show(TouchShow::Auto);
/// assert_eq!(controls.len(), 4);
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TouchControls {
    pub(crate) specs: Vec<Spec>,
    pub(crate) show: TouchShow,
    pub(crate) over_screens: bool,
    /// Every control's look.
    style: TouchStyle,
    /// Controls with a look of their own, by action.
    styles: Vec<(String, TouchStyle)>,
    /// Controls the game placed, by action.
    places: Vec<(String, TouchPlace)>,
}

impl TouchControls {
    /// No controls, shown on touch devices ([`TouchShow::Auto`]).
    pub fn new() -> Self {
        Self::default()
    }
    /// A stick at the bottom left, driving `action`.
    #[must_use]
    pub fn stick(mut self, action: impl Into<String>) -> Self {
        self.specs.push(Spec::Stick {
            action: action.into(),
            right: false,
        });
        self
    }
    /// A stick at the bottom right, driving `action` (a camera, aiming):
    /// the buttons move up above it.
    #[must_use]
    pub fn right_stick(mut self, action: impl Into<String>) -> Self {
        self.specs.push(Spec::Stick {
            action: action.into(),
            right: true,
        });
        self
    }
    /// A round button at the bottom right showing `label`, holding `action`.
    #[must_use]
    pub fn button(mut self, action: impl Into<String>, label: impl Into<String>) -> Self {
        self.specs.push(Spec::Button {
            action: action.into(),
            label: label.into(),
        });
        self
    }
    /// A small button at the top right showing `label`, for `action`s
    /// pressed now and then (pause, a menu, the map).
    #[must_use]
    pub fn corner_button(mut self, action: impl Into<String>, label: impl Into<String>) -> Self {
        self.specs.push(Spec::Corner {
            action: action.into(),
            label: label.into(),
        });
        self
    }
    /// When they are shown (and take touches): [`TouchShow::Auto`], the
    /// default, shows them on touch devices only.
    #[must_use]
    pub fn show(mut self, show: TouchShow) -> Self {
        self.show = show;
        self
    }
    /// Keeps the controls up (and taking touches) while a retained UI
    /// screen is open. By default they step aside while one is (a title
    /// menu, a pause dialog, a settings page), so the fingers press the
    /// menu's buttons; HUDs don't count. For a game that plays with a
    /// screen up (its in-game buttons on a modeless screen), turn this on.
    #[must_use]
    pub fn over_screens(mut self, on: bool) -> Self {
        self.over_screens = on;
        self
    }
    /// How every control looks (the engine's see-through white discs by
    /// default): colors, images, size. See [`TouchStyle`].
    #[must_use]
    pub fn style(mut self, style: TouchStyle) -> Self {
        self.style = style;
        self
    }
    /// How the control driving `action` looks, over [`style`](Self::style).
    #[must_use]
    pub fn control_style(mut self, action: impl Into<String>, style: TouchStyle) -> Self {
        let action = action.into();
        self.styles.retain(|(a, _)| *a != action);
        self.styles.push((action, style));
        self
    }
    /// Puts the control driving `action` at `place` instead of where the
    /// engine lays it out. The player can still move it
    /// ([`TouchLayout`]).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::ui::touch::{TouchControls, TouchPlace, TouchShow};
    /// Game::new("touch", 960, 540).run_headless(0, |ctx| {
    ///     // The jump button low in the middle of the right half.
    ///     let jump = TouchPlace::new(vec2(0.75, 1.0), vec2(0.0, -60.0));
    ///     ctx.input.set_touch_controls(TouchControls::new().button("jump", "A").place("jump", jump).show(TouchShow::Always));
    ///     assert_eq!(ctx.input.touch_control_center("jump"), Some(vec2(720.0, 480.0)));
    /// });
    /// ```
    #[must_use]
    pub fn place(mut self, action: impl Into<String>, place: TouchPlace) -> Self {
        let action = action.into();
        self.places.retain(|(a, _)| *a != action);
        self.places.push((action, place));
        self
    }
    /// How the control driving `action` looks: its own style, or the set's.
    #[must_use]
    pub fn style_of(&self, action: &str) -> TouchStyle {
        self.styles
            .iter()
            .find(|(a, _)| a == action)
            .map_or(self.style, |(_, s)| *s)
    }
    /// Where the game put the control driving `action`, if it placed it.
    #[must_use]
    pub fn place_of(&self, action: &str) -> Option<TouchPlace> {
        self.places.iter().find(|(a, _)| a == action).map(|(_, p)| *p)
    }
    /// How many controls the set has.
    pub fn len(&self) -> usize {
        self.specs.len()
    }
    /// Whether the set has none.
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }
}

/// One placed control: its nodes.
#[derive(Debug, Clone)]
struct Placed {
    spec: Spec,
    /// Shown (see [`Input::set_touch_control_shown`](crate::input::Input::set_touch_control_shown)).
    shown: bool,
    /// The disc (the stick's base, the button).
    node: UiId,
    /// The stick's knob, the button's label.
    inner: UiId,
    /// Its look, with the player's opacity.
    style: TouchStyle,
}

/// The on-screen controls' nodes and the layout they were last given.
#[derive(Debug, Default)]
pub(crate) struct TouchControlsUi {
    root: Option<UiId>,
    placed: Vec<Placed>,
    /// The nodes are laid out where the zones are.
    laid_out: Option<()>,
    /// Which set of the input side's controls the nodes are for.
    generation: u64,
    /// What each control showed last: a knob's offset, a button held.
    shown: Vec<(Vec2, bool)>,
    /// Which placement of the zones the nodes are laid out for.
    moved: u64,
    /// The disc the controls are drawn with (made once).
    pub(crate) texture: Option<Texture>,
    /// The arrange screen, while it is open ([`Ctx::arrange_touch_controls`]).
    pub(crate) arrange: Option<Box<arrange::Arrange>>,
}

/// `color`, `opacity` times as opaque.
fn faded(color: Color, opacity: f32) -> Color {
    Color {
        a: color.a * opacity,
        ..color
    }
}

/// `centre` moved so a disc of `radius` around it is inside a view `view`
/// large (where it fits).
fn keep_inside(centre: Vec2, radius: f32, view: Vec2) -> Vec2 {
    let axis = |c: f32, size: f32| {
        if size >= 2.0 * radius {
            c.clamp(radius, size - radius)
        } else {
            c
        }
    };
    vec2(axis(centre.x, view.x), axis(centre.y, view.y))
}

/// The disc the controls are drawn with: a see-through fill with an opaque
/// rim, white (the paint tints it), antialiased.
pub(crate) fn disc_pixels() -> Vec<u8> {
    let r = DISC as f32 / 2.0;
    let rim = DISC as f32 * 0.06;
    let mut rgba = Vec::with_capacity((DISC * DISC * 4) as usize);
    for y in 0..DISC {
        for x in 0..DISC {
            let d = vec2(x as f32 + 0.5 - r, y as f32 + 0.5 - r).length();
            let coverage = (r - d).clamp(0.0, 1.0);
            let in_rim = ((d - (r - rim)) + 0.5).clamp(0.0, 1.0);
            let alpha = coverage * (0.45 + 0.55 * in_rim);
            rgba.extend_from_slice(&[255, 255, 255, (alpha * 255.0).round() as u8]);
        }
    }
    rgba
}

/// Where each control goes, in UI units, for a view `view` units large:
/// `(centre, radius, reach)`, in the order of `specs`.
fn place(specs: &[Spec], view: Vec2) -> Vec<(Vec2, f32, f32)> {
    let base = view.x.min(view.y);
    let stick = (base * 0.14).clamp(44.0, 96.0);
    let button = (base * 0.075).clamp(26.0, 48.0);
    let corner = (base * 0.045).clamp(18.0, 28.0);
    let margin = (base * 0.04).clamp(12.0, 32.0);
    let right_stick = specs.iter().any(|s| matches!(s, Spec::Stick { right: true, .. }));
    let first_button = if right_stick {
        vec2(view.x - margin - button, view.y - 2.0 * margin - 2.0 * stick - button)
    } else {
        vec2(view.x - margin - button, view.y - margin - button)
    };
    let (mut buttons, mut corners) = (0, 0);
    specs
        .iter()
        .map(|spec| match spec {
            Spec::Stick { right, .. } => {
                let x = if *right {
                    view.x - margin - stick
                } else {
                    margin + stick
                };
                (vec2(x, view.y - margin - stick), stick, stick * 1.6)
            }
            Spec::Button { .. } => {
                buttons += 1;
                let (column, row) = ((buttons - 1) % 2, (buttons - 1) / 2);
                let offset = vec2(
                    -(column as f32) * 2.3 * button,
                    -(row as f32) * 2.3 * button - column as f32 * 0.9 * button,
                );
                (first_button + offset, button, button * 1.25)
            }
            Spec::Corner { .. } => {
                let i = corners as f32;
                corners += 1;
                (
                    vec2(
                        view.x - margin - corner - i * (2.0 * corner + margin / 2.0),
                        margin + corner,
                    ),
                    corner,
                    corner * 1.5,
                )
            }
        })
        .collect()
}

/// A disc of `radius` around `centre`, in UI units of its parent.
fn disc_layout(centre: Vec2, radius: f32) -> UiLayout {
    UiLayout::anchored(0.0, 0.0, 0.0, 0.0)
        .left(centre.x - radius)
        .top(centre.y - radius)
        .width(2.0 * radius)
        .height(2.0 * radius)
}

/// The knob's share of a stick's radius.
const KNOB_SHARE: f32 = 0.45;

/// The input zones of `controls`, not placed yet.
pub(crate) fn zones(controls: &TouchControls) -> Vec<Zone> {
    controls
        .specs
        .iter()
        .map(|spec| Zone {
            kind: match spec {
                Spec::Stick { .. } => ZoneKind::Stick,
                _ => ZoneKind::Button,
            },
            action: spec.action().to_string(),
            slot: None,
            center: Vec2::ZERO,
            radius: 1.0,
            reach: 0.0,
            deadzone: 0.1,
            touch: None,
            holders: 0,
            value: Vec2::ZERO,
            shown: true,
        })
        .collect()
}

/// Places `zones` (those of `controls`) for a window `window` pixels large
/// at UI scale `scale`, in window pixels: where the engine lays them out,
/// or where the game put them, or where the player moved them, at the
/// sizes of their style and the player's layout, inside the view.
pub(crate) fn place_zones(
    controls: &TouchControls,
    layout: &TouchLayout,
    zones: &mut [Zone],
    window: Vec2,
    scale: f32,
) {
    let scale = scale.max(1e-3);
    let view = window / scale;
    let spots = place(&controls.specs, view);
    for ((zone, spec), (centre, radius, reach)) in zones.iter_mut().zip(&controls.specs).zip(spots) {
        let action = spec.action();
        let adjust = layout.adjusted(action);
        let style = controls.style_of(action).size;
        let size = (if style.is_finite() { style.max(0.1) } else { 1.0 }) * layout.size * adjust.size;
        let (radius, reach) = (radius * size, reach * size);
        let centre = adjust
            .place
            .or_else(|| controls.place_of(action))
            .map_or(centre, |p| p.centre(view));
        zone.center = keep_inside(centre, radius, view) * scale;
        zone.radius = radius * scale;
        zone.reach = reach * scale;
    }
}

/// Where the centre of a control `radius` window pixels large dragged to
/// `at` goes, in window pixels: inside the window.
pub(crate) fn drag_to(at: Vec2, radius: f32, view: (Vec2, f32)) -> Vec2 {
    keep_inside(at, radius, view.0)
}

impl Ctx {
    /// Engine side, every frame: builds the nodes of controls set since the
    /// last frame, places them for the window, shows or hides them, moves
    /// the knobs and lights the held buttons.
    pub(crate) fn update_touch_controls(&mut self) {
        self.update_touch_arrange();
        if self.ui_state.touch.generation != self.input.touch.generation {
            self.build_touch_controls();
        }
        let Some(root) = self.ui_state.touch.root else {
            return;
        };
        // The window or the UI scale changed: place the zones again.
        let view = (self.window_size(), self.ui.scale());
        let touch = &mut self.input.touch;
        if touch.view != view {
            touch.view = view;
            place_zones(&touch.controls, &touch.layout, &mut touch.zones, view.0, view.1);
            self.ui_state.touch.laid_out = None;
        }
        self.input.touch.suspended = !self.input.touch.controls.over_screens && self.ui.top_screen().is_some();
        // Being moved by the player, they show even under a screen.
        let active = self.input.touch.controls_drawn();
        match (active, self.ui.is_hud(root)) {
            (true, false) => self.ui.show_hud(root),
            (false, true) => self.ui.hide_hud(root),
            _ => {}
        }
        // Controls shown or hidden on the input side since the last frame,
        // or moved by the player.
        let state = &mut self.ui_state.touch;
        if state.moved != self.input.touch.moved {
            state.moved = self.input.touch.moved;
            state.laid_out = None;
        }
        for (p, zone) in state.placed.iter_mut().zip(&self.input.touch.zones) {
            if p.shown != zone.shown {
                p.shown = zone.shown;
                state.laid_out = None;
            }
        }
        if state.laid_out.is_none() {
            self.lay_out_touch_controls();
        }
        let scale = self.ui.scale().max(1e-3);
        let state = &mut self.ui_state.touch;
        // The one the player is moving or picked to resize.
        let dragged = self.input.touch.edited_zone().or(self.input.touch.picked_zone());
        for (i, p) in state.placed.iter().enumerate() {
            let Some(zone) = self.input.touch.zones.get(i) else {
                continue;
            };
            // The one the player is moving shows as held.
            let now = (zone.value, zone.held() || dragged == Some(i));
            if state.shown[i] == now {
                continue;
            }
            state.shown[i] = now;
            match p.spec {
                Spec::Stick { .. } => {
                    let radius = zone.radius / scale;
                    let centre = Vec2::splat(radius) + zone.value * radius;
                    self.ui.set_layout(p.inner, disc_layout(centre, radius * KNOB_SHARE));
                }
                _ => {
                    if let Some(mut paint) = self.ui.paint(p.node) {
                        paint.color = if now.1 { p.style.held } else { p.style.button };
                        self.ui.set_paint(p.node, paint);
                    }
                }
            }
            if matches!(p.spec, Spec::Stick { .. })
                && let Some(mut paint) = self.ui.paint(p.node)
            {
                let color = if dragged == Some(i) { p.style.held } else { p.style.base };
                if paint.color != color {
                    paint.color = color;
                    self.ui.set_paint(p.node, paint);
                }
            }
        }
    }

    /// Replaces the controls' nodes with those of the set on the input side.
    fn build_touch_controls(&mut self) {
        let state = &mut self.ui_state.touch;
        state.generation = self.input.touch.generation;
        if let Some(root) = state.root.take() {
            self.ui.hide_hud(root);
            self.ui.remove(root);
        }
        state.placed.clear();
        state.shown.clear();
        state.laid_out = None;
        if self.input.touch.controls.is_empty() {
            return;
        }
        let texture = match self.ui_state.touch.texture {
            Some(t) => t,
            None => {
                let t = self.gfx.make_texture(DISC, DISC, &disc_pixels());
                self.ui_state.touch.texture = Some(t);
                t
            }
        };
        let ui = &mut self.ui;
        let root = ui
            .root()
            .layout(UiLayout::fill())
            .background(Color::TRANSPARENT)
            .name("touch.controls")
            .id();
        let disc = |color: Color, image: Option<Texture>| UiPaint {
            background: Color::TRANSPARENT,
            hover: Color::TRANSPARENT,
            pressed: Color::TRANSPARENT,
            disabled: Color::TRANSPARENT,
            color,
            texture: Some(image.unwrap_or(texture)),
            ..Default::default()
        };
        let controls = &self.input.touch.controls;
        let opacity = self.input.touch.layout.opacity;
        let mut placed = Vec::with_capacity(controls.specs.len());
        for spec in &controls.specs {
            let mut style = controls.style_of(spec.action());
            for color in [
                &mut style.base,
                &mut style.knob,
                &mut style.button,
                &mut style.held,
                &mut style.label,
            ] {
                *color = faded(*color, opacity);
            }
            let (node, inner) = match spec {
                Spec::Stick { action, .. } => {
                    let base = ui
                        .panel(root)
                        .paint(disc(style.base, style.image))
                        .name(format!("touch.{action}"))
                        .accessible_name(format!("{action} stick"))
                        .accessible_description("on-screen stick: drag to move")
                        .id();
                    let knob = ui
                        .panel(base)
                        .paint(disc(style.knob, style.knob_image.or(style.image)))
                        .id();
                    (base, knob)
                }
                Spec::Button { action, label } | Spec::Corner { action, label } => {
                    let button = ui.add_child(root, UiWidget::Button, UiLayout::auto());
                    ui.set_paint(button, disc(style.button, style.image));
                    ui.set_name(button, format!("touch.{action}"));
                    ui.set_accessible_name(button, if label.is_empty() { action } else { label });
                    let text = ui
                        .label(button, label)
                        .background(Color::TRANSPARENT)
                        .color(style.label)
                        .align(UiTextAlign::Center)
                        .id();
                    (button, text)
                }
            };
            placed.push(Placed {
                spec: spec.clone(),
                shown: true,
                node,
                inner,
                style,
            });
        }
        let state = &mut self.ui_state.touch;
        state.root = Some(root);
        state.shown = vec![(Vec2::ZERO, false); placed.len()];
        state.placed = placed;
    }

    /// Lays the controls' nodes out where their zones are.
    fn lay_out_touch_controls(&mut self) {
        let scale = self.ui.scale().max(1e-3);
        let state = &mut self.ui_state.touch;
        state.laid_out = Some(());
        for (i, p) in state.placed.iter().enumerate() {
            let Some(zone) = self.input.touch.zones.get(i) else {
                continue;
            };
            let (centre, radius) = (zone.center / scale, zone.radius / scale);
            let layout = disc_layout(centre, radius);
            self.ui
                .set_layout(p.node, if p.shown { layout } else { layout.hidden() });
            match p.spec {
                Spec::Stick { .. } => {
                    let knob = radius * KNOB_SHARE;
                    self.ui.set_layout(p.inner, disc_layout(Vec2::splat(radius), knob));
                    state.shown[i].0 = Vec2::ZERO;
                }
                _ => {
                    let size = radius * p.style.label_size.clamp(0.0, 2.0);
                    let mut paint = self.ui.paint(p.inner).unwrap_or_default();
                    paint.font_size = size;
                    self.ui.set_paint(p.inner, paint);
                    self.ui.set_layout(
                        p.inner,
                        UiLayout::anchored(0.0, 0.0, 1.0, 0.0)
                            .top(radius - size * 0.62)
                            .height(size * 1.3),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_sit_in_their_corners_inside_the_view() {
        let specs = TouchControls::new()
            .stick("move")
            .button("a", "A")
            .button("b", "B")
            .button("c", "C")
            .corner_button("pause", "II")
            .corner_button("map", "M")
            .specs;
        let view = vec2(960.0, 540.0);
        let spots = place(&specs, view);
        for &(c, r, reach) in &spots {
            assert!(
                c.x - r >= 0.0 && c.y - r >= 0.0 && c.x + r <= view.x && c.y + r <= view.y,
                "{c} {r}"
            );
            assert!(reach >= r);
        }
        let (stick, a, b, c, pause, map) = (spots[0].0, spots[1].0, spots[2].0, spots[3].0, spots[4].0, spots[5].0);
        assert!(stick.x < view.x / 4.0 && stick.y > view.y * 0.6, "bottom left: {stick}");
        assert!(a.x > view.x * 0.8 && a.y > view.y * 0.75, "bottom right: {a}");
        assert!(b.x < a.x && b.y < a.y, "the second left of and above the first");
        assert!(c.y < a.y - 2.0 * spots[1].1, "the third above the first");
        assert!(
            pause.y < view.y / 4.0 && pause.x > map.x,
            "corner buttons from the right"
        );
        // No two overlap.
        for i in 0..spots.len() {
            for j in i + 1..spots.len() {
                let (a, b) = (spots[i], spots[j]);
                assert!(a.0.distance(b.0) >= a.1 + b.1, "{i} and {j} overlap");
            }
        }
    }

    #[test]
    fn a_right_stick_lifts_the_buttons_above_it() {
        let specs = TouchControls::new()
            .stick("move")
            .right_stick("look")
            .button("jump", "A")
            .specs;
        let spots = place(&specs, vec2(1280.0, 720.0));
        let (look, jump) = (spots[1], spots[2]);
        assert!(look.0.x > 1000.0);
        assert!(jump.0.y + jump.1 <= look.0.y - look.1, "the button is above the stick");
    }

    #[test]
    fn a_control_the_player_moved_keeps_its_distance_from_its_corner_when_the_window_changes() {
        let controls = TouchControls::new().stick("move").button("jump", "A");
        let mut layout = TouchLayout::default();
        // 100 left of and 80 above the bottom right corner, at UI scale 2.
        layout.adjust("jump").place = Some(TouchPlace::nearest_corner(vec2(380.0, 190.0), vec2(480.0, 270.0)));
        let mut zones = zones(&controls);
        place_zones(&controls, &layout, &mut zones, vec2(960.0, 540.0), 2.0);
        assert_eq!(zones[1].center, vec2(760.0, 380.0));
        // The phone turns: the same distance from the same corner.
        place_zones(&controls, &layout, &mut zones, vec2(540.0, 960.0), 2.0);
        assert_eq!(zones[1].center, vec2(340.0, 800.0));
        // A view too small for it: moved in.
        place_zones(&controls, &layout, &mut zones, vec2(200.0, 200.0), 1.0);
        let (c, r) = (zones[1].center, zones[1].radius);
        assert!(
            c.x - r >= 0.0 && c.y - r >= 0.0 && c.x + r <= 200.0 && c.y + r <= 200.0,
            "{c} {r}"
        );
    }

    #[test]
    fn sizes_multiply_the_styles_and_the_players() {
        let controls = TouchControls::new()
            .button("jump", "A")
            .button("fire", "B")
            .control_style(
                "fire",
                TouchStyle {
                    size: 2.0,
                    ..TouchStyle::default()
                },
            );
        let mut layout = TouchLayout {
            size: 1.5,
            ..TouchLayout::default()
        };
        layout.adjust("fire").size = 0.5;
        let mut zones = zones(&controls);
        place_zones(&controls, &TouchLayout::default(), &mut zones, vec2(960.0, 540.0), 1.0);
        let plain = zones[0].radius;
        place_zones(&controls, &layout, &mut zones, vec2(960.0, 540.0), 1.0);
        assert!((zones[0].radius - plain * 1.5).abs() < 1e-4);
        assert!((zones[1].radius - plain * 2.0 * 1.5 * 0.5).abs() < 1e-4);
        assert!(zones[1].reach >= zones[1].radius);
    }

    #[test]
    fn nonsense_numbers_in_a_layout_file_are_made_usable() {
        let mut layout = TouchLayout {
            size: f32::NAN,
            opacity: 7.0,
            ..TouchLayout::default()
        };
        layout.adjust("jump").size = -3.0;
        layout.adjust("fire").place = Some(TouchPlace::new(vec2(f32::INFINITY, 0.0), Vec2::ZERO));
        let layout = layout.sane();
        assert_eq!((layout.size, layout.opacity), (1.0, 1.0));
        assert_eq!(layout.adjusted("jump").size, 0.1);
        assert_eq!(layout.adjusted("fire").place, None);
    }

    #[test]
    fn the_disc_has_a_solid_rim_a_see_through_middle_and_clear_corners() {
        let px = disc_pixels();
        let alpha = |x: u32, y: u32| px[((y * DISC + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(DISC / 2, 1), 255, "the rim");
        let middle = alpha(DISC / 2, DISC / 2);
        assert!((100..140).contains(&middle), "{middle}");
    }
}
