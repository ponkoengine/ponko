//! The `sky_kit` example, run headless: its scene builds, its day moves
//! the sun and the moon, and its keys step the clouds. No window or GPU
//! (`examples/sky_kit/scene.rs` is the example's own code).
#![cfg(feature = "3d")]

#[path = "../examples/sky_kit/scene.rs"]
mod scene;

use ponko::prelude::*;
use ponko::render::CloudQuality;
use std::cell::RefCell;
use std::rc::Rc;

/// Runs the example's scene from `hour` for `ticks` ticks (its frame
/// system, run as a sim system: headless runs have no frames), calling
/// `each` after every tick with the state.
fn run(hour: f32, ticks: u64, mut each: impl FnMut(&mut Ctx, &mut scene::SkyDemo) + 'static) {
    let demo = Rc::new(RefCell::new(None::<scene::SkyDemo>));
    let state = demo.clone();
    Game::new("sky kit", 64, 64)
        .setup(move |ctx| *state.borrow_mut() = Some(scene::setup(ctx, hour)))
        .sim_system(move |ctx: &mut Ctx| {
            let mut demo = demo.borrow_mut();
            let demo = demo.as_mut().expect("set up");
            scene::update(ctx, demo);
            each(ctx, demo);
        })
        .run_headless(ticks);
}

#[test]
fn the_example_turns_the_kit_on_and_its_day_passes() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    // Two seconds at half an hour a second: six to seven o'clock.
    run(6.0, 120, move |ctx, demo| {
        log.borrow_mut()
            .push((demo.day.hour, ctx.scene3d.lighting.sun_direction.y));
        assert!(ctx.scene3d.sky.kit.is_some(), "the kit is the sky");
    });
    let seen = seen.borrow();
    let (first, last) = (seen[0], seen[seen.len() - 1]);
    assert!(last.0 > first.0 + 0.9, "the clock ran: {first:?} to {last:?}");
    assert!(last.1 < first.1, "the sun climbed (its light points down more)");
}

#[test]
fn holding_the_clock_keeps_the_sun_still() {
    let suns = Rc::new(RefCell::new(Vec::new()));
    let log = suns.clone();
    run(14.0, 30, move |ctx, demo| {
        demo.paused = true;
        log.borrow_mut().push(ctx.scene3d.lighting.sun_direction);
    });
    let suns = suns.borrow();
    assert!(suns[1..].windows(2).all(|w| w[0] == w[1]), "paused, the sun holds");
}

#[test]
fn the_keys_step_the_cloud_cover_and_quality() {
    Game::new("sky kit", 64, 64).run_headless(1, |ctx| {
        scene::setup(ctx, 12.0);
        let kit = |ctx: &Ctx| ctx.scene3d.sky.kit.unwrap();
        assert_eq!(kit(ctx).clouds.coverage, scene::COVERS[1]);
        let covers: Vec<f32> = (0..4)
            .map(|_| {
                scene::next_cover(ctx);
                kit(ctx).clouds.coverage
            })
            .collect();
        assert_eq!(
            covers,
            [scene::COVERS[2], scene::COVERS[3], scene::COVERS[0], scene::COVERS[1]]
        );
        assert_eq!(kit(ctx).quality, CloudQuality::Medium);
        scene::next_quality(ctx);
        assert_eq!(kit(ctx).quality, CloudQuality::High);
        scene::next_quality(ctx);
        assert_eq!(kit(ctx).quality, CloudQuality::Flat, "round to the cheapest");
    });
}
