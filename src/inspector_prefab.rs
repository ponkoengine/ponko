//! The editor app's prefab panel: for a selected prefab instance, where it
//! differs from its prefab, with Revert and Apply (see `editor_prefab.rs`);
//! for an open prefab document, Apply to scenes.

use egui::{Color32, RichText, Ui};
use ponko_ecs::Entity;
use serde_json::Value;

use super::Panels;
use super::app::Dialog;
use crate::ctx::Ctx;
use crate::editor::DocKind;
use crate::editor::prefab::{self, Comparison, OverrideKind};

/// An instance's comparison, kept until the world changes.
#[derive(Debug, Clone)]
pub(crate) struct PrefabView {
    root: Entity,
    /// `Editor::revision` and `Ctx::scene_loads` it was made at.
    at: (u64, u64),
    result: Result<Comparison, String>,
}

impl Panels {
    /// The comparison of `root` with its prefab, made again only after a
    /// change (comparing spawns a copy).
    pub(crate) fn prefab_view(&mut self, ctx: &mut Ctx, root: Entity) -> Result<Comparison, String> {
        let at = (ctx.editor.revision(), ctx.scene_loads);
        if let Some(app) = &self.app
            && let Some(view) = &app.prefab
            && view.root == root
            && view.at == at
        {
            return view.result.clone();
        }
        let result = prefab::compare(ctx, root).map_err(String::from);
        if let Some(app) = &mut self.app {
            app.prefab = Some(PrefabView {
                root,
                at: (ctx.editor.revision(), ctx.scene_loads),
                result: result.clone(),
            });
        }
        result
    }

    /// The Prefab section for entity `e`, if it belongs to an instance.
    pub(super) fn prefab_section(&mut self, ui: &mut Ui, ctx: &mut Ctx, e: Entity) {
        let Some(root) = prefab::instance_root(&ctx.world, e) else {
            return;
        };
        let path = prefab::instance_path(&ctx.world, root).unwrap_or_default();
        egui::CollapsingHeader::new("Prefab")
            .id_salt("editor-prefab")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Instance of");
                    ui.label(RichText::new(&path).monospace());
                    if ui.small_button("Open prefab").on_hover_text("Edit the prefab alone").clicked() {
                        if ctx.editor.dirty() {
                            self.set_dialog(Some(Dialog::Open { path: path.clone() }));
                        } else {
                            self.open_file(ctx, &path);
                        }
                    }
                });
                if ctx.editor.playing() {
                    ui.label(RichText::new("Compared with its prefab once stopped").weak());
                    return;
                }
                let cmp = match self.prefab_view(ctx, root) {
                    Ok(cmp) => cmp,
                    Err(e) => {
                        ui.colored_label(Color32::LIGHT_RED, e);
                        return;
                    }
                };
                if cmp.is_clean() {
                    ui.label(RichText::new("Same as the prefab").weak());
                    return;
                }
                ui.label("Overrides:");
                let mut revert = None;
                for o in &cmp.overrides {
                    let what = match o.kind() {
                        OverrideKind::Changed => match o.fields() {
                            fields if fields.is_empty() => "changed".to_string(),
                            fields => format!("changed: {}", fields.join(", ")),
                        },
                        OverrideKind::Added => "added".into(),
                        OverrideKind::Removed => "removed".into(),
                    };
                    ui.horizontal(|ui| {
                        if ui
                            .small_button("Revert")
                            .on_hover_text("Back to the prefab's value")
                            .clicked()
                        {
                            revert = Some((o.member, o.component.clone()));
                        }
                        let who = if o.member == 0 {
                            String::new()
                        } else {
                            format!("{} › ", super::scene::label(ctx, o.entity))
                        };
                        ui.label(format!("{who}{}", o.component))
                            .on_hover_text(override_tip(&o.prefab, &o.instance));
                        ui.label(RichText::new(what).weak());
                    });
                }
                for &a in &cmp.added {
                    ui.label(format!("+ {} (not in the prefab)", super::scene::label(ctx, a)));
                }
                if cmp.missing > 0 {
                    ui.label(format!("{} of the prefab's entities deleted", cmp.missing));
                }
                ui.horizontal(|ui| {
                    if ui
                        .button("Revert all")
                        .on_hover_text("Make it exactly the prefab again (one undo step)")
                        .clicked()
                    {
                        let done = ctx.edit(|ed, ctx| ed.revert_instance(ctx, root)).map_err(String::from);
                        if let Some(new) = self.report(done) {
                            self.select(Some(new), false);
                            self.status = Some(format!("reverted to {path}"));
                        }
                    }
                    if ui
                        .button("Apply to prefab")
                        .on_hover_text(
                            "Write this instance into the prefab file; the other instances here follow, keeping their own overrides",
                        )
                        .clicked()
                    {
                        self.apply_to_prefab(ctx, root, &path);
                    }
                });
                if let Some(which) = revert {
                    let done = ctx.edit(|ed, ctx| ed.revert_overrides(ctx, root, &[which]));
                    self.report(done);
                }
            });
    }

    pub(crate) fn apply_to_prefab(&mut self, ctx: &mut Ctx, root: Entity, path: &str) {
        let done = ctx.edit(|ed, ctx| ed.apply_to_prefab(ctx, root)).map_err(String::from);
        if let Some(others) = self.report(done) {
            self.status = Some(format!("applied to {path}; {others} other instance(s) here updated"));
            self.scene.forget_assets();
            self.prefabs = None;
        }
    }

    /// For an open prefab document: Apply to scenes, once it was saved with
    /// changes.
    pub(super) fn prefab_document(&mut self, ui: &mut Ui, ctx: &mut Ctx) {
        if ctx.editor.doc.kind != DocKind::Prefab {
            return;
        }
        let Some(path) = ctx.editor.doc.path.clone() else {
            return;
        };
        let base = self.app.as_ref().and_then(|a| a.prefab_base.clone());
        let Some((base_path, base)) = base else { return };
        if base_path != path {
            return;
        }
        let now = read_json(ctx, &path);
        let changed = now.as_ref().is_some_and(|now| *now != base);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Prefab").strong());
            let can = changed && !ctx.editor.dirty();
            let tip = if ctx.editor.dirty() {
                "Save the prefab first"
            } else if changed {
                "Update the instances in every scene in assets/ (opened, updated and saved in turn), keeping their overrides"
            } else {
                "Nothing saved to apply yet"
            };
            if ui
                .add_enabled(can, egui::Button::new("Apply to scenes"))
                .on_hover_text(tip)
                .on_disabled_hover_text(tip)
                .clicked()
            {
                self.apply_to_scenes(ctx, &path, &base);
            }
        });
        ui.separator();
    }

    pub(crate) fn apply_to_scenes(&mut self, ctx: &mut Ctx, path: &str, base: &Value) {
        let done = ctx
            .edit(|ed, ctx| ed.apply_prefab_to_scenes(ctx, path, base))
            .map_err(String::from);
        if let Some(scenes) = self.report(done) {
            self.status = Some(match scenes.len() {
                0 => "no scene has an instance of it".into(),
                n => format!("updated {n} scene(s): {}", scenes.join(", ")),
            });
            self.select(None, false);
            let now = read_json(ctx, path);
            if let (Some(app), Some(now)) = (&mut self.app, now) {
                app.prefab_base = Some((path.to_string(), now));
            }
        }
    }

    /// Remembers what an opened prefab file held, for Apply to scenes.
    pub(super) fn remember_prefab_base(&mut self, ctx: &mut Ctx) {
        let doc = &ctx.editor.doc;
        let path = match (&doc.path, doc.kind) {
            (Some(p), DocKind::Prefab) => p.clone(),
            _ => {
                if let Some(app) = &mut self.app {
                    app.prefab_base = None;
                }
                return;
            }
        };
        if self
            .app
            .as_ref()
            .is_some_and(|a| a.prefab_base.as_ref().is_some_and(|(p, _)| *p == path))
        {
            return;
        }
        let base = read_json(ctx, &path);
        if let (Some(app), Some(base)) = (&mut self.app, base) {
            app.prefab_base = Some((path, base));
        }
    }
}

fn read_json(ctx: &Ctx, path: &str) -> Option<Value> {
    let (_, text) = ctx.assets.read_text(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// "prefab: … / here: …" for an override's hover.
fn override_tip(prefab: &Option<Value>, instance: &Option<Value>) -> String {
    let show = |v: &Option<Value>| v.as_ref().map_or("(none)".to_string(), |v| v.to_string());
    format!("prefab: {}\nhere: {}", show(prefab), show(instance))
}
