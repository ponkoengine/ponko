//! The editor app's glTF scene panel: for a selected node (or root) of a
//! glTF scene, where the scene differs from its file, with Revert, as the
//! prefab panel lists a prefab instance's overrides (see `editor_gltf.rs`).

use egui::{RichText, Ui};
use ponko_ecs::Entity;

use super::Panels;
use crate::ctx::Ctx;
use crate::three::{GltfOverride, GltfOverrideKind};

/// A scene's overrides, kept until the world changes.
#[derive(Debug, Clone)]
pub(crate) struct GltfView {
    root: Entity,
    /// `Editor::revision` and `Ctx::scene_loads` it was made at.
    at: (u64, u64),
    overrides: Vec<GltfOverride>,
}

impl Panels {
    /// The overrides of `root`'s scene, listed again only after a change.
    fn gltf_view(&mut self, ctx: &Ctx, root: Entity) -> Vec<GltfOverride> {
        let at = (ctx.editor.revision(), ctx.scene_loads);
        if let Some(app) = &self.app
            && let Some(view) = &app.gltf
            && view.root == root
            && view.at == at
        {
            return view.overrides.clone();
        }
        let overrides = ctx.gltf_overrides(root);
        if let Some(app) = &mut self.app {
            app.gltf = Some(GltfView {
                root,
                at,
                overrides: overrides.clone(),
            });
        }
        overrides
    }

    /// The glTF scene section for entity `e`, if it is a scene's root or
    /// one of its nodes.
    pub(super) fn gltf_section(&mut self, ui: &mut Ui, ctx: &mut Ctx, e: Entity) {
        let Some(root) = crate::editor::gltf::scene_of(ctx, e) else {
            return;
        };
        let file = ctx
            .world
            .get::<crate::d3::GltfRoot>(root)
            .map(|r| r.path.clone())
            .unwrap_or_default();
        egui::CollapsingHeader::new("glTF scene")
            .id_salt("editor-gltf")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    match ctx.world.get::<crate::d3::GltfNode>(e) {
                        Some(node) => ui.label(format!("Node {} of", node.path)),
                        None => ui.label("Scene of"),
                    };
                    ui.label(RichText::new(&file).monospace());
                });
                if ctx.editor.playing() {
                    ui.label(RichText::new("Compared with its file once stopped").weak());
                    return;
                }
                let overrides = self.gltf_view(ctx, root);
                if overrides.is_empty() {
                    ui.label(RichText::new("Same as the file").weak());
                    return;
                }
                ui.label("Overrides:");
                let mut revert: Option<Vec<GltfOverride>> = None;
                for o in &overrides {
                    let (what, detail) = match &o.kind {
                        GltfOverrideKind::Changed { name, fields } => (
                            name.clone(),
                            match fields {
                                fields if fields.is_empty() => "changed".to_string(),
                                fields => format!("changed: {}", fields.join(", ")),
                            },
                        ),
                        GltfOverrideKind::Added(name) => (name.clone(), "added".into()),
                        GltfOverrideKind::Removed(name) => (name.clone(), "removed".into()),
                        GltfOverrideKind::Parent(to) => (
                            "parent".into(),
                            format!("attached to {}", super::scene::label(ctx, *to)),
                        ),
                        GltfOverrideKind::Deleted => (String::new(), "deleted".into()),
                    };
                    ui.horizontal(|ui| {
                        let tip = match o.kind {
                            GltfOverrideKind::Deleted => "Spawn it again from the file",
                            _ => "Back to the file's value",
                        };
                        if ui.small_button("Revert").on_hover_text(tip).clicked() {
                            revert = Some(vec![o.clone()]);
                        }
                        let label = if what.is_empty() {
                            o.node.clone()
                        } else {
                            format!("{} › {what}", o.node)
                        };
                        ui.label(label);
                        ui.label(RichText::new(detail).weak());
                    });
                }
                if ui
                    .button("Revert all")
                    .on_hover_text("Make the scene exactly its file again (one undo step)")
                    .clicked()
                {
                    revert = Some(overrides.clone());
                }
                if let Some(which) = revert {
                    let done = ctx
                        .edit(|ed, ctx| ed.revert_gltf(ctx, root, &which))
                        .map_err(String::from);
                    if self.report(done).is_some() {
                        self.status = Some(format!("reverted to {file}"));
                    }
                }
            });
    }
}
