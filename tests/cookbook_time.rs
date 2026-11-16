//! Cookbook, "Time": delays, repeating timers, tweens, a day and night cycle
//! and slow motion. Every recipe between `ANCHOR` lines is what
//! `book/src/cookbook/time.md` shows, and each test runs it headless for a few
//! ticks and checks that it did its job.
#![allow(clippy::type_complexity)]

use ponko::actions::InputEvent;
use ponko::d2::*;
use ponko::prelude::*;
use ponko::scenario::InputScenario;

fn count<T: Component>(world: &mut World) -> usize {
    world.query::<&T>().count()
}

// ANCHOR: fuse
/// Seconds left until it goes off.
#[derive(Component)]
struct Fuse(f32);

struct Exploded(Vec2);

fn burn_fuses(ctx: &mut Ctx) {
    let dt = ctx.time.dt(); // the length of one simulation tick, in seconds
    ctx.world
        .query::<(Entity, &mut Fuse, &Transform)>()
        .for_each(|(e, fuse, t)| {
            fuse.0 -= dt;
            if fuse.0 <= 0.0 {
                ctx.events.send(Exploded(t.position));
                ctx.commands.despawn(e);
            }
        });
}
// ANCHOR_END: fuse

#[test]
fn a_two_second_fuse_goes_off_once_after_two_seconds() {
    #[derive(Default)]
    struct Booms(Vec<(u64, Vec2)>);
    let world = Game::new("fuse", 640, 480)
        .setup(|ctx| {
            ctx.world.insert_resource(Booms::default());
            ctx.world.spawn((Fuse(2.0), Transform::at(vec2(30.0, 40.0))));
        })
        .sim_system(burn_fuses)
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let seen: Vec<_> = ctx.events.read::<Exploded>().iter().map(|e| (tick, e.0)).collect();
            ctx.world.resource_mut::<Booms>().0.extend(seen);
        })
        .run_headless(200);
    let booms = &world.resource::<Booms>().0;
    assert_eq!(booms.len(), 1, "once");
    assert!(
        (119..=121).contains(&booms[0].0),
        "after 120 ticks: tick {}",
        booms[0].0
    );
    assert_eq!(booms[0].1, vec2(30.0, 40.0));
}

// ANCHOR: repeat
#[derive(Component)]
struct Spawner {
    every: f32,
    left: f32, // seconds until the next one
}

#[derive(Component)]
struct Critter;

fn spawn_on_a_timer(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    ctx.world
        .query::<(&mut Spawner, &Transform)>()
        .for_each(|(spawner, t)| {
            spawner.left -= dt;
            // A `while`, so a slow tick never loses one: the leftover time carries over.
            while spawner.left <= 0.0 {
                spawner.left += spawner.every;
                ctx.commands.spawn((Critter, Transform::at(t.position)));
            }
        });
}
// ANCHOR_END: repeat

#[test]
fn a_spawner_with_a_two_second_period_makes_five_critters_in_ten_seconds() {
    let mut world = Game::new("spawner", 640, 480)
        .setup(|ctx| {
            ctx.world
                .spawn((Spawner { every: 2.0, left: 2.0 }, Transform::at(Vec2::ZERO)));
        })
        .sim_system(spawn_on_a_timer)
        .run_headless(601);
    assert_eq!(count::<Critter>(&mut world), 5);
}

// ANCHOR: tween
/// Slides something from one place to another, slowing down as it arrives.
#[derive(Component)]
struct Slide {
    from: Vec2,
    to: Vec2,
    seconds: f32,
    elapsed: f32,
}

/// 0 to 1, fast at first and gentle at the end. Swap in your favourite curve.
fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn slide(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    ctx.world.query::<(&mut Slide, &mut Transform)>().for_each(|(s, t)| {
        s.elapsed = (s.elapsed + dt).min(s.seconds);
        let progress = ease_out_cubic(s.elapsed / s.seconds);
        t.position = s.from.lerp(s.to, progress);
    });
}
// ANCHOR_END: tween

#[test]
fn a_slide_eases_out_and_ends_exactly_on_the_target() {
    let run = |ticks| {
        let mut world = Game::new("slide", 640, 480)
            .setup(|ctx| {
                let slide = Slide {
                    from: Vec2::ZERO,
                    to: vec2(100.0, 0.0),
                    seconds: 1.0,
                    elapsed: 0.0,
                };
                ctx.world.spawn((slide, Transform::at(Vec2::ZERO)));
            })
            .sim_system(slide)
            .run_headless(ticks);
        world.query::<&Transform>().iter().next().unwrap().position.x
    };
    assert!(run(30) > 85.0, "half the time takes it most of the way: {}", run(30));
    assert!(run(30) < 100.0);
    assert_eq!(run(120), 100.0, "and then it stays put");
}

// ANCHOR: day_night
/// How much light there is, 0 (midnight) to 1 (noon). A day lasts `DAY` seconds.
const DAY: f64 = 120.0;

fn daylight(clock: f64) -> f32 {
    let time_of_day = (clock % DAY) / DAY; // 0 is midnight, 0.5 is noon
    (0.5 - 0.5 * (time_of_day * std::f64::consts::TAU).cos()) as f32
}

/// The colour to tint the world with: deep blue at night, white at noon.
#[derive(Default)]
struct Tint(Color);

fn day_and_night(ctx: &mut Ctx) {
    let light = daylight(ctx.time.sim_time());
    let (night, day) = (Color::rgb(0.1, 0.1, 0.3), Color::WHITE);
    let mix = |a: f32, b: f32| a + (b - a) * light;
    ctx.world.resource_mut::<Tint>().0 = Color::rgb(mix(night.r, day.r), mix(night.g, day.g), mix(night.b, day.b));
}
// ANCHOR_END: day_night

#[test]
fn it_is_dark_at_midnight_bright_at_noon_and_the_day_repeats() {
    assert_eq!(daylight(0.0), 0.0);
    assert!((daylight(60.0) - 1.0).abs() < 1e-6, "noon");
    assert!((daylight(DAY + 60.0) - 1.0).abs() < 1e-6, "the next noon");
    let at = |ticks: u64| {
        let world = Game::new("day", 640, 480)
            .setup(|ctx| ctx.world.insert_resource(Tint::default()))
            .sim_system(day_and_night)
            .run_headless(ticks);
        world.resource::<Tint>().0
    };
    let midnight = at(1);
    let noon = at(3601); // 60 seconds in
    assert!(midnight.b > midnight.r, "night is blue: {midnight:?}");
    assert!(
        noon.r > 0.99 && noon.g > 0.99 && noon.b > 0.99,
        "noon is white: {noon:?}"
    );
}

// ANCHOR: slow_motion
fn slow_motion_while_held(ctx: &mut Ctx) {
    // A quarter speed runs a quarter as many ticks a second; each tick is as
    // long as ever, so the simulation gives the same results, only later.
    let speed = if ctx.input.key_down(Key::Tab) { 0.25 } else { 1.0 };
    ctx.time.set_speed(speed);
}
// ANCHOR_END: slow_motion

#[test]
fn holding_tab_drops_the_speed_to_a_quarter_and_letting_go_restores_it() {
    #[derive(Default)]
    struct Speeds(Vec<f32>);
    let world = Game::new("slow", 640, 480)
        .setup(|ctx| ctx.world.insert_resource(Speeds::default()))
        .sim_system(slow_motion_while_held)
        .sim_system(|ctx: &mut Ctx| {
            let speed = ctx.time.speed();
            ctx.world.resource_mut::<Speeds>().0.push(speed);
        })
        .input_scenario(
            InputScenario::new()
                .at(2, InputEvent::Key(Key::Tab, true))
                .at(4, InputEvent::Key(Key::Tab, false)),
        )
        .run_headless(6);
    assert_eq!(world.resource::<Speeds>().0, [1.0, 1.0, 0.25, 0.25, 1.0, 1.0]);
}
