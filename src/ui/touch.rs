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
#![deny(missing_docs)]

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
    /// The disc the controls are drawn with (made once).
    pub(crate) texture: Option<Texture>,
}

/// Colors of the controls: white, see-through so the game shows under them.
const BASE: Color = Color::rgba(1.0, 1.0, 1.0, 0.22);
const KNOB: Color = Color::rgba(1.0, 1.0, 1.0, 0.6);
const IDLE: Color = Color::rgba(1.0, 1.0, 1.0, 0.3);
const HELD: Color = Color::rgba(1.0, 1.0, 1.0, 0.7);

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
/// at UI scale `scale`, in window pixels.
pub(crate) fn place_zones(controls: &TouchControls, zones: &mut [Zone], window: Vec2, scale: f32) {
    let scale = scale.max(1e-3);
    let spots = place(&controls.specs, window / scale);
    for (zone, (centre, radius, reach)) in zones.iter_mut().zip(spots) {
        zone.center = centre * scale;
        zone.radius = radius * scale;
        zone.reach = reach * scale;
    }
}

impl Ctx {
    /// Engine side, every frame: builds the nodes of controls set since the
    /// last frame, places them for the window, shows or hides them, moves
    /// the knobs and lights the held buttons.
    pub(crate) fn update_touch_controls(&mut self) {
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
            place_zones(&touch.controls, &mut touch.zones, view.0, view.1);
            self.ui_state.touch.laid_out = None;
        }
        self.input.touch.suspended = !self.input.touch.controls.over_screens && self.ui.top_screen().is_some();
        let active = self.input.touch.controls_active();
        match (active, self.ui.is_hud(root)) {
            (true, false) => self.ui.show_hud(root),
            (false, true) => self.ui.hide_hud(root),
            _ => {}
        }
        // Controls shown or hidden on the input side since the last frame.
        let state = &mut self.ui_state.touch;
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
        for (i, p) in state.placed.iter().enumerate() {
            let Some(zone) = self.input.touch.zones.get(i) else {
                continue;
            };
            let now = (zone.value, zone.held());
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
                        paint.color = if now.1 { HELD } else { IDLE };
                        self.ui.set_paint(p.node, paint);
                    }
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
        let disc = |color: Color| UiPaint {
            background: Color::TRANSPARENT,
            hover: Color::TRANSPARENT,
            pressed: Color::TRANSPARENT,
            disabled: Color::TRANSPARENT,
            color,
            texture: Some(texture),
            ..Default::default()
        };
        let specs = &self.input.touch.controls.specs;
        let mut placed = Vec::with_capacity(specs.len());
        for spec in specs {
            let (node, inner) = match spec {
                Spec::Stick { action, .. } => {
                    let base = ui
                        .panel(root)
                        .paint(disc(BASE))
                        .name(format!("touch.{action}"))
                        .accessible_name(format!("{action} stick"))
                        .accessible_description("on-screen stick: drag to move")
                        .id();
                    let knob = ui.panel(base).paint(disc(KNOB)).id();
                    (base, knob)
                }
                Spec::Button { action, label } | Spec::Corner { action, label } => {
                    let button = ui.add_child(root, UiWidget::Button, UiLayout::auto());
                    ui.set_paint(button, disc(IDLE));
                    ui.set_name(button, format!("touch.{action}"));
                    ui.set_accessible_name(button, if label.is_empty() { action } else { label });
                    let text = ui
                        .label(button, label)
                        .background(Color::TRANSPARENT)
                        .color(Color::WHITE)
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
                    let size = radius * 0.8;
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
    fn the_disc_has_a_solid_rim_a_see_through_middle_and_clear_corners() {
        let px = disc_pixels();
        let alpha = |x: u32, y: u32| px[((y * DISC + x) * 4 + 3) as usize];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(DISC / 2, 1), 255, "the rim");
        let middle = alpha(DISC / 2, DISC / 2);
        assert!((100..140).contains(&middle), "{middle}");
    }
}
