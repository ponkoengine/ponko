//! The benchmark switch that forces the engine's small parallel passes to
//! share their work out or not (`ponko::bench_sharing`, unstable,
//! `bench-internals`), which the web threads fixture's threshold sweep uses.
//! Its own file: the switch is global, and would disturb other tests.
#![cfg(feature = "bench-internals")]

use ponko::bench_sharing::{Sharing, set_sharing, shared_runs};
use ponko::prelude::*;

/// Draws `n` skeletons once (the 2D skeleton pass) and says whether it
/// handed the drawing to the frame pool.
fn skeletons_shared(n: usize) -> bool {
    let shared = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = shared.clone();
    Game::new("t", 1, 1)
        .sim_system(move |ctx| {
            let before = shared_runs();
            ctx.bench_skeleton2d_draw();
            ctx.bench_skeleton2d_finish();
            seen.set(shared_runs() > before);
        })
        .run_headless(1, move |ctx| {
            use ponko::skeleton2d::{BonePose, Skeleton, SkeletonData, SpritePart};
            let pixel = ctx.gfx.create_texture(1, 1, &[255; 4]);
            let mut data = SkeletonData::new();
            let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
            let slot = data.add_slot("slot", root);
            data.add_attachment(slot, "part", SpritePart::new(pixel).with_size(vec2(4.0, 4.0)));
            let data = std::sync::Arc::new(data);
            for i in 0..n {
                ctx.world
                    .spawn((d2::Transform::at(vec2(i as f32, 0.0)), Skeleton::new(data.clone())));
            }
        });
    shared.get()
}

#[test]
fn forced_sharing_overrides_the_threshold_until_set_back_to_auto() {
    let from = ponko::bench_sharing::skeletons_from();
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    if threads < 2 {
        // One thread: the pass never shares by its threshold.
        return;
    }
    assert!(
        !skeletons_shared(from - 1),
        "below the threshold: on the calling thread"
    );
    assert!(skeletons_shared(from), "at the threshold: shared out");
    set_sharing(Sharing::Never);
    assert!(!skeletons_shared(from * 2), "never shares, even past the threshold");
    set_sharing(Sharing::Always);
    assert!(skeletons_shared(2), "always shares, even far below the threshold");
    set_sharing(Sharing::Auto);
    assert!(
        skeletons_shared(from) && !skeletons_shared(from - 1),
        "by the threshold again"
    );
}
