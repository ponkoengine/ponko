//! Joint-track clips made in code from C (`ponko_ctx_create_clip`): they
//! pose skeletons as loaded clips do, bake into crowds, and the crowd shows
//! in the versioned live counts (`ponko_ctx_live_counts_ex`).
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;
use ponko_ffi::*;

fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}

/// Spawns an entity with a two-joint skeleton: `root`, then `arm` above it.
unsafe fn skeleton(ctx: *mut PonkoCtx) -> u64 {
    unsafe {
        let world = ponko_ctx_world(ctx);
        let mut transform = 0;
        assert_eq!(ponko_transform3d_component(world, &mut transform), PONKO_OK);
        let at = ponko_transform3d_at(v3(0.0, 0.0, 0.0));
        let part = PonkoValue {
            component: transform,
            data: (&raw const at).cast(),
            size: size_of::<PonkoTransform3D>(),
        };
        let mut e = 0;
        assert_eq!(ponko_spawn(world, &part, 1, &mut e), PONKO_OK);
        let joints = [
            PonkoJoint {
                name: c"root".as_ptr(),
                parent: -1,
                rest: ponko_transform3d_at(v3(0.0, 0.0, 0.0)),
            },
            PonkoJoint {
                name: c"arm".as_ptr(),
                parent: 0,
                rest: ponko_transform3d_at(v3(0.0, 1.0, 0.0)),
            },
        ];
        assert_eq!(ponko_ctx_set_skeleton(ctx, e, joints.as_ptr(), 2), PONKO_OK);
        e
    }
}

const TIMES: [f32; 2] = [0.0, 1.0];
/// `root` slides from x 0 to x 2.
const SLIDE: [f32; 8] = [0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0];
/// `arm` turns a quarter around Y (a quaternion per key).
const TURN: [f32; 8] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.70710677, 0.0, 0.70710677];

fn track(joint: u32, channel: PonkoJointChannel, values: &[f32]) -> PonkoJointTrack {
    PonkoJointTrack {
        joint,
        channel: channel as u32,
        interpolation: 1,
        times: TIMES.as_ptr(),
        values: values.as_ptr(),
        count: 2,
    }
}

#[test]
fn a_clip_made_from_joint_tracks_poses_the_skeleton() {
    unsafe extern "C" fn setup(ctx: *mut PonkoCtx, _: *mut c_void) {
        unsafe {
            use PonkoJointChannel::*;
            let e = skeleton(ctx);
            let tracks = [
                track(0, PONKO_JOINT_TRANSLATION, &SLIDE),
                track(1, PONKO_JOINT_ROTATION, &TURN),
            ];
            let mut clip = null_mut();
            assert_eq!(
                ponko_ctx_create_clip(ctx, e, c"walk".as_ptr(), tracks.as_ptr(), 2, &mut clip),
                PONKO_OK
            );
            assert_eq!(ponko_clip_duration(clip), 1.0);
            assert_eq!(ponko_ctx_sample_clip(ctx, clip, e, 0.5, 1.0), PONKO_OK);
            let mut root = ponko_transform3d_at(v3(9.0, 9.0, 9.0));
            assert_eq!(ponko_ctx_joint_pose(ctx, e, 0, &mut root), PONKO_OK);
            assert!((root.position.x - 1.0).abs() < 1e-4, "half way: {:?}", root.position);
            let mut arm = root;
            assert_eq!(ponko_ctx_joint_pose(ctx, e, 1, &mut arm), PONKO_OK);
            assert!(arm.rotation.y > 0.3 && arm.rotation.y < 0.4, "an eighth turn");
            assert_eq!(arm.position.y, 1.0, "a channel no track names keeps its rest");
            ponko_clip_free(clip);
        }
    }
    run(setup);
}

#[test]
fn clips_refuse_joints_outside_the_skeleton_channels_given_twice_and_bad_keys() {
    unsafe extern "C" fn setup(ctx: *mut PonkoCtx, _: *mut c_void) {
        unsafe {
            use PonkoJointChannel::*;
            let e = skeleton(ctx);
            let mut clip = null_mut();
            let make = |tracks: &[PonkoJointTrack]| {
                let mut made = null_mut();
                let status = ponko_ctx_create_clip(ctx, e, c"bad".as_ptr(), tracks.as_ptr(), tracks.len(), &mut made);
                if status != PONKO_OK {
                    assert!(made.is_null(), "nothing was made");
                }
                ponko_clip_free(made);
                status
            };
            assert_eq!(make(&[track(2, PONKO_JOINT_SCALE, &SLIDE)]), PONKO_INVALID_ARGUMENT);
            assert_eq!(
                make(&[track(0, PONKO_JOINT_SCALE, &SLIDE), track(0, PONKO_JOINT_SCALE, &SLIDE)]),
                PONKO_INVALID_ARGUMENT
            );
            let mut odd = track(0, PONKO_JOINT_SCALE, &SLIDE);
            odd.channel = 7;
            assert_eq!(make(&[odd]), PONKO_INVALID_ARGUMENT);
            odd = track(0, PONKO_JOINT_SCALE, &SLIDE);
            odd.interpolation = 5;
            assert_eq!(make(&[odd]), PONKO_INVALID_ARGUMENT);
            odd = track(0, PONKO_JOINT_SCALE, &SLIDE);
            odd.times = null();
            assert_eq!(make(&[odd]), PONKO_INVALID_ARGUMENT);
            let falling = [1.0f32, 0.0];
            odd = track(0, PONKO_JOINT_SCALE, &SLIDE);
            odd.times = falling.as_ptr();
            assert_eq!(make(&[odd]), PONKO_INVALID_ARGUMENT, "times must rise");
            // No tracks is an empty clip; a plain entity has no skeleton.
            assert_eq!(make(&[]), PONKO_OK);
            let mut none = 0;
            ponko_spawn(ponko_ctx_world(ctx), null(), 0, &mut none);
            assert_eq!(
                ponko_ctx_create_clip(ctx, none, c"x".as_ptr(), null(), 0, &mut clip),
                PONKO_INVALID_ARGUMENT
            );
        }
    }
    run(setup);
}

#[test]
fn a_crowd_baked_from_a_code_clip_shows_in_the_live_counts() {
    unsafe extern "C" fn setup(ctx: *mut PonkoCtx, _: *mut c_void) {
        unsafe {
            let e = skeleton(ctx);
            let tracks = [track(0, PonkoJointChannel::PONKO_JOINT_TRANSLATION, &SLIDE)];
            let mut clip = null_mut();
            assert_eq!(
                ponko_ctx_create_clip(ctx, e, c"slide".as_ptr(), tracks.as_ptr(), 1, &mut clip),
                PONKO_OK
            );
            let clips = [PonkoCrowdClip { clip, looping: true }];
            let mut animation = null_mut();
            assert_eq!(
                ponko_ctx_bake_crowd_animation(ctx, e, clips.as_ptr(), 1, 30.0, &mut animation),
                PONKO_OK
            );
            let mut index = 9;
            assert_eq!(
                ponko_crowd_animation_clip(animation, c"slide".as_ptr(), &mut index),
                PONKO_OK
            );
            assert_eq!(index, 0);

            // A post skinned to the root joint as the crowd's one level.
            let post = ponko_mesh_data_cube(0.2);
            let mut view: PonkoMeshView = std::mem::zeroed();
            assert_eq!(ponko_mesh_data_view(post, &mut view), PONKO_OK);
            let n = view.vertex_count;
            let joints = vec![0u16; n * 4];
            let mut weights = vec![0f32; n * 4];
            weights.iter_mut().step_by(4).for_each(|w| *w = 1.0);
            assert_eq!(
                ponko_mesh_data_set_skin(post, joints.as_ptr(), weights.as_ptr(), n),
                PONKO_OK
            );
            let mut material = std::mem::zeroed();
            assert_eq!(ponko_ctx_default_material(ctx, &mut material), PONKO_OK);
            let mut mesh = std::mem::zeroed();
            assert_eq!(
                ponko_ctx_create_skinned_mesh(ctx, post, material, e, &mut mesh),
                PONKO_OK
            );
            let level = PonkoCrowdLevel {
                mesh,
                min_distance: 0.0,
                max_distance: f32::INFINITY,
                camera: true,
                shadows: true,
                influences: 4,
            };
            let people = [ponko_crowd_instance_at(v3(0.0, 0.0, 0.0))];
            let mut crowd = PonkoCrowd::default();
            assert_eq!(
                ponko_ctx_create_crowd(
                    ctx,
                    animation,
                    &level,
                    1,
                    people.as_ptr(),
                    1,
                    PonkoCrowdOptions { cast_shadows: true },
                    &mut crowd
                ),
                PONKO_OK
            );

            let mut counts: PonkoLiveCountsEx = std::mem::zeroed();
            counts.struct_size = size_of::<PonkoLiveCountsEx>() as u32;
            counts.version = PONKO_LIVE_COUNTS_EX_VERSION;
            assert_eq!(ponko_ctx_live_counts_ex(ctx, &mut counts), PONKO_OK);
            assert_eq!(counts.crowds, 1);
            let mut plain = PonkoLiveCounts::default();
            assert_eq!(ponko_ctx_live_counts(ctx, &mut plain), PONKO_OK);
            assert_eq!(counts.counts, plain, "the older counts are its first part");
            assert_eq!(ponko_ctx_free_crowd(ctx, crowd), PONKO_OK);
            assert_eq!(ponko_ctx_live_counts_ex(ctx, &mut counts), PONKO_OK);
            assert_eq!(counts.crowds, 0);

            counts.version = 0;
            assert_eq!(ponko_ctx_live_counts_ex(ctx, &mut counts), PONKO_INVALID_ARGUMENT);
            ponko_crowd_animation_free(animation);
            ponko_mesh_data_free(post);
            ponko_clip_free(clip);
        }
    }
    run(setup);
}

fn run(setup: unsafe extern "C" fn(*mut PonkoCtx, *mut c_void)) {
    unsafe {
        let game = ponko_game_new(c"clips".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 0, Some(setup), null_mut(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
}
