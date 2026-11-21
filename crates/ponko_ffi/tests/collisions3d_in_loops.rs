//! 3D contacts read inside a C loop: `ponko_ctx_collisions` hands the 3D
//! physics step's started and stopped pairs as one array, inside a
//! `ponko_query` callback, so a loop checks each body against them without
//! a call per pair. Refused on `ponko_query_par` threads.
#![cfg(feature = "physics3d")]
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoBodyKind::*;
use PonkoStatus::*;
use ponko_ffi::*;

fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}

#[derive(Default)]
struct Scene {
    ctx: usize,
    marker: u32,
    floor: u64,
    balls: Vec<u64>,
    /// Ball entities a loop saw land, in visit order.
    landed: Vec<u64>,
    /// This tick's started pairs, read once by the loop's first visit.
    pairs: Option<(*const PonkoCollision, usize)>,
    /// What a second read in the same system found.
    again: Vec<usize>,
    /// Statuses of the reads inside `ponko_query_par`.
    parallel: std::sync::Mutex<Vec<PonkoStatus>>,
}

/// For each ball the loop visits: did it start touching the floor this tick?
unsafe extern "C" fn land(chunk: *const PonkoChunk, user: *mut c_void) {
    unsafe {
        let scene = &mut *user.cast::<Scene>();
        let ctx = scene.ctx as *mut PonkoCtx;
        let entities = (*chunk).entities.cast::<u64>();
        for i in 0..(*chunk).count {
            let e = entities.add(i).read();
            // One read for the whole loop: it hands every pair as one array,
            // valid until the system returns.
            let (data, count) = *scene.pairs.get_or_insert_with(|| {
                let (mut data, mut count) = (null(), 0);
                assert_eq!(ponko_ctx_collisions(ctx, true, &mut data, &mut count), PONKO_OK);
                (data, count)
            });
            let pairs = if count == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(data, count)
            };
            let mine = |c: &&PonkoCollision| {
                let (a, b) = (ponko_entity_bits(c.a), ponko_entity_bits(c.b));
                (a == e && b == scene.floor) || (b == e && a == scene.floor)
            };
            if pairs.iter().any(|c| mine(&c)) {
                scene.landed.push(e);
            }
        }
    }
}

unsafe extern "C" fn read_par(chunk: *const PonkoChunk, user: *mut c_void) {
    unsafe {
        // Shared: this runs on several threads at once.
        let scene = &*user.cast::<Scene>();
        let (mut data, mut count) = (null(), 0);
        for _ in 0..(*chunk).count {
            let s = ponko_ctx_collisions(scene.ctx as *mut PonkoCtx, true, &mut data, &mut count);
            scene.parallel.lock().unwrap().push(s);
        }
    }
}

unsafe extern "C" fn watch(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let scene = &mut *user.cast::<Scene>();
        scene.ctx = ctx as usize;
        let w = ponko_ctx_world(ctx);
        let ids = [scene.marker];
        scene.pairs = None;
        assert_eq!(ponko_query(w, ids.as_ptr(), 1, Some(land), user), PONKO_OK);
        if scene.pairs.is_some_and(|(_, n)| n > 0) {
            let (mut data, mut count) = (null(), 0);
            assert_eq!(ponko_ctx_collisions(ctx, true, &mut data, &mut count), PONKO_OK);
            scene.again.push(count);
        }
        if ponko_ctx_sim_tick(ctx) == 1 {
            assert_eq!(ponko_query_par(w, ids.as_ptr(), 1, Some(read_par), user), PONKO_OK);
        }
    }
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let scene = &mut *user.cast::<Scene>();
        let w = ponko_ctx_world(ctx);
        let (mut transform, mut body, mut collider) = (0, 0, 0);
        assert_eq!(ponko_transform3d_component(w, &mut transform), PONKO_OK);
        assert_eq!(ponko_rigid_body3d_component(w, &mut body), PONKO_OK);
        assert_eq!(ponko_collider3d_component(w, &mut collider), PONKO_OK);
        assert_eq!(
            ponko_component_register(w, c"ball".as_ptr(), 4, 4, &mut scene.marker),
            PONKO_OK
        );
        let value = |component: u32, data: *const c_void, size: usize| PonkoValue { component, data, size };
        // A floor whose top is at y = 0.
        let at = ponko_transform3d_at(v3(0.0, -0.5, 0.0));
        let fixed = ponko_rigid_body3d(PONKO_BODY_FIXED as u32);
        let slab = ponko_collider3d_box(v3(40.0, 1.0, 40.0));
        let parts = [
            value(transform, (&raw const at).cast(), size_of::<PonkoTransform3D>()),
            value(body, (&raw const fixed).cast(), size_of::<PonkoRigidBody3D>()),
            value(collider, (&raw const slab).cast(), size_of::<PonkoCollider3D>()),
        ];
        assert_eq!(ponko_spawn(w, parts.as_ptr(), 3, &mut scene.floor), PONKO_OK);
        // Two balls dropped from different heights, so they land apart.
        let dynamic = ponko_rigid_body3d(PONKO_BODY_DYNAMIC as u32);
        let ball = ponko_collider3d_sphere(0.5);
        let one = 1u32;
        for (x, y) in [(-3.0, 1.0), (3.0, 3.0)] {
            let at = ponko_transform3d_at(v3(x, y, 0.0));
            let parts = [
                value(transform, (&raw const at).cast(), size_of::<PonkoTransform3D>()),
                value(body, (&raw const dynamic).cast(), size_of::<PonkoRigidBody3D>()),
                value(collider, (&raw const ball).cast(), size_of::<PonkoCollider3D>()),
                value(scene.marker, (&raw const one).cast(), 4),
            ];
            let mut e = 0;
            assert_eq!(ponko_spawn(w, parts.as_ptr(), 4, &mut e), PONKO_OK);
            scene.balls.push(e);
        }
    }
}

#[test]
fn three_d_contacts_are_read_inside_a_query_callback_as_one_array() {
    let mut scene = Scene::default();
    let user = (&raw mut scene).cast::<c_void>();
    unsafe {
        let g = ponko_game_new(c"contacts-in-loops".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, -9.81, 0.0)), PONKO_OK);
        assert_eq!(
            ponko_game_sim_system(g, c"watch".as_ptr(), 1, Some(watch), user),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(g, 120, Some(setup), user, &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    let mut first = scene.landed.clone();
    first.dedup();
    first.truncate(2);
    assert_eq!(
        first,
        [scene.balls[0], scene.balls[1]],
        "both balls seen landing, the lower one first"
    );
    assert!(
        !scene.again.is_empty() && scene.again.iter().all(|n| *n == 0),
        "a system reads each pair once"
    );
    let parallel = scene.parallel.into_inner().unwrap();
    assert_eq!(parallel.len(), 2);
    assert!(
        parallel.iter().all(|s| *s == PONKO_BUSY),
        "events are off on query_par threads"
    );
}
