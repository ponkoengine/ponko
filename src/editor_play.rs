//! Keeping edits made while playing. Stop puts the world back as it was
//! at Play; before that, the editor app lists what was changed through
//! the editor while the game ran, and the ones ticked are written again
//! after the world is back, as one undo step.

use ponko_ecs::Entity;
use serde_json::Value;

use super::{Editor, Undo};
use crate::ctx::Ctx;
use crate::reflect::WorldReflect;

/// Something changed through the editor while playing, that Stop can keep.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum PlayEdit {
    /// Component `name` of an entity that was there at Play.
    Component(Entity, String),
    /// Resource `name`.
    Resource(String),
}

impl Editor {
    /// What was changed through the editor since Play, each once, oldest
    /// first: components of entities that were there at Play (and still
    /// are), and resources. Entities spawned or deleted while playing
    /// aren't listed: Stop always puts those back.
    pub fn play_edits(&self, ctx: &Ctx) -> Vec<PlayEdit> {
        let Some(played) = &self.played_from else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for step in &self.undo[played.steps.min(self.undo.len())..] {
            self.collect(&step.undo, &mut out);
        }
        let mut seen = std::collections::HashSet::new();
        out.retain(|edit| {
            let there = match edit {
                PlayEdit::Component(e, _) => played.alive.contains(e) && ctx.world.is_alive(*e),
                PlayEdit::Resource(_) => true,
            };
            there && seen.insert(edit.clone())
        });
        out
    }

    fn collect(&self, step: &Undo, out: &mut Vec<PlayEdit>) {
        match step {
            Undo::Component { e, name, .. } => out.push(PlayEdit::Component(self.resolve(*e), name.clone())),
            Undo::Resource { name, .. } => out.push(PlayEdit::Resource(name.clone())),
            // Groups are undone last part first; listed in the order made.
            Undo::Group(parts) => {
                for part in parts {
                    self.collect(part, out);
                }
            }
            Undo::Spawned(_) | Undo::Despawned(_) => {}
            #[cfg(all(feature = "editor", feature = "3d"))]
            Undo::GltfLink { .. } => {}
            // Files, not the world: Stop leaves them as they are.
            #[cfg(feature = "editor")]
            Undo::Asset { .. } => {}
        }
    }

    /// [`stop`](Self::stop), then writes `keep`'s values as they are now
    /// (the edit, and whatever the game did to it since) back into the
    /// world that was put back, as one undo step. A component removed while
    /// playing is removed again.
    pub fn stop_keeping(&mut self, ctx: &mut Ctx, keep: &[PlayEdit]) -> Result<(), crate::error::Error> {
        let values: Vec<(PlayEdit, Option<Value>)> = keep
            .iter()
            .map(|edit| {
                let now = match edit {
                    PlayEdit::Component(e, name) => ctx.world.get_json(*e, name).ok(),
                    PlayEdit::Resource(name) => ctx.world.resource_json(name).ok(),
                };
                (edit.clone(), now)
            })
            .collect();
        self.stop(ctx)?;
        let mark = self.undo.len();
        for (edit, now) in values {
            match edit {
                PlayEdit::Component(e, name) => {
                    // A glTF node is another entity after Stop.
                    let e = self.resolve(e);
                    if !ctx.world.is_alive(e) {
                        continue;
                    }
                    let then = ctx.world.get_json(e, &name).ok();
                    match now {
                        Some(value) if then.as_ref() != Some(&value) => self.set(ctx, e, &name, value)?,
                        None if then.is_some() => self.remove(ctx, e, &name)?,
                        _ => {}
                    }
                }
                PlayEdit::Resource(name) => {
                    if let Some(value) = now
                        && ctx.world.resource_json(&name).ok().as_ref() != Some(&value)
                    {
                        self.set_resource(ctx, &name, value)?;
                    }
                }
            }
        }
        self.group_since(mark);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d2::Transform as T2;
    use glam::Vec2;
    use ponko_ecs::Component;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    #[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug, Component)]
    #[serde(default)]
    struct Speed(f32);

    #[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug)]
    #[serde(default)]
    struct Gravity(f32);

    /// A paused game with two entities that move, a reflected resource and
    /// a system that changes them each tick.
    fn game() -> (Ctx, Entity, Entity) {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(800.0, 600.0));
        ctx.world.reflect::<Speed>();
        ctx.world.reflect_resource::<Gravity>();
        ctx.world.insert_resource(Gravity(9.8));
        let a = ctx.world.spawn((T2::at(Vec2::new(10.0, 0.0)), Speed(1.0)));
        let b = ctx.world.spawn((T2::at(Vec2::new(-10.0, 0.0)), Speed(2.0)));
        ctx.time.pause();
        ctx.edit(|_, _| ());
        (ctx, a, b)
    }

    /// What the game does each tick while playing.
    fn tick(ctx: &mut Ctx) {
        for (t, s) in ctx.world.query::<(&mut T2, &Speed)>().iter() {
            t.position.x += s.0;
        }
        ctx.world.resource_mut::<Gravity>().0 += 0.5;
    }

    #[test]
    fn stop_restores_the_exact_world_saved_at_play() {
        let (mut ctx, a, b) = game();
        ctx.edit(|ed, ctx| ed.set(ctx, a, "Speed", json!(3.0))).unwrap();
        let before = ctx.world.save();
        let (undo, dirty) = (ctx.editor.undo.len(), ctx.editor.dirty());

        ctx.edit(|ed, ctx| ed.play(ctx));
        for _ in 0..5 {
            tick(&mut ctx);
        }
        ctx.edit(|ed, ctx| ed.set(ctx, b, "Speed", json!(7.0))).unwrap();
        ctx.edit(|ed, ctx| ed.set_resource(ctx, "Gravity", json!(1.0))).unwrap();
        ctx.edit(|ed, ctx| ed.delete(ctx, a)).unwrap();
        ctx.edit(|ed, ctx| ed.spawn_empty(ctx, Vec2::ONE));
        ctx.world.spawn((T2::default(),));
        ctx.edit(|ed, ctx| ed.stop(ctx)).unwrap();

        assert_eq!(ctx.world.save(), before, "the whole world, byte for byte as saved");
        assert_eq!(ctx.editor.undo.len(), undo, "steps taken while playing are gone");
        assert_eq!(ctx.editor.dirty(), dirty);
        assert!(ctx.time.is_paused());
    }

    #[test]
    fn edits_made_while_playing_are_listed_once_and_only_for_entities_from_before_play() {
        let (mut ctx, a, b) = game();
        ctx.edit(|ed, ctx| ed.set(ctx, a, "Speed", json!(3.0))).unwrap();
        ctx.edit(|ed, ctx| ed.play(ctx));
        assert!(
            ctx.editor.play_edits(&ctx).is_empty(),
            "edits before Play aren't listed"
        );
        ctx.edit(|ed, ctx| ed.set(ctx, b, "Speed", json!(5.0))).unwrap();
        ctx.edit(|ed, ctx| ed.set(ctx, b, "Speed", json!(6.0))).unwrap();
        ctx.edit(|ed, ctx| ed.set_resource(ctx, "Gravity", json!(1.0))).unwrap();
        let new = ctx.edit(|ed, ctx| ed.spawn_empty(ctx, Vec2::ONE));
        ctx.edit(|ed, ctx| ed.set(ctx, new, "Speed", json!(1.0))).unwrap();
        assert_eq!(
            ctx.editor.play_edits(&ctx),
            [
                PlayEdit::Component(b, "Speed".into()),
                PlayEdit::Resource("Gravity".into())
            ]
        );
    }

    #[test]
    fn stop_keeping_writes_the_chosen_edits_back_as_one_undo_step() {
        let (mut ctx, a, b) = game();
        ctx.edit(|ed, ctx| ed.play(ctx));
        tick(&mut ctx);
        ctx.edit(|ed, ctx| ed.set(ctx, a, "Speed", json!(4.0))).unwrap();
        ctx.edit(|ed, ctx| ed.set(ctx, b, "Speed", json!(9.0))).unwrap();
        ctx.edit(|ed, ctx| ed.set_resource(ctx, "Gravity", json!(1.0))).unwrap();
        tick(&mut ctx);
        let keep = [
            PlayEdit::Component(a, "Speed".into()),
            PlayEdit::Resource("Gravity".into()),
        ];
        ctx.edit(|ed, ctx| ed.stop_keeping(ctx, &keep)).unwrap();

        assert!(!ctx.editor.playing() && ctx.time.is_paused());
        assert_eq!(ctx.world.get::<Speed>(a), Some(&Speed(4.0)), "kept");
        assert_eq!(ctx.world.get::<Speed>(b), Some(&Speed(2.0)), "not chosen: put back");
        assert_eq!(ctx.world.resource::<Gravity>().0, 1.5, "kept as it was at Stop");
        assert_eq!(
            ctx.world.get::<T2>(a).unwrap().position.x,
            10.0,
            "what the game moved is put back"
        );
        assert!(ctx.editor.dirty());

        ctx.edit(|ed, ctx| ed.undo(ctx)).unwrap();
        assert_eq!(
            ctx.world.get::<Speed>(a),
            Some(&Speed(1.0)),
            "one undo takes all kept edits back"
        );
        assert_eq!(ctx.world.resource::<Gravity>().0, 9.8);
        assert!(!ctx.editor.dirty());
    }

    #[test]
    fn a_component_removed_while_playing_is_removed_again_when_kept() {
        let (mut ctx, a, _) = game();
        ctx.edit(|ed, ctx| ed.play(ctx));
        ctx.edit(|ed, ctx| ed.remove(ctx, a, "Speed")).unwrap();
        let edits = ctx.editor.play_edits(&ctx);
        assert_eq!(edits, [PlayEdit::Component(a, "Speed".into())]);
        ctx.edit(|ed, ctx| ed.stop_keeping(ctx, &edits)).unwrap();
        assert!(ctx.world.get::<Speed>(a).is_none());
    }
}
