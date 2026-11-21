//! The inspector's **Debug drawing** section: shows and flips what
//! `ctx.debug` draws, as the control endpoint's `debug.draw` does: debug
//! drawing itself, the built-in views, how far the colliders view draws
//! mesh triangles, the toggles the game and its plugins added with
//! [`Game::debug_toggle`](crate::game::Game::debug_toggle), and (with 3D)
//! whether each 3D camera draws the 3D lines and labels: its
//! [`debug_draw`](crate::d3::Camera::debug_draw), Default, On or Off.

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
    #[cfg(feature = "3d")]
    cameras(ui, ctx);
}

/// A row per 3D camera (the window's, each extra view's, each target
/// camera's) with its `debug_draw`: Default, On or Off.
#[cfg(feature = "3d")]
fn cameras(ui: &mut Ui, ctx: &mut Ctx) {
    ui.label(RichText::new("Cameras").strong()).on_hover_text(
        "Whether each 3D camera draws the 3D lines and labels (Camera::debug_draw). \
         Default: the window's cameras do, a target camera while it draws particles",
    );
    let scene = &mut ctx.scene3d;
    debug_draw_row(ui, "Window camera", &mut scene.camera.debug_draw);
    for (i, view) in scene.views.iter_mut().enumerate() {
        debug_draw_row(ui, &format!("View {}", i + 1), &mut view.camera.debug_draw);
    }
    for (i, target) in scene.passes.target_cameras.iter_mut().enumerate() {
        debug_draw_row(ui, &format!("Target camera {}", i + 1), &mut target.camera.debug_draw);
    }
}

/// `name`, then Default (`None`), On and Off: the one `value` is, shown
/// selected; a click sets it.
#[cfg(feature = "3d")]
fn debug_draw_row(ui: &mut Ui, name: &str, value: &mut Option<bool>) {
    ui.horizontal(|ui| {
        ui.label(name);
        for (text, state) in [("Default", None), ("On", Some(true)), ("Off", Some(false))] {
            ui.selectable_value(value, state, text);
        }
    });
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

    /// Clicks `text` on the row labelled `row`.
    #[cfg(feature = "3d")]
    fn click_in_row(egui: &egui::Context, ctx: &mut Ctx, row: &str, text: &str) {
        let labels = frame(egui, ctx, vec![]);
        let (_, name) = labels
            .iter()
            .find(|(t, _)| t == row)
            .unwrap_or_else(|| panic!("no `{row}` in {labels:?}"));
        let on_row: Vec<(String, egui::Rect)> = labels
            .iter()
            .filter(|(_, r)| (r.center().y - name.center().y).abs() < 4.0)
            .cloned()
            .collect();
        click(egui, ctx, &on_row, text);
    }

    #[test]
    #[cfg(feature = "3d")]
    fn each_3d_cameras_debug_draw_is_shown_and_set_as_default_on_or_off() {
        let mut ctx = game();
        // A target's texture handle: nothing is drawn here.
        let target = crate::render::RenderTarget::from_texture(crate::texture::Texture::new(1, 64, 64));
        let mut camera = ctx.scene3d.camera;
        camera.debug_draw = Some(false);
        ctx.scene3d
            .passes
            .target_cameras
            .push(crate::render::TargetCamera::new(camera, target));
        let egui = egui::Context::default();
        let labels = frame(&egui, &mut ctx, vec![]);
        let texts: Vec<&str> = labels.iter().map(|(t, _)| t.as_str()).collect();
        for name in ["Cameras", "Window camera", "Target camera 1"] {
            assert!(texts.contains(&name), "`{name}` is shown: {texts:?}");
        }
        assert_eq!(texts.iter().filter(|t| **t == "Default").count(), 2, "{texts:?}");

        click_in_row(&egui, &mut ctx, "Window camera", "On");
        assert_eq!(ctx.scene3d.camera.debug_draw, Some(true));
        click_in_row(&egui, &mut ctx, "Window camera", "Off");
        assert_eq!(ctx.scene3d.camera.debug_draw, Some(false));
        click_in_row(&egui, &mut ctx, "Window camera", "Default");
        assert_eq!(ctx.scene3d.camera.debug_draw, None);
        // The target camera's row sets its own flag only.
        let target_flag = |ctx: &Ctx| ctx.scene3d.passes.target_cameras[0].camera.debug_draw;
        assert_eq!(target_flag(&ctx), Some(false), "untouched so far");
        click_in_row(&egui, &mut ctx, "Target camera 1", "Default");
        assert_eq!(target_flag(&ctx), None);
        click_in_row(&egui, &mut ctx, "Target camera 1", "On");
        assert_eq!(target_flag(&ctx), Some(true));
        assert_eq!(ctx.scene3d.camera.debug_draw, None, "the window camera's stays");
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
