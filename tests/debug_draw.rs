//! Debug drawing (`ctx.debug`) in headless runs: how long shapes stay when
//! simulation and frame systems draw them, view systems drawing through
//! `sys.debug` (also in parallel batches), and turning it off.
use ponko::access::SystemAccess;
use ponko::debug::DebugStats;
use ponko::prelude::*;
use ponko::system_view::SystemView;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

type Seen = Rc<RefCell<Vec<DebugStats>>>;

/// A draw-phase system that records what debug drawing shows.
fn record(seen: &Seen) -> impl FnMut(&mut Ctx) + 'static {
    let seen = seen.clone();
    move |ctx: &mut Ctx| seen.borrow_mut().push(ctx.debug.stats())
}

#[test]
fn a_sim_systems_shapes_are_shown_by_the_frames_after_its_tick() {
    let seen = Seen::default();
    Game::new("debug", 64, 64)
        .debug_draw(true)
        .headless_frames(true)
        .sim_system(|ctx: &mut Ctx| {
            // Draws on even ticks only.
            if ctx.time.sim_tick().is_multiple_of(2) {
                ctx.debug.line(Vec3::ZERO, Vec3::X, Color::RED);
            }
        })
        .add(Phase::Draw, record(&seen))
        .run_headless(4, |_| {});
    let lines: Vec<usize> = seen.borrow().iter().map(|s| s.lines_3d).collect();
    assert_eq!(lines, [1, 0, 1, 0]);
}

#[test]
fn a_frame_systems_shapes_last_one_frame_and_timed_ones_their_seconds() {
    let seen = Seen::default();
    let mut frame = 0;
    Game::new("debug", 64, 64)
        .debug_draw(true)
        .headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            if frame == 0 {
                ctx.debug.circle(vec2(0.0, 0.0), 10.0, Color::GREEN);
                // A long time: every headless frame below is within it.
                ctx.debug.text(vec2(0.0, 0.0), "hello", Color::WHITE).seconds(3600.0);
            }
            frame += 1;
        })
        .add(Phase::Draw, record(&seen))
        .run_headless(3, |_| {});
    let seen = seen.borrow();
    assert_eq!(seen[0].lines_2d, 32);
    assert_eq!((seen[1].lines_2d, seen[1].labels), (0, 1));
    assert_eq!(seen[2].labels, 1);
}

fn draw_lines(n: usize) -> impl FnMut(&mut SystemView<'_>) + Send + 'static {
    move |sys: &mut SystemView<'_>| {
        for i in 0..n {
            sys.debug.line(Vec3::ZERO, Vec3::splat(i as f32), Color::BLUE);
        }
    }
}

#[derive(Component)]
struct A;
#[derive(Component)]
struct B;

#[test]
fn view_systems_draw_through_sys_debug_alone_and_in_parallel_batches() {
    for threshold in [Duration::MAX, Duration::ZERO] {
        let seen = Seen::default();
        Game::new("debug", 64, 64)
            .debug_draw(true)
            .headless_frames(true)
            .parallel_systems_env(false)
            .parallel_systems(true)
            .parallel_threshold(threshold)
            // Two systems with no conflicting access: one batch.
            .add(Phase::Frame, draw_lines(3).access(SystemAccess::new().read::<A>()))
            .add(Phase::Frame, draw_lines(4).access(SystemAccess::new().read::<B>()))
            .add(Phase::Draw, record(&seen))
            .add(Phase::Draw, |ctx: &mut Ctx| {
                let d = ctx.ecs_diagnostics();
                assert!(
                    d.schedule.stages.iter().any(|s| s.batch && s.len == 2),
                    "the two drawing systems share a batch"
                );
            })
            .run_headless(2, |_| {});
        let lines: Vec<usize> = seen.borrow().iter().map(|s| s.lines_3d).collect();
        assert_eq!(lines, [7, 7], "threshold {threshold:?}");
    }
}

#[test]
fn turned_off_debug_drawing_records_nothing_from_any_system() {
    let seen = Seen::default();
    Game::new("debug", 64, 64)
        .debug_draw(false)
        .headless_frames(true)
        .sim_system(|ctx: &mut Ctx| {
            ctx.debug.sphere(Vec3::ZERO, 1.0, Color::RED).seconds(10.0);
        })
        .add(Phase::Frame, draw_lines(5).access(SystemAccess::new().read::<A>()))
        .add(Phase::Draw, record(&seen))
        .run_headless(2, |_| {});
    assert!(seen.borrow().iter().all(|s| *s == DebugStats::default()));
}

#[test]
fn a_system_can_turn_debug_drawing_on_at_runtime() {
    let seen = Seen::default();
    Game::new("debug", 64, 64)
        .debug_draw(false)
        .headless_frames(true)
        .system(|ctx: &mut Ctx| {
            if ctx.time.frame() == 2 {
                ctx.debug.set_enabled(true);
            }
            ctx.debug.line(vec2(0.0, 0.0), vec2(1.0, 1.0), Color::RED);
        })
        .add(Phase::Draw, record(&seen))
        .run_headless(3, |_| {});
    let lines: Vec<usize> = seen.borrow().iter().map(|s| s.lines_2d).collect();
    assert_eq!(lines, [0, 1, 1]);
}
