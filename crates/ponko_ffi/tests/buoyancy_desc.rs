//! `PonkoBuoyancyDesc` from C: custom sample points, the water's density
//! and the ripple switch reach the floating body, and bad descriptors are
//! refused with the reason.
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
    salty: u64,
    pointed: u64,
}

fn v2(x: f32, y: f32) -> PonkoVec2 {
    PonkoVec2 { x, y }
}
fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}
fn blank() -> PonkoBuoyancyDesc {
    // SAFETY: a plain C struct; zero is a valid bit pattern for every field.
    let mut d: PonkoBuoyancyDesc = unsafe { std::mem::zeroed() };
    (d.struct_size, d.version) = (size_of::<PonkoBuoyancyDesc>() as u32, PONKO_BUOYANCY_DESC_VERSION);
    d
}

/// A unit box of density 500 kg/m³ at `at`.
unsafe fn crate_at(ctx: *mut PonkoCtx, s: &State, at: PonkoVec3) -> u64 {
    unsafe {
        let mut collider = ponko_collider3d_box(v3(1.0, 1.0, 1.0));
        collider.density = 500.0;
        let (place, kind) = (ponko_transform3d_at(at), ponko_rigid_body3d(PONKO_BODY_DYNAMIC as u32));
        let values = [
            PonkoValue {
                component: s.transform,
                data: (&raw const place).cast(),
                size: size_of::<PonkoTransform3D>(),
            },
            PonkoValue {
                component: s.body,
                data: (&raw const kind).cast(),
                size: size_of::<PonkoRigidBody3D>(),
            },
            PonkoValue {
                component: s.collider,
                data: (&raw const collider).cast(),
                size: size_of::<PonkoCollider3D>(),
            },
        ];
        let mut e = 0;
        assert_eq!(ponko_spawn(ponko_ctx_world(ctx), values.as_ptr(), 3, &mut e), PONKO_OK);
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

        // A still pond at 1 m, so the depths are exact.
        let mut pond: PonkoWaterDesc = std::mem::zeroed();
        (pond.struct_size, pond.version) = (size_of::<PonkoWaterDesc>() as u32, PONKO_WATER_DESC_VERSION);
        assert_eq!(
            ponko_water_desc_lake(&mut pond, v2(0.0, 0.0), v2(40.0, 40.0), 1.0),
            PONKO_OK
        );
        pond.wave_count = 0;
        assert_eq!(ponko_ctx_spawn_water(ctx, &pond, &mut s.pond), PONKO_OK);

        // Water twice as dense: the crate floats a quarter of its height higher.
        s.salty = crate_at(ctx, s, v3(0.0, 2.5, 0.0));
        let mut salty = blank();
        assert_eq!(ponko_buoyancy_desc_box(&mut salty, v3(1.0, 1.0, 1.0)), PONKO_OK);
        assert_eq!(salty.point_count, 8);
        assert_eq!(salty.water_density, PONKO_FRESH_WATER_DENSITY);
        assert!(salty.ripples, "the presets ripple");
        salty.water_density = 2.0 * PONKO_FRESH_WATER_DENSITY;
        salty.ripples = false;
        assert_eq!(ponko_ctx_set_buoyancy(ctx, s.salty, &salty), PONKO_OK);

        // The same box from points of the caller's own, in fresh water.
        s.pointed = crate_at(ctx, s, v3(5.0, 2.5, 0.0));
        let mut points = Vec::new();
        for y in [-0.25, 0.25] {
            for x in [-0.25, 0.25] {
                for z in [-0.25, 0.25] {
                    points.push(v3(x, y, z));
                }
            }
        }
        let mut pointed = blank();
        assert_eq!(
            ponko_buoyancy_desc_points(&mut pointed, points.as_ptr(), points.len() as u32, 1.0, 0.5),
            PONKO_OK
        );
        assert_eq!(pointed.drag, PONKO_BUOYANCY_DRAG);
        assert_eq!(ponko_ctx_set_buoyancy(ctx, s.pointed, &pointed), PONKO_OK);
    }
}

#[test]
fn density_and_points_from_c_set_the_depth_a_body_floats_at() {
    let mut s = State::default();
    let user = (&raw mut s).cast::<c_void>();
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"buoyancy".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, -9.81, 0.0)), PONKO_OK);
        assert_eq!(ponko_game_water(g), PONKO_OK);
        // 8 s at the default 50 steps a second.
        assert_eq!(ponko_game_run_headless(g, 400, Some(setup), user, &mut world), PONKO_OK);

        let mut t = ponko_transform3d_at(v3(0.0, 0.0, 0.0));
        let size = size_of::<PonkoTransform3D>();
        assert_eq!(
            ponko_get(world, s.salty, s.transform, (&raw mut t).cast(), size),
            PONKO_OK
        );
        // h · (0.5 − ρ / ρw) = 1 · (0.5 − 500 / 2000) above the level.
        assert!(
            (t.position.y - 1.25).abs() < 0.03,
            "dense water floats it higher: {}",
            t.position.y
        );
        assert_eq!(
            ponko_get(world, s.pointed, s.transform, (&raw mut t).cast(), size),
            PONKO_OK
        );
        assert!((t.position.y - 1.0).abs() < 0.03, "half under: {}", t.position.y);
        ponko_world_free(world);
    }
}

unsafe extern "C" fn round_trip(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        setup(ctx, user);
        let s = &*user.cast::<State>();
        let mut read = blank();
        assert_eq!(ponko_ctx_buoyancy(ctx, s.salty, &mut read), PONKO_OK);
        assert_eq!(read.water_density, 2000.0);
        assert!(!read.ripples, "the ripple switch reached the body");
        assert_eq!((read.point_count, read.volume, read.height), (8, 1.0, 0.5));
        assert_eq!(read.points[7], v3(0.25, 0.25, 0.25));

        // Change one setting and give it back.
        read.drag = 1.5;
        assert_eq!(ponko_ctx_set_buoyancy(ctx, s.salty, &read), PONKO_OK);
        let mut again = blank();
        assert_eq!(ponko_ctx_buoyancy(ctx, s.salty, &mut again), PONKO_OK);
        assert_eq!(again, read);

        assert_eq!(ponko_ctx_remove_buoyancy(ctx, s.salty), PONKO_OK);
        assert_eq!(ponko_ctx_buoyancy(ctx, s.salty, &mut read), PONKO_NOT_FOUND);
        assert_eq!(
            ponko_ctx_buoyancy(ctx, s.pond, &mut read),
            PONKO_NOT_FOUND,
            "water doesn't float"
        );
    }
}

#[test]
fn a_bodys_buoyancy_reads_back_as_it_was_set() {
    let mut s = State::default();
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"buoyancy".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, -9.81, 0.0)), PONKO_OK);
        assert_eq!(
            ponko_game_run_headless(g, 0, Some(round_trip), (&raw mut s).cast(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
}

unsafe extern "C" fn refuse(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        setup(ctx, user);
        let s = &*user.cast::<State>();
        let e = s.pointed;
        let mut good = blank();
        assert_eq!(ponko_buoyancy_desc_sphere(&mut good, 0.5), PONKO_OK);
        assert_eq!(good.point_count, 7);
        let refused = |d: PonkoBuoyancyDesc, why: &str| {
            assert_eq!(ponko_ctx_set_buoyancy(ctx, e, &d), PONKO_INVALID_ARGUMENT, "{why}");
        };
        refused(PonkoBuoyancyDesc { point_count: 0, ..good }, "no points");
        refused(
            PonkoBuoyancyDesc {
                point_count: PONKO_BUOYANCY_MAX_POINTS as u32 + 1,
                ..good
            },
            "too many points",
        );
        let mut nan = good;
        nan.points[0].x = f32::NAN;
        refused(nan, "a point that isn't finite");
        refused(
            PonkoBuoyancyDesc {
                water_density: 0.0,
                ..good
            },
            "no density",
        );
        refused(PonkoBuoyancyDesc { volume: -1.0, ..good }, "negative volume");
        refused(
            PonkoBuoyancyDesc {
                height: f32::INFINITY,
                ..good
            },
            "endless column",
        );
        refused(PonkoBuoyancyDesc { drag: -1.0, ..good }, "negative drag");
        refused(PonkoBuoyancyDesc { version: 0, ..good }, "header not set");
        refused(
            PonkoBuoyancyDesc {
                version: PONKO_BUOYANCY_DESC_VERSION + 1,
                ..good
            },
            "a newer header",
        );
        refused(PonkoBuoyancyDesc { struct_size: 8, ..good }, "struct_size too small");
        assert_eq!(ponko_ctx_set_buoyancy(ctx, e, null()), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_set_buoyancy(ctx, u64::MAX, &good),
            PONKO_INVALID_ARGUMENT,
            "no entity"
        );

        let mut d = blank();
        assert_eq!(
            ponko_buoyancy_desc_box(&mut d, v3(0.0, 1.0, 1.0)),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_buoyancy_desc_sphere(&mut d, -1.0), PONKO_INVALID_ARGUMENT);
        let one = [v3(0.0, 0.0, 0.0)];
        assert_eq!(
            ponko_buoyancy_desc_points(&mut d, null(), 1, 1.0, 1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_buoyancy_desc_points(&mut d, one.as_ptr(), 0, 1.0, 1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_buoyancy_desc_points(&mut d, one.as_ptr(), 1, 0.0, 1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_buoyancy_desc_points(&mut d, one.as_ptr(), 1, 1.0, 1.0), PONKO_OK);
        assert_eq!((d.point_count, d.volume, d.height), (1, 1.0, 1.0));
        assert_eq!(
            ponko_buoyancy_desc_box(null_mut(), v3(1.0, 1.0, 1.0)),
            PONKO_INVALID_ARGUMENT
        );
    }
}

#[test]
fn bad_buoyancy_descriptors_are_refused() {
    let mut s = State::default();
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"buoyancy".as_ptr(), 1, 1);
        assert_eq!(ponko_game_physics3d(g, v3(0.0, -9.81, 0.0)), PONKO_OK);
        assert_eq!(
            ponko_game_run_headless(g, 0, Some(refuse), (&raw mut s).cast(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
}
