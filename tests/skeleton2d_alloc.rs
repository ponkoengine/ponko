//! 2D skeletons allocate nothing once warm: posing (clips crossfading, a
//! mesh deform, keyed IK, transform, path and physics constraints) and
//! drawing (a skinned mesh, a sprite, a
//! clipping mask), both for a few skeletons (drawn on one thread) and for
//! many (drawn in chunks on all of the pool's threads).
//!
//! It counts with profiling scopes, which count the allocations made on
//! their own thread while they are open: the game thread's work in `pose`
//! and `draw`, and each chunk on whichever worker draws it
//! (`skeleton2d/draw_chunk`). What rayon's workers allocate for themselves
//! (a worker's sleep lock, boxed the first time it parks, which on a busy
//! machine can be in the middle of the count) is on no scope's thread at
//! those moments, so nothing has to be allowed for it.
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

/// Rayon's worker threads, a fixed number whatever the machine has.
const WORKERS: usize = 4;
/// Ticks counted, after the warm-up.
const TICKS: u64 = 120;

/// The one allocation of rayon's own that lands on the game thread: the
/// queue the game thread hands jobs to takes a new block every 63 jobs,
/// allocated by whichever push reaches the block's end. Drawing many
/// skeletons hands it two jobs a tick (drawing and merging), and the
/// control system one. A number of jobs, not a matter of load: the same on
/// any machine. Drawing that allocated would do so on every tick (120 or
/// more).
const INJECTOR_BLOCKS: u64 = (3 * TICKS).div_ceil(63) + 1;

/// Makes the global pool `WORKERS` threads and starts every one of them
/// (their first allocations, thread and queue set-up, happen before
/// anything is counted).
fn start_pool() {
    rayon::ThreadPoolBuilder::new()
        .num_threads(WORKERS)
        .build_global()
        .expect("the first use of rayon in this process");
    rayon::broadcast(|_| ());
}

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
    profile_scope!("pose");
    let dt = 1.0 / 60.0;
    ctx.world.query::<&mut Skeleton>().for_each(|s| s.update(dt));
}

fn draw(ctx: &mut Ctx) {
    profile_scope!("draw");
    ponko::systems::draw_sprites(ctx);
}

/// The negative control: one allocation a tick on the game thread, and one
/// per skeleton on the pool's workers, each inside its scope.
fn allocate(ctx: &mut Ctx) {
    use rayon::prelude::*;
    profile_scope!("allocate");
    std::hint::black_box(Box::new(1u8));
    let n = ctx.world.query::<&Skeleton>().count();
    (0..n).into_par_iter().for_each(|_| {
        profile_scope!("allocate/each");
        std::hint::black_box(Box::new(1u8));
    });
}

#[test]
fn posing_and_drawing_skeletons_allocates_nothing_once_warm() {
    start_pool();
    for count in [8u64, 400] {
        let (_world, report) = Game::new("t", 1, 1)
            .sim_system(pose)
            .sim_system(draw)
            .sim_system(allocate)
            .profile_headless(30, TICKS, move |ctx| {
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
        let allocs = |scope: &str| {
            let s = report.scope(scope).unwrap_or_else(|| panic!("{scope} ran: {report}"));
            (s.allocs_per_frame * TICKS as f32).round() as u64
        };
        assert_eq!(
            allocs("allocate/each"),
            count * TICKS,
            "every worker's allocations count: {report}"
        );
        assert!(allocs("allocate") >= TICKS, "the game thread's count: {report}");
        let many = count >= 64;
        assert_eq!(allocs("pose"), 0, "{count} skeletons, posing: {report}");
        if many {
            assert_eq!(
                allocs("skeleton2d/draw_chunk"),
                0,
                "{count} skeletons, a chunk: {report}"
            );
        }
        let at_most = if many { INJECTOR_BLOCKS } else { 0 };
        let drawing = allocs("draw");
        assert!(
            drawing <= at_most,
            "{count} skeletons, drawing: {drawing} allocations in {TICKS} ticks: {report}"
        );
    }
}
