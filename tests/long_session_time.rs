//! Ten simulated hours, headless: the simulation clock counts every tick,
//! `sim_time` is the tick count times the step to the nanosecond, the frame
//! clock agrees with it, and a sprite animation still changes frame on
//! schedule. The limits for longer sessions are in the guide ("Very long
//! sessions" in `book/src/sim-clock.md`).

use std::cell::Cell;
use std::rc::Rc;

use ponko::prelude::*;
use ponko::sheet::Animation;

const HOURS: u64 = 10;

#[test]
fn ten_simulated_hours_keep_the_clocks_exact() {
    let ticks = HOURS * 3600 * 60;
    let end = Rc::new(Cell::new((0u64, 0.0f64, 0.0f64, 0.0f32)));
    let seen = end.clone();
    Game::new("ten hours", 1, 1)
        .fixed_hz(60)
        .sim_system(move |ctx: &mut Ctx| {
            let t = &ctx.time;
            seen.set((t.sim_tick(), t.sim_time(), t.elapsed(), t.sim_dt()));
        })
        .run_headless(ticks, |_| {});
    let (tick, sim_time, elapsed, sim_dt) = end.get();
    // The last tick seen is the one before `ticks` (they count from 0).
    assert_eq!(tick, ticks - 1);
    let expected = (ticks - 1) as f64 / 60.0;
    assert!(
        (sim_time - expected).abs() < 1e-9,
        "sim_time {sim_time}, expected {expected}"
    );
    // One frame per tick headless: the frame clock is the simulation's,
    // one step ahead (frames count from 1).
    assert!(
        (elapsed - (expected + 1.0 / 60.0)).abs() < 1e-6,
        "elapsed {elapsed}, sim_time {sim_time}"
    );
    assert_eq!(sim_dt, 1.0 / 60.0, "the step never changes");
}

#[test]
fn a_sprite_animation_changes_frame_on_schedule_after_ten_hours() {
    let frames = Rc::new(Cell::new(Vec::new()));
    let log = frames.clone();
    let ticks = HOURS * 3600 * 60;
    Game::new("animation", 1, 1)
        .fixed_hz(60)
        .sim_system(ponko::systems::animate_sprites)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() + 120 >= ticks {
                let mut seen = log.take();
                seen.extend(ctx.world.query::<&Animation>().iter().map(|a| a.current_frame()));
                log.set(seen);
            }
        })
        .run_headless(ticks, |ctx| {
            let texture = ctx.gfx.create_texture(64, 16, &[255; 64 * 16 * 4]);
            let sheet = SpriteSheet::new(texture, 16, 16);
            ctx.world
                .spawn((Sprite::new(texture), Animation::new(sheet, 0..4, 10.0)));
        });
    // 10 frames a second at 60 ticks a second: the frame changes every 6
    // ticks, 20 times in the last two seconds (19 or 20 by where they start).
    let frames = frames.take();
    assert_eq!(frames.len(), 120);
    let changes = frames.windows(2).filter(|w| w[0] != w[1]).count();
    assert!((19..=20).contains(&changes), "{changes} changes: {frames:?}");
    let runs: Vec<usize> = frames.chunk_by(|a, b| a == b).map(<[u32]>::len).collect();
    assert!(
        runs[1..runs.len() - 1].iter().all(|&n| n == 6),
        "each frame shows for 6 ticks: {runs:?}"
    );
}
