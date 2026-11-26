//! Cookbook, "Fighting and collecting": recipes for shooting, pickups,
//! damage, health bars, waves and enemy behaviour. Every recipe between
//! `ANCHOR` lines is what `book/src/cookbook/gameplay.md` shows, and each test
//! runs it headless for a few ticks and checks that it did its job.
#![allow(clippy::type_complexity)]

use ponko::actions::InputEvent;
use ponko::d2::*;
use ponko::prelude::*;
use ponko::scenario::InputScenario;

fn hold(scenario: InputScenario, key: Key, from: u64, to: u64) -> InputScenario {
    scenario
        .at(from, InputEvent::Key(key, true))
        .at(to, InputEvent::Key(key, false))
}

fn count<T: Component>(world: &mut World) -> usize {
    world.query::<&T>().count()
}

#[derive(Component)]
struct Player;

// ANCHOR: shoot
#[derive(Component)]
struct Gun {
    ready_at: f64, // simulation time of the next shot
}

#[derive(Component)]
struct Bullet {
    velocity: Vec2,
    life: f32, // seconds left
}

fn shoot(ctx: &mut Ctx) {
    let now = ctx.time.sim_time();
    let firing = ctx.input.key_down(Key::Space);
    ctx.world.query::<(&mut Gun, &Transform)>().for_each(|(gun, t)| {
        if firing && now >= gun.ready_at {
            gun.ready_at = now + 0.25; // four shots a second
            let aim = Vec2::from_angle(t.rotation);
            // Commands wait until the system is done, and `spawn` hands back the entity.
            ctx.commands.spawn((
                Transform::at(t.position + aim * 20.0),
                Bullet {
                    velocity: aim * 600.0,
                    life: 0.5,
                },
            ));
        }
    });
}
// ANCHOR_END: shoot

// ANCHOR: bullet_life
fn fly_bullets(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    ctx.world
        .query::<(Entity, &mut Bullet, &mut Transform)>()
        .for_each(|(e, bullet, t)| {
            t.position += bullet.velocity * dt;
            bullet.life -= dt;
            if bullet.life <= 0.0 {
                ctx.commands.despawn(e); // gone at the end of this system
            }
        });
}
// ANCHOR_END: bullet_life

#[test]
fn holding_fire_shoots_four_bullets_a_second_in_the_aimed_direction() {
    let mut world = Game::new("shoot", 640, 480)
        .setup(|ctx| {
            ctx.world
                .spawn((Player, Gun { ready_at: 0.0 }, Transform::at(vec2(100.0, 100.0))));
        })
        .sim_system(shoot)
        .input_scenario(hold(InputScenario::new(), Key::Space, 0, 55))
        .run_headless(60);
    assert_eq!(count::<Bullet>(&mut world), 4, "at 0, 0.25, 0.5 and 0.75 s");
    let first = world
        .query::<(&Bullet, &Transform)>()
        .iter()
        .map(|(b, t)| (b.velocity, t.position))
        .next()
        .unwrap();
    assert_eq!(first.0, vec2(600.0, 0.0));
    assert_eq!(first.1, vec2(120.0, 100.0), "20 pixels in front of the gun");
}

#[test]
fn bullets_fly_and_vanish_after_half_a_second() {
    let mut world = Game::new("bullets", 640, 480)
        .setup(|ctx| {
            ctx.world.spawn((
                Bullet {
                    velocity: vec2(600.0, 0.0),
                    life: 0.5,
                },
                Transform::at(Vec2::ZERO),
            ));
        })
        .sim_system(fly_bullets)
        .run_headless(15);
    let at = world.query::<(&Bullet, &Transform)>().iter().next().unwrap().1.position;
    assert!(
        (at.x - 150.0).abs() < 1.0,
        "a quarter second at 600 pixels per second: {at}"
    );
    let mut world = Game::new("bullets", 640, 480)
        .setup(|ctx| {
            ctx.world.spawn((
                Bullet {
                    velocity: vec2(600.0, 0.0),
                    life: 0.5,
                },
                Transform::at(Vec2::ZERO),
            ));
        })
        .sim_system(fly_bullets)
        .run_headless(40);
    assert_eq!(count::<Bullet>(&mut world), 0, "the bullet's time ran out");
}

// ANCHOR: pickup
#[derive(Component)]
struct Coin;

#[derive(Default)]
struct Coins(u32);

// Turned on with `Game::spatial_grid(32.0)`: the engine files every entity with
// a `Transform` into `ctx.spatial` at the start of each tick.
fn collect_coins(ctx: &mut Ctx) {
    let Some(at) = ctx
        .world
        .query::<(&Player, &Transform)>()
        .iter()
        .next()
        .map(|(_, t)| t.position)
    else {
        return;
    };
    let mut near = Vec::new();
    ctx.spatial
        .for_each_in_radius(at, 24.0, |entry| near.push(entry.entity));
    for e in near {
        if ctx.world.has::<Coin>(e) {
            ctx.commands.despawn(e);
            ctx.world.resource_mut::<Coins>().0 += 1;
        }
    }
}
// ANCHOR_END: pickup

#[test]
fn walking_over_a_coin_picks_it_up_and_far_coins_stay() {
    let mut world = Game::new("coins", 640, 480)
        .spatial_grid(32.0)
        .setup(|ctx| {
            ctx.world.insert_resource(Coins::default());
            ctx.world.spawn((Player, Transform::at(vec2(100.0, 100.0))));
            ctx.world.spawn((Coin, Transform::at(vec2(110.0, 100.0)))); // close
            ctx.world.spawn((Coin, Transform::at(vec2(300.0, 100.0)))); // far
        })
        .sim_system(collect_coins)
        .run_headless(3);
    assert_eq!(world.resource::<Coins>().0, 1);
    assert_eq!(count::<Coin>(&mut world), 1, "the far one is still there");
}

// ANCHOR: damage
#[derive(Component)]
struct Health {
    hp: i32,
    max: i32,
}

/// Anything that hurts something sends one of these.
struct Damage {
    target: Entity,
    amount: i32,
}

/// Sent once when something runs out of health.
struct Died(Entity);

fn apply_damage(ctx: &mut Ctx) {
    for hit in ctx.events.read::<Damage>() {
        if let Some(health) = ctx.world.get_mut::<Health>(hit.target) {
            health.hp = (health.hp - hit.amount).max(0);
        }
    }
}

fn deaths(ctx: &mut Ctx) {
    ctx.world.query::<(Entity, &Health)>().for_each(|(e, health)| {
        if health.hp == 0 {
            ctx.events.send(Died(e)); // for sounds, score, loot...
            ctx.commands.despawn(e);
        }
    });
}
// ANCHOR_END: damage

#[test]
fn damage_lowers_health_and_the_last_point_removes_the_entity() {
    #[derive(Default)]
    struct Funerals(Vec<Entity>);
    let mut world = Game::new("damage", 640, 480)
        .setup(|ctx| {
            ctx.world.insert_resource(Funerals::default());
            let tough = ctx.world.spawn((Health { hp: 10, max: 10 },));
            let weak = ctx.world.spawn((Health { hp: 3, max: 3 },));
            ctx.events.send(Damage {
                target: tough,
                amount: 4,
            });
            ctx.events.send(Damage {
                target: weak,
                amount: 99,
            }); // overkill stops at zero
        })
        .sim_system(apply_damage)
        .sim_system(deaths)
        .sim_system(|ctx: &mut Ctx| {
            let dead: Vec<Entity> = ctx.events.read::<Died>().iter().map(|d| d.0).collect();
            ctx.world.resource_mut::<Funerals>().0.extend(dead);
        })
        .run_headless(3);
    let left: Vec<i32> = world.query::<&Health>().iter().map(|h| h.hp).collect();
    assert_eq!(left, [6], "the tough one lost four points");
    assert_eq!(world.resource::<Funerals>().0.len(), 1, "one death was announced, once");
}

// ANCHOR: invincibility
const INVINCIBLE_SECS: f32 = 1.0; // how long a hit protects
const BLINK_SECS: f32 = 0.1; // how long each blink phase lasts

/// On an entity that was just hurt: the seconds of protection left.
#[derive(Component)]
struct Invincible {
    left: f32,
}

/// Like `apply_damage`, but a protected entity ignores the hit, and a hit that lands protects.
fn hurt(ctx: &mut Ctx) {
    for hit in ctx.events.read::<Damage>() {
        if ctx.world.get::<Invincible>(hit.target).is_some() {
            continue; // still protected
        }
        let Some(health) = ctx.world.get_mut::<Health>(hit.target) else {
            continue;
        };
        health.hp = (health.hp - hit.amount).max(0);
        ctx.world.insert(hit.target, Invincible { left: INVINCIBLE_SECS });
    }
}

/// Counts the protection down and blinks the sprite while it lasts.
fn blink(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    ctx.world
        .query::<(Entity, &mut Invincible, &mut Sprite)>()
        .for_each(|(e, protection, sprite)| {
            protection.left -= dt;
            if protection.left <= 0.0 {
                sprite.color.a = 1.0;
                ctx.commands.remove::<Invincible>(e); // hurtable again
            } else {
                // Faint in every other phase of BLINK_SECS.
                let phase = (protection.left / BLINK_SECS) as u32;
                sprite.color.a = if phase.is_multiple_of(2) { 1.0 } else { 0.25 };
            }
        });
}
// ANCHOR_END: invincibility

#[test]
fn a_hit_protects_for_a_second_the_sprite_blinks_and_the_next_hit_lands_after() {
    #[derive(Default)]
    struct Seen {
        hp: Vec<i32>,
        alpha: Vec<f32>,
        protected: Vec<bool>,
    }
    let world = Game::new("invincibility", 640, 480)
        .setup(|ctx| {
            ctx.world.insert_resource(Seen::default());
            ctx.world
                .spawn((Health { hp: 10, max: 10 }, Sprite::new(Texture::NONE)));
        })
        .sim_system(|ctx: &mut Ctx| {
            // Three hits: at the start, while protected, and after it ended.
            if [0, 10, 80].contains(&ctx.time.sim_tick()) {
                let target = ctx.world.query::<Entity>().iter().next().unwrap();
                ctx.events.send(Damage { target, amount: 3 });
            }
        })
        .sim_system(hurt)
        .sim_system(blink)
        .sim_system(|ctx: &mut Ctx| {
            let mut now = (0, 0.0);
            ctx.world.query::<(&Health, &Sprite)>().for_each(|(health, sprite)| {
                now = (health.hp, sprite.color.a);
            });
            let (hp, alpha) = now;
            let protected = ctx.world.query::<&Invincible>().count() == 1;
            let seen = ctx.world.resource_mut::<Seen>();
            seen.hp.push(hp);
            seen.alpha.push(alpha);
            seen.protected.push(protected);
        })
        .run_headless(100);
    let seen = world.resource::<Seen>();
    assert_eq!(seen.hp[5], 7, "the first hit landed");
    assert_eq!(seen.hp[40], 7, "the hit during the protection was ignored");
    assert_eq!(seen.hp[99], 4, "the hit after it ended landed, and nothing else did");
    assert!(
        seen.protected[5] && !seen.protected[70],
        "protected for about a second, then not"
    );
    let blinking = &seen.alpha[1..50];
    assert!(
        blinking.contains(&1.0) && blinking.contains(&0.25),
        "the sprite alternates while it lasts"
    );
    let flips = blinking.windows(2).filter(|w| w[0] != w[1]).count();
    assert!(flips >= 6, "it blinks every few ticks, not once: {flips} flips");
    assert!(seen.alpha[70] == 1.0, "back to normal when it ends");
}

// ANCHOR: health_bar
/// The empty bar and the full part of it, above a unit at `at`.
fn bar_rects(at: Vec2, health: &Health) -> (Rect, Rect) {
    let width = 40.0;
    let corner = at + vec2(-width / 2.0, -30.0);
    let full = width * health.hp as f32 / health.max as f32;
    (
        Rect::new(corner.x, corner.y, width, 5.0),
        Rect::new(corner.x, corner.y, full, 5.0),
    )
}

/// A frame system: bars are drawn in world units, so they follow the unit and the camera.
fn draw_health_bars(ctx: &mut Ctx) {
    ctx.world.query::<(&Transform, &Health)>().for_each(|(t, health)| {
        let (back, front) = bar_rects(t.position, health);
        ctx.gfx.draw_rect(back, Color::hex(0x402020));
        ctx.gfx.draw_rect(front, Color::hex(0x40c040));
    });
}
// ANCHOR_END: health_bar

#[test]
fn the_bar_shrinks_with_health_and_the_system_draws_in_a_frame() {
    let (back, front) = bar_rects(vec2(100.0, 100.0), &Health { hp: 3, max: 4 });
    assert_eq!((back.max.x - back.min.x, front.max.x - front.min.x), (40.0, 30.0));
    assert_eq!(front.min, vec2(80.0, 70.0));
    // Frames on: the drawing system runs headless too (the draw calls are dropped).
    Game::new("bar", 640, 480)
        .headless_frames(true)
        .setup(|ctx| {
            ctx.world
                .spawn((Health { hp: 1, max: 4 }, Transform::at(vec2(100.0, 100.0))));
        })
        .system(draw_health_bars)
        .run_headless(3);
}

// ANCHOR: waves
#[derive(Component)]
struct Enemy;

#[derive(Default)]
struct Wave(u32);

/// When the field is clear, the next wave comes: one more enemy each time.
fn spawn_waves(ctx: &mut Ctx) {
    if ctx.world.query::<&Enemy>().count() > 0 {
        return;
    }
    let wave = ctx.world.resource_mut::<Wave>();
    wave.0 += 1;
    let number = wave.0;
    let centre = ctx.window_size() / 2.0;
    let mut random = ctx.random().stream(number as u64); // the same waves in every run
    for _ in 0..number + 2 {
        let edge = centre + random.unit_vec2() * 300.0; // on a ring around the screen
        ctx.commands.spawn((Enemy, Transform::at(edge)));
    }
}
// ANCHOR_END: waves

#[test]
fn each_cleared_wave_brings_one_more_enemy() {
    let mut world = Game::new("waves", 640, 480)
        .setup(|ctx| ctx.world.insert_resource(Wave::default()))
        .sim_system(spawn_waves)
        // The player clears the field on ticks 5 and 10.
        .sim_system(|ctx: &mut Ctx| {
            if [5, 10].contains(&ctx.time.sim_tick()) {
                let all: Vec<Entity> = ctx.world.query::<(Entity, &Enemy)>().iter().map(|(e, _)| e).collect();
                all.into_iter().for_each(|e| {
                    ctx.world.despawn(e);
                });
            }
        })
        .run_headless(12);
    assert_eq!(world.resource::<Wave>().0, 3);
    assert_eq!(count::<Enemy>(&mut world), 5, "wave 3 has 3 + 2 enemies");
}

// ANCHOR: chase
fn chase_the_player(ctx: &mut Ctx) {
    let Some(target) = ctx
        .world
        .query::<(&Player, &Transform)>()
        .iter()
        .next()
        .map(|(_, t)| t.position)
    else {
        return;
    };
    let step = 80.0 * ctx.time.dt();
    ctx.world.query::<(&Enemy, &mut Transform)>().for_each(|(_, t)| {
        let to = target - t.position;
        if to.length() > 24.0 {
            t.position += to.normalize() * step; // stop at arm's length
        }
    });
}
// ANCHOR_END: chase

#[test]
fn enemies_close_in_and_stop_at_arms_length() {
    let mut world = Game::new("chase", 640, 480)
        .setup(|ctx| {
            ctx.world.spawn((Player, Transform::at(vec2(200.0, 200.0))));
            ctx.world.spawn((Enemy, Transform::at(vec2(200.0, 300.0))));
        })
        .sim_system(chase_the_player)
        .run_headless(300);
    let at = world.query::<(&Enemy, &Transform)>().iter().next().unwrap().1.position;
    assert!((at.distance(vec2(200.0, 200.0)) - 24.0).abs() < 2.0, "{at}");
}

// ANCHOR: patrol
#[derive(Component)]
enum Guard {
    Patrol { a: Vec2, b: Vec2, going_to_b: bool },
    Chase,
}

fn guard_ai(ctx: &mut Ctx) {
    let Some(player) = ctx
        .world
        .query::<(&Player, &Transform)>()
        .iter()
        .next()
        .map(|(_, t)| t.position)
    else {
        return;
    };
    let step = 60.0 * ctx.time.dt();
    ctx.world
        .query::<(&mut Guard, &mut Transform)>()
        .for_each(|(guard, t)| {
            let near = t.position.distance(player) < 100.0;
            match guard {
                Guard::Patrol { .. } if near => *guard = Guard::Chase,
                Guard::Chase if !near => {
                    *guard = Guard::Patrol {
                        a: t.position,
                        b: t.position + vec2(150.0, 0.0),
                        going_to_b: true,
                    }
                }
                Guard::Patrol { a, b, going_to_b } => {
                    let goal = if *going_to_b { *b } else { *a };
                    if t.position.distance(goal) <= step {
                        *going_to_b = !*going_to_b; // turn round at the end
                    }
                    t.position += (goal - t.position).normalize_or_zero() * step;
                }
                Guard::Chase => t.position += (player - t.position).normalize_or_zero() * step * 1.5,
            }
        });
}
// ANCHOR_END: patrol

#[test]
fn a_guard_walks_its_beat_then_chases_the_player_who_comes_near() {
    let guard = || Guard::Patrol {
        a: vec2(0.0, 0.0),
        b: vec2(150.0, 0.0),
        going_to_b: true,
    };
    // Far away: the guard walks to the end of its beat and turns back.
    let mut world = Game::new("guard", 640, 480)
        .setup(move |ctx| {
            ctx.world.spawn((Player, Transform::at(vec2(0.0, 1000.0))));
            ctx.world.spawn((guard(), Transform::at(Vec2::ZERO)));
        })
        .sim_system(guard_ai)
        .run_headless(200);
    let x = world
        .query::<(&Guard, &Transform)>()
        .iter()
        .next()
        .unwrap()
        .1
        .position
        .x;
    assert!(
        (0.0..=150.5).contains(&x) && x < 140.0,
        "turned back from the end after 150 pixels: {x}"
    );
    assert!(matches!(
        world.query::<&Guard>().iter().next().unwrap(),
        Guard::Patrol { .. }
    ));
    // A player within 100 pixels: the chase starts and closes in.
    let mut world = Game::new("guard", 640, 480)
        .setup(move |ctx| {
            ctx.world.spawn((Player, Transform::at(vec2(0.0, 90.0))));
            ctx.world.spawn((guard(), Transform::at(Vec2::ZERO)));
        })
        .sim_system(guard_ai)
        .run_headless(30);
    assert!(matches!(world.query::<&Guard>().iter().next().unwrap(), Guard::Chase));
    let y = world
        .query::<(&Guard, &Transform)>()
        .iter()
        .next()
        .unwrap()
        .1
        .position
        .y;
    assert!(y > 20.0, "walking towards the player: {y}");
}

// ANCHOR: loot
#[derive(Component)]
struct Drop;

/// Each death may leave a coin behind: 30 per cent of the time.
fn drop_loot(ctx: &mut Ctx) {
    let tick = ctx.time.sim_tick();
    let mut random = ctx.random().stream(tick);
    let spots: Vec<Vec2> = ctx
        .events
        .read::<Died>()
        .iter()
        .filter_map(|died| ctx.world.get::<Transform>(died.0).map(|t| t.position))
        .collect();
    for at in spots {
        if random.chance(0.3) {
            ctx.commands.spawn((Drop, Transform::at(at)));
        }
    }
}
// ANCHOR_END: loot

#[test]
fn about_three_in_ten_deaths_leave_loot_and_the_same_seed_the_same_loot() {
    let run = || {
        let mut world = Game::new("loot", 640, 480)
            .seed(7)
            .setup(|ctx| {
                for i in 0..400 {
                    let e = ctx.world.spawn((Transform::at(vec2(i as f32, 0.0)),));
                    ctx.events.send(Died(e));
                }
            })
            .sim_system(drop_loot)
            .run_headless(2);
        count::<Drop>(&mut world)
    };
    let drops = run();
    assert!((80..160).contains(&drops), "30% of 400 is 120, got {drops}");
    assert_eq!(drops, run());
}
