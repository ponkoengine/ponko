//! The inspector's view of a selected agent's brain: its tree, each node
//! colored by what it did at the last think, utility scores, the action.
use egui::{Color32, RichText, Ui};
use ponko_ecs::{Entity, World};

use super::{Brain, Status};

const RUNNING: Color32 = Color32::from_rgb(240, 200, 80);
const SUCCESS: Color32 = Color32::from_rgb(110, 210, 120);
const FAILURE: Color32 = Color32::from_rgb(230, 100, 90);

/// The text of each row the view shows for `brain`, with its color:
/// what [`show`] draws, and what tests read.
pub(crate) fn rows(brain: &Brain) -> Vec<(String, Option<Color32>)> {
    let mut rows = Vec::new();
    let action = brain.action().map_or("none", |k| k.name());
    rows.push((
        format!(
            "{}: action {action} ({:?}), period {}, {} thinks",
            brain.tree(),
            brain.status(),
            brain.period(),
            brain.thinks()
        ),
        None,
    ));
    if !brain.has_tree() {
        rows.push((format!("tree \"{}\" is not loaded", brain.tree()), Some(FAILURE)));
        return rows;
    }
    for line in brain.trace() {
        let mark = match line.status {
            Some(Status::Running) => "▶",
            Some(Status::Success) => "✔",
            Some(Status::Failure) => "✖",
            None => "·",
        };
        let score = line.score.map(|s| format!("  [{s:.2}]")).unwrap_or_default();
        let color = line.status.map(|s| match s {
            Status::Running => RUNNING,
            Status::Success => SUCCESS,
            Status::Failure => FAILURE,
        });
        rows.push((
            format!("{}{mark} {}{score}", "  ".repeat(line.depth), line.label),
            color,
        ));
    }
    rows
}

/// Shows the brain of `e`, if it has one, under a "Behaviour tree" header.
pub(crate) fn show(ui: &mut Ui, world: &World, e: Entity) {
    let Some(brain) = world.get::<Brain>(e) else {
        return;
    };
    egui::CollapsingHeader::new("Behaviour tree")
        .id_salt(("ponko-ai-brain", e.to_bits()))
        .default_open(true)
        .show(ui, |ui| {
            for (text, color) in rows(brain) {
                let text = RichText::new(text).monospace();
                ui.label(match color {
                    Some(c) => text.color(c),
                    None => text.weak(),
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::ai::{BehaviorTree, Blackboard, Key};

    fn drawn_text(world: &World, e: Entity) -> String {
        let egui = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            ..Default::default()
        };
        // A few frames: the first lays the collapsing header out.
        let mut out = egui.run_ui(input.clone(), |ui| show(ui, world, e));
        for _ in 0..2 {
            out.textures_delta.clear();
            out = egui.run_ui(input.clone(), |ui| show(ui, world, e));
        }
        out.textures_delta.clear();
        let mut text = String::new();
        for shape in &out.shapes {
            if let egui::epaint::Shape::Text(t) = &shape.shape {
                text.push_str(t.galley.text());
                text.push('\n');
            }
        }
        text
    }

    #[test]
    fn the_inspector_view_draws_each_node_with_what_it_did_and_its_score() {
        let tree = Arc::new(
            BehaviorTree::from_json(
                r#"{"root": {"utility": [
                    {"score": [{"key": "hunger", "from": 0, "to": 10}], "do": "eat"},
                    {"score": 0.1, "do": "sleep"}
                ]}}"#,
            )
            .unwrap(),
        );
        let mut world = World::new();
        let mut brain = Brain::new("villager");
        let mut bb = Blackboard::new().with(Key::new("hunger"), 5.0);
        brain.think(&tree, &mut bb, 0, 0.1);
        let e = world.spawn((brain, bb));
        let text = drawn_text(&world, e);
        assert!(text.contains("Behaviour tree"), "{text}");
        assert!(text.contains("villager: action eat (Running)"), "{text}");
        assert!(text.contains("▶ do eat  [0.50]"), "{text}");
        assert!(text.contains("· do sleep  [0.10]"), "{text}");

        // Negative control: an entity without a brain draws nothing.
        let plain = world.spawn((Blackboard::new(),));
        assert_eq!(drawn_text(&world, plain), "");
    }

    #[test]
    fn the_inspector_view_says_when_the_tree_is_not_loaded() {
        let mut world = World::new();
        let e = world.spawn((Brain::new("ai/missing.json"),));
        let rows = rows(world.get::<Brain>(e).unwrap());
        assert_eq!(rows[1].0, "tree \"ai/missing.json\" is not loaded");
    }
}
