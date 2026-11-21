//! glTF scenes in the editor: how a scene's nodes differ from its file
//! (its overrides, see `three/gltf_scene/overrides.rs`), put back with
//! undo as a prefab instance's are.

use ponko_ecs::Entity;

use super::{Editor, Undo};
use crate::ctx::Ctx;
use crate::error::{Error, ErrorKind};
use crate::three::{GltfOverride, GltfOverrideKind};

/// The root of the glTF scene `e` belongs to: `e` itself for a root, the
/// scene that lists it among its nodes for a node.
pub(crate) fn scene_of(ctx: &mut Ctx, e: Entity) -> Option<Entity> {
    use crate::d3::GltfRoot;
    if ctx.world.get::<GltfRoot>(e).is_some() {
        return Some(e);
    }
    ctx.world.get::<crate::d3::GltfNode>(e)?;
    ctx.world
        .query::<(Entity, &GltfRoot)>()
        .iter()
        .find(|(_, r)| r.entities().any(|(_, n)| n == e))
        .map(|(root, _)| root)
}

impl Editor {
    /// Puts back `which` overrides of the glTF scene of `root`, as one undo
    /// step: a component as the file made it (or taken off if the file
    /// doesn't give it), a node attached elsewhere back on the file's
    /// parent, a deleted node (with the deleted nodes below it) spawned
    /// again. Nothing changes if one of them fails.
    pub fn revert_gltf(&mut self, ctx: &mut Ctx, root: Entity, which: &[GltfOverride]) -> Result<(), Error> {
        let mark = self.undo.len();
        // Deleted nodes first: the others may name them (a node put back
        // on a parent that was deleted).
        let (deleted, others): (Vec<&GltfOverride>, Vec<&GltfOverride>) =
            which.iter().partition(|o| o.kind == GltfOverrideKind::Deleted);
        for o in deleted.into_iter().chain(others) {
            if let Err(e) = self.revert_gltf_one(ctx, root, o) {
                while self.undo.len() > mark {
                    self.undo(ctx)?;
                }
                self.redo.clear();
                return Err(e);
            }
        }
        super::settle(&mut ctx.world);
        self.group_since(mark);
        Ok(())
    }

    fn revert_gltf_one(&mut self, ctx: &mut Ctx, root: Entity, o: &GltfOverride) -> Result<(), Error> {
        let Some(name) = o.kind.component() else {
            // Deleted: the scene spawns it again; undo deletes it again.
            let back = ctx.restore_gltf_nodes(root, &o.node)?;
            if !back.is_empty() {
                self.record(Undo::Spawned(back));
            }
            return Ok(());
        };
        let e = o.entity;
        if !ctx.world.is_alive(e) {
            return Err(Error::new(
                ErrorKind::NotFound,
                format!("node \"{}\" of the glTF scene is gone", o.node),
            ));
        }
        match ctx.gltf_file_value(root, &o.node, name) {
            Some(value) => self.set(ctx, e, name, value),
            None if matches!(o.kind, GltfOverrideKind::Added(_) | GltfOverrideKind::Parent(_)) => {
                self.remove(ctx, e, name)
            }
            None => Err(Error::new(
                ErrorKind::NotFound,
                format!("node \"{}\": the file's {name} isn't known", o.node),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d3::{GltfRoot, Transform as T3};
    use crate::reflect::WorldReflect;
    use glam::{Vec2, vec3};
    use ponko_ecs::Component;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    #[derive(Serialize, Deserialize, Default, Clone, Copy, PartialEq, Debug, Component)]
    struct Hp(f32);

    /// A `.gltf` asset written for one test.
    struct Temp {
        name: String,
        file: std::path::PathBuf,
    }

    impl Temp {
        fn new(what: &str, text: &str) -> Self {
            let name = format!("ponko-test-editor-gltf-{what}-{}.gltf", std::process::id());
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets")
                .join(&name);
            std::fs::write(&file, text).unwrap();
            Self { name, file }
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.file);
        }
    }

    /// A crate, and a shed with a wall and, on the wall, a lamp.
    fn yard(wall_x: f32) -> String {
        crate::three::cube_gltf(&json!([
            { "name": "Crate", "mesh": 0, "extras": { "ponko:Hp": 3.0 } },
            { "name": "Shed", "translation": [0.0, 0.0, -4.0], "children": [2] },
            { "name": "Wall", "mesh": 0, "translation": [wall_x, 0.0, 0.0], "children": [3] },
            { "name": "Lamp", "mesh": 0, "translation": [0.0, 2.0, 0.0] },
        ]))
    }

    fn ctx() -> Ctx {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(640.0, 360.0));
        ctx.set_headless(true);
        ctx.world.reflect::<Hp>();
        ctx
    }

    fn node(ctx: &Ctx, root: Entity, path: &str) -> Entity {
        ctx.world.get::<GltfRoot>(root).unwrap().entity(path).unwrap()
    }

    fn at(ctx: &Ctx, e: Entity) -> glam::Vec3 {
        ctx.world.get::<T3>(e).unwrap().position
    }

    fn parent(ctx: &Ctx, e: Entity) -> Option<Entity> {
        ctx.world.get::<crate::attach::Attached3D>(e).map(|a| a.to)
    }

    #[test]
    fn a_deleted_node_stays_deleted_through_a_reload_and_undo_brings_it_back_whole() {
        let file = Temp::new("delete", &yard(1.0));
        let mut ctx = ctx();
        let root = ctx.spawn_prefab(&file.name, T3::default()).unwrap();
        let wall = node(&ctx, root, "Wall");
        ctx.edit(|ed, ctx| ed.delete(ctx, wall)).unwrap();
        let lamp = node(&ctx, root, "Lamp");
        assert!(
            !ctx.world.is_alive(lamp),
            "the lamp hangs off the wall: deleted with it"
        );

        ctx.reload_gltf(&file.name).unwrap();
        assert!(!ctx.world.is_alive(node(&ctx, root, "Wall")), "not spawned again");
        assert!(!ctx.world.is_alive(node(&ctx, root, "Lamp")));
        let kinds: Vec<(String, GltfOverrideKind)> =
            ctx.gltf_overrides(root).into_iter().map(|o| (o.node, o.kind)).collect();
        assert_eq!(
            kinds,
            [
                ("Shed/Wall".into(), GltfOverrideKind::Deleted),
                ("Shed/Wall/Lamp".into(), GltfOverrideKind::Deleted)
            ]
        );

        ctx.edit(|ed, ctx| ed.undo(ctx)).unwrap();
        let (wall, lamp) = (node(&ctx, root, "Wall"), node(&ctx, root, "Lamp"));
        assert!(ctx.world.is_alive(wall) && ctx.world.is_alive(lamp), "back");
        assert!(
            ctx.world.get::<crate::render::Model>(wall).is_some(),
            "with what saves leave out"
        );
        assert!(ctx.world.get::<crate::reflect::NotSaved>(wall).is_some());
        assert_eq!(parent(&ctx, lamp), Some(wall));
        assert!(
            (at(&ctx, lamp) - vec3(1.0, 2.0, -4.0)).length() < 1e-4,
            "{}",
            at(&ctx, lamp)
        );
        assert!(ctx.gltf_overrides(root).is_empty(), "as the file has it");
        ctx.record_gltf_overrides(None);
        ctx.reload_gltf(&file.name).unwrap();
        assert_eq!(node(&ctx, root, "Wall"), wall, "a reload keeps it");

        ctx.edit(|ed, ctx| ed.redo(ctx)).unwrap();
        assert!(!ctx.world.is_alive(node(&ctx, root, "Wall")), "redo deletes it again");
    }

    #[test]
    fn a_node_attached_to_another_parent_stays_there_through_a_reload_and_a_save() {
        let file = Temp::new("parent", &yard(1.0));
        let mut ctx = ctx();
        let root = ctx.spawn_prefab(&file.name, T3::default()).unwrap();
        let (crate_e, lamp) = (node(&ctx, root, "Crate"), node(&ctx, root, "Lamp"));
        ctx.edit(|ed, ctx| ed.set_parent(ctx, lamp, Some(crate_e))).unwrap();
        // A saved entity of the world takes the wall.
        let post = ctx.world.spawn((T3::at(vec3(5.0, 0.0, 0.0)),));
        let wall = node(&ctx, root, "Wall");
        ctx.edit(|ed, ctx| ed.set_parent(ctx, wall, Some(post))).unwrap();
        ctx.edit(|ed, ctx| ed.set_parent(ctx, crate_e, None)).unwrap();

        // The file moves the wall: it stays on the post, where it was put.
        std::fs::write(&file.file, yard(2.0)).unwrap();
        ctx.reload_gltf(&file.name).unwrap();
        assert_eq!(parent(&ctx, lamp), Some(crate_e));
        assert!(
            (at(&ctx, lamp) - vec3(1.0, 2.0, -4.0)).length() < 1e-4,
            "{}",
            at(&ctx, lamp)
        );
        assert_eq!(parent(&ctx, wall), Some(post));
        assert!(
            (at(&ctx, wall) - vec3(1.0, 0.0, -4.0)).length() < 1e-4,
            "{}",
            at(&ctx, wall)
        );
        assert_eq!(parent(&ctx, crate_e), None, "taken off its parent");

        // Saved, loaded by a game and spawned again.
        ctx.record_gltf_overrides(None);
        let save = ctx.world.save();
        let mut game = self::ctx();
        game.replace_world(&save).unwrap();
        let root = game.spawn_prefab(&file.name, T3::default()).unwrap();
        let (crate_e, lamp, wall) = (
            node(&game, root, "Crate"),
            node(&game, root, "Lamp"),
            node(&game, root, "Wall"),
        );
        assert_eq!(parent(&game, lamp), Some(crate_e));
        assert_eq!(parent(&game, crate_e), None);
        let post = parent(&game, wall).expect("on the post");
        assert_eq!(at(&game, post), vec3(5.0, 0.0, 0.0), "the saved entity");
        assert!(
            (at(&game, wall) - vec3(1.0, 0.0, -4.0)).length() < 1e-4,
            "{}",
            at(&game, wall)
        );
    }

    #[test]
    fn revert_puts_each_override_back_as_one_undo_step() {
        let file = Temp::new("revert", &yard(1.0));
        let mut ctx = ctx();
        let root = ctx.spawn_prefab(&file.name, T3::default()).unwrap();
        let (crate_e, lamp, shed) = (
            node(&ctx, root, "Crate"),
            node(&ctx, root, "Lamp"),
            node(&ctx, root, "Shed"),
        );
        ctx.edit(|ed, ctx| ed.set(ctx, crate_e, "Hp", json!(9.0))).unwrap();
        ctx.edit(|ed, ctx| ed.set_parent(ctx, lamp, Some(shed))).unwrap();
        ctx.edit(|ed, ctx| ed.delete(ctx, node(ctx, root, "Wall"))).unwrap();
        let all = ctx.gltf_overrides(root);
        assert_eq!(all.len(), 3, "{all:?}");
        assert!(matches!(all[0].kind, GltfOverrideKind::Changed { ref name, .. } if name == "Hp"));
        assert_eq!(
            (all[1].node.as_str(), &all[1].kind),
            ("Shed/Wall", &GltfOverrideKind::Deleted)
        );
        assert_eq!(all[2].kind, GltfOverrideKind::Parent(shed));

        ctx.edit(|ed, ctx| ed.revert_gltf(ctx, root, &all)).unwrap();
        assert!(ctx.gltf_overrides(root).is_empty(), "{:?}", ctx.gltf_overrides(root));
        assert_eq!(ctx.world.get::<Hp>(crate_e), Some(&Hp(3.0)));
        let wall = node(&ctx, root, "Wall");
        assert!(ctx.world.is_alive(wall));
        assert_eq!(parent(&ctx, lamp), Some(wall), "back on the wall");

        ctx.edit(|ed, ctx| ed.undo(ctx)).unwrap();
        assert_eq!(ctx.gltf_overrides(root).len(), 3, "one undo takes them all back");
        assert_eq!(ctx.world.get::<Hp>(crate_e), Some(&Hp(9.0)));
        assert_eq!(parent(&ctx, lamp), Some(shed));
    }
}
