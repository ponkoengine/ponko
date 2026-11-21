//! Drawing the widget set: the parts each widget adds to its box (a
//! dropdown's arrow, a radio button's circle, a progress bar's fill, a
//! spinner's dots, ...) and what is drawn over the whole tree (the tooltip,
//! a dragged item and where it would land).

use super::retained::{Drag, Ui, UiId, UiWidget};
use crate::{camera::Rect, color::Color, ctx::Ctx, text::TextParams};
use glam::{Vec2, vec2};

impl Ui {
    /// Whether a key-binding field is listening (for the engine's input).
    pub(crate) fn capturing(&self) -> bool {
        self.widgets.capturing.is_some()
    }

    /// The fill node `id` is drawn with in `state`, the chosen tint
    /// included. `None` for an id not in the tree.
    pub(crate) fn fill_in(&self, id: UiId, state: PaintState) -> Option<Color> {
        let n = self.node(id)?;
        Some(match state {
            PaintState::Disabled => n.paint.disabled,
            PaintState::Pressed => self.widget_fill(id, n.paint.pressed),
            PaintState::Hovered | PaintState::Focused => self.widget_fill(id, n.paint.hover),
            PaintState::Idle => self.widget_fill(id, n.paint.background),
        })
    }

    /// The state node `id` is drawn in now.
    pub(super) fn paint_state(&self, id: UiId) -> PaintState {
        match self.node(id) {
            Some(n) if !n.enabled => PaintState::Disabled,
            _ if self.pressed == Some(id) => PaintState::Pressed,
            _ if self.hovered == Some(id) => PaintState::Hovered,
            _ if self.focus == Some(id) => PaintState::Focused,
            _ => PaintState::Idle,
        }
    }

    /// The fill a node is drawn with, given the one its state picked: a
    /// chosen list item or dropdown option is tinted with the accent, a
    /// chosen tab is drawn as if hovered.
    pub(super) fn widget_fill(&self, id: UiId, fill: Color) -> Color {
        let Some(n) = self.node(id) else { return fill };
        let chosen = match n.widget {
            UiWidget::ListItem | UiWidget::MenuItem => self.access_state(id).selected,
            UiWidget::Tab => {
                return if n.value >= 0.5 && n.enabled {
                    n.paint.hover
                } else {
                    fill
                };
            }
            _ => false,
        };
        if !chosen || !n.enabled {
            return fill;
        }
        // The accent over the fill, a third of the way.
        let (a, t) = (self.theme.accent, 0.35);
        if fill.a < 0.01 {
            Color::rgba(a.r, a.g, a.b, t)
        } else {
            let mix = |f: f32, a: f32| f + (a - f) * t;
            Color::rgba(mix(fill.r, a.r), mix(fill.g, a.g), mix(fill.b, a.b), fill.a)
        }
    }

    /// The corner radius a node is drawn with, in window pixels.
    pub(super) fn corner_radius(&self, id: UiId) -> f32 {
        let round = self.node(id).is_some_and(|n| {
            !matches!(
                n.widget,
                UiWidget::Label | UiWidget::Image | UiWidget::Splitter | UiWidget::Tabs | UiWidget::Split
            )
        });
        if round {
            self.theme.radius.max(0.0) * self.scale
        } else {
            0.0
        }
    }
}

/// Whether `widget` frames its text in a box (a control): its text is
/// centred on its height and kept three quarters of the theme's spacing
/// from its sides.
pub(super) fn framed(widget: UiWidget) -> bool {
    use UiWidget::*;
    matches!(
        widget,
        Button | TextInput | Toggle | Dropdown | Radio | NumberField | KeyBinding | Tab | ListItem | MenuItem
    )
}

/// The states a node is drawn in, each with its own fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaintState {
    Idle,
    Hovered,
    Focused,
    Pressed,
    Disabled,
}

/// An outline `width` thick just inside `rect`.
pub(super) fn outline(gfx: &mut crate::gfx::Graphics, rect: Rect, width: f32, color: Color, z: f32) {
    let (min, max, w) = (rect.min, rect.max, width);
    gfx.ui_fill(Rect::new(min.x, min.y, rect.size().x, w), color, z);
    gfx.ui_fill(Rect::new(min.x, max.y - w, rect.size().x, w), color, z);
    gfx.ui_fill(Rect::new(min.x, min.y, w, rect.size().y), color, z);
    gfx.ui_fill(Rect::new(max.x - w, min.y, w, rect.size().y), color, z);
}

impl Ctx {
    /// What node `id`'s widget draws inside its `content` box, under its
    /// text; `text` is narrowed to leave room for it.
    pub(super) fn draw_widget_parts(&mut self, ui: &Ui, id: UiId, content: Rect, text: &mut Rect, z: f32) {
        let Some(n) = ui.node(id) else { return };
        let scale = ui.scale();
        let (ink, accent) = (n.paint.color, n.paint.border);
        let gfx = &mut self.gfx;
        match n.widget {
            UiWidget::Dropdown => {
                // A downward arrow at the right end.
                let (w, h) = (10.0 * scale, 6.0 * scale);
                let right = content.max.x;
                let top = content.center().y - h * 0.5;
                for row in 0..3 {
                    let width = w * (3 - row) as f32 / 3.0;
                    gfx.ui_fill(
                        Rect::new(
                            right - w * 0.5 - width * 0.5,
                            top + row as f32 * h / 3.0,
                            width,
                            h / 3.0,
                        ),
                        ink,
                        z + 0.4,
                    );
                }
                text.max.x -= w + 6.0 * scale;
            }
            UiWidget::Radio => {
                // The circle, centred on the row.
                let side = content.size().y.min(20.0 * scale);
                let center = Vec2::new(content.min.x + side * 0.5, content.center().y);
                gfx.ui_circle(center, side * 0.5, accent, z + 0.4);
                gfx.ui_circle(
                    center,
                    (side * 0.5 - 2.0 * scale).max(0.0),
                    n.paint.background,
                    z + 0.45,
                );
                if n.value >= 0.5 {
                    gfx.ui_circle(center, (side * 0.5 - 5.0 * scale).max(0.0), ink, z + 0.5);
                }
                text.min.x += side + 6.0 * scale;
            }
            UiWidget::Progress => {
                let fill = Rect::new(
                    content.min.x,
                    content.min.y,
                    content.size().x * n.value,
                    content.size().y,
                );
                let radius = ui.corner_radius(id).min(fill.size().x * 0.5);
                gfx.ui_fill_rounded(fill, accent, z + 0.4, radius);
            }
            UiWidget::Spinner => {
                // Eight dots round a circle; the bright one goes round
                // 1.25 times a second.
                let center = content.center();
                let r = content.size().min_element() * 0.5;
                let dot = r * 0.2;
                let head = (ui.widgets.time * 10.0).rem_euclid(8.0);
                for i in 0..8 {
                    let angle = i as f32 * std::f32::consts::TAU / 8.0 - std::f32::consts::FRAC_PI_2;
                    let at = center + Vec2::new(angle.cos(), angle.sin()) * (r - dot);
                    let behind = (head - i as f32).rem_euclid(8.0) / 8.0;
                    let alpha = 1.0 - behind * 0.8;
                    gfx.ui_circle(
                        at,
                        dot,
                        Color::rgba(accent.r, accent.g, accent.b, accent.a * alpha),
                        z + 0.4,
                    );
                }
            }
            UiWidget::NumberField => {
                // `-` and `+` in the quarters a click steps with; the
                // number between them.
                let quarter = n.rect.size().x * 0.25;
                text.min.x = text.min.x.max(n.rect.min.x + quarter);
                text.max.x = text.max.x.min(n.rect.max.x - quarter);
                let (bar, thick) = ((8.0 * scale).min(quarter * 0.6), (2.0 * scale).max(1.0));
                let y = content.center().y;
                for (x, plus) in [
                    (n.rect.min.x + quarter * 0.5, false),
                    (n.rect.max.x - quarter * 0.5, true),
                ] {
                    gfx.ui_fill(Rect::new(x - bar * 0.5, y - thick * 0.5, bar, thick), ink, z + 0.4);
                    if plus {
                        gfx.ui_fill(Rect::new(x - thick * 0.5, y - bar * 0.5, thick, bar), ink, z + 0.4);
                    }
                }
            }
            UiWidget::Tab if n.value >= 0.5 => {
                let h = 3.0 * scale;
                gfx.ui_fill(
                    Rect::new(n.rect.min.x, n.rect.max.y - h, n.rect.size().x, h),
                    accent,
                    z + 0.4,
                );
            }
            UiWidget::Splitter => {
                // A grip of three dots across the middle.
                let c = n.rect.center();
                let along = if n.rect.size().x < n.rect.size().y {
                    Vec2::new(0.0, 6.0 * scale)
                } else {
                    Vec2::new(6.0 * scale, 0.0)
                };
                for k in [-1.0, 0.0, 1.0] {
                    gfx.ui_circle(c + along * k, 1.5 * scale, accent, z + 0.4);
                }
            }
            _ => {}
        }
    }

    /// What goes over the whole tree at `z`: the item being dragged (and
    /// where it would land), the item carried with the keyboard, the tooltip.
    ///
    /// Their text is laid out once and kept in `kept` while it, its font and
    /// size, and the draw cache's `epoch` (what every node's drawing depends
    /// on) hold; `None`: laid out each time. A tooltip following the
    /// pointer, or an item dragged, places its glyphs again without laying
    /// its text out.
    pub(super) fn draw_widget_overlays(&mut self, ui: &Ui, z: f32, kept: &mut OverlayText, epoch: Option<u64>) {
        let scale = ui.scale();
        let theme = *ui.theme();
        let ring = 2.0 * scale;
        if let Some(item) = ui.widgets.carrying {
            if let Some(r) = ui.rect(item) {
                outline(&mut self.gfx, r, ring, theme.accent, z);
            }
            if let Some(target) = ui.focus().filter(|&t| t != item && ui.is_drop_target(t))
                && let Some(r) = ui.rect(target)
            {
                outline(&mut self.gfx, r, ring, theme.accent, z);
            }
        }
        if let (Some(item), Some(Drag::Item(_, start))) = (ui.widgets.dragging, ui.drag)
            && let Some(n) = ui.node(item)
        {
            let pointer = ui.pointer;
            if let Some(target) = ui.drop_target_at(item, pointer)
                && let Some(r) = ui.rect(target)
            {
                outline(&mut self.gfx, r, ring, theme.accent, z);
            }
            // The item follows the pointer, a little see-through.
            let offset = pointer - start;
            let ghost = Rect {
                min: n.rect.min + offset,
                max: n.rect.max + offset,
            };
            let mut fill = n.paint.background;
            if fill.a < 0.1 {
                fill = theme.control;
            }
            fill.a *= 0.8;
            self.gfx.ui_fill_rounded(ghost, fill, z + 0.1, ui.corner_radius(item));
            if let Some(texture) = n.paint.texture {
                self.gfx.ui_list.push(
                    texture.id,
                    z + 0.2,
                    crate::render::batch::SpriteInstance::new(
                        ghost.min.into(),
                        ghost.size().into(),
                        [0.0, 0.0],
                        0.0,
                        n.paint.color.to_rgba8(),
                    ),
                );
            }
            let params = TextParams {
                font: n.paint.font,
                size: n.paint.font_size * scale,
                color: n.paint.color,
                z: z + 0.3,
            };
            // Its text in the middle, as on the item.
            let drag = kept.slot(epoch, |k| &mut k.drag);
            let size = drag.lay_out(&mut self.gfx, epoch, &n.text, params);
            self.gfx.push_glyphs(&drag.quads, ghost.center() - size * 0.5, params);
        }
        if let Some((id, rect)) = ui.shown_tooltip()
            && let Some(text) = ui.tooltip(id)
        {
            let mut back = theme.panel;
            back.a = back.a.max(0.95);
            let radius = theme.radius.max(0.0) * scale;
            self.gfx.ui_fill_rounded(rect, theme.accent, z + 1.0, radius);
            self.gfx
                .ui_fill_rounded(rect.expand(-scale), back, z + 1.1, (radius - scale).max(0.0));
            let params = TextParams {
                font: theme.font,
                size: theme.text_size * scale,
                color: theme.text,
                z: z + 1.2,
            };
            let tooltip = kept.slot(epoch, |k| &mut k.tooltip);
            tooltip.lay_out(&mut self.gfx, epoch, text, params);
            self.gfx.push_glyphs(
                &tooltip.quads,
                rect.min + vec2(theme.spacing, theme.spacing * 0.5) * scale,
                params,
            );
        }
    }
}

/// The text the overlays drew last, laid out ([`Ctx::draw_widget_overlays`]).
#[derive(Default)]
pub(super) struct OverlayText {
    tooltip: KeptText,
    drag: KeptText,
    /// Text laid out with nothing kept, leaving the others as they were.
    scratch: KeptText,
}

impl OverlayText {
    /// Where to lay text out: `which` while drawings are kept in an
    /// `epoch`, else the scratch slot.
    fn slot(&mut self, epoch: Option<u64>, which: impl FnOnce(&mut Self) -> &mut KeptText) -> &mut KeptText {
        if epoch.is_some() {
            which(self)
        } else {
            &mut self.scratch
        }
    }
}

/// One string laid out, and what it was laid out from.
#[derive(Default)]
struct KeptText {
    /// The draw cache's epoch it was laid out in; 0: never.
    epoch: u64,
    text: String,
    font: crate::text::Font,
    size: f32,
    measured: Vec2,
    quads: Vec<crate::gfx::GlyphQuad>,
}

impl KeptText {
    /// `text` laid out in `params`' font and size, in `quads`: kept from
    /// the last time while nothing it depends on changed in `epoch`
    /// (`None`: laid out again). Returns its size, in window pixels.
    fn lay_out(&mut self, gfx: &mut crate::gfx::Graphics, epoch: Option<u64>, text: &str, params: TextParams) -> Vec2 {
        let (font, size) = (params.font, params.size);
        let kept = epoch == Some(self.epoch) && self.font == font && self.size == size && self.text == text;
        if !kept {
            self.quads.clear();
            self.measured = gfx.lay_out_glyphs(text, font, size, &mut self.quads);
            self.epoch = epoch.unwrap_or(0);
            (self.font, self.size) = (font, size);
            self.text.clear();
            self.text.push_str(text);
        }
        self.measured
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{UiAxis, UiLayout, UiTheme};

    /// WCAG's minimum contrast for body text.
    const TEXT_CONTRAST: f32 = 4.5;

    fn linear(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    /// WCAG relative luminance of an sRGB color.
    fn luminance(c: Color) -> f32 {
        0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
    }

    /// WCAG contrast ratio, 1 to 21.
    fn contrast(a: Color, b: Color) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// `top` over an opaque `under`, as blending draws it.
    fn over(top: Color, under: Color) -> Color {
        let mix = |t: f32, u: f32| t * top.a + u * (1.0 - top.a);
        Color::rgba(mix(top.r, under.r), mix(top.g, under.g), mix(top.b, under.b), 1.0)
    }

    /// What is drawn under node `id`: its ancestors' fills, root first, on
    /// the theme's panel color.
    fn backdrop(ui: &Ui, id: UiId) -> Color {
        let mut chain = Vec::new();
        let mut at = ui.parent(id);
        while let Some(a) = at {
            chain.push(a);
            at = ui.parent(a);
        }
        chain.iter().rev().fold(ui.theme().panel, |under, &a| {
            over(ui.fill_in(a, PaintState::Idle).unwrap(), under)
        })
    }

    /// Every widget with text, a chosen tab, list item and option among
    /// them; `menu` opens a context menu, otherwise the dropdown is open.
    fn sheet(theme: UiTheme, menu: bool) -> Ui {
        let mut ui = Ui::default();
        ui.set_theme(theme);
        let root = ui.root().layout(UiLayout::fill().padding(10.0).gap(6.0)).id();
        ui.push_modeless(root);
        let row = ui.panel(root).layout(UiLayout::row(32.0).gap(6.0)).id();
        let dropdown = ui
            .dropdown(row, ["Low", "High"])
            .layout(UiLayout::fixed(120.0, 32.0))
            .id();
        ui.number_field(row, 5.0).layout(UiLayout::fixed(90.0, 32.0));
        ui.key_binding(row, None).layout(UiLayout::fixed(120.0, 32.0));
        ui.button(row, "OK").layout(UiLayout::fixed(60.0, 32.0));
        ui.text_input(root).text("Name").layout(UiLayout::fixed(160.0, 32.0));
        let music = ui
            .toggle(root, "Music")
            .context_menu(["Mute", "Solo"])
            .layout(UiLayout::fixed(120.0, 28.0))
            .id();
        let group = ui.radio_group(root).id();
        ui.radio(group, "Off").layout(UiLayout::fixed(90.0, 28.0));
        ui.radio(group, "On").layout(UiLayout::fixed(90.0, 28.0));
        ui.set_selected(group, Some(1));
        let tabs = ui.tabs(root).layout(UiLayout::row(60.0)).id();
        for title in ["One", "Two"] {
            let page = ui.tab(tabs, title).id();
            ui.label(page, "Page");
        }
        ui.set_selected(tabs, Some(1));
        let list = ui.list(root, 24.0).layout(UiLayout::fixed(160.0, 72.0)).id();
        ui.set_items(list, ["A", "B", "C"]);
        ui.set_selected(list, Some(1));
        ui.split(root, UiAxis::Horizontal).layout(UiLayout::row(30.0));
        ui.label(root, "A label");
        ui.layout(vec2(640.0, 480.0));
        if menu {
            ui.open_context_menu(music);
        } else {
            ui.set_focus(dropdown);
            ui.frame(
                vec2(640.0, 480.0),
                &crate::ui::UiInput {
                    confirm: true,
                    ..Default::default()
                },
            );
            assert_eq!(ui.popup_owner(), Some(dropdown));
        }
        ui.layout(vec2(640.0, 480.0));
        ui
    }

    #[test]
    fn control_text_has_body_text_contrast_on_its_own_fill_in_every_state() {
        let themes = [
            ("default", UiTheme::default()),
            ("dark", UiTheme::dark()),
            ("light", UiTheme::light()),
        ];
        let states = [
            PaintState::Idle,
            PaintState::Hovered,
            PaintState::Focused,
            PaintState::Pressed,
            PaintState::Disabled,
        ];
        let mut failures = Vec::new();
        let mut checked = 0;
        for (name, theme) in themes {
            for menu in [false, true] {
                let ui = sheet(theme, menu);
                for &id in &ui.visible {
                    let n = ui.node(id).unwrap();
                    let shows_text = !n.text.is_empty() || framed(n.widget);
                    if !shows_text {
                        continue;
                    }
                    let under = backdrop(&ui, id);
                    // Only controls are hovered, pressed and focused.
                    let some = if framed(n.widget) { &states[..] } else { &states[..1] };
                    for &state in some {
                        let fill = over(ui.fill_in(id, state).unwrap(), under);
                        let ratio = contrast(n.paint.color, fill);
                        checked += 1;
                        if ratio < TEXT_CONTRAST {
                            failures.push(format!("{name}: {:?} {:?} {:?}: {ratio:.2}", n.widget, n.text, state));
                        }
                    }
                }
            }
        }
        assert!(checked > 200, "{checked} pairs checked");
        assert!(failures.is_empty(), "below {TEXT_CONTRAST}:1:\n{}", failures.join("\n"));
    }

    /// A state transition mixes two fills that each keep the text's
    /// contrast; the mixes on the way keep it too, and the ends are
    /// exactly the states' fills.
    #[test]
    fn control_text_keeps_its_contrast_through_state_transitions() {
        use crate::ui::{UiInput, UiTransition};
        let themes = [
            ("default", UiTheme::default()),
            ("dark", UiTheme::dark()),
            ("light", UiTheme::light()),
        ];
        let states = [
            PaintState::Idle,
            PaintState::Hovered,
            PaintState::Focused,
            PaintState::Pressed,
            PaintState::Disabled,
        ];
        let mix = |a: Color, b: Color, k: f32| {
            let m = |x: f32, y: f32| x + (y - x) * k;
            Color::rgba(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b), m(a.a, b.a))
        };
        let mut failures = Vec::new();
        let mut checked = 0;
        for (name, theme) in themes {
            let ui = sheet(theme, false);
            for &id in &ui.visible {
                let n = ui.node(id).unwrap();
                if !framed(n.widget) {
                    continue;
                }
                let under = backdrop(&ui, id);
                for &a in &states {
                    for &b in &states {
                        for k in [0.25, 0.5, 0.75] {
                            let fill = mix(ui.fill_in(id, a).unwrap(), ui.fill_in(id, b).unwrap(), k);
                            let ratio = contrast(n.paint.color, over(fill, under));
                            checked += 1;
                            if ratio < TEXT_CONTRAST {
                                failures.push(format!("{name}: {:?} {a:?}->{b:?} at {k}: {ratio:.2}", n.widget));
                            }
                        }
                    }
                }
            }
            // The ends: a transition settles on the state's own fill.
            let mut ui = sheet(
                UiTheme {
                    transition: UiTransition::new(0.2),
                    ..theme
                },
                false,
            );
            ui.frame(
                glam::vec2(640.0, 480.0),
                &UiInput {
                    dt: 1.0,
                    ..Default::default()
                },
            );
            for &id in &ui.visible {
                assert_eq!(ui.shown_fill(id), ui.fill_in(id, ui.paint_state(id)), "{name}");
            }
        }
        assert!(checked > 1000, "{checked} mixes checked");
        assert!(failures.is_empty(), "below {TEXT_CONTRAST}:1:\n{}", failures.join("\n"));
    }

    #[test]
    fn contrast_matches_the_wcag_examples() {
        assert!((contrast(Color::WHITE, Color::BLACK) - 21.0).abs() < 0.01);
        assert!((contrast(Color::WHITE, Color::WHITE) - 1.0).abs() < 1e-6);
        // #767676 on white is the classic 4.54:1.
        let grey = Color::rgb(118.0 / 255.0, 118.0 / 255.0, 118.0 / 255.0);
        assert!((contrast(grey, Color::WHITE) - 4.54).abs() < 0.01);
    }
}
