//! The 3D ray casts, shape casts and overlap queries inside a `ponko_query`
//! callback (a ground check for each entity a loop visits), and refused in
//! `ponko_query_par`.
#![cfg(feature = "physics3d")]
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};

use PonkoBodyKind::*;
use PonkoStatus::*;

fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}

/// What one entity's visit saw: every call's status, and what a few found.
#[derive(Debug, Default)]
struct Seen {
    statuses: Vec<PonkoStatus>,
    ray: Option<PonkoHit3D>,
    all_hits: usize,
    overlapping: usize,
    closest: Option<PonkoPointHit3D>,
    pushes: usize,
}

struct Scene {
    /// The ctx of the system running the loops (a ctx is only valid during its callback).
    ctx: AtomicPtr<PonkoCtx>,
    marker: u32,
    wall: u64,
    /// A lock, because `ponko_query_par` may visit entities on several threads.
    seen: Mutex<Vec<Seen>>,
}

/// Calls every 3D query that reads physics, plus one that changes it (it
/// stays refused inside a loop), once for each entity of the chunk.
unsafe extern "C" fn probe(chunk: *const PonkoChunk, user: *mut c_void) {
    unsafe {
        let scene = &*user.cast::<Scene>();
        let ctx = scene.ctx.load(Ordering::Acquire);
        for _ in 0..(*chunk).count {
            let solid = ponko_query_filter3d();
            let q = ponko_quat_identity();
            let (origin, right) = (v3(0.0, 0.0, 0.0), v3(1.0, 0.0, 0.0));
            let at_wall = v3(10.0, 0.0, 0.0);
            let mut s = Seen::default();
            let (mut found, mut hit) = (false, PonkoHit3D::default());
            s.statuses.push(ponko_ctx_cast_ray3d(
                ctx, origin, right, 50.0, solid, &mut found, &mut hit,
            ));
            s.ray = found.then_some(hit);
            let mut hits = [PonkoHit3D::default(); 4];
            let mut n = 0;
            s.statuses.push(ponko_ctx_cast_ray3d_all(
                ctx,
                origin,
                right,
                50.0,
                solid,
                hits.as_mut_ptr(),
                4,
                &mut n,
            ));
            s.all_hits = n;
            s.statuses.push(ponko_ctx_cast_sphere3d(
                ctx, origin, 0.5, right, 50.0, solid, &mut found, &mut hit,
            ));
            s.statuses.push(ponko_ctx_cast_capsule3d(
                ctx, origin, 1.0, 0.5, right, 50.0, solid, &mut found, &mut hit,
            ));
            s.statuses.push(ponko_ctx_cast_box3d(
                ctx,
                origin,
                v3(1.0, 1.0, 1.0),
                q,
                right,
                50.0,
                solid,
                &mut found,
                &mut hit,
            ));
            let mut near = [PonkoEntity::default(); 4];
            let mut count = |status: PonkoStatus, n: usize| {
                s.statuses.push(status);
                s.overlapping += n;
            };
            let mut n = 0;
            let status = ponko_ctx_overlap_sphere3d(ctx, at_wall, 1.0, solid, near.as_mut_ptr(), 4, &mut n);
            count(status, n);
            let status =
                ponko_ctx_overlap_box3d(ctx, at_wall, v3(1.0, 1.0, 1.0), q, solid, near.as_mut_ptr(), 4, &mut n);
            count(status, n);
            let status = ponko_ctx_overlap_capsule3d(ctx, at_wall, 1.0, 0.5, solid, near.as_mut_ptr(), 4, &mut n);
            count(status, n);
            let status = ponko_ctx_overlap_point3d(ctx, at_wall, solid, near.as_mut_ptr(), 4, &mut n);
            count(status, n);
            let mut point = PonkoPointHit3D::default();
            s.statuses.push(ponko_ctx_closest_point3d(
                ctx, origin, 50.0, solid, &mut found, &mut point,
            ));
            s.closest = found.then_some(point);
            let mut pushes = [PonkoPenetration3D::default(); 4];
            s.statuses.push(ponko_ctx_penetrations3d(
                ctx,
                ponko_collider3d_sphere(1.0),
                v3(8.4, 0.0, 0.0),
                q,
                solid,
                pushes.as_mut_ptr(),
                4,
                &mut n,
            ));
            s.pushes = n;
            // The versioned-filter spelling goes through the same accessor.
            s.statuses.push(ponko_ctx_cast_ray3d_ex(
                ctx,
                origin,
                right,
                50.0,
                std::ptr::null(),
                &mut found,
                &mut hit,
            ));
            // Negative control: what changes physics stays refused in a loop.
            s.statuses
                .push(ponko_ctx_apply_impulse3d(ctx, scene.wall, v3(0.0, 1.0, 0.0)));
            scene.seen.lock().unwrap().push(s);
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
            ponko_component_register(w, c"visited".as_ptr(), 4, 4, &mut scene.marker),
            PONKO_OK
        );
        // A wall whose -X face is at x = 9.
        let at = ponko_transform3d_at(v3(10.0, 0.0, 0.0));
        let fixed = ponko_rigid_body3d(PONKO_BODY_FIXED as u32);
        let shape = ponko_collider3d_box(v3(2.0, 20.0, 20.0));
        let value = |component: u32, data: *const c_void, size: usize| PonkoValue { component, data, size };
        let parts = [
            value(transform, (&raw const at).cast(), size_of::<PonkoTransform3D>()),
            value(body, (&raw const fixed).cast(), size_of::<PonkoRigidBody3D>()),
            value(collider, (&raw const shape).cast(), size_of::<PonkoCollider3D>()),
        ];
        let mut e = 0;
        assert_eq!(ponko_spawn(w, parts.as_ptr(), 3, &mut e), PONKO_OK);
        scene.wall = e;
        // The two entities the loops visit.
        let one = 1u32;
        let marked = [value(scene.marker, (&raw const one).cast(), 4)];
        for _ in 0..2 {
            assert_eq!(ponko_spawn(w, marked.as_ptr(), 1, &mut e), PONKO_OK);
        }
    }
}

/// Runs both loops once, in the second tick (after the wall joined physics).
unsafe extern "C" fn drive(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let scene = &*user.cast::<Scene>();
        if ponko_ctx_sim_tick(ctx) != 1 {
            return;
        }
        scene.ctx.store(ctx, Ordering::Release);
        let world = ponko_ctx_world(ctx);
        assert_eq!(
            ponko_query(world, [scene.marker].as_ptr(), 1, Some(probe), user),
            PONKO_OK
        );
        // Mark the second pass: its results come after the first two entities'.
        scene.seen.lock().unwrap().push(Seen::default());
        assert_eq!(
            ponko_query_par(world, [scene.marker].as_ptr(), 1, Some(probe), user),
            PONKO_OK
        );
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn physics3d_queries_work_inside_a_query_callback_but_not_a_parallel_one() {
    let mut scene = Scene {
        ctx: AtomicPtr::new(null_mut()),
        marker: 0,
        wall: 0,
        seen: Mutex::default(),
    };
    let user = (&raw mut scene).cast::<c_void>();
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"queries-in-loops".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, 0.0, 0.0)), PONKO_OK);
        assert_eq!(
            ponko_game_sim_system(g, c"drive".as_ptr(), 1, Some(drive), user),
            PONKO_OK
        );
        assert_eq!(ponko_game_run_headless(g, 2, Some(setup), user, &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    let seen = scene.seen.into_inner().unwrap();
    assert_eq!(seen.len(), 5, "two entities, a separator, two entities");
    let (serial, parallel) = (&seen[..2], &seen[3..]);

    for s in serial {
        // Twelve reads, then the call that changes physics.
        let (reads, change) = s.statuses.split_at(s.statuses.len() - 1);
        assert_eq!(reads.len(), 12, "{:?}", s.statuses);
        assert_eq!(reads, [PONKO_OK; 12], "every query works inside a loop");
        assert_eq!(change, [PONKO_BUSY], "changing physics stays refused");
        let ray = s.ray.expect("the ray reaches the wall");
        assert_eq!(ponko_entity_bits(ray.entity), scene.wall);
        assert!(close(ray.distance, 9.0) && close(ray.normal.x, -1.0), "{ray:?}");
        assert_eq!(s.all_hits, 1);
        assert_eq!(s.overlapping, 4, "each overlap query finds the wall");
        let closest = s.closest.expect("the wall is the nearest collider");
        assert!(close(closest.distance, 9.0) && !closest.inside, "{closest:?}");
        assert_eq!(s.pushes, 1);
    }
    for s in parallel {
        assert_eq!(s.statuses, [PONKO_BUSY; 13], "refused on worker threads");
        assert!(s.ray.is_none() && s.closest.is_none());
    }
}
