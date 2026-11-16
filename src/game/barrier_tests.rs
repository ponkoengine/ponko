//! The windowed frame loop's command and event barriers, driven without a
//! window: `begin_frame` then `run_systems`, as `window.rs` does each frame.
//! Headless runs are covered in `tests/schedule_barriers.rs`.

use super::*;
use ponko_ecs::Component;

#[derive(Default)]
struct Log(Vec<String>);

#[derive(Clone, Copy)]
struct Ping(u64);

#[derive(Component)]
struct Marker;

fn log(ctx: &mut Ctx, line: String) {
    ctx.world.resource_mut::<Log>().0.push(line);
}

fn take(ctx: &mut Ctx) -> Vec<String> {
    std::mem::take(&mut ctx.world.resource_mut::<Log>().0)
}

fn ctx() -> Ctx {
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(800.0, 600.0));
    ctx.world.insert_resource(Log::default());
    ctx
}

/// A game without the built-in drawing systems.
fn bare(game: Game) -> Game {
    game.remove(Systems::ANIMATE_SPRITES)
        .remove(Systems::DRAW_TILEMAPS)
        .remove(Systems::DRAW_SPRITES)
        .remove(Systems::DRAW_PARTICLES)
        .remove(Systems::UPDATE_ANIMATORS)
        .remove(Systems::UPDATE_PARTICLES)
        .remove(Systems::UPDATE_AUDIO3D)
}

/// One windowed frame running `ticks` simulation ticks.
fn frame(game: &mut Game, ctx: &mut Ctx, ticks: u32) {
    ctx.begin_frame(FrameTiming {
        dt: 1.0 / 60.0,
        seconds: 1.0 / 60.0,
        fixed_steps: ticks,
        alpha: 0.0,
    });
    game.run_systems(ctx, ticks);
}

fn read_pings(ctx: &mut Ctx) -> Vec<u64> {
    ctx.events.read::<Ping>().iter().map(|p| p.0).collect()
}

#[test]
fn frame_systems_see_events_and_commands_from_every_tick_of_their_frame() {
    let mut game = bare(Game::new("t", 1, 1))
        .sim_system(|ctx: &mut Ctx| {
            ctx.events.send(Ping(ctx.time.sim_tick()));
            ctx.commands.spawn((Marker,));
        })
        .system(|ctx: &mut Ctx| {
            let pings = read_pings(ctx);
            let n = ctx.world.query::<&Marker>().count();
            log(ctx, format!("frame {pings:?} markers={n}"));
        });
    let mut ctx = ctx();
    frame(&mut game, &mut ctx, 3);
    frame(&mut game, &mut ctx, 0);
    frame(&mut game, &mut ctx, 2);
    assert_eq!(
        take(&mut ctx),
        [
            "frame [0, 1, 2] markers=3",
            "frame [] markers=3",
            "frame [3, 4] markers=5"
        ]
    );
}

#[test]
fn sim_systems_see_frame_events_on_the_next_frames_first_tick() {
    let mut game = bare(Game::new("t", 1, 1))
        .sim_system(|ctx: &mut Ctx| {
            let pings = read_pings(ctx);
            log(ctx, format!("tick {} {pings:?}", ctx.time.sim_tick()));
        })
        .system(|ctx: &mut Ctx| {
            // Stamped with the next tick: the frame's ticks already ran.
            ctx.events.send(Ping(100 + ctx.time.sim_tick()));
        });
    let mut ctx = ctx();
    frame(&mut game, &mut ctx, 2);
    frame(&mut game, &mut ctx, 2);
    assert_eq!(take(&mut ctx), ["tick 0 []", "tick 1 []", "tick 2 [102]", "tick 3 []"]);
}

#[test]
fn frame_events_wait_for_sim_readers_while_the_sim_is_paused() {
    // A frame system (a pause menu) sends while no ticks run. The sim reader
    // still gets every event on resume: events expire only once the tick
    // counter has moved past them, so they accumulate while paused.
    let mut game = bare(Game::new("t", 1, 1))
        .sim_system(|ctx: &mut Ctx| {
            let pings = read_pings(ctx);
            log(ctx, format!("tick {} {pings:?}", ctx.time.sim_tick()));
        })
        .system(|ctx: &mut Ctx| ctx.events.send(Ping(ctx.time.frame())));
    let mut ctx = ctx();
    frame(&mut game, &mut ctx, 1);
    for _ in 0..5 {
        frame(&mut game, &mut ctx, 0);
    }
    assert_eq!(ctx.events.peek::<Ping>().len(), 6, "nothing expires while paused");
    frame(&mut game, &mut ctx, 1);
    assert_eq!(take(&mut ctx), ["tick 0 []", "tick 1 [1, 2, 3, 4, 5, 6]"]);
    for _ in 0..3 {
        frame(&mut game, &mut ctx, 1);
    }
    // Kept: the sends of the last three frames (stamped with ticks 3, 4, 5).
    assert_eq!(ctx.events.peek::<Ping>().len(), 3, "expired once ticks moved on");
}

#[test]
fn sim_events_live_at_least_one_whole_frame_for_frame_readers() {
    let mut game = bare(Game::new("t", 1, 1))
        .sim_system(|ctx: &mut Ctx| ctx.events.send(Ping(ctx.time.sim_tick())))
        .system(|ctx: &mut Ctx| {
            let kept = ctx.events.peek::<Ping>().len();
            log(ctx, format!("kept {kept}"));
        });
    let mut ctx = ctx();
    // Ten ticks in one frame: all are kept through that frame and the next,
    // then only the last tick's (not yet older than `keep_ticks`) survives.
    frame(&mut game, &mut ctx, 10);
    frame(&mut game, &mut ctx, 0);
    frame(&mut game, &mut ctx, 0);
    assert_eq!(take(&mut ctx), ["kept 10", "kept 10", "kept 1"]);
    frame(&mut game, &mut ctx, 3);
    assert_eq!(take(&mut ctx), ["kept 4"]);
}

#[test]
fn commands_from_frame_systems_apply_before_late_and_draw_phases() {
    let mut game = bare(Game::new("t", 1, 1))
        .add(Phase::Draw, |ctx: &mut Ctx| {
            let n = ctx.world.query::<&Marker>().count();
            log(ctx, format!("draw {n}"));
        })
        .add(Phase::Late, |ctx: &mut Ctx| {
            let n = ctx.world.query::<&Marker>().count();
            log(ctx, format!("late {n}"));
            ctx.commands.spawn((Marker,));
        })
        .add(Phase::Animate, |ctx: &mut Ctx| {
            let n = ctx.world.query::<&Marker>().count();
            log(ctx, format!("animate {n}"));
            ctx.commands.spawn((Marker,));
        })
        .add(Phase::Frame, |ctx: &mut Ctx| {
            ctx.commands.spawn((Marker,));
        });
    let mut ctx = ctx();
    frame(&mut game, &mut ctx, 0);
    assert_eq!(take(&mut ctx), ["animate 1", "late 2", "draw 3"]);
}

// ---- change detection -------------------------------------------------------------

use ponko_ecs::{Changed, Entity, Tick, With};

#[derive(Component)]
struct Hp;

fn changed_hp(ctx: &mut Ctx) -> usize {
    ctx.world.query::<&Hp>().filter::<Changed<Hp>>().count()
}

#[test]
fn frame_systems_see_the_changes_of_every_tick_since_their_last_frame() {
    let mut game = bare(Game::new("t", 1, 1))
        .sim_system(|ctx: &mut Ctx| {
            // One new entity per tick; frame systems change nothing.
            ctx.world.spawn((Hp,));
            let n = changed_hp(ctx);
            log(ctx, format!("sim {n}"));
        })
        .system(|ctx: &mut Ctx| {
            let n = changed_hp(ctx);
            log(ctx, format!("frame {n}"));
        })
        .add(Phase::Late, |ctx: &mut Ctx| {
            let removed = ctx.world.removed::<Hp>().len();
            let first = ctx.world.query::<Entity>().filter::<With<Hp>>().iter().next();
            if let Some(e) = first {
                ctx.commands.despawn(e);
            }
            log(ctx, format!("late removed {removed}"));
        });
    let mut ctx = ctx();
    ctx.world.track_changes::<Hp>();
    frame(&mut game, &mut ctx, 3);
    frame(&mut game, &mut ctx, 0);
    frame(&mut game, &mut ctx, 1);
    assert_eq!(
        take(&mut ctx),
        [
            // A sim system sees what it changed earlier in the same run,
            // but not its own spawn from the tick before.
            "sim 1",
            "sim 1",
            "sim 1",
            // The frame system sees all three ticks' spawns at once.
            "frame 3",
            "late removed 0",
            "frame 0",
            // The late system's own despawn isn't reported back to it.
            "late removed 0",
            "sim 1",
            "frame 1",
            "late removed 0",
        ]
    );
    // Seen by every system (the sim system last, in the third frame's tick),
    // the first two removals were dropped; the last one is still held for
    // the systems that haven't run since.
    assert_eq!(ctx.world.removed_since::<Hp>(Tick::default()).len(), 1);
}
