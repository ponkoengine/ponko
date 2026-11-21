//! Crowd levels that crossfade from C (`PonkoCrowdDesc`,
//! `ponko_ctx_create_crowd_ex`, Rust `CrowdLevel::with_fade`): the fades are
//! the engine's to check, a level without one switches at once, and the
//! older call keeps working.
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

/// A baked one-clip animation and a skinned post to draw it with.
unsafe fn parts(ctx: *mut PonkoCtx) -> (*mut PonkoCrowdAnimation, PonkoMesh, *mut PonkoMeshData, *mut PonkoClip) {
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
        (animation, mesh, post, clip)
    }
}

fn level(mesh: PonkoMesh, min_distance: f32, max_distance: f32) -> PonkoCrowdLevel {
    PonkoCrowdLevel {
        mesh,
        min_distance,
        max_distance,
        camera: true,
        shadows: true,
        influences: 4,
    }
}

#[test]
fn crowd_levels_from_c_crossfade_within_their_bands_and_bad_fades_are_refused() {
    unsafe extern "C" fn setup(ctx: *mut PonkoCtx, _: *mut c_void) {
        unsafe {
            let (animation, mesh, post, clip) = parts(ctx);
            let levels = [level(mesh, 0.0, 30.0), level(mesh, 30.0, f32::INFINITY)];
            let people = [
                ponko_crowd_instance_at(v3(0.0, 0.0, 0.0)),
                ponko_crowd_instance_at(v3(2.0, 0.0, 0.0)),
            ];
            let mut d = ponko_crowd_desc_defaults();
            assert!(d.cast_shadows, "shadows on, as Rust's default");
            d.animation = animation;
            d.levels = levels.as_ptr();
            d.level_count = 2;
            d.instances = people.as_ptr();
            d.count = 2;

            // Fading the near level over its last 5 m.
            let fades = [5.0f32, 0.0];
            d.fades = fades.as_ptr();
            let mut crowd = PonkoCrowd::default();
            assert_eq!(ponko_ctx_create_crowd_ex(ctx, &d, &mut crowd), PONKO_OK);
            let mut len = 0;
            assert_eq!(ponko_ctx_crowd_len(ctx, crowd, &mut len), PONKO_OK);
            assert_eq!(len, 2);
            assert_eq!(ponko_ctx_free_crowd(ctx, crowd), PONKO_OK);

            // No fades: switch at once, as ponko_ctx_create_crowd does.
            d.fades = null();
            assert_eq!(ponko_ctx_create_crowd_ex(ctx, &d, &mut crowd), PONKO_OK);
            assert_eq!(ponko_ctx_free_crowd(ctx, crowd), PONKO_OK);

            // The engine checks each fade: wider than the band, negative, not finite.
            for bad in [31.0f32, -1.0, f32::NAN] {
                let fades = [bad, 0.0];
                d.fades = fades.as_ptr();
                assert_eq!(
                    ponko_ctx_create_crowd_ex(ctx, &d, &mut crowd),
                    PONKO_INVALID_ARGUMENT,
                    "fade {bad}"
                );
            }
            let msg = std::ffi::CStr::from_ptr(ponko_last_error())
                .to_string_lossy()
                .into_owned();
            assert!(msg.contains("fade"), "{msg}");

            // The header is checked, and so are the levels.
            d.fades = null();
            d.version = 0;
            assert_eq!(ponko_ctx_create_crowd_ex(ctx, &d, &mut crowd), PONKO_INVALID_ARGUMENT);
            d.version = PONKO_CROWD_DESC_VERSION;
            d.level_count = 0;
            assert_eq!(ponko_ctx_create_crowd_ex(ctx, &d, &mut crowd), PONKO_INVALID_ARGUMENT);
            d.level_count = 2;
            d.animation = null();
            assert_eq!(ponko_ctx_create_crowd_ex(ctx, &d, &mut crowd), PONKO_INVALID_ARGUMENT);
            assert_eq!(
                ponko_ctx_create_crowd_ex(ctx, null(), &mut crowd),
                PONKO_INVALID_ARGUMENT
            );
            d.animation = animation;
            assert_eq!(ponko_ctx_create_crowd_ex(ctx, &d, null_mut()), PONKO_INVALID_ARGUMENT);

            ponko_crowd_animation_free(animation);
            ponko_mesh_data_free(post);
            ponko_clip_free(clip);
        }
    }
    unsafe {
        let game = ponko_game_new(c"fades".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 0, Some(setup), null_mut(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
}
