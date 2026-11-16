//! Humanoid retargeting from C: maps detected from joint names, a
//! retargeter between two skeletons of different sizes and names, a clip
//! retargeted once and a live pose copied every frame. The source skeleton
//! and its clip come from a small glTF file written to the temp folder; the
//! target is made from C with `ponko_ctx_set_skeleton`. Everything runs
//! headless inside a setup callback, which records what it saw: a failed
//! assertion there would abort the process, so the test checks afterwards.
use ponko_ffi::*;
use std::ffi::{CStr, CString, c_void};
use std::ptr::{null, null_mut};

use PonkoHumanBone::*;
use PonkoStatus::*;

/// Each joint's parent and its offset from it at rest (meters, before
/// scaling): hips, spine, head, then each arm and leg in three parts.
const PARENTS: [i32; 15] = [-1, 0, 1, 1, 3, 4, 1, 6, 7, 0, 9, 10, 0, 12, 13];
const AT: [[f32; 3]; 15] = [
    [0., 1., 0.],
    [0., 0.1, 0.],
    [0., 0.5, 0.],
    [0.2, 0.4, 0.],
    [0.3, 0., 0.],
    [0.3, 0., 0.],
    [-0.2, 0.4, 0.],
    [-0.3, 0., 0.],
    [-0.3, 0., 0.],
    [0.1, -0.05, 0.],
    [0., -0.45, 0.],
    [0., -0.45, 0.],
    [-0.1, -0.05, 0.],
    [0., -0.45, 0.],
    [0., -0.45, 0.],
];
const SOURCE_NAMES: [&str; 15] = [
    "Hips",
    "Spine",
    "Head",
    "LeftArm",
    "LeftForeArm",
    "LeftHand",
    "RightArm",
    "RightForeArm",
    "RightHand",
    "LeftUpLeg",
    "LeftLeg",
    "LeftFoot",
    "RightUpLeg",
    "RightLeg",
    "RightFoot",
];
const TARGET_NAMES: [&CStr; 15] = [
    c"pelvis",
    c"spine_01",
    c"head",
    c"upperarm_l",
    c"lowerarm_l",
    c"hand_l",
    c"upperarm_r",
    c"lowerarm_r",
    c"hand_r",
    c"thigh_l",
    c"calf_l",
    c"foot_l",
    c"thigh_r",
    c"calf_r",
    c"foot_r",
];

/// The source humanoid at full size with a one-second `walk` clip moving
/// its hips from x 0 to x 1 (at their rest height).
fn gltf() -> String {
    let mut bytes = Vec::new();
    let mut floats = |v: &[f32]| v.iter().for_each(|f| bytes.extend(f.to_le_bytes()));
    floats(&[0., 0., 0., 0.1, 0., 0., 0., 0.1, 0.]); // 0: a triangle on the head
    floats(&[0., 1.]); // 36: key times
    floats(&[0., 1., 0., 1., 1., 0.]); // 44: hips from (0, 1, 0) to (1, 1, 0)
    let nodes: Vec<String> = (0..15)
        .map(|i| {
            let children: Vec<String> = (0..15)
                .filter(|&c| PARENTS[c] == i as i32)
                .map(|c| c.to_string())
                .collect();
            let [x, y, z] = AT[i];
            let mesh = if i == 2 { r#", "mesh": 0"# } else { "" };
            format!(
                r#"{{"name": "{}", "translation": [{x}, {y}, {z}], "children": [{}]{mesh}}}"#,
                SOURCE_NAMES[i],
                children.join(", ")
            )
        })
        .collect();
    format!(
        r#"{{"asset": {{"version": "2.0"}}, "scene": 0, "scenes": [{{"nodes": [0]}}],
        "nodes": [{}],
        "meshes": [{{"primitives": [{{"attributes": {{"POSITION": 0}}}}]}}],
        "animations": [
            {{"name": "walk", "samplers": [{{"input": 1, "output": 2, "interpolation": "LINEAR"}}],
              "channels": [{{"sampler": 0, "target": {{"node": 0, "path": "translation"}}}}]}}],
        "buffers": [{{"byteLength": 68, "uri": "data:application/octet-stream;base64,{}"}}],
        "bufferViews": [{{"buffer": 0, "byteLength": 36}}, {{"buffer": 0, "byteOffset": 36, "byteLength": 8}},
            {{"buffer": 0, "byteOffset": 44, "byteLength": 24}}],
        "accessors": [
            {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [0.1, 0.1, 0]}},
            {{"bufferView": 1, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0], "max": [1]}},
            {{"bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC3"}}]}}"#,
        nodes.join(",\n"),
        base64(&bytes)
    )
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// What the setup callback saw.
#[derive(Default)]
struct Seen {
    path: CString,
    failures: Vec<String>,
    scale: f32,
    missing_after_detect: usize,
    clip_duration_kept: bool,
    /// The target's hips (joint 0) after sampling the retargeted clip at 1 s.
    clip_hips: [f32; 3],
    /// The target's hips after copying the source's live pose at 1 s.
    pose_hips: [f32; 3],
    /// The retargeter still worked after both entities were despawned.
    outlives_entities: bool,
}

macro_rules! expect {
    ($seen:expr, $status:expr, $want:expr) => {{
        let got = $status;
        if got != $want {
            $seen.failures.push(format!(
                "line {}: {} gave {got:?}, wanted {:?} ({})",
                line!(),
                stringify!($status),
                $want,
                last_error()
            ));
        }
    }};
}

fn last_error() -> String {
    // SAFETY: the library's static, NUL-terminated message.
    unsafe { CStr::from_ptr(ponko_last_error()) }
        .to_string_lossy()
        .into_owned()
}

fn rest(i: usize, size: f32) -> PonkoTransform3D {
    let [x, y, z] = AT[i];
    PonkoTransform3D {
        reserved: Default::default(),
        position: PonkoVec3 {
            x: x * size,
            y: y * size,
            z: z * size,
        },
        rotation: PonkoQuat {
            x: 0.,
            y: 0.,
            z: 0.,
            w: 1.,
        },
        scale: PonkoVec3 { x: 1., y: 1., z: 1. },
    }
}

unsafe fn hips(ctx: *mut PonkoCtx, e: u64) -> [f32; 3] {
    let mut pose: PonkoTransform3D = unsafe { std::mem::zeroed() };
    unsafe { ponko_ctx_joint_pose(ctx, e, 0, &mut pose) };
    [pose.position.x, pose.position.y, pose.position.z]
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    let seen = unsafe { &mut *user.cast::<Seen>() };
    unsafe {
        let w = ponko_ctx_world(ctx);
        let (mut source, mut target, mut bare) = (0, 0, 0);
        expect!(seen, ponko_spawn(w, null(), 0, &mut source), PONKO_OK);
        expect!(seen, ponko_spawn(w, null(), 0, &mut target), PONKO_OK);
        expect!(seen, ponko_spawn(w, null(), 0, &mut bare), PONKO_OK);
        expect!(seen, ponko_ctx_load_skeleton(ctx, source, seen.path.as_ptr()), PONKO_OK);
        let joints: Vec<PonkoJoint> = (0..15)
            .map(|i| PonkoJoint {
                name: TARGET_NAMES[i].as_ptr(),
                parent: PARENTS[i],
                rest: rest(i, 0.5),
            })
            .collect();
        expect!(
            seen,
            ponko_ctx_set_skeleton(ctx, target, joints.as_ptr(), joints.len()),
            PONKO_OK
        );

        // Detect both maps; every required bone is found on both.
        let (mut source_map, mut target_map) = (null_mut(), null_mut());
        expect!(
            seen,
            ponko_ctx_detect_humanoid_map(ctx, source, &mut source_map),
            PONKO_OK
        );
        expect!(
            seen,
            ponko_ctx_detect_humanoid_map(ctx, target, &mut target_map),
            PONKO_OK
        );
        let mut missing = 0;
        expect!(
            seen,
            ponko_humanoid_map_missing(target_map, null_mut(), 0, &mut missing),
            PONKO_OK
        );
        seen.missing_after_detect = missing;
        let mut joint = -2;
        expect!(
            seen,
            ponko_humanoid_map_get(target_map, PONKO_HUMAN_BONE_LEFT_FOOT as u32, &mut joint),
            PONKO_OK
        );
        if joint != 11 {
            seen.failures
                .push(format!("left foot detected as joint {joint}, not 11"));
        }
        // An entity without a skeleton: not found.
        let mut none = null_mut();
        expect!(
            seen,
            ponko_ctx_detect_humanoid_map(ctx, bare, &mut none),
            PONKO_NOT_FOUND
        );

        // A map missing a required bone can't make a retargeter, and says which.
        let mut broken = null_mut();
        expect!(seen, ponko_humanoid_map_new(&mut broken), PONKO_OK);
        let mut r = null_mut();
        expect!(
            seen,
            ponko_ctx_create_retargeter(ctx, source, broken, target, target_map, &mut r),
            PONKO_INVALID_ARGUMENT
        );
        let message = last_error();
        if !message.contains("hips") {
            seen.failures
                .push(format!("missing-bone message doesn't name the bone: {message}"));
        }
        let mut wanted = [0u32; 32];
        let mut count = 0;
        expect!(
            seen,
            ponko_humanoid_map_missing(broken, wanted.as_mut_ptr(), 32, &mut count),
            PONKO_OK
        );
        if count == 0 || !wanted[..count.min(32)].contains(&(PONKO_HUMAN_BONE_HIPS as u32)) {
            seen.failures
                .push("an empty map doesn't report the hips missing".into());
        }
        ponko_humanoid_map_free(broken);
        // A joint the skeleton doesn't have.
        let mut bad = null_mut();
        expect!(seen, ponko_ctx_detect_humanoid_map(ctx, target, &mut bad), PONKO_OK);
        expect!(
            seen,
            ponko_humanoid_map_set(bad, PONKO_HUMAN_BONE_HEAD as u32, 99),
            PONKO_OK
        );
        expect!(
            seen,
            ponko_ctx_create_retargeter(ctx, source, source_map, target, bad, &mut r),
            PONKO_INVALID_ARGUMENT
        );
        ponko_humanoid_map_free(bad);

        // The real one: half the size.
        expect!(
            seen,
            ponko_ctx_create_retargeter(ctx, source, source_map, target, target_map, &mut r),
            PONKO_OK
        );
        ponko_humanoid_map_free(source_map);
        ponko_humanoid_map_free(target_map);
        expect!(seen, ponko_retargeter_scale(r, &mut seen.scale), PONKO_OK);
        expect!(seen, ponko_retargeter_set_sample_rate(r, 0.0), PONKO_INVALID_ARGUMENT);
        expect!(seen, ponko_retargeter_set_sample_rate(r, 60.0), PONKO_OK);
        expect!(seen, ponko_retargeter_set_foot_ik(r, false), PONKO_OK);

        // A clip, retargeted once, then played on the target.
        let mut walk = null_mut();
        expect!(
            seen,
            ponko_ctx_load_clip(ctx, seen.path.as_ptr(), c"walk".as_ptr(), &mut walk),
            PONKO_OK
        );
        let mut walk_b = null_mut();
        expect!(seen, ponko_retarget_clip(r, walk, &mut walk_b), PONKO_OK);
        seen.clip_duration_kept = ponko_clip_duration(walk_b) == ponko_clip_duration(walk) && !walk_b.is_null();
        expect!(seen, ponko_ctx_sample_clip(ctx, walk_b, target, 1.0, 1.0), PONKO_OK);
        seen.clip_hips = hips(ctx, target);

        // A live pose, copied: reset the target, pose the source, copy.
        expect!(
            seen,
            ponko_ctx_set_skeleton(ctx, target, joints.as_ptr(), joints.len()),
            PONKO_OK
        );
        expect!(seen, ponko_ctx_sample_clip(ctx, walk, source, 1.0, 1.0), PONKO_OK);
        expect!(seen, ponko_ctx_retarget_pose(ctx, r, source, target), PONKO_OK);
        seen.pose_hips = hips(ctx, target);
        expect!(
            seen,
            ponko_ctx_retarget_pose(ctx, r, source, source),
            PONKO_INVALID_ARGUMENT
        );
        expect!(seen, ponko_ctx_retarget_pose(ctx, r, source, bare), PONKO_NOT_FOUND);

        // It copied the rest data: despawning both skeletons doesn't break it.
        expect!(seen, ponko_despawn(w, source), PONKO_OK);
        expect!(seen, ponko_despawn(w, target), PONKO_OK);
        let mut again = null_mut();
        seen.outlives_entities = ponko_retarget_clip(r, walk, &mut again) == PONKO_OK && !again.is_null();
        ponko_clip_free(again);

        ponko_clip_free(walk);
        ponko_clip_free(walk_b);
        ponko_retargeter_free(r);
    }
}

#[test]
fn a_clip_and_a_live_pose_retarget_onto_a_smaller_humanoid_from_c() {
    let file = std::env::temp_dir().join("ponko-ffi-retarget-humanoid.gltf");
    std::fs::write(&file, gltf()).unwrap();
    let mut seen = Seen {
        path: CString::new(file.to_str().unwrap()).unwrap(),
        ..Seen::default()
    };
    unsafe {
        let g = ponko_game_new(c"retarget".as_ptr(), 64, 64);
        let mut world = null_mut();
        let user = (&raw mut seen).cast();
        assert_eq!(ponko_game_run_headless(g, 0, Some(setup), user, &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    let _ = std::fs::remove_file(&file);
    assert!(seen.failures.is_empty(), "{:#?}", seen.failures);
    assert_eq!(seen.missing_after_detect, 0);
    assert!((seen.scale - 0.5).abs() < 1e-4, "scale {}", seen.scale);
    assert!(seen.clip_duration_kept);
    // The source's hips moved 1 m in x; the half-size target's move 0.5 m.
    let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3);
    assert!(near(seen.clip_hips, [0.5, 0.5, 0.]), "clip hips {:?}", seen.clip_hips);
    assert!(near(seen.pose_hips, [0.5, 0.5, 0.]), "pose hips {:?}", seen.pose_hips);
    assert!(seen.outlives_entities);
}

#[test]
fn human_bones_have_names_and_null_arguments_fail_cleanly() {
    unsafe {
        let name = CStr::from_ptr(ponko_human_bone_name(PONKO_HUMAN_BONE_RIGHT_LITTLE_DISTAL as u32));
        assert_eq!(name.to_str(), Ok("right little distal"));
        assert!(ponko_human_bone_name(53).is_null());
        assert!(ponko_human_bone_is_required(PONKO_HUMAN_BONE_LEFT_FOOT as u32));
        assert!(!ponko_human_bone_is_required(PONKO_HUMAN_BONE_LEFT_TOES as u32));
        assert!(!ponko_human_bone_is_required(999));

        let mut m = null_mut();
        assert_eq!(ponko_humanoid_map_new(&mut m), PONKO_OK);
        assert_eq!(ponko_humanoid_map_set(m, 53, 0), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_humanoid_map_set(m, 0, -2), PONKO_INVALID_ARGUMENT);
        let mut j = 0;
        assert_eq!(ponko_humanoid_map_get(m, 999, &mut j), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_humanoid_map_missing(m, null_mut(), 4, &mut 0),
            PONKO_INVALID_ARGUMENT
        );
        ponko_humanoid_map_free(m);
        ponko_humanoid_map_free(null_mut());
        ponko_retargeter_free(null_mut());
        assert_eq!(ponko_humanoid_map_get(null(), 0, &mut j), PONKO_INVALID_ARGUMENT);
        let mut s = 0.0;
        assert_eq!(ponko_retargeter_scale(null(), &mut s), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_retargeter_set_foot_ik(null_mut(), true), PONKO_INVALID_ARGUMENT);
    }
}
