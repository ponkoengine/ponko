//! The water kit from C: descriptors and their checks, spawning and
//! despawning water, the CPU wave query, flow maps, contact ripples and
//! buoyant bodies floating on the fixed step.
#![cfg(feature = "physics3d")]
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoBodyKind::*;
use PonkoStatus::*;

#[derive(Default)]
struct State {
    transform: u32,
    body: u32,
    collider: u32,
    pond: u64,
    river: u64,
    sea: u64,
    raft: u64,
    ball: u64,
    submerged: f32,
    ticks: u32,
}

fn value<T>(component: u32, v: &T) -> PonkoValue {
    PonkoValue {
        component,
        data: (v as *const T).cast(),
        size: size_of::<T>(),
    }
}
fn v2(x: f32, y: f32) -> PonkoVec2 {
    PonkoVec2 { x, y }
}
fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}
fn blank() -> PonkoWaterDesc {
    // SAFETY: a plain C struct; zero is a valid bit pattern for every field.
    let mut d: PonkoWaterDesc = unsafe { std::mem::zeroed() };
    (d.struct_size, d.version) = (size_of::<PonkoWaterDesc>() as u32, PONKO_WATER_DESC_VERSION);
    d
}

unsafe fn body(ctx: *mut PonkoCtx, s: &State, at: PonkoVec3, collider: PonkoCollider3D) -> u64 {
    unsafe {
        let w = ponko_ctx_world(ctx);
        let (place, kind) = (ponko_transform3d_at(at), ponko_rigid_body3d(PONKO_BODY_DYNAMIC as u32));
        let values = [
            value(s.transform, &place),
            value(s.body, &kind),
            value(s.collider, &collider),
        ];
        let mut e = 0;
        assert_eq!(ponko_spawn(w, values.as_ptr(), 3, &mut e), PONKO_OK);
        e
    }
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let s = &mut *user.cast::<State>();
        let w = ponko_ctx_world(ctx);
        assert_eq!(ponko_transform3d_component(w, &mut s.transform), PONKO_OK);
        assert_eq!(ponko_rigid_body3d_component(w, &mut s.body), PONKO_OK);
        assert_eq!(ponko_collider3d_component(w, &mut s.collider), PONKO_OK);

        // A still pond (no waves) at 1 m, so the raft's depth is exact.
        let mut pond = blank();
        assert_eq!(
            ponko_water_desc_lake(&mut pond, v2(0.0, 0.0), v2(40.0, 40.0), 1.0),
            PONKO_OK
        );
        pond.wave_count = 0;
        assert_eq!(ponko_ctx_spawn_water(ctx, &pond, &mut s.pond), PONKO_OK);

        // A river whose 2 x 2 flow map runs along +x at its full speed.
        let mut river = blank();
        assert_eq!(
            ponko_water_desc_river(&mut river, v2(100.0, 0.0), v2(10.0, 10.0), 0.0, v2(0.0, 0.0)),
            PONKO_OK
        );
        let texels = [255u8, 128, 0, 255].repeat(4);
        (river.flow_map, river.flow_map_width, river.flow_map_height) = (texels.as_ptr(), 2, 2);
        river.flow_map_max_speed = 2.0;
        assert_eq!(ponko_ctx_spawn_water(ctx, &river, &mut s.river), PONKO_OK);

        let mut sea = blank();
        assert_eq!(ponko_water_desc_sea(&mut sea, -50.0, v2(1.0, 0.0), 1.2), PONKO_OK);
        sea.quality = PonkoWaterQuality::PONKO_WATER_QUALITY_LOW as u32;
        sea.reflection = PonkoWaterReflection::PONKO_WATER_REFLECTION_SKY as u32;
        assert_eq!(ponko_ctx_spawn_water(ctx, &sea, &mut s.sea), PONKO_OK);

        // Half as dense as water: floats half under, its centre at the level.
        let mut crate_ = ponko_collider3d_box(v3(1.0, 1.0, 1.0));
        crate_.density = 500.0;
        s.raft = body(ctx, s, v3(0.0, 2.5, 0.0), crate_);
        assert_eq!(
            ponko_ctx_set_buoyancy_box(ctx, s.raft, v3(1.0, 1.0, 1.0), PONKO_BUOYANCY_DRAG),
            PONKO_OK
        );
        let mut ball = ponko_collider3d_sphere(0.5);
        ball.density = 500.0;
        s.ball = body(ctx, s, v3(5.0, 2.5, 0.0), ball);
        assert_eq!(ponko_ctx_set_buoyancy_sphere(ctx, s.ball, 0.5, 2.0), PONKO_OK);
    }
}

unsafe extern "C" fn watch(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let s = &mut *user.cast::<State>();
        s.ticks += 1;
        assert_eq!(ponko_ctx_buoyancy_submerged(ctx, s.raft, &mut s.submerged), PONKO_OK);
        if s.ticks == 2 {
            assert_eq!(ponko_ctx_water_touch(ctx, s.pond, v2(1.0, 1.0), 0.5, 0.1), PONKO_OK);
            assert_eq!(
                ponko_ctx_water_touch(ctx, s.raft, v2(1.0, 1.0), 0.5, 0.1),
                PONKO_NOT_FOUND,
                "a crate is no water"
            );
        }
    }
}

#[test]
fn c_bodies_float_on_c_water_at_the_depth_their_density_gives() {
    let mut s = State::default();
    let user = (&raw mut s).cast::<c_void>();
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"water".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, -9.81, 0.0)), PONKO_OK);
        assert_eq!(ponko_game_water(g), PONKO_OK);
        assert_eq!(
            ponko_game_sim_system(g, c"watch".as_ptr(), 1, Some(watch), user),
            PONKO_OK
        );
        // 8 s at the default 50 steps a second.
        assert_eq!(ponko_game_run_headless(g, 400, Some(setup), user, &mut world), PONKO_OK);

        let mut t = ponko_transform3d_at(v3(0.0, 0.0, 0.0));
        let size = size_of::<PonkoTransform3D>();
        assert_eq!(
            ponko_get(world, s.raft, s.transform, (&raw mut t).cast(), size),
            PONKO_OK
        );
        assert!((t.position.y - 1.0).abs() < 0.03, "half under: {}", t.position.y);
        assert!((s.submerged - 0.5).abs() < 0.05, "submerged {}", s.submerged);
        assert_eq!(
            ponko_get(world, s.ball, s.transform, (&raw mut t).cast(), size),
            PONKO_OK
        );
        assert!((t.position.y - 1.0).abs() < 0.08, "the ball floats: {}", t.position.y);
        ponko_world_free(world);
    }
}

unsafe extern "C" fn query(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let s = &mut *user.cast::<State>();
        setup(ctx, user);
        let mut sample = PonkoWaterSample::default();
        assert_eq!(
            ponko_ctx_water_sample(ctx, s.pond, v2(3.0, -2.0), 1.0, &mut sample),
            PONKO_OK
        );
        assert_eq!(sample.height, 1.0, "still water stays at its level");
        assert_eq!((sample.normal.y, sample.squeeze), (1.0, 1.0));

        assert_eq!(
            ponko_ctx_water_sample(ctx, s.river, v2(100.0, 0.0), 0.0, &mut sample),
            PONKO_OK
        );
        assert!(
            (sample.velocity.x - 2.0).abs() < 0.02,
            "the flow map: {:?}",
            sample.velocity
        );

        // The sea moves: its height changes over time, within its swell.
        let mut later = PonkoWaterSample::default();
        assert_eq!(
            ponko_ctx_water_sample(ctx, s.sea, v2(3.0, 1.0), 0.0, &mut sample),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_water_sample(ctx, s.sea, v2(3.0, 1.0), 2.0, &mut later),
            PONKO_OK
        );
        assert!(sample.height != later.height);
        assert!((sample.height + 50.0).abs() < 1.2 && (later.height + 50.0).abs() < 1.2);

        assert_eq!(ponko_ctx_remove_buoyancy(ctx, s.ball), PONKO_OK);
        assert_eq!(ponko_ctx_remove_buoyancy(ctx, s.ball), PONKO_NOT_FOUND);
        let mut share = -1.0;
        assert_eq!(ponko_ctx_buoyancy_submerged(ctx, s.ball, &mut share), PONKO_NOT_FOUND);
        assert_eq!(share, -1.0);
        assert_eq!(
            ponko_ctx_set_buoyancy_box(ctx, s.ball, v3(0.0, 1.0, 1.0), 4.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_set_buoyancy_sphere(ctx, s.ball, 0.5, -1.0),
            PONKO_INVALID_ARGUMENT
        );

        assert_eq!(ponko_ctx_despawn_water(ctx, s.river), PONKO_OK);
        assert_eq!(ponko_ctx_despawn_water(ctx, s.river), PONKO_NOT_FOUND);
        assert_eq!(
            ponko_ctx_water_sample(ctx, s.river, v2(0.0, 0.0), 0.0, &mut sample),
            PONKO_NOT_FOUND
        );
    }
}

#[test]
fn c_water_answers_queries_and_despawns() {
    let mut s = State::default();
    let user = (&raw mut s).cast::<c_void>();
    unsafe {
        let g = ponko_game_new(c"water".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, -9.81, 0.0)), PONKO_OK);
        assert_eq!(ponko_game_water(g), PONKO_OK);
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(g, 1, Some(query), user, &mut world), PONKO_OK);
        ponko_world_free(world);
    }
}

unsafe extern "C" fn refuse(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        let mut e = u64::MAX;
        let mut d = blank();
        assert_eq!(ponko_water_desc_sea(&mut d, 0.0, v2(1.0, 0.0), 1.0), PONKO_OK);
        let good = d;

        d.wave_count = PONKO_WATER_MAX_WAVES as u32 + 1;
        assert_eq!(ponko_ctx_spawn_water(ctx, &d, &mut e), PONKO_INVALID_ARGUMENT);
        d = good;
        d.quality = 7;
        assert_eq!(ponko_ctx_spawn_water(ctx, &d, &mut e), PONKO_INVALID_ARGUMENT);
        d = good;
        d.reflection = 3;
        assert_eq!(ponko_ctx_spawn_water(ctx, &d, &mut e), PONKO_INVALID_ARGUMENT);
        d = good;
        d.level = f32::NAN;
        assert_eq!(ponko_ctx_spawn_water(ctx, &d, &mut e), PONKO_INVALID_ARGUMENT);
        d = good;
        d.waves[0].wavelength = 0.0;
        assert_eq!(ponko_ctx_spawn_water(ctx, &d, &mut e), PONKO_INVALID_ARGUMENT);
        d = good;
        let texels = [128u8; 4];
        (d.flow_map, d.flow_map_width, d.flow_map_height, d.flow_map_max_speed) = (texels.as_ptr(), 1, 1, 0.0);
        assert_eq!(
            ponko_ctx_spawn_water(ctx, &d, &mut e),
            PONKO_INVALID_ARGUMENT,
            "no max speed"
        );
        d = good;
        d.version = PONKO_WATER_DESC_VERSION + 1;
        assert_eq!(
            ponko_ctx_spawn_water(ctx, &d, &mut e),
            PONKO_INVALID_ARGUMENT,
            "newer than the library"
        );
        assert_eq!(ponko_ctx_spawn_water(ctx, null(), &mut e), PONKO_INVALID_ARGUMENT);
        assert_eq!(e, u64::MAX, "nothing was spawned");
        assert_eq!(ponko_ctx_spawn_water(ctx, &good, &mut e), PONKO_OK);
        assert_ne!(e, u64::MAX);
    }
}

#[test]
fn bad_water_descriptors_are_refused() {
    unsafe {
        let g = ponko_game_new(c"water".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, 0, Some(refuse), null_mut(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
}

/// What the ripple test saw: the lake, the box and the most contacts that
/// waited on the lake at any step.
#[derive(Default)]
struct Splash {
    state: State,
    lake: u64,
    ripples: bool,
    most_waiting: u32,
}

unsafe extern "C" fn drop_box(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let s = &mut *user.cast::<Splash>();
        let w = ponko_ctx_world(ctx);
        assert_eq!(ponko_transform3d_component(w, &mut s.state.transform), PONKO_OK);
        assert_eq!(ponko_rigid_body3d_component(w, &mut s.state.body), PONKO_OK);
        assert_eq!(ponko_collider3d_component(w, &mut s.state.collider), PONKO_OK);
        let mut lake = blank();
        assert_eq!(
            ponko_water_desc_lake(&mut lake, v2(0.0, 0.0), v2(20.0, 20.0), 0.0),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_spawn_water(ctx, &lake, &mut s.lake), PONKO_OK);
        let mut collider = ponko_collider3d_box(v3(0.8, 0.8, 0.8));
        collider.density = 500.0;
        let falling = body(ctx, &s.state, v3(1.0, 1.5, 0.0), collider);
        // SAFETY: a plain C struct; zero is a valid bit pattern for every field.
        let mut d: PonkoBuoyancyDesc = std::mem::zeroed();
        (d.struct_size, d.version) = (size_of::<PonkoBuoyancyDesc>() as u32, PONKO_BUOYANCY_DESC_VERSION);
        assert_eq!(ponko_buoyancy_desc_box(&mut d, v3(0.8, 0.8, 0.8)), PONKO_OK);
        d.ripples = s.ripples;
        assert_eq!(ponko_ctx_set_buoyancy(ctx, falling, &d), PONKO_OK);
    }
}

unsafe extern "C" fn count_waiting(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let s = &mut *user.cast::<Splash>();
        let mut n = u32::MAX;
        assert_eq!(ponko_ctx_water_waiting_contacts(ctx, s.lake, &mut n), PONKO_OK);
        s.most_waiting = s.most_waiting.max(n);
    }
}

/// Drops a box into a still lake with its ripple switch at `ripples`; returns
/// the most ripples that waited on the lake after any step.
fn splashed(ripples: bool) -> u32 {
    let mut s = Splash {
        ripples,
        ..Splash::default()
    };
    let user = (&raw mut s).cast::<c_void>();
    unsafe {
        let g = ponko_game_new(c"splash".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, -9.81, 0.0)), PONKO_OK);
        assert_eq!(ponko_game_water(g), PONKO_OK);
        assert_eq!(
            ponko_game_sim_system(g, c"count".as_ptr(), 1, Some(count_waiting), user),
            PONKO_OK
        );
        let mut world = null_mut();
        // 2 s at the default 50 steps a second: the box falls in and settles.
        assert_eq!(
            ponko_game_run_headless(g, 100, Some(drop_box), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    s.most_waiting
}

#[test]
fn a_body_falling_in_queues_ripples_only_with_its_ripple_switch_on() {
    assert!(splashed(true) > 0, "the splash queued contact ripples");
    assert_eq!(splashed(false), 0, "a body with the switch off starts none");
}

#[test]
fn waiting_contacts_count_what_touch_queued_and_refuse_what_is_no_water() {
    let mut s = Splash::default();
    let user = (&raw mut s).cast::<c_void>();
    unsafe {
        let g = ponko_game_new(c"touch".as_ptr(), 1, 1);
        assert_eq!(ponko_game_water(g), PONKO_OK);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, 0, Some(drop_box_free), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    unsafe extern "C" fn drop_box_free(ctx: *mut PonkoCtx, user: *mut c_void) {
        unsafe {
            let s = &mut *user.cast::<Splash>();
            let mut lake = blank();
            assert_eq!(
                ponko_water_desc_lake(&mut lake, v2(0.0, 0.0), v2(20.0, 20.0), 0.0),
                PONKO_OK
            );
            assert_eq!(ponko_ctx_spawn_water(ctx, &lake, &mut s.lake), PONKO_OK);
            let mut n = u32::MAX;
            assert_eq!(ponko_ctx_water_waiting_contacts(ctx, s.lake, &mut n), PONKO_OK);
            assert_eq!(n, 0);
            for i in 0..3 {
                assert_eq!(
                    ponko_ctx_water_touch(ctx, s.lake, v2(i as f32, 0.0), 0.5, 0.1),
                    PONKO_OK
                );
            }
            assert_eq!(
                ponko_ctx_water_touch(ctx, s.lake, v2(500.0, 0.0), 0.5, 0.1),
                PONKO_OK,
                "outside the ripple area: dropped"
            );
            assert_eq!(ponko_ctx_water_waiting_contacts(ctx, s.lake, &mut n), PONKO_OK);
            assert_eq!(n, 3);
            assert_eq!(
                ponko_ctx_water_waiting_contacts(ctx, 0xdead_beef, &mut n),
                PONKO_NOT_FOUND
            );
            assert_eq!(
                ponko_ctx_water_waiting_contacts(ctx, s.lake, null_mut()),
                PONKO_INVALID_ARGUMENT
            );
            assert_eq!(
                ponko_ctx_water_waiting_contacts(std::ptr::null_mut(), s.lake, &mut n),
                PONKO_INVALID_ARGUMENT
            );
        }
    }
}
