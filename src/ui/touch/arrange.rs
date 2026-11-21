//! The ready-made screen for arranging the on-screen controls
//! ([`Input::arrange_touch_controls`](crate::input::Input::arrange_touch_controls)): a thin front, built with the retained
//! UI, over the edit mode ([`Input::edit_touch_controls`](crate::input::Input::edit_touch_controls))
//! and the player's [`TouchLayout`].

use glam::{Vec2, vec2};

use super::{Spec, TouchLayout};
use crate::camera::Rect;
use crate::color::Color;
use crate::ctx::Ctx;
use crate::ui::{UiEvent, UiId, UiLayout, UiTextAlign};

/// The panel's width at most, in UI units.
const CARD_WIDTH: f32 = 360.0;
/// Space between the panel and the view's edges, in UI units.
const MARGIN: f32 = 12.0;
const PADDING: f32 = 12.0;
const GAP: f32 = 6.0;
/// The rows' heights: title, note, picked control, size, opacity, buttons.
const ROWS: [f32; 6] = [28.0, 18.0, 40.0, 30.0, 30.0, 40.0];
/// The title's text size, and a line of text's height, in UI units: the
/// labels sit in the middle of their rows.
const TITLE_SIZE: f32 = 22.0;
const LINE: f32 = 20.0;
/// The panel's height, in UI units.
const CARD_HEIGHT: f32 = 2.0 * PADDING + 5.0 * GAP + ROWS[0] + ROWS[1] + ROWS[2] + ROWS[3] + ROWS[4] + ROWS[5];
/// What "Size" spans (every control at once).
const SIZES: (f32, f32) = (0.5, 2.0);
/// What "Opacity" spans.
const OPACITIES: (f32, f32) = (0.2, 1.0);
/// One control's size: its range, and what a press of `-` or `+` changes.
const OWN_SIZES: (f32, f32) = (0.5, 2.0);
const OWN_STEP: f32 = 0.1;
const NOTE: &str = "Drag a control to move it, tap one to resize it.";

/// The open arrange screen's nodes, and the layout from when it opened.
#[derive(Debug)]
pub(crate) struct Arrange {
    root: UiId,
    card: UiId,
    note: UiId,
    picked: UiId,
    smaller: UiId,
    own_size: UiId,
    bigger: UiId,
    size: UiId,
    opacity: UiId,
    reset: UiId,
    cancel: UiId,
    save: UiId,
    /// The player's arrangement when the screen opened: Cancel puts it back.
    before: TouchLayout,
    /// Where the panel is laid out, in UI units (`None`: hidden), and the
    /// control the picked row was last filled for.
    card_at: Option<Option<Vec2>>,
    shown_pick: Option<Option<(usize, u32)>>,
}

/// Slider position (0..=1) of `value` in `range`.
fn slider_of(value: f32, range: (f32, f32)) -> f32 {
    ((value - range.0) / (range.1 - range.0)).clamp(0.0, 1.0)
}

/// The value at slider position `at` in `range`.
fn value_of(at: f32, range: (f32, f32)) -> f32 {
    range.0 + at.clamp(0.0, 1.0) * (range.1 - range.0)
}

/// The size of the panel in a view `view` UI units large.
fn card_size(view: Vec2) -> Vec2 {
    vec2(CARD_WIDTH.min(view.x - 2.0 * MARGIN).max(0.0), CARD_HEIGHT)
}

/// Where the panel's top left goes in a view `view` UI units large so it
/// hides as little as it can of `discs` (centre, radius, in UI units): the
/// middle of the view, else its top, bottom, left or right.
pub(crate) fn card_spot(view: Vec2, discs: impl Iterator<Item = (Vec2, f32)> + Clone) -> Vec2 {
    let size = card_size(view);
    let free = (view - size).max(Vec2::ZERO);
    let middle = free / 2.0;
    let near = Vec2::splat(MARGIN).min(middle);
    let far = (free - Vec2::splat(MARGIN)).max(middle);
    let spots = [
        middle,
        vec2(middle.x, near.y),
        vec2(middle.x, far.y),
        vec2(near.x, middle.y),
        vec2(far.x, middle.y),
    ];
    let hidden = |at: Vec2| -> f32 {
        discs
            .clone()
            .map(|(c, r)| {
                let lo = (c - Vec2::splat(r)).max(at);
                let hi = (c + Vec2::splat(r)).min(at + size);
                let overlap = (hi - lo).max(Vec2::ZERO);
                overlap.x * overlap.y
            })
            .sum()
    };
    let mut best = (spots[0], hidden(spots[0]));
    for &spot in &spots[1..] {
        let h = hidden(spot);
        if h < best.1 {
            best = (spot, h);
        }
    }
    best.0
}

impl Ctx {
    /// Builds the arrange screen [`Input::arrange_touch_controls`](crate::input::Input::arrange_touch_controls)
    /// asked for; `before` is the arrangement Cancel puts back.
    fn open_touch_arrange(&mut self, before: TouchLayout) {
        let ui = &mut self.ui;
        let root = ui
            .root()
            .layout(UiLayout::fill())
            .background(Color::TRANSPARENT)
            .name("touch.arrange")
            .accessible_name("Arrange the controls")
            .id();
        // In the game's theme; where its buttons are the panels' color
        // (the default theme), on a darker card, so they stand out.
        let theme = ui.theme();
        let mut background = theme.panel;
        if theme.control == theme.panel {
            background = Color {
                r: background.r * 0.55,
                g: background.g * 0.55,
                b: background.b * 0.55,
                a: background.a.max(0.92),
            };
        }
        let card = ui
            .panel(root)
            .layout(UiLayout::fixed(CARD_WIDTH, CARD_HEIGHT).padding(PADDING).gap(GAP))
            .background(background)
            .name("touch.arrange.card")
            .id();
        ui.title(card, "Arrange the controls")
            .layout(UiLayout::row(ROWS[0]))
            .size(TITLE_SIZE)
            .align(UiTextAlign::Center)
            .name("touch.arrange.title");
        let note = ui
            .label(card, NOTE)
            .layout(UiLayout::row(ROWS[1]))
            .size(13.0)
            .align(UiTextAlign::Center)
            .name("touch.arrange.note")
            .id();
        let row = |ui: &mut crate::ui::Ui, height: f32| {
            ui.panel(card)
                .layout(UiLayout::row(height).gap(GAP))
                .background(Color::TRANSPARENT)
                .id()
        };
        let pick_row = row(ui, ROWS[2]);
        let picked = ui
            .label(pick_row, "")
            .layout(UiLayout::auto().grow(1.0).height(LINE).top((ROWS[2] - LINE) / 2.0))
            .name("touch.arrange.picked")
            .id();
        let smaller = ui
            .button(pick_row, "-")
            .layout(UiLayout::fixed(44.0, ROWS[2]))
            .accessible_name("Smaller")
            .name("touch.arrange.smaller")
            .id();
        let own_size = ui
            .label(pick_row, "")
            .layout(UiLayout::fixed(56.0, LINE).top((ROWS[2] - LINE) / 2.0))
            .align(UiTextAlign::Center)
            .name("touch.arrange.picked_size")
            .id();
        let bigger = ui
            .button(pick_row, "+")
            .layout(UiLayout::fixed(44.0, ROWS[2]))
            .accessible_name("Bigger")
            .name("touch.arrange.bigger")
            .id();
        let slider_row = |ui: &mut crate::ui::Ui, height: f32, label: &str, part: &str, value: f32| {
            let r = row(ui, height);
            ui.label(r, label)
                .layout(UiLayout::fixed(84.0, LINE).top((height - LINE) / 2.0))
                .name(format!("touch.arrange.{part}_label"));
            ui.slider(r)
                .layout(UiLayout::auto().grow(1.0))
                .value(value)
                .accessible_name(label)
                .name(format!("touch.arrange.{part}"))
                .id()
        };
        let size = slider_row(ui, ROWS[3], "Size", "size", slider_of(before.size, SIZES));
        let opacity = slider_row(ui, ROWS[4], "Opacity", "opacity", slider_of(before.opacity, OPACITIES));
        let buttons = row(ui, ROWS[5]);
        let mut button = |label: &str, part: &str| {
            ui.button(buttons, label)
                .layout(UiLayout::auto().grow(1.0))
                .name(format!("touch.arrange.{part}"))
                .id()
        };
        let reset = button("Reset", "reset");
        let cancel = button("Cancel", "cancel");
        let save = button("Save", "save");
        ui.push_modeless(root);
        self.ui_state.touch.arrange = Some(Box::new(Arrange {
            root,
            card,
            note,
            picked,
            smaller,
            own_size,
            bigger,
            size,
            opacity,
            reset,
            cancel,
            save,
            before,
            card_at: None,
            shown_pick: None,
        }));
    }

    /// Engine side, after each pass of the retained UI: the arrange
    /// screen's buttons and sliders take effect, its panel keeps clear of
    /// the controls, and it closes when the player is done.
    pub(crate) fn update_touch_arrange(&mut self) {
        if self.ui_state.touch.arrange.is_none() {
            let Some(before) = self.input.touch.arrange_before.take() else {
                return;
            };
            self.open_touch_arrange(before);
        }
        let Some(mut screen) = self.ui_state.touch.arrange.take() else {
            return;
        };
        // Closed from outside: Escape, the game. The arrangement goes back.
        if !self.ui.is_screen(screen.root) {
            self.close_touch_arrange(&screen, false);
            return;
        }
        if !self.input.editing_touch_controls() {
            // The game ended edit mode: the screen goes, the layout stays.
            self.close_touch_arrange(&screen, true);
            return;
        }
        let mut done = None;
        let ids = [
            screen.smaller,
            screen.bigger,
            screen.size,
            screen.opacity,
            screen.reset,
            screen.cancel,
            screen.save,
        ];
        let mut i = 0;
        while i < self.ui.events.len() {
            let event = self.ui.events[i];
            let id = match event {
                UiEvent::Clicked(id) | UiEvent::Changed(id) if ids.contains(&id) => id,
                _ => {
                    i += 1;
                    continue;
                }
            };
            self.ui.events.remove(i);
            let mut layout = self.input.touch.layout.clone();
            if id == screen.smaller || id == screen.bigger {
                let Some(action) = self.picked_action() else { continue };
                let step = if id == screen.bigger { OWN_STEP } else { -OWN_STEP };
                let adjust = layout.adjust(&action);
                // Rounded to the step, so ten presses make a round number.
                let next = ((adjust.size + step) / OWN_STEP).round() * OWN_STEP;
                adjust.size = next.clamp(OWN_SIZES.0, OWN_SIZES.1);
            } else if id == screen.size {
                layout.size = value_of(self.ui.value(id).unwrap_or(0.0), SIZES);
            } else if id == screen.opacity {
                layout.opacity = value_of(self.ui.value(id).unwrap_or(1.0), OPACITIES);
            } else if id == screen.reset {
                layout = TouchLayout::default();
                self.ui.set_value(screen.size, slider_of(layout.size, SIZES));
                self.ui.set_value(screen.opacity, slider_of(layout.opacity, OPACITIES));
            } else if id == screen.cancel {
                done = Some(false);
                continue;
            } else {
                done = Some(true);
                continue;
            }
            self.input.set_touch_layout(layout);
            screen.shown_pick = None;
        }
        match done {
            Some(false) => return self.close_touch_arrange(&screen, false),
            Some(true) => match self.input.save_touch_layout(&mut self.storage) {
                Ok(()) => return self.close_touch_arrange(&screen, true),
                Err(e) => {
                    self.ui.set_text(screen.note, format!("Could not save: {e}"));
                }
            },
            None => {}
        }
        self.fill_touch_arrange(&mut screen);
        self.ui_state.touch.arrange = Some(screen);
    }

    /// The action of the control the player picked, if any.
    fn picked_action(&self) -> Option<String> {
        let zone = self.input.touch.zones.get(self.input.touch.picked_zone()?)?;
        Some(zone.action.clone())
    }

    /// The picked row says which control is picked and its size; the panel
    /// goes where it hides the fewest controls, and hides while one is
    /// dragged.
    fn fill_touch_arrange(&mut self, screen: &mut Arrange) {
        let touch = &self.input.touch;
        let pick = touch
            .picked_zone()
            .filter(|&i| i < touch.zones.len())
            .map(|i| (i, touch.layout.adjusted(&touch.zones[i].action).size.to_bits()));
        if screen.shown_pick != Some(pick) {
            screen.shown_pick = Some(pick);
            let name = pick.and_then(|(i, _)| {
                let spec = touch.controls.specs.get(i)?;
                Some(match spec {
                    Spec::Stick { action, .. } => format!("{action} stick"),
                    Spec::Button { action, label } | Spec::Corner { action, label } => {
                        if label.is_empty() {
                            action.clone()
                        } else {
                            label.clone()
                        }
                    }
                })
            });
            let size = pick.map(|(i, _)| touch.layout.adjusted(&touch.zones[i].action).size);
            self.ui
                .set_text(screen.picked, name.as_deref().unwrap_or("Pick a control"));
            self.ui.set_text(
                screen.own_size,
                size.map_or(String::new(), |s| format!("{:.0} %", s * 100.0)),
            );
            self.ui
                .set_enabled(screen.smaller, size.is_some_and(|s| s > OWN_SIZES.0 + 1e-3));
            self.ui
                .set_enabled(screen.bigger, size.is_some_and(|s| s < OWN_SIZES.1 - 1e-3));
        }
        let touch = &self.input.touch;
        let scale = self.ui.scale().max(1e-3);
        // The panel stays in the safe area, clear of a notch and a home bar.
        let safe = self.safe_area();
        let (origin, view) = (safe.min / scale, safe.size() / scale);
        let at = if touch.edited_zone().is_some() {
            None
        } else {
            let discs = touch
                .zones
                .iter()
                .filter(|z| z.shown)
                .map(|z| (z.center / scale - origin, z.radius / scale));
            Some(origin + card_spot(view, discs))
        };
        let size = card_size(view);
        self.input.touch.shield = at.map(|at| Rect {
            min: at * scale,
            max: (at + size) * scale,
        });
        if screen.card_at != Some(at) {
            screen.card_at = Some(at);
            let layout = UiLayout::anchored(0.0, 0.0, 0.0, 0.0)
                .left(at.unwrap_or(Vec2::ZERO).x)
                .top(at.unwrap_or(Vec2::ZERO).y)
                .width(size.x)
                .height(size.y)
                .padding(PADDING)
                .gap(GAP);
            self.ui
                .set_layout(screen.card, if at.is_some() { layout } else { layout.hidden() });
        }
    }

    /// Closes the arrange screen: edit mode ends, and the arrangement from
    /// when it opened comes back unless `keep`.
    fn close_touch_arrange(&mut self, screen: &Arrange, keep: bool) {
        self.input.touch.shield = None;
        self.input.touch.arranging = false;
        self.input.edit_touch_controls(false);
        if !keep {
            self.input.set_touch_layout(screen.before.clone());
        }
        let root = screen.root;
        let open = self.ui.is_screen(root);
        if self.ui.top_screen() == Some(root) {
            self.ui.pop_screen();
        }
        // Removing the nodes drops their queued events: the closing one is
        // queued after (Escape queued one already).
        let escaped = self.ui.closed_this_frame(root);
        self.ui.remove(root);
        if open || escaped {
            self.ui.screen_closed(root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_moves_off_the_controls_it_would_hide() {
        let view = vec2(960.0, 540.0);
        let middle = card_spot(view, std::iter::empty());
        assert_eq!(middle, (view - card_size(view)) / 2.0, "nothing in the way: the middle");
        // A control in the middle of a wide view: the panel goes left.
        let at = card_spot(view, [(view / 2.0, 40.0)].into_iter());
        assert_eq!(at, vec2(MARGIN, middle.y));
        // And one on the left too: right.
        let at = card_spot(view, [(view / 2.0, 40.0), (vec2(150.0, 270.0), 40.0)].into_iter());
        assert_eq!(at.x, view.x - CARD_WIDTH - MARGIN);
        // A tall view with a control in the middle: the top.
        let tall = vec2(390.0, 844.0);
        let at = card_spot(tall, [(tall / 2.0, 40.0)].into_iter());
        assert_eq!(at.y, MARGIN);
    }

    #[test]
    fn sliders_map_their_ranges_both_ways() {
        assert_eq!(slider_of(1.0, (0.5, 2.0)), 1.0 / 3.0);
        assert!((value_of(1.0 / 3.0, (0.5, 2.0)) - 1.0).abs() < 1e-6);
        assert_eq!(slider_of(9.0, OPACITIES), 1.0);
        assert_eq!(value_of(-1.0, OPACITIES), OPACITIES.0);
    }
}
