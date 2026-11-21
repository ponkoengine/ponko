//! The inspector's **Debug drawing** section: shows and flips what
//! `ctx.debug` draws, as the control endpoint's `debug.draw` does: debug
//! drawing itself, the built-in views, how far the colliders view draws
//! mesh triangles, and the toggles the game and its plugins added with
//! [`Game::debug_toggle`](crate::game::Game::debug_toggle).

use egui::{DragValue, RichText, Ui};

use crate::ctx::Ctx;
use crate::debug::DebugView;

/// Draws the section into `ui` and applies what was flipped in it.
pub(super) fn panel(ui: &mut Ui, ctx: &mut Ctx) {
    let mut on = ctx.debug.enabled();
    if ui
        .checkbox(&mut on, "Draw")
        .on_hover_text("ctx.debug: on in debug builds; off forgets what was drawn")
        .changed()
    {
        ctx.debug.set_enabled(on);
    }
    let stats = ctx.debug.stats();
    ui.label(
        RichText::new(format!(
            "{} 3D lines, {} 2D lines, {} labels",
            stats.lines_3d, stats.lines_2d, stats.labels
        ))
        .weak(),
    );
    ui.label(RichText::new("Views").strong());
    for view in DebugView::ALL {
        let mut showing = ctx.debug.showing(view);
        if ui.checkbox(&mut showing, view.name()).changed() {
            ctx.debug.show(view, showing);
        }
    }
    ui.horizontal(|ui| {
        let mut distance = ctx.debug.collider_distance();
        ui.label("Collider distance")
            .on_hover_text("How far from the 3D camera the colliders view draws mesh and terrain triangles");
        let field = DragValue::new(&mut distance).range(0.0..=100_000.0).speed(1.0);
        if ui.add(field).changed() {
            ctx.debug.set_collider_distance(distance);
        }
    });
    ui.label(RichText::new("Toggles").strong());
    let mut flipped = None;
    let mut any = false;
    for (name, state) in ctx.debug.toggles() {
        any = true;
        let mut state = state;
        if ui.checkbox(&mut state, name).changed() {
            flipped = Some((name.to_owned(), state));
        }
    }
    if !any {
        ui.label(RichText::new("none: Game::debug_toggle adds them").weak());
    }
    if let Some((name, state)) = flipped {
        // A name just listed: always a toggle.
        let _ = ctx.debug.set_toggle(&name, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    /// One frame of `panel` at 400x600 points with these input events;
    /// returns where each checkbox's label was drawn.
    fn frame(egui: &egui::Context, ctx: &mut Ctx, events: Vec<egui::Event>) -> Vec<(String, egui::Rect)> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut out = egui.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| panel(ui, ctx));
        });
        out.textures_delta.clear();
        let mut labels = Vec::new();
        for shape in &out.shapes {
            collect_text(&shape.shape, &mut labels);
        }
        labels
    }

    fn collect_text(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => out.push((t.galley.text().to_owned(), t.visual_bounding_rect())),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect_text(s, out)),
            _ => {}
        }
    }

    /// Clicks the checkbox labelled `name`.
    fn click(egui: &egui::Context, ctx: &mut Ctx, labels: &[(String, egui::Rect)], name: &str) {
        let (_, rect) = labels
            .iter()
            .find(|(text, _)| text == name)
            .unwrap_or_else(|| panic!("no `{name}` in {labels:?}"));
        let at = rect.center();
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(egui, ctx, vec![egui::Event::PointerMoved(at), button(true)]);
        frame(egui, ctx, vec![button(false)]);
    }

    fn game() -> Ctx {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(1280.0, 720.0));
        ctx.debug.set_enabled(true);
        ctx.debug
            .set_toggles(&[("ai.paths".to_owned(), false), ("ai.targets".to_owned(), true)]);
        ctx
    }

    #[test]
    fn the_section_lists_debug_drawing_its_views_and_every_toggle() {
        let mut ctx = game();
        let egui = egui::Context::default();
        let labels = frame(&egui, &mut ctx, vec![]);
        let texts: Vec<&str> = labels.iter().map(|(t, _)| t.as_str()).collect();
        for name in ["Draw", "ai.paths", "ai.targets", "Collider distance"]
            .into_iter()
            .chain(DebugView::ALL.iter().map(|v| v.name()))
        {
            assert!(texts.contains(&name), "`{name}` is shown: {texts:?}");
        }
        // A game without toggles says how to add them.
        let mut bare = Ctx::new(1.0 / 60.0, Vec2::new(1280.0, 720.0));
        let labels = frame(&egui::Context::default(), &mut bare, vec![]);
        assert!(
            labels.iter().any(|(t, _)| t.contains("Game::debug_toggle")),
            "{labels:?}"
        );
    }

    #[test]
    fn the_inspector_panel_has_the_section_open_when_the_game_has_toggles() {
        let mut ctx = game();
        let egui = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 4000.0))),
            ..Default::default()
        };
        let mut panels = super::super::Panels::default();
        let mut out = egui.run_ui(input, |ui| panels.show(ui, &mut ctx));
        out.textures_delta.clear();
        let mut labels = Vec::new();
        for shape in &out.shapes {
            collect_text(&shape.shape, &mut labels);
        }
        let texts: Vec<&str> = labels.iter().map(|(t, _)| t.as_str()).collect();
        assert!(texts.contains(&"Debug drawing"), "{texts:?}");
        assert!(texts.contains(&"ai.paths"), "open, listing the toggles: {texts:?}");
    }

    #[test]
    fn clicking_flips_toggles_views_and_debug_drawing() {
        let mut ctx = game();
        let egui = egui::Context::default();
        let labels = frame(&egui, &mut ctx, vec![]);
        click(&egui, &mut ctx, &labels, "ai.paths");
        assert!(ctx.debug.toggled("ai.paths"), "off, then clicked on");
        click(&egui, &mut ctx, &labels, "ai.targets");
        assert!(!ctx.debug.toggled("ai.targets"), "on, then clicked off");
        // The other toggle is left alone.
        assert!(ctx.debug.toggled("ai.paths"));
        assert!(!ctx.debug.showing(DebugView::Colliders));
        click(&egui, &mut ctx, &labels, DebugView::Colliders.name());
        assert!(ctx.debug.showing(DebugView::Colliders));
        click(&egui, &mut ctx, &labels, "Draw");
        assert!(!ctx.debug.enabled(), "debug drawing clicked off");
        // Toggles keep their state while drawing is off.
        assert!(
            !ctx.debug.toggled("ai.paths"),
            "nothing is toggled while drawing is off"
        );
        assert!(ctx.debug.toggles().any(|(n, on)| n == "ai.paths" && on));
    }
}
