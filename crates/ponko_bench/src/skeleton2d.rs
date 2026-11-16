//! 2D skeletons without a window (`skeleton2d/...`): 1,000 animated
//! skeletons, each advanced through its state machine, posed, solved and
//! drawn into the sprite draw list, which is then ordered and batched as
//! the renderer does. The time is that CPU side of one frame.
//!
//! Each skeleton: 16 bones (a torso chain and four three-bone limbs), 15
//! rigid parts, two skinned meshes of 20 vertices and 24 triangles whose
//! vertices follow two bones each, a 1D blend of a walk and a run that key
//! every bone, and a two-bone IK constraint on one leg.
//!
//! - `1k`: skeletons advance on all cores.
//! - `1k-z`: the same, with every other skeleton one layer up, so the
//!   frame's vertices are sorted by draw order.
use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ponko::anim::{AnimationParameter, ParamId, StateId};
use ponko::d2::Transform;
use ponko::prelude::*;
use ponko::skeleton2d::{
    BonePose, BoneTrack, Clip, IkConstraint, Key, MeshVertex, Skeleton, SkeletonData, SkeletonGraph, SkeletonMotion,
    SkeletonState, SkinnedMesh, SpritePart,
};

use crate::{Bench, report};

const SKELETONS: usize = 1_000;
const WARMUP: usize = 30;
const FRAMES: usize = 200;
const DT: f32 = 1.0 / 60.0;

/// Every timed frame and the vertices drawn in the last one.
struct Times(Vec<Duration>, usize, usize);

/// A 4 x 5 grid of vertices along bone `a` into bone `b`, weighted from
/// one to the other along its length.
fn mesh(texture: Texture, a: usize, b: usize) -> SkinnedMesh {
    let mut vertices = Vec::new();
    for row in 0..5 {
        for col in 0..4 {
            let t = row as f32 / 4.0;
            let at = vec2(col as f32 * 4.0 - 6.0, row as f32 * 10.0);
            vertices.push(MeshVertex::new(
                at,
                vec2(col as f32 / 3.0, t),
                &[(a, 1.0 - t + 0.01), (b, t + 0.01)],
            ));
        }
    }
    let mut triangles = Vec::new();
    for row in 0..4u16 {
        for col in 0..3u16 {
            let i = row * 4 + col;
            triangles.extend_from_slice(&[i, i + 1, i + 4, i + 1, i + 5, i + 4]);
        }
    }
    SkinnedMesh::new(texture, vertices, triangles)
}

fn rig(texture: Texture) -> Arc<SkeletonData> {
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let mut chain = root;
    for (i, name) in ["hip", "chest", "head"].into_iter().enumerate() {
        let pose = BonePose::at(vec2(0.0, if i == 0 { -20.0 } else { -15.0 })).with_rotation(-0.1);
        chain = data.add_bone(name, Some(chain), pose, 15.0);
    }
    let mut limbs = Vec::new();
    for (limb, parent) in [("arm-l", 2), ("arm-r", 2), ("leg-l", 1), ("leg-r", 1)] {
        let mut bone = parent;
        for part in 0..3 {
            let pose = BonePose::at(vec2(0.0, if part == 0 { 0.0 } else { 12.0 })).with_rotation(1.4);
            bone = data.add_bone(&format!("{limb}-{part}"), Some(bone), pose, 12.0);
            limbs.push(bone);
        }
    }
    let bones = data.bones().len();
    for bone in 1..bones {
        let slot = data.add_slot(&format!("part-{bone}"), bone);
        let part = SpritePart::new(texture)
            .with_size(vec2(6.0, 12.0))
            .with_pivot(vec2(0.5, 0.0));
        data.add_attachment(slot, "part", part);
    }
    for (name, (a, b)) in [("skin-chest", (1, 2)), ("skin-leg", (limbs[6], limbs[7]))] {
        let slot = data.add_slot(name, a);
        data.add_attachment(slot, name, mesh(texture, a, b));
    }
    let goal = data.add_bone("goal", Some(root), BonePose::at(vec2(6.0, 0.0)), 0.0);
    data.add_ik(IkConstraint::new("leg", &[limbs[9], limbs[10]], goal).with_mix(0.5));
    let mut clips = Vec::new();
    for (name, swing) in [("walk", 0.4), ("run", 0.9)] {
        let mut clip = Clip::new(name, 1.0);
        for bone in 1..bones {
            let mut track = BoneTrack::new(bone);
            let phase = bone as f32 * 0.37;
            track.rotate = (0..5)
                .map(|k| {
                    let t = k as f32 / 4.0;
                    Key::new(t, swing * (std::f32::consts::TAU * t + phase).sin())
                })
                .collect();
            track.translate = vec![
                Key::new(0.0, Vec2::ZERO),
                Key::new(0.5, vec2(0.0, -2.0)),
                Key::new(1.0, Vec2::ZERO),
            ];
            clip = clip.with_bone_track(track);
        }
        clips.push(data.add_clip(clip.with_event(0.5, "step")));
    }
    let graph = SkeletonGraph::new(
        vec![("speed", AnimationParameter::Float(0.0))],
        vec![SkeletonState::new(
            "move",
            SkeletonMotion::Blend1D {
                parameter: ParamId(0),
                children: vec![(0.0, clips[0]), (1.0, clips[1])],
            },
        )],
        vec![],
        StateId(0),
    )
    .expect("a valid graph");
    data.set_graph(graph).expect("its clips exist");
    Arc::new(data)
}

pub fn run(b: &mut Bench) {
    for (name, layered) in [("skeleton2d/1k", false), ("skeleton2d/1k-z", true)] {
        if b.skips(name) {
            continue;
        }
        let mut world = Game::new("skeleton bench", 1280, 720).run_headless(0, move |ctx| {
            let texture = ctx.gfx.create_texture(1, 1, &[255; 4]);
            let data = rig(texture);
            for i in 0..SKELETONS {
                let mut s = Skeleton::new(data.clone());
                let speed = s.parameter_id("speed").expect("the parameter");
                s.set_float(speed, (i % 10) as f32 / 9.0);
                s.set_normalized_time(i as f32 / SKELETONS as f32);
                s.z = if layered { (i % 2) as f32 } else { 0.0 };
                let at = vec2((i % 40) as f32 * 32.0, (i / 40) as f32 * 28.0 + 60.0);
                ctx.world.spawn((Transform::at(at), s));
            }
            let mut times = Vec::with_capacity(FRAMES);
            let (mut drawn, mut batches) = (0, 0);
            for frame in 0..WARMUP + FRAMES {
                let start = Instant::now();
                let counts = ctx.bench_skeleton2d_frame(DT);
                let took = start.elapsed();
                black_box(counts);
                ctx.world
                    .query::<&mut Skeleton>()
                    .for_each(|s| s.drain_events().for_each(drop));
                if frame >= WARMUP {
                    times.push(took);
                    (drawn, batches) = counts;
                }
            }
            ctx.world.insert_resource(Times(times, drawn, batches));
        });
        let Times(mut times, drawn, batches) = world.remove_resource::<Times>().expect("the times");
        // 15 parts x 6 vertices + 2 meshes x 24 triangles x 3.
        assert_eq!(drawn, SKELETONS * (15 * 6 + 2 * 24 * 3), "every part drawn");
        eprintln!("{name}: {drawn} vertices in {batches} batches");
        b.report(report::Record::new(name, report::Kind::Steady, WARMUP, &mut times));
    }
}
