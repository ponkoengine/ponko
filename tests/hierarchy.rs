//! The entity hierarchy: parent and children links kept by the engine,
//! iteration, recursive despawn and transform propagation.

mod common;

use common::TempAsset;
use ponko::attach::Attached;
use ponko::d2::Transform as T2;
use ponko::hierarchy::Children;
use ponko::prelude::*;

fn at(world: &World, e: Entity) -> Vec2 {
    world.get::<T2>(e).unwrap().position
}

fn kids(world: &World, e: Entity) -> Vec<Entity> {
    world.children(e).collect()
}

/// A root at (10, 0) with three children spawned at x = 11, 12, 13.
fn family(world: &mut World) -> (Entity, [Entity; 3]) {
    let root = world.spawn((T2::at(vec2(10.0, 0.0)),));
    let kids = [11.0, 12.0, 13.0].map(|x| world.spawn((T2::at(vec2(x, 0.0)),)));
    for k in kids {
        world.set_parent(k, root).unwrap();
    }
    (root, kids)
}

#[test]
fn set_parent_keeps_the_child_where_it_is_and_it_follows_from_then_on() {
    let mut world = World::new();
    let parent = world.spawn((T2::at(vec2(10.0, 0.0)).rotated(std::f32::consts::FRAC_PI_2),));
    let child = world.spawn((T2::at(vec2(10.0, 3.0)).rotated(0.5),));
    world.set_parent(child, parent).unwrap();
    world.update_hierarchy();
    let t = world.get::<T2>(child).unwrap();
    assert!((t.position - vec2(10.0, 3.0)).length() < 1e-5, "{}", t.position);
    assert!((t.rotation - 0.5).abs() < 1e-6);

    world.get_mut::<T2>(parent).unwrap().position.x = 20.0;
    world.update_hierarchy();
    assert!((at(&world, child) - vec2(20.0, 3.0)).length() < 1e-5);
    assert_eq!(world.parent(child), Some(parent));
}

#[test]
fn children_keep_their_order_and_set_child_index_reorders_them() {
    let mut world = World::new();
    let (root, [a, b, c]) = family(&mut world);
    assert_eq!(kids(&world, root), [a, b, c]);
    assert!(world.set_child_index(c, 0));
    assert_eq!(kids(&world, root), [c, a, b]);
    assert!(world.set_child_index(c, 99), "past the end means last");
    assert_eq!(kids(&world, root), [a, b, c]);
    assert!(!world.set_child_index(root, 0), "a root has no siblings");
}

#[test]
fn reparenting_moves_the_child_between_children_lists() {
    let mut world = World::new();
    let (root, [a, b, c]) = family(&mut world);
    world.set_parent(c, a).unwrap();
    assert_eq!(kids(&world, root), [a, b]);
    assert_eq!(kids(&world, a), [c]);
    assert_eq!(
        world.get::<Children>(root).unwrap().len(),
        2,
        "no stale entry left behind"
    );
}

#[test]
fn remove_parent_makes_a_root_where_it_is() {
    let mut world = World::new();
    let (root, [a, b, _]) = family(&mut world);
    assert!(world.remove_parent(b));
    assert!(!world.remove_parent(b));
    assert_eq!(world.parent(b), None);
    assert!(!kids(&world, root).contains(&b));
    world.get_mut::<T2>(root).unwrap().position.y = 7.0;
    world.update_hierarchy();
    assert_eq!(at(&world, b), vec2(12.0, 0.0), "b stays put");
    assert_eq!(at(&world, a), vec2(11.0, 7.0), "a still follows");
}

#[test]
fn set_parent_refuses_loops_dead_entities_and_mixed_dimensions() {
    let mut world = World::new();
    let (root, [a, ..]) = family(&mut world);
    let err = world.set_parent(root, a).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Invalid);
    assert!(err.to_string().contains("loop"), "{err}");
    assert_eq!(world.set_parent(a, a).unwrap_err().kind(), ErrorKind::Invalid);
    let gone = world.spawn((T2::default(),));
    world.despawn(gone);
    assert_eq!(world.set_parent(a, gone).unwrap_err().kind(), ErrorKind::NotFound);
    let bare = world.spawn((Marker,));
    assert_eq!(world.set_parent(bare, root).unwrap_err().kind(), ErrorKind::Invalid);
    assert_eq!(world.parent(a), Some(root), "a refused link changes nothing");
}

#[test]
fn descendants_are_depth_first_in_child_order_and_ancestors_nearest_first() {
    let mut world = World::new();
    let (root, [a, b, c]) = family(&mut world);
    let a1 = world.spawn((T2::default(),));
    let a2 = world.spawn((T2::default(),));
    let a1x = world.spawn((T2::default(),));
    world.set_parent(a1, a).unwrap();
    world.set_parent(a2, a).unwrap();
    world.set_parent(a1x, a1).unwrap();
    assert_eq!(world.descendants(root).collect::<Vec<_>>(), [a, a1, a1x, a2, b, c]);
    assert_eq!(world.ancestors(a1x).collect::<Vec<_>>(), [a1, a, root]);
    assert_eq!(world.ancestors(root).count(), 0);
}

#[test]
fn despawn_recursive_takes_the_whole_subtree_and_leaves_the_parents_list() {
    let mut world = World::new();
    let (root, [a, b, c]) = family(&mut world);
    let under = world.spawn((T2::default(),));
    world.set_parent(under, b).unwrap();
    assert_eq!(world.despawn_recursive(b), 2);
    assert!(!world.is_alive(b) && !world.is_alive(under));
    assert_eq!(kids(&world, root), [a, c]);
    assert_eq!(world.get::<Children>(root).unwrap().len(), 2);
    assert_eq!(world.despawn_recursive(b), 0);
    assert_eq!(world.despawn_recursive(root), 3);
    assert!(world.is_empty());
}

#[test]
fn attachments_inserted_directly_join_their_parent_after_the_others_in_entity_order() {
    let mut world = World::new();
    let (root, [a, b, c]) = family(&mut world);
    let late = [world.spawn((T2::default(),)), world.spawn((T2::default(),))];
    // Inserted in reverse: the list still takes them in entity order.
    world.insert(late[1], Attached::to(root).offset(vec2(0.0, 2.0)));
    world.insert(late[0], Attached::to(root).offset(vec2(0.0, 1.0)));
    world.update_hierarchy();
    assert_eq!(kids(&world, root), [a, b, c, late[0], late[1]]);
    assert_eq!(at(&world, late[1]), vec2(10.0, 2.0), "placed at once");
}

#[test]
fn a_child_linked_elsewhere_by_insert_leaves_its_old_parents_children() {
    let mut world = World::new();
    let (root, [a, b, c]) = family(&mut world);
    world.insert(c, Attached::to(a));
    assert_eq!(kids(&world, root), [a, b], "reads skip it at once");
    world.update_hierarchy();
    assert_eq!(kids(&world, a), [c]);
    assert_eq!(
        world.get::<Children>(root).unwrap().len(),
        2,
        "the next update drops it, though nothing moved"
    );
}

#[test]
fn despawned_children_leave_a_still_parents_list_at_the_next_update() {
    let mut world = World::new();
    let root = world.spawn((T2::default(),));
    let all: Vec<Entity> = (0..40)
        .map(|_| world.spawn((T2::default(), Attached::to(root))))
        .collect();
    world.update_hierarchy();
    // One child, then many at once (the big-group path).
    world.despawn(all[3]);
    world.update_hierarchy();
    assert_eq!(world.get::<Children>(root).unwrap().len(), 39);
    for &e in &all[10..30] {
        world.despawn(e);
    }
    world.remove::<Attached>(all[0]);
    world.update_hierarchy();
    let left: Vec<Entity> = all
        .iter()
        .copied()
        .enumerate()
        .filter(|&(i, _)| i != 0 && i != 3 && !(10..30).contains(&i))
        .map(|(_, e)| e)
        .collect();
    assert_eq!(kids(&world, root), left, "the rest keep their order");
    assert_eq!(world.get::<Children>(root).unwrap().len(), left.len());
}

#[test]
fn an_attachment_removed_and_put_back_keeps_its_place() {
    let mut world = World::new();
    let (root, [a, b, c]) = family(&mut world);
    world.update_hierarchy();
    let link = *world.get::<Attached>(a).unwrap();
    world.remove::<Attached>(a);
    world.insert(a, link);
    world.update_hierarchy();
    assert_eq!(kids(&world, root), [a, b, c]);
}

#[test]
fn a_parent_despawned_in_the_middle_of_a_chain_takes_what_hangs_off_it() {
    let mut world = World::new();
    let root = world.spawn((T2::default(),));
    let mid = world.spawn((T2::default(), Attached::to(root)));
    let low = world.spawn((T2::default(), Attached::to(mid)));
    let lowest = world.spawn((T2::default(), Attached::to(low)));
    let other = world.spawn((T2::default(), Attached::to(root)));
    world.update_hierarchy();
    world.despawn(mid);
    world.update_hierarchy();
    assert!(!world.is_alive(low) && !world.is_alive(lowest));
    assert_eq!(kids(&world, root), [other]);
    assert_eq!(world.get::<Children>(root).unwrap().len(), 1);
}

#[test]
fn a_parent_that_loses_its_children_list_gets_it_back() {
    let mut world = World::new();
    let (root, [a, b, c]) = family(&mut world);
    world.update_hierarchy();
    world.remove::<Children>(root);
    world.update_hierarchy();
    assert_eq!(kids(&world, root), [a, b, c]);
    world.get_mut::<T2>(root).unwrap().position.y = 3.0;
    world.update_hierarchy();
    assert_eq!(at(&world, c).y, 3.0, "and its subtree still follows");
}

#[test]
fn iteration_order_is_the_same_for_the_same_edits() {
    let build = || {
        let mut world = World::new();
        let root = world.spawn((T2::default(),));
        let mut all = Vec::new();
        for i in 0..20 {
            let e = world.spawn((T2::default(),));
            all.push(e);
            if i % 3 == 0 {
                world.set_parent(e, root).unwrap();
            }
        }
        for (i, &e) in all.iter().enumerate().rev() {
            if i % 3 != 0 {
                world.insert(e, Attached::to(root));
            }
        }
        world.set_child_index(all[9], 1);
        world.update_hierarchy();
        world.descendants(root).collect::<Vec<_>>()
    };
    let first = build();
    assert_eq!(first.len(), 20);
    assert_eq!(first, build());
}

#[test]
fn a_still_subtree_is_left_alone_and_moves_again_with_its_root() {
    let mut world = World::new();
    let (root, [a, ..]) = family(&mut world);
    world.update_hierarchy();
    // Nothing moved: an update doesn't touch the subtree (this write would
    // be overwritten if it did).
    world.get_mut::<T2>(a).unwrap().position = vec2(-1.0, -1.0);
    world.update_hierarchy();
    assert_eq!(at(&world, a), vec2(-1.0, -1.0));
    world.get_mut::<T2>(root).unwrap().position.y = 1.0;
    world.update_hierarchy();
    assert_eq!(at(&world, a), vec2(11.0, 1.0));
}

#[test]
fn an_edited_offset_moves_its_subtree_only() {
    let mut world = World::new();
    let (root, [a, b, _]) = family(&mut world);
    let under = world.spawn((T2::at(vec2(11.0, 5.0)),));
    world.set_parent(under, a).unwrap();
    world.update_hierarchy();
    world.get_mut::<T2>(b).unwrap().position = vec2(-1.0, -1.0); // a marker
    world.get_mut::<Attached>(a).unwrap().offset = vec2(0.0, 3.0);
    world.update_hierarchy();
    assert_eq!(at(&world, a), vec2(10.0, 3.0));
    assert_eq!(at(&world, under), vec2(10.0, 8.0));
    assert_eq!(at(&world, b), vec2(-1.0, -1.0), "its sibling isn't placed again");
    let _ = root;
}

#[test]
fn chains_of_any_depth_are_placed() {
    let mut world = World::new();
    let root = world.spawn((T2::default(),));
    let mut last = root;
    for _ in 0..10_000 {
        last = world.spawn((T2::default(), Attached::to(last).offset(vec2(1.0, 0.0))));
    }
    world.update_hierarchy();
    assert_eq!(at(&world, last), vec2(10_000.0, 0.0));
    assert_eq!(world.ancestors(last).count(), 10_000);
    world.get_mut::<T2>(root).unwrap().position.y = 2.0;
    world.update_hierarchy();
    assert_eq!(at(&world, last), vec2(10_000.0, 2.0));
}

#[test]
fn links_that_loop_are_despawned_with_what_hangs_off_them() {
    let mut world = World::new();
    let a = world.spawn((T2::default(),));
    let b = world.spawn((T2::default(), Attached::to(a)));
    let c = world.spawn((T2::default(), Attached::to(b)));
    world.insert(a, Attached::to(b));
    let fine = world.spawn((T2::default(),));
    let kept = world.spawn((T2::default(), Attached::to(fine)));
    world.update_hierarchy();
    assert!(!world.is_alive(a) && !world.is_alive(b) && !world.is_alive(c));
    assert!(world.is_alive(fine) && world.is_alive(kept));
}

#[test]
fn a_despawned_parent_takes_its_subtree_at_the_next_update() {
    let mut world = World::new();
    let (root, [a, b, c]) = family(&mut world);
    let under = world.spawn((T2::default(),));
    world.set_parent(under, a).unwrap();
    world.update_hierarchy();
    world.despawn(a);
    world.update_hierarchy();
    assert!(!world.is_alive(under));
    assert_eq!(kids(&world, root), [b, c]);
    assert_eq!(world.get::<Children>(root).unwrap().len(), 2);
    world.despawn(root);
    world.update_hierarchy();
    assert!(!world.is_alive(b) && !world.is_alive(c));
}

#[test]
fn children_linked_since_the_last_update_go_with_a_parent_despawned_before_the_next() {
    let mut world = World::new();
    world.update_hierarchy();
    let (root, kids) = family(&mut world);
    world.despawn(root);
    world.update_hierarchy();
    assert!(kids.iter().all(|&k| !world.is_alive(k)));
}

#[test]
fn a_games_own_hook_on_children_runs_next_to_the_hierarchys() {
    #[derive(Default)]
    struct Orphaned(usize);
    fn count(world: &mut HookWorld, parent: Entity) {
        let n = world.get::<Children>(parent).map_or(0, Children::len);
        world.resource_mut::<Orphaned>().0 += n;
    }
    let mut world = World::new();
    world.insert_resource(Orphaned::default());
    world.on_remove::<Children>(count);
    let (root, [a, b, c]) = family(&mut world);
    world.update_hierarchy(); // the hierarchy adds its own hook
    world.despawn(a);
    world.despawn(root);
    world.update_hierarchy();
    assert!(![b, c].iter().any(|&k| world.is_alive(k)));
    assert_eq!(world.resource::<Orphaned>().0, 3, "the root's list, before it went");
}

#[test]
fn the_game_keeps_children_lists_for_attachments_added_any_way() {
    let mut ids = None;
    let world = Game::new("hierarchy", 1, 1)
        .sim_system(systems::follow_attachments)
        .run_headless(2, |ctx| {
            let root = ctx.world.spawn((T2::at(vec2(1.0, 0.0)),));
            let kid = ctx
                .world
                .spawn((T2::default(), Attached::to(root).offset(vec2(0.0, 1.0))));
            ids = Some((root, kid));
        });
    let (root, kid) = ids.unwrap();
    assert_eq!(kids(&world, root), [kid]);
    assert_eq!(at(&world, kid), vec2(1.0, 1.0));
}

#[test]
fn saved_scenes_keep_the_children_order() {
    let file = TempAsset::new("test-hierarchy-scene.json");
    let mut ids = None;
    Game::new("hierarchy", 1, 1).run_headless(0, |ctx| {
        let (root, [a, b, c]) = family(&mut ctx.world);
        ctx.world.set_child_index(c, 0);
        ids = Some((root, [c, a, b]));
        ctx.assets.save_scene(&ctx.world, file.name()).unwrap();
    });
    let world = Game::new("hierarchy", 1, 1)
        .sim_system(systems::follow_attachments)
        .run_headless(1, |ctx| {
            ctx.load_scene(file.name()).unwrap();
        });
    let (root, order) = ids.unwrap();
    assert_eq!(kids(&world, root), order);
    assert_eq!(at(&world, order[0]), vec2(13.0, 0.0));
}

#[test]
fn prefabs_with_children_spawn_with_their_children_lists() {
    let file = TempAsset::new("test-hierarchy-prefab.json");
    Game::new("hierarchy", 1, 1).run_headless(0, |ctx| {
        let (root, [a, _, c]) = family(&mut ctx.world);
        ctx.world.set_child_index(c, 0);
        ctx.world.set_parent(a, c).unwrap();
        ctx.assets.save_prefab(&ctx.world, root, file.name()).unwrap();
    });
    let mut spawned = Vec::new();
    let world = Game::new("hierarchy", 1, 1)
        .sim_system(systems::follow_attachments)
        .run_headless(1, |ctx| {
            for x in [100.0, 200.0] {
                spawned.push(ctx.spawn_prefab(file.name(), vec2(x, 0.0)).unwrap());
            }
        });
    for (root, x) in spawned.into_iter().zip([100.0, 200.0]) {
        let order: Vec<f32> = world.descendants(root).map(|e| at(&world, e).x - x).collect();
        // c (13) first, with a (11) under it, then b (12).
        assert_eq!(order, [3.0, 1.0, 2.0]);
    }
}

#[cfg(feature = "3d")]
#[test]
fn hierarchies_work_in_3d() {
    use ponko::d3::Transform as T3;
    let mut world = World::new();
    let turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let cart = world.spawn((T3::at(vec3(10.0, 0.0, 0.0)).rotated(turn),));
    let lamp = world.spawn((T3::at(vec3(10.0, 2.0, 0.0)),));
    world.set_parent(lamp, cart).unwrap();
    assert!(world.get::<ponko::attach::Attached3D>(lamp).is_some());
    world.update_hierarchy();
    assert!((world.get::<T3>(lamp).unwrap().position - vec3(10.0, 2.0, 0.0)).length() < 1e-5);
    world.get_mut::<T3>(cart).unwrap().position.x = 0.0;
    world.update_hierarchy();
    assert!((world.get::<T3>(lamp).unwrap().position - vec3(0.0, 2.0, 0.0)).length() < 1e-5);
    assert_eq!(world.descendants(cart).collect::<Vec<_>>(), [lamp]);
}

#[derive(Component)]
struct Marker;

// ANCHOR: hierarchy
#[test]
fn a_body_with_an_arm_and_a_hand() {
    let mut world = World::new();
    let body = world.spawn((T2::at(vec2(10.0, 0.0)),));
    let arm = world.spawn((T2::at(vec2(12.0, 0.0)),));
    let hand = world.spawn((T2::at(vec2(14.0, 0.0)),));
    world.set_parent(arm, body).unwrap(); // keeps it where it is
    world.set_parent(hand, arm).unwrap();

    world.get_mut::<T2>(body).unwrap().position.y = 5.0;
    world.update_hierarchy(); // systems::follow_attachments does this each tick
    assert_eq!(world.get::<T2>(hand).unwrap().position, vec2(14.0, 5.0));
    assert_eq!(world.descendants(body).collect::<Vec<_>>(), [arm, hand]);
    assert_eq!(world.despawn_recursive(body), 3);
}
// ANCHOR_END: hierarchy

#[test]
fn placed_transforms_count_as_changed_for_readers_that_track_them() {
    let mut world = World::new();
    world.track_changes::<T2>();
    let (root, [a, b, c]) = family(&mut world);
    world.update_hierarchy();
    let seen = world.advance_change_tick();
    world.get_mut::<T2>(root).unwrap().position.y = 1.0;
    world.update_hierarchy();
    let mut moved: Vec<Entity> = world
        .query::<Entity>()
        .filter::<Changed<T2>>()
        .since(seen)
        .iter()
        .collect();
    moved.sort_unstable();
    assert_eq!(moved, [root, a, b, c]);
}

#[test]
fn wide_levels_and_sparse_edits_preserve_other_subtrees_and_reused_entities() {
    let mut world = World::new();
    let root = world.spawn((T2::default(),));
    let other = world.spawn((T2::default(),));
    let mut children = Vec::new();
    for _ in 0..4096 {
        children.push(world.spawn((T2::default(), Attached::to(root).offset(vec2(1.0, 0.0)))));
    }
    let untouched = world.spawn((T2::default(), Attached::to(other).offset(vec2(2.0, 0.0))));
    world.update_hierarchy();
    world.get_mut::<T2>(root).unwrap().position.y = 3.0;
    world.update_hierarchy();
    assert!(children.iter().all(|&e| at(&world, e) == vec2(1.0, 3.0)));
    assert_eq!(at(&world, untouched), vec2(2.0, 0.0));
    world.despawn(children[0]);
    let replacement = world.spawn((T2::at(vec2(99.0, 99.0)),));
    world.get_mut::<Attached>(children[1]).unwrap().offset.x = 7.0;
    world.update_hierarchy();
    assert_eq!(at(&world, children[1]), vec2(7.0, 3.0));
    assert_eq!(at(&world, replacement), vec2(99.0, 99.0));
}

#[cfg(feature = "3d")]
#[test]
fn wide_3d_levels_stamp_changes_when_transform_tracking_is_enabled() {
    use ponko::d3::Transform as T3;
    for tracked in [false, true] {
        let mut world = World::new();
        if tracked {
            world.track_changes::<T3>();
        }
        let root = world.spawn((T3::default(),));
        for _ in 0..4096 {
            world.spawn((T3::default(), ponko::attach::Attached3D::to(root)));
        }
        world.update_hierarchy();
        let seen = world.advance_change_tick();
        world.get_mut::<T3>(root).unwrap().position.y = 3.0;
        world.update_hierarchy();
        assert!(world.query::<&T3>().iter().all(|t| t.position.y == 3.0));
        if tracked {
            assert_eq!(
                world.query::<Entity>().filter::<Changed<T3>>().since(seen).count(),
                4097
            );
        }
    }
}

#[test]
fn world_saves_restore_order_and_restart_propagation() {
    fn reflected() -> World {
        let mut world = World::new();
        world.reflect::<T2>();
        world.reflect::<Attached>();
        world.reflect::<Children>();
        world
    }
    let mut world = reflected();
    let (root, [a, b, c]) = family(&mut world);
    world.set_child_index(c, 0);
    world.update_hierarchy();
    let save = world.save();
    let mut loaded = reflected();
    loaded.load(&save).unwrap();
    loaded.get_mut::<T2>(root).unwrap().position.y = 9.0;
    loaded.update_hierarchy();
    assert_eq!(kids(&loaded, root), [c, a, b]);
    assert!(loaded.children(root).all(|e| at(&loaded, e).y == 9.0));
}

#[test]
fn commands_despawn_the_subtree_as_it_is_when_they_are_applied() {
    let mut world = World::new();
    let (root, [a, b, c]) = family(&mut world);
    let grandchild = world.spawn((T2::default(),));
    world.set_parent(grandchild, b).unwrap();
    let elsewhere = world.spawn((T2::default(),));
    let mut commands = world.commands();
    commands.despawn_recursive(root);
    // The tree changes between recording and applying: one child joins,
    // one moves to another parent, one becomes a root.
    let joined = world.spawn((T2::default(),));
    world.set_parent(joined, root).unwrap();
    world.set_parent(a, elsewhere).unwrap();
    world.remove_parent(c);
    world.apply(&mut commands);
    for gone in [root, b, grandchild, joined] {
        assert!(!world.is_alive(gone));
    }
    for kept in [a, c, elsewhere] {
        assert!(world.is_alive(kept));
    }
    assert_eq!(kids(&world, elsewhere), [a]);
    world.update_hierarchy();
    assert!(world.is_alive(a) && world.is_alive(c));
}

#[test]
fn commands_see_children_linked_by_earlier_commands_of_the_same_buffer() {
    let mut world = World::new();
    let root = world.spawn((T2::default(),));
    let mut commands = world.commands();
    // Linked through the children list by an earlier command...
    let listed = commands.spawn((T2::default(),));
    commands.add(move |w| w.set_parent(listed, root).unwrap());
    // ...and by an attachment, which joins the list at the next update.
    let attached = commands.spawn((T2::default(), Attached::to(root)));
    commands.despawn_recursive(root);
    world.apply(&mut commands);
    assert!(!world.is_alive(root) && !world.is_alive(listed));
    world.update_hierarchy();
    assert!(!world.is_alive(attached), "its parent is gone");
}

#[test]
fn a_recursive_despawn_of_an_entity_dead_by_then_does_nothing() {
    let mut world = World::new();
    let (root, kids_before) = family(&mut world);
    let mut commands = world.commands();
    commands.despawn(root);
    commands.despawn_recursive(root);
    world.apply(&mut commands);
    // The plain despawn ran first: the children are left for the update.
    assert!(kids_before.iter().all(|&k| world.is_alive(k)));
    world.update_hierarchy();
    assert!(kids_before.iter().all(|&k| !world.is_alive(k)));
    let mut commands = world.commands();
    commands.despawn_recursive(root);
    world.apply(&mut commands);
    assert!(world.is_empty());
}

#[derive(Component)]
struct Doomed;

/// Three trees of a root and two children; the roots of `doomed` carry
/// [`Doomed`].
fn forest(world: &mut World, doomed: &[bool]) -> Vec<(Entity, [Entity; 2])> {
    doomed
        .iter()
        .map(|&d| {
            let root = world.spawn((T2::default(),));
            if d {
                world.insert(root, Doomed);
            }
            let kids = [0, 1].map(|_| world.spawn((T2::default(),)));
            for k in kids {
                world.set_parent(k, root).unwrap();
            }
            (root, kids)
        })
        .collect()
}

#[test]
fn view_systems_despawn_subtrees_through_their_commands() {
    let doomed = [true, false, true];
    let mut world = Game::new("tree", 1, 1)
        .add(
            Phase::AfterPhysics,
            (|sys: &mut ponko::system_view::SystemView| {
                let mut found = Vec::new();
                sys.world.query::<(Entity, &Doomed)>().for_each(|(e, _)| found.push(e));
                for e in found {
                    sys.commands.despawn_recursive(e);
                }
            })
            .access(ponko::access::SystemAccess::new().read::<Doomed>().commands()),
        )
        .run_headless(1, |ctx| {
            forest(&mut ctx.world, &doomed);
        });
    // One tree of three is left.
    assert_eq!(world.len(), 3);
    assert_eq!(world.query::<&Doomed>().count(), 0);
}

#[test]
fn a_parallel_query_despawns_subtrees_through_shared_commands() {
    use std::sync::Mutex;
    let mut world = World::new();
    let doomed: Vec<bool> = (0..300).map(|i| i % 3 == 0).collect();
    let trees = forest(&mut world, &doomed);
    let commands = Mutex::new(world.commands());
    world.query::<(Entity, &Doomed)>().par_for_each(|(e, _)| {
        commands.lock().unwrap().despawn_recursive(e);
    });
    world.apply(&mut commands.into_inner().unwrap());
    for ((root, kids), d) in trees.into_iter().zip(doomed) {
        for e in [root, kids[0], kids[1]] {
            assert_eq!(world.is_alive(e), !d);
        }
    }
}

/// A forest of 5000 roots, one child each and 3000 grandchildren, of every
/// kind a link can be: 2D, 3D, both, a link without a transform, a child
/// placed in one dimension with a grandchild in the other. Returns every
/// entity.
fn mixed_forest(world: &mut World) -> Vec<Entity> {
    let mut all = Vec::new();
    for i in 0..5000 {
        let x = i as f32;
        #[cfg(feature = "3d")]
        let root = world.spawn((T2::at(vec2(x, 0.0)), ponko::d3::Transform::at(vec3(x, 0.0, 0.0))));
        #[cfg(not(feature = "3d"))]
        let root = world.spawn((T2::at(vec2(x, 0.0)),));
        let link = Attached::to(root).offset(vec2(0.0, 1.0)).rotated(0.5);
        let child = match i % 5 {
            1 => world.spawn((link,)),
            #[cfg(feature = "3d")]
            2 => world.spawn((T2::default(), ponko::d3::Transform::default(), link)),
            #[cfg(feature = "3d")]
            3 => world.spawn((ponko::d3::Transform::default(), link3d(root))),
            #[cfg(feature = "3d")]
            4 => world.spawn((T2::default(), ponko::d3::Transform::default(), link, link3d(root))),
            _ => world.spawn((T2::default(), link)),
        };
        all.extend([root, child]);
        if i % 5 == 0 {
            all.push(world.spawn((T2::default(), Attached::to(child).offset(vec2(2.0, 0.0)))));
        }
        #[cfg(feature = "3d")]
        if i % 5 == 2 || i % 5 == 4 {
            all.push(world.spawn((ponko::d3::Transform::default(), link3d(child))));
        }
    }
    all
}

#[cfg(feature = "3d")]
fn link3d(to: Entity) -> ponko::attach::Attached3D {
    ponko::attach::Attached3D::to(to).offset(ponko::d3::Transform::at(vec3(0.0, 0.0, 1.0)))
}

#[test]
fn big_levels_are_placed_the_same_with_and_without_transform_tracking() {
    // Tracking transforms makes the update look each child up on its own;
    // without it, a big level is one pass over everything attached.
    let mut worlds = [World::new(), World::new()];
    worlds[1].track_changes::<T2>();
    #[cfg(feature = "3d")]
    worlds[1].track_changes::<ponko::d3::Transform>();
    let all: Vec<_> = worlds.iter_mut().map(mixed_forest).collect();
    assert_eq!(all[0], all[1]);
    for step in 0..3 {
        for w in &mut worlds {
            if step > 0 {
                w.query::<&mut T2>()
                    .filter::<Without<Attached>>()
                    .for_each(|t| t.position.y += 1.0);
                #[cfg(feature = "3d")]
                w.query::<&mut ponko::d3::Transform>()
                    .filter::<(Without<Attached>, Without<ponko::attach::Attached3D>)>()
                    .for_each(|t| t.position.y += 1.0);
            }
            w.update_hierarchy();
        }
        for &e in &all[0] {
            assert_eq!(worlds[0].get::<T2>(e), worlds[1].get::<T2>(e), "step {step}");
            #[cfg(feature = "3d")]
            assert_eq!(
                worlds[0].get::<ponko::d3::Transform>(e),
                worlds[1].get::<ponko::d3::Transform>(e),
                "step {step}"
            );
            assert_eq!(kids(&worlds[0], e), kids(&worlds[1], e));
        }
    }
    // The grandchildren follow two levels down.
    let tip = all[0][2];
    assert_eq!(
        at(&worlds[0], tip),
        at(&worlds[0], all[0][1]) + Vec2::from_angle(0.5).rotate(vec2(2.0, 0.0))
    );
}
