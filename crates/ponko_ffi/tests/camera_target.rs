//! Camera targets through the C ABI: a rig kept in the world follows the
//! entity marked with `ponko_ctx_set_camera_target`, with no camera code of
//! the game's, in 2D and 3D.
mod common;
use common::last_error;
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Mutex;

use PonkoStatus::*;

/// What the late system saw on the last frame.
#[derive(Clone, Copy, Default)]
struct Seen {
    follow: Option<PonkoFollowCamera2D>,
    orbit: Option<PonkoOrbitCamera>,
    target: Option<u64>,
}

static SEEN: Mutex<Option<Seen>> = Mutex::new(None);
static TESTS: Mutex<()> = Mutex::new(());

fn header<T>(version: u32) -> T {
    // SAFETY: the descriptors are plain C structs; all-zero is a valid start.
    let mut d: T = unsafe { std::mem::zeroed() };
    // SAFETY: every descriptor starts with `uint32_t struct_size, version`.
    unsafe {
        let words = (&raw mut d).cast::<u32>();
        words.write(size_of::<T>() as u32);
        words.add(1).write(version);
    }
    d
}

unsafe extern "C" fn look(ctx: *mut PonkoCtx, _: *mut c_void) {
    let mut seen = Seen::default();
    unsafe {
        let mut follow: PonkoFollowCamera2D = header(PONKO_FOLLOW_CAMERA2D_VERSION);
        let mut kept = false;
        assert_eq!(ponko_ctx_follow_camera2d_rig(ctx, &mut follow, &mut kept), PONKO_OK);
        seen.follow = kept.then_some(follow);
        let mut orbit: PonkoOrbitCamera = header(PONKO_ORBIT_CAMERA_VERSION);
        assert_eq!(ponko_ctx_orbit_camera_rig(ctx, &mut orbit, &mut kept), PONKO_OK);
        seen.orbit = kept.then_some(orbit);
        let (mut e, mut found) = (0, false);
        assert_eq!(ponko_ctx_camera_target(ctx, &mut e, &mut found), PONKO_OK);
        seen.target = found.then_some(e);
    }
    *SEEN.lock().unwrap() = Some(seen);
}

/// Runs `ticks` frames of a headless game whose setup is `setup`, and
/// returns what the late system saw last.
fn frames(ticks: u64, setup: impl FnOnce(*mut PonkoCtx)) -> Seen {
    *SEEN.lock().unwrap() = None;
    unsafe {
        let game = ponko_game_new(c"camera-target".as_ptr(), 640, 360);
        assert_eq!(ponko_game_hot_reload(game, false), PONKO_OK);
        assert_eq!(ponko_game_headless_frames(game, true), PONKO_OK);
        assert_eq!(
            ponko_game_late_system(game, c"look".as_ptr(), Some(look), null_mut()),
            PONKO_OK
        );
        let world = common::run_game(game, ticks, setup);
        ponko_world_free(world);
    }
    SEEN.lock().unwrap().expect("the late system ran")
}

unsafe fn spawn2d(ctx: *mut PonkoCtx, x: f32, y: f32) -> u64 {
    unsafe {
        let world = ponko_ctx_world(ctx);
        let mut id = 0;
        assert_eq!(ponko_transform_component(world, &mut id), PONKO_OK);
        let t = PonkoTransform2D {
            position: PonkoVec2 { x, y },
            ..Default::default()
        };
        let value = PonkoValue {
            component: id,
            data: (&raw const t).cast(),
            size: size_of::<PonkoTransform2D>(),
        };
        let mut e = 0;
        assert_eq!(ponko_spawn(world, &value, 1, &mut e), PONKO_OK, "{}", last_error());
        e
    }
}

#[test]
fn a_kept_2d_rig_follows_the_marked_entity_with_no_camera_code() {
    let _one = TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut hero = 0;
    let seen = frames(3, |ctx| unsafe {
        let mut follow: PonkoFollowCamera2D = header(PONKO_FOLLOW_CAMERA2D_VERSION);
        assert_eq!(ponko_follow_camera2d_defaults(&mut follow), PONKO_OK);
        assert_eq!(ponko_ctx_set_follow_camera2d_rig(ctx, &follow), PONKO_OK);
        hero = spawn2d(ctx, 500.0, 200.0);
        assert_eq!(ponko_ctx_set_camera_target(ctx, hero, true), PONKO_OK);
    });
    assert_eq!(seen.target, Some(hero));
    let follow = seen.follow.expect("the rig is kept");
    assert!(follow.started, "the engine put the camera on the target");
    assert!(
        (follow.center.x - 500.0).abs() < 1e-3 && (follow.center.y - 200.0).abs() < 1e-3,
        "the first frame cuts to the target: {:?}",
        follow.center
    );
}

#[test]
fn rigs_are_left_alone_while_no_entity_is_marked() {
    let _one = TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let seen = frames(3, |ctx| unsafe {
        let mut follow: PonkoFollowCamera2D = header(PONKO_FOLLOW_CAMERA2D_VERSION);
        assert_eq!(ponko_follow_camera2d_defaults(&mut follow), PONKO_OK);
        assert_eq!(ponko_ctx_set_follow_camera2d_rig(ctx, &follow), PONKO_OK);
        let hero = spawn2d(ctx, 500.0, 200.0);
        // Marked, then unmarked: no target.
        assert_eq!(ponko_ctx_set_camera_target(ctx, hero, true), PONKO_OK);
        assert_eq!(ponko_ctx_set_camera_target(ctx, hero, false), PONKO_OK);
    });
    assert_eq!(seen.target, None);
    assert!(!seen.follow.expect("kept").started, "no target: the rig never moved");
}

#[test]
fn a_kept_rig_can_be_taken_away_and_a_dead_entity_is_not_a_target() {
    let _one = TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let seen = frames(1, |ctx| unsafe {
        let mut follow: PonkoFollowCamera2D = header(PONKO_FOLLOW_CAMERA2D_VERSION);
        assert_eq!(ponko_follow_camera2d_defaults(&mut follow), PONKO_OK);
        assert_eq!(ponko_ctx_set_follow_camera2d_rig(ctx, &follow), PONKO_OK);
        assert_eq!(ponko_ctx_set_follow_camera2d_rig(ctx, std::ptr::null()), PONKO_OK);
        let gone = spawn2d(ctx, 0.0, 0.0);
        assert_eq!(ponko_despawn(ponko_ctx_world(ctx), gone), PONKO_OK);
        assert_eq!(ponko_ctx_set_camera_target(ctx, gone, true), PONKO_NOT_FOUND);
        // A header of version 0 is refused.
        let bad: PonkoFollowCamera2D = header(0);
        assert_eq!(ponko_ctx_set_follow_camera2d_rig(ctx, &bad), PONKO_INVALID_ARGUMENT);
    });
    assert!(seen.follow.is_none(), "taken away");
}

#[test]
fn a_kept_orbit_rig_turns_around_the_marked_3d_entity() {
    let _one = TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut target = 0;
    let seen = frames(2, |ctx| unsafe {
        let mut orbit: PonkoOrbitCamera = header(PONKO_ORBIT_CAMERA_VERSION);
        assert_eq!(ponko_orbit_camera_defaults(&mut orbit), PONKO_OK);
        assert_eq!(ponko_ctx_set_orbit_camera_rig(ctx, &orbit), PONKO_OK);
        let world = ponko_ctx_world(ctx);
        let mut id = 0;
        assert_eq!(ponko_transform3d_component(world, &mut id), PONKO_OK);
        let t = ponko_transform3d_at(PonkoVec3 { x: 3.0, y: 0.0, z: 4.0 });
        let value = PonkoValue {
            component: id,
            data: (&raw const t).cast(),
            size: size_of::<PonkoTransform3D>(),
        };
        assert_eq!(ponko_spawn(world, &value, 1, &mut target), PONKO_OK);
        assert_eq!(ponko_ctx_set_camera_target(ctx, target, true), PONKO_OK);
    });
    assert_eq!(seen.target, Some(target));
    let orbit = seen.orbit.expect("kept");
    assert_eq!((orbit.target.x, orbit.target.y, orbit.target.z), (3.0, 0.0, 4.0));
    assert!(orbit.started);
}
