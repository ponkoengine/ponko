//! Batches of view systems in the windowed frame loop (`run_systems`): the
//! frame, late and draw lists, timings, and what a panic leaves behind.
//! Headless runs are covered in `tests/system_schedule.rs`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;
use crate::access::SystemAccess;
use crate::system_view::SystemView;
use ponko_ecs::Component;

#[derive(Component)]
struct A(u32);
#[derive(Component)]
struct B(u32);
struct Total(u32);
#[derive(Clone, Copy)]
struct Tick(u32);

fn ctx() -> Ctx {
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(800.0, 600.0));
    ctx.world.insert_resource(Total(0));
    ctx.world.spawn((A(0), B(0)));
    ctx
}

/// A game without the built-in frame systems, with every batch on threads.
fn bare() -> Game {
    Game::new("t", 1, 1)
        .parallel_systems(true)
        .parallel_threshold(Duration::ZERO)
        .parallel_systems_env(false)
        .remove(Systems::ANIMATE_SPRITES)
        .remove(Systems::DRAW_TILEMAPS)
        .remove(Systems::DRAW_SPRITES)
        .remove(Systems::DRAW_PARTICLES)
        .remove(Systems::UPDATE_ANIMATORS)
        .remove(Systems::UPDATE_PARTICLES)
        .remove(Systems::UPDATE_SKELETONS2D)
        .remove(Systems::UPDATE_CAMERA_RIGS)
        .remove(Systems::UPDATE_AUDIO3D)
}

fn bump_a(sys: &mut SystemView) {
    sys.world.query::<&mut A>().for_each(|a| a.0 += 1);
}

fn bump_b(sys: &mut SystemView) {
    sys.world.query::<&mut B>().for_each(|b| b.0 += 10);
}

fn values(ctx: &mut Ctx) -> (u32, u32, u32) {
    let (a, b) = (ctx.world.query::<(&A, &B)>().iter())
        .map(|(a, b)| (a.0, b.0))
        .next()
        .unwrap();
    (a, b, ctx.world.resource::<Total>().0)
}

#[test]
fn frame_late_and_draw_phases_run_their_batches() {
    let a = || SystemAccess::new().write::<A>();
    let b = || SystemAccess::new().write::<B>();
    let mut game = bare()
        .add(Phase::AfterPhysics, bump_a.access(a()))
        .add(Phase::AfterPhysics, bump_b.access(b()))
        .add(Phase::Frame, bump_a.access(a()))
        .add(Phase::Frame, bump_b.access(b()))
        .add(Phase::Late, bump_a.access(a()))
        .add(Phase::Late, bump_b.access(b()))
        .add(Phase::Draw, bump_a.access(a()))
        .add(Phase::Draw, bump_b.access(b()));
    let plan = game.schedule_plan();
    assert_eq!(plan.batches().count(), 4);
    let mut ctx = ctx();
    // Two ticks, then one frame: 2 + 3 runs of each system.
    game.run_systems(&mut ctx, 2);
    assert_eq!(values(&mut ctx), (5, 50, 0));
    // Every system is listed and timed once, in registration order.
    let timings = ctx.perf.system_timings();
    assert_eq!(timings.len(), 8);
    assert!(timings.iter().step_by(2).all(|t| t.name.ends_with("bump_a")));
    assert!(timings.iter().take(2).all(|t| t.sim) && timings.iter().skip(2).all(|t| !t.sim));
}

#[test]
fn a_batch_in_a_frame_phase_delivers_events_and_commands_at_its_end() {
    let mut game = bare()
        .add(
            Phase::Frame,
            (|sys: &mut SystemView| sys.events.send(Tick(1)))
                .access(SystemAccess::new().write::<A>().send_events::<Tick>()),
        )
        .add(
            Phase::Frame,
            (|sys: &mut SystemView| {
                sys.events.send(Tick(2));
                sys.commands.spawn((A(100), B(100)));
            })
            .access(SystemAccess::new().write::<B>().send_events::<Tick>().commands()),
        )
        .add(
            Phase::Late,
            (|sys: &mut SystemView| {
                let seen: Vec<u32> = sys.events.read::<Tick>().iter().map(|t| t.0).collect();
                assert_eq!(seen, [1, 2]);
                // The batch's spawn was applied before this phase.
                sys.world.resource_mut::<Total>().0 += sys.world.len() as u32;
            })
            .access(SystemAccess::new().read_events::<Tick>().write_resource::<Total>()),
        );
    let mut ctx = ctx();
    game.run_systems(&mut ctx, 0);
    assert_eq!(ctx.world.resource::<Total>().0, 2);
    assert_eq!(ctx.perf.entities, 2);
}

#[test]
fn the_scheduler_off_runs_view_systems_one_at_a_time() {
    let mut game = bare()
        .parallel_systems(false)
        .add(Phase::Frame, bump_a.access(SystemAccess::new().write::<A>()))
        .add(Phase::Frame, bump_b.access(SystemAccess::new().write::<B>()));
    let mut ctx = ctx();
    game.run_systems(&mut ctx, 0);
    assert_eq!(values(&mut ctx), (1, 10, 0));
    // The plan still shows what would share a batch.
    assert_eq!(game.schedule_plan().batches().count(), 1);
}

#[test]
fn systems_added_after_a_run_join_the_schedule() {
    let mut game = bare().add(Phase::Frame, bump_a.access(SystemAccess::new().write::<A>()));
    let mut ctx = ctx();
    game.run_systems(&mut ctx, 0);
    let mut game = game.add(Phase::Frame, bump_b.access(SystemAccess::new().write::<B>()));
    game.run_systems(&mut ctx, 0);
    assert_eq!(values(&mut ctx), (2, 10, 0));
    let mut game = game.barrier(Phase::Frame).add(Phase::Frame, |ctx: &mut Ctx| {
        ctx.world.resource_mut::<Total>().0 += 1;
    });
    game.run_systems(&mut ctx, 0);
    assert_eq!(values(&mut ctx), (3, 20, 1));
}

#[test]
fn a_panic_in_a_batch_leaves_the_game_and_the_world_whole() {
    let mut game = bare()
        .add(
            Phase::Frame,
            (|sys: &mut SystemView| {
                sys.world.resource_mut::<Total>().0 += 1;
                sys.world.query::<&mut A>().for_each(|a| a.0 += 1);
            })
            .access(SystemAccess::new().write::<A>().write_resource::<Total>()),
        )
        .add(
            Phase::Frame,
            (|sys: &mut SystemView| {
                let mut fail = false;
                sys.world.query::<&mut B>().for_each(|b| {
                    b.0 += 10;
                    fail = b.0 == 20;
                });
                assert!(!fail, "second frame fails");
            })
            .access(SystemAccess::new().write::<B>()),
        );
    let mut ctx = ctx();
    game.run_systems(&mut ctx, 0);
    let failed = catch_unwind(AssertUnwindSafe(|| game.run_systems(&mut ctx, 0)));
    let message = *failed.unwrap_err().downcast::<&str>().unwrap();
    assert_eq!(message, "second frame fails");
    // The lent resource is back, and the systems still run.
    assert!(ctx.world.get_resource::<Total>().is_some());
    game.run_systems(&mut ctx, 0);
    assert_eq!(values(&mut ctx), (3, 30, 3));
}

#[test]
fn a_batch_keeps_its_usual_run_time_for_the_thread_decision() {
    const SLOW_MS: u64 = 200;
    let mut game = bare()
        .parallel_threshold(Duration::MAX)
        .add(
            Phase::Frame,
            (|_: &mut SystemView| {
                std::thread::sleep(Duration::from_millis(SLOW_MS));
            })
            .access(SystemAccess::new().write::<A>()),
        )
        .add(Phase::Frame, bump_b.access(SystemAccess::new().write::<B>()));
    let mut ctx = ctx();
    game.run_systems(&mut ctx, 0);
    let usual = |s: &System| match &s.run {
        Run::View(Some(runner)) => runner.task.usual_us,
        _ => panic!("a view system"),
    };
    // The sleeper's time is a lower bound; the other system's must stay its
    // own. A wide margin keeps a loaded machine's scheduling jitter from
    // mixing the two up: the quick system (microseconds of work) would have
    // to be held up 100 ms, where a thread asleep for 1 ms woke at most
    // 21 ms late at a load average of 166 on 14 cores.
    let slow_us = SLOW_MS as f32 * 1000.0;
    assert!(usual(&game.systems[0]) >= slow_us, "{}", usual(&game.systems[0]));
    assert!(usual(&game.systems[1]) < slow_us / 2.0, "{}", usual(&game.systems[1]));
    // Both were timed, each with its own time.
    let t = ctx.perf.system_timings();
    let slow_ms = SLOW_MS as f32;
    assert!(t[0].last_ms >= slow_ms && t[1].last_ms < slow_ms / 2.0, "{t:?}");
}

#[derive(Component)]
struct Marker;
struct Runs(u32);
struct OwnChanges(u32);

#[test]
fn a_panic_in_a_batch_drops_what_the_batch_recorded_on_threads_and_in_order() {
    let run = |threshold: Duration| {
        let mut game = bare()
            .parallel_threshold(threshold)
            // Sends an event every frame, and notes how many of its own
            // writes it is shown as changes.
            .add(
                Phase::Frame,
                (|sys: &mut SystemView| {
                    let own = sys.world.query::<&A>().filter::<ponko_ecs::Changed<A>>().count() as u32;
                    sys.world.resource_mut::<OwnChanges>().0 = own;
                    sys.events.send(Tick(1));
                    sys.world.query::<&mut A>().for_each(|a| a.0 += 1);
                })
                .access(
                    (SystemAccess::new().write::<A>())
                        .write_resource::<OwnChanges>()
                        .send_events::<Tick>(),
                ),
            )
            // Panics in the second frame.
            .add(
                Phase::Frame,
                (|sys: &mut SystemView| {
                    let mut fail = false;
                    sys.world.query::<&mut B>().for_each(|b| {
                        b.0 += 10;
                        fail = b.0 == 20;
                    });
                    assert!(!fail, "second frame fails");
                })
                .access(SystemAccess::new().write::<B>()),
            )
            // After it in the batch: sends, records a spawn, counts its runs.
            .add(
                Phase::Frame,
                (|sys: &mut SystemView| {
                    sys.world.resource_mut::<Runs>().0 += 1;
                    sys.events.send(Tick(2));
                    sys.commands.spawn((Marker,));
                })
                .access(
                    (SystemAccess::new().send_events::<Tick>())
                        .write_resource::<Runs>()
                        .commands(),
                ),
            )
            // The next stage counts the events it is shown.
            .add(Phase::Late, |ctx: &mut Ctx| {
                let n = ctx.events.read::<Tick>().len() as u32;
                ctx.world.resource_mut::<Total>().0 += n;
            });
        let mut ctx = ctx();
        ctx.world.track_changes::<A>();
        ctx.world.insert_resource(Runs(0));
        ctx.world.insert_resource(OwnChanges(0));
        game.run_systems(&mut ctx, 0);
        let before = ctx.world.change_tick().to_bits();
        let failed = catch_unwind(AssertUnwindSafe(|| game.run_systems(&mut ctx, 0)));
        assert_eq!(*failed.unwrap_err().downcast::<&str>().unwrap(), "second frame fails");
        // The batch's three ticks ended, as if its systems had run alone.
        assert_eq!(ctx.world.change_tick().to_bits(), before + 3);
        // Every system of the batch ran in the failed frame, also the one
        // after the panic, and what they wrote to the world directly stays.
        let after_failure = (values(&mut ctx), ctx.world.resource::<Runs>().0);
        assert_eq!(after_failure, ((2, 20, 2), 2));
        // But the batch had no barrier: its spawn was not applied...
        let markers = |ctx: &mut Ctx| ctx.world.query::<&Marker>().count();
        assert_eq!(markers(&mut ctx), 1);
        game.run_systems(&mut ctx, 0);
        // ...and neither its commands nor its events were left for the
        // next batch to deliver: one spawn and two events per good frame.
        assert_eq!(markers(&mut ctx), 2);
        assert_eq!(ctx.world.resource::<Total>().0, 4);
        // A system that returned in the failed frame isn't shown the
        // writes it made there, as after any run of its own.
        assert_eq!(ctx.world.resource::<OwnChanges>().0, 0);
        (after_failure, values(&mut ctx), ctx.world.change_tick())
    };
    // In order and on threads: the same, to the change tick.
    assert_eq!(run(Duration::MAX), run(Duration::ZERO));
}

#[test]
fn the_environment_variable_overrides_the_builder() {
    use super::batch::resolve;
    let default = crate::schedule::DEFAULT_PARALLEL_THRESHOLD;
    let mine = Duration::from_micros(7);
    assert_eq!(resolve(false, None, None), (false, default));
    assert_eq!(resolve(true, Some(mine), None), (true, mine));
    assert_eq!(resolve(false, Some(mine), Some("1")), (true, mine));
    assert_eq!(resolve(true, None, Some("0")), (false, default));
    assert_eq!(resolve(false, Some(mine), Some("always")), (true, Duration::ZERO));
    // Anything else leaves the builder's settings alone.
    assert_eq!(resolve(true, None, Some("yes")), (true, default));
}

#[test]
fn settings_ignore_the_environment_when_told_to() {
    // Whatever the variable says in this test run.
    let on = super::batch::Settings::new(true, Some(Duration::MAX), false);
    assert!(on.on);
    let off = super::batch::Settings::new(false, None, false);
    assert!(!off.on);
}
