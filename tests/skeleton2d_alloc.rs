//! 2D skeletons allocate nothing once warm: posing (clips crossfading, a
//! mesh deform, keyed IK, transform, path and physics constraints) and
//! drawing (a skinned mesh, a sprite, a
//! clipping mask), both for a few skeletons (drawn on one thread) and for
//! many (drawn in chunks on all cores). Its own process, so nothing else
//! allocates while it counts.
use std::sync::Arc;

use ponko::perf::CountingAllocator;
use ponko::prelude::*;
use ponko::skeleton2d::{
    BonePose, BoneTrack, Clip, ClippingMask, DeformTrack, IkConstraint, IkTrack, Key, MeshVertex, PathConstraint,
    PathRotate, PathShape, PathTrack, PhysicsConstraint, PhysicsTrack, Skeleton, SkeletonData, SkinnedMesh, SpritePart,
    TransformConstraint, TransformTrack,
};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn rig(texture: Texture) -> Arc<SkeletonData> {
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let upper = data.add_bone("upper", Some(root), BonePose::IDENTITY, 10.0);
    let lower = data.add_bone("lower", Some(upper), BonePose::at(vec2(10.0, 0.0)), 10.0);
    let goal = data.add_bone("goal", Some(root), BonePose::at(vec2(12.0, 6.0)), 0.0);
    let follower = data.add_bone("follower", Some(root), BonePose::IDENTITY, 0.0);
    let tail1 = data.add_bone("tail1", Some(root), BonePose::IDENTITY, 5.0);
    let tail2 = data.add_bone("tail2", Some(tail1), BonePose::at(vec2(5.0, 0.0)), 5.0);
    let hair = data.add_bone("hair", Some(lower), BonePose::at(vec2(10.0, 0.0)), 6.0);
    data.add_ik(IkConstraint::new("reach", &[upper, lower], goal));
    let mut follow = TransformConstraint::new("follow", &[follower], lower);
    follow.mix_rotate = 0.5;
    data.add_transform_constraint(follow);
    let mask = data.add_slot("mask", root);
    let skin = data.add_slot("skin", upper);
    let part = data.add_slot("part", follower);
    let track = data.add_slot("track", root);
    let mut along = PathConstraint::new("along", &[tail1, tail2], track);
    along.rotate_mode = PathRotate::Chain;
    data.add_path_constraint(along);
    let mut sway = PhysicsConstraint::new("sway", hair);
    (sway.rotate, sway.x, sway.gravity) = (1.0, 1.0, 20.0);
    data.add_physics_constraint(sway);
    let point = |x, y| MeshVertex::new(vec2(x, y), Vec2::ZERO, &[(root, 1.0)]);
    let curve = [
        (0.0, 0.0),
        (5.0, 0.0),
        (10.0, 0.0),
        (20.0, 10.0),
        (25.0, 10.0),
        (30.0, 10.0),
    ];
    let shape = PathShape::new(curve.iter().map(|&(x, y)| point(x, y)).collect());
    data.add_attachment(track, "track", shape);
    let corner = |x, y| MeshVertex::new(vec2(x, y), Vec2::ZERO, &[(root, 1.0)]);
    let outline = vec![
        corner(-5.0, -5.0),
        corner(15.0, -5.0),
        corner(15.0, 5.0),
        corner(5.0, 0.0),
        corner(-5.0, 5.0),
    ];
    data.add_attachment(mask, "mask", ClippingMask::new(outline, Some(skin)));
    let vertices = vec![
        MeshVertex::new(vec2(0.0, 0.0), vec2(0.0, 0.0), &[(upper, 1.0)]),
        MeshVertex::new(vec2(20.0, 0.0), vec2(1.0, 0.0), &[(lower, 1.0)]),
        MeshVertex::new(vec2(10.0, 4.0), vec2(0.5, 1.0), &[(upper, 1.0), (lower, 1.0)]),
    ];
    data.add_attachment(skin, "skin", SkinnedMesh::new(texture, vertices, vec![0, 1, 2]));
    data.add_attachment(part, "part", SpritePart::new(texture).with_size(vec2(4.0, 4.0)));
    let mut swing = BoneTrack::new(upper);
    swing.rotate = vec![Key::new(0.0, 0.0), Key::new(0.5, 1.0), Key::new(1.0, 0.0)];
    let mut bulge = DeformTrack::new(skin, 0, "skin");
    let still: Arc<[Vec2]> = vec![Vec2::ZERO; 12].into();
    let mut moved = vec![Vec2::ZERO; 12];
    moved[8] = vec2(0.0, 3.0);
    bulge.keys = vec![
        Key::new(0.0, still.clone()),
        Key::new(0.5, moved.into()),
        Key::new(1.0, still),
    ];
    let mut reach = IkTrack::new(0);
    reach.mix = vec![Key::new(0.0, 1.0), Key::new(1.0, 0.5)];
    reach.bend_positive = vec![Key::new(0.5, false)];
    let mut follows = TransformTrack::new(0);
    follows.mix_translate = vec![Key::new(0.0, Vec2::ZERO), Key::new(1.0, Vec2::ONE)];
    let mut slide = PathTrack::new(0);
    slide.position = vec![Key::new(0.0, 0.0), Key::new(1.0, 0.5)];
    let mut gust = PhysicsTrack::new(Some(0));
    gust.wind = vec![Key::new(0.0, 0.0), Key::new(1.0, 30.0)];
    gust.reset = vec![0.5];
    data.add_clip(
        Clip::new("wave", 1.0)
            .with_bone_track(swing)
            .with_deform_track(bulge)
            .with_ik_track(reach)
            .with_transform_track(follows)
            .with_path_track(slide)
            .with_physics_track(gust),
    );
    let mut rest = IkTrack::new(0);
    rest.mix = vec![Key::new(0.0, 0.25)];
    let mut back = PathTrack::new(0);
    back.position = vec![Key::new(0.0, 1.0), Key::new(0.7, 0.2)];
    data.add_clip(Clip::new("rest", 0.7).with_ik_track(rest).with_path_track(back));
    Arc::new(data)
}

fn pose(ctx: &mut Ctx) {
    let dt = 1.0 / 60.0;
    ctx.world.query::<&mut Skeleton>().for_each(|s| s.update(dt));
}

#[test]
fn posing_and_drawing_skeletons_allocates_nothing_once_warm() {
    for count in [8, 400] {
        let (_world, report) = Game::new("t", 1, 1)
            .sim_system(pose)
            .sim_system(ponko::systems::draw_sprites)
            .profile_headless(30, 120, move |ctx| {
                let data = rig(ctx.gfx.create_texture(1, 1, &[255; 4]));
                for i in 0..count {
                    let mut s = Skeleton::new(data.clone());
                    s.play("wave");
                    s.set_normalized_time(i as f32 / count as f32);
                    // Long enough to blend for every frame counted.
                    assert!(s.crossfade("rest", 1000.0));
                    ctx.world.spawn((ponko::d2::Transform::at(vec2(i as f32, 0.0)), s));
                }
            });
        assert!(report.allocs.is_some(), "the counting allocator is installed");
        // The counts are the whole process's: rayon's worker threads (the
        // engine poses skeletons on them, and many are drawn on them) now
        // and then take a block for their job queues or deferred frees, and
        // that lands on whichever system runs then, more often on a busy
        // machine. Posing or drawing that allocated would do so every frame
        // (and per skeleton), so it shows as one or more a frame.
        for system in ["pose", "draw_sprites"] {
            let allocs = report.system(system).unwrap().allocs_per_frame;
            assert!(allocs < 0.5, "{count} skeletons, {system}: {report}");
        }
    }
}
