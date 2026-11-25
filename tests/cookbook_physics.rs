//! Cookbook, "Bodies and collisions" (2D physics): damage on contact, a
//! knockback push, line of sight, clicking a body and a platformer's walk and
//! jump. Every recipe between `ANCHOR` lines is what
//! `book/src/cookbook/physics.md` shows, and each test runs it headless for a
//! few ticks and checks that it did its job.
#![cfg(feature = "physics")]
#![allow(clippy::type_complexity)]

use ponko::actions::InputEvent;
use ponko::d2::*;
use ponko::prelude::*;
use ponko::scenario::InputScenario;

#[derive(Component)]
struct Player;

#[derive(Component)]
struct Health {
    hp: i32,
}

fn hold(scenario: InputScenario, key: Key, from: u64, to: u64) -> InputScenario {
    scenario
        .at(from, InputEvent::Key(key, true))
        .at(to, InputEvent::Key(key, false))
}

/// The `move` action, resolved once (as in the guide's first recipe).
#[derive(Clone, Copy)]
struct Move(ActionId);

fn bind_move(ctx: &mut Ctx) {
    ctx.input.set_action(
        "move",
        Action::Axis2(vec![Axis2Binding::Keys {
            left: Key::A,
            right: Key::D,
            up: Key::W,
            down: Key::S,
        }]),
    );
    let movement = ctx.input.action_id("move");
    ctx.world.insert_resource(Move(movement));
}

// ANCHOR: contact_damage
#[derive(Component)]
struct Hazard {
    damage: i32,
}

/// Physics sends `CollisionStarted` once when two colliders begin to touch.
fn hazards_hurt(ctx: &mut Ctx) {
    for contact in ctx.events.read::<CollisionStarted>() {
        for (hazard, victim) in [(contact.a, contact.b), (contact.b, contact.a)] {
            let Some(damage) = ctx.world.get::<Hazard>(hazard).map(|h| h.damage) else {
                continue;
            };
            if let Some(health) = ctx.world.get_mut::<Health>(victim) {
                health.hp -= damage;
                ctx.commands.despawn(hazard); // a spent hazard
            }
        }
    }
}
// ANCHOR_END: contact_damage

#[test]
fn a_hazard_that_touches_the_player_costs_one_hit_and_is_spent() {
    let mut world = Game::new("hazard", 640, 480)
        .physics(Vec2::ZERO)
        .setup(|ctx| {
            ctx.world.spawn((
                Player,
                Health { hp: 3 },
                Transform::at(vec2(200.0, 100.0)),
                RigidBody::dynamic(),
                Collider::circle(10.0),
            ));
            // Flies right, from 100 pixels away.
            ctx.world.spawn((
                Hazard { damage: 2 },
                Transform::at(vec2(100.0, 100.0)),
                RigidBody::dynamic(),
                Collider::circle(5.0),
                Velocity::linear(vec2(300.0, 0.0)),
            ));
        })
        .sim_system(hazards_hurt)
        .run_headless(60);
    assert_eq!(world.query::<&Health>().iter().next().unwrap().hp, 1);
    assert_eq!(world.query::<&Hazard>().count(), 0, "the hazard is gone");
}

// ANCHOR: knockback
/// Pushes `victim` away from `from`, as if hit hard enough to fly at `speed` pixels a second.
fn knock_back(ctx: &mut Ctx, victim: Entity, from: Vec2, speed: f32) {
    let Some(at) = ctx.world.get::<Transform>(victim).map(|t| t.position) else {
        return;
    };
    let away = (at - from).normalize_or(Vec2::X);
    if let Some(mass) = ctx.physics.mass(victim) {
        ctx.physics.apply_impulse(victim, away * mass * speed); // an impulse is mass times speed
    }
}
// ANCHOR_END: knockback

#[test]
fn knockback_sends_the_body_away_from_the_blow_at_the_given_speed() {
    let mut world = Game::new("knockback", 640, 480)
        .physics(Vec2::ZERO)
        .setup(|ctx| {
            let crate_ = ctx.world.spawn((
                Transform::at(vec2(100.0, 100.0)),
                RigidBody::dynamic(),
                Collider::rect(vec2(20.0, 20.0)),
            ));
            ctx.world.insert_resource(crate_);
        })
        .sim_system(|ctx: &mut Ctx| {
            // Bodies join the physics world at the first step; push on the second tick.
            if ctx.time.sim_tick() == 1 {
                let victim = *ctx.world.resource::<Entity>();
                knock_back(ctx, victim, vec2(50.0, 100.0), 300.0);
            }
        })
        .run_headless(32);
    // About 30 ticks of 300 pixels a second, straight right.
    let at = world.query::<&Transform>().iter().next().unwrap().position;
    assert!((at.x - 250.0).abs() < 6.0, "{at}");
    assert!((at.y - 100.0).abs() < 0.01, "{at}");
}

// ANCHOR: line_of_sight
fn can_see(ctx: &Ctx, looker: Entity, target: Entity) -> bool {
    let (Some(from), Some(to)) = (ctx.world.get::<Transform>(looker), ctx.world.get::<Transform>(target)) else {
        return false;
    };
    let way = to.position - from.position;
    // The nearest thing along the line, not counting the looker itself.
    let hit = ctx
        .physics
        .cast_ray(from.position, way, way.length(), QueryFilter::default().exclude(looker));
    hit.is_some_and(|hit| hit.entity == target) // a wall in between would be hit first
}
// ANCHOR_END: line_of_sight

#[test]
fn a_wall_between_two_bodies_blocks_the_view_and_removing_it_clears_it() {
    let seen = |with_wall: bool| {
        let mut world = Game::new("sight", 640, 480)
            .physics(Vec2::ZERO)
            .setup(move |ctx| {
                let guard = ctx.world.spawn((
                    Transform::at(vec2(50.0, 100.0)),
                    RigidBody::fixed(),
                    Collider::circle(8.0),
                ));
                let thief = ctx.world.spawn((
                    Transform::at(vec2(350.0, 100.0)),
                    RigidBody::fixed(),
                    Collider::circle(8.0),
                ));
                if with_wall {
                    ctx.world.spawn((
                        Transform::at(vec2(200.0, 100.0)),
                        RigidBody::fixed(),
                        Collider::rect(vec2(10.0, 80.0)),
                    ));
                }
                ctx.world.insert_resource((guard, thief));
            })
            .sim_system(|ctx: &mut Ctx| {
                if ctx.time.sim_tick() == 2 {
                    let (guard, thief) = *ctx.world.resource::<(Entity, Entity)>();
                    let sees = can_see(ctx, guard, thief);
                    ctx.world.insert_resource(sees);
                }
            })
            .run_headless(4);
        *world.resource_mut::<bool>()
    };
    assert!(!seen(true), "the wall is in the way");
    assert!(seen(false), "nothing is in the way");
}

// ANCHOR: click_body
#[derive(Component)]
struct Selected;

/// Click a body to select it (the topmost one if several overlap).
fn click_to_select(ctx: &mut Ctx) {
    if !ctx.input.mouse_pressed(MouseButton::Left) || ctx.ui.wants_pointer() {
        return;
    }
    let mut under = Vec::new();
    ctx.physics
        .overlap_point(ctx.mouse_world(), QueryFilter::default(), &mut under);
    let old: Vec<Entity> = ctx
        .world
        .query::<(Entity, &Selected)>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    for e in old {
        ctx.commands.remove::<Selected>(e);
    }
    if let Some(&clicked) = under.first() {
        ctx.commands.insert(clicked, Selected);
    }
}
// ANCHOR_END: click_body

#[test]
fn clicking_a_body_selects_it_and_clicking_the_ground_deselects() {
    let run = |click_at: Vec2| {
        let mut world = Game::new("select", 640, 480)
            .physics(Vec2::ZERO)
            .setup(move |ctx| {
                ctx.input.feed(InputEvent::MouseMove(click_at)); // the camera is at the origin: pixels are world units
                ctx.world.spawn((
                    Transform::at(vec2(100.0, 100.0)),
                    RigidBody::fixed(),
                    Collider::circle(20.0),
                ));
                ctx.world.spawn((
                    Transform::at(vec2(300.0, 100.0)),
                    RigidBody::fixed(),
                    Collider::circle(20.0),
                ));
                let first = ctx.world.query::<Entity>().iter().next().unwrap();
                ctx.world.insert(first, Selected); // already selected: a click elsewhere clears it
            })
            .sim_system(click_to_select)
            .input_scenario(
                InputScenario::new()
                    .at(3, InputEvent::Mouse(MouseButton::Left, true))
                    .at(4, InputEvent::Mouse(MouseButton::Left, false)),
            )
            .run_headless(8);
        world
            .query::<(&Selected, &Transform)>()
            .iter()
            .map(|(_, t)| t.position)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        run(vec2(305.0, 95.0)),
        [vec2(300.0, 100.0)],
        "the second body is selected, the first is not"
    );
    assert!(run(vec2(500.0, 400.0)).is_empty(), "empty ground selects nothing");
}

// ANCHOR: platformer
/// Runs before the physics step, so the new speed is what the step uses.
fn platformer_controls(ctx: &mut Ctx) {
    let Move(movement) = *ctx.world.resource::<Move>(); // the `move` action's id
    let run = ctx.input.action_axis2(movement).x;
    let jump = ctx.input.key_pressed(Key::Space);
    ctx.world
        .query::<(Entity, &Player, &Transform, &mut Velocity)>()
        .for_each(|(me, _, t, v)| {
            v.linear.x = run * 200.0; // gravity keeps handling y
            // Standing on something: a short ray down from the feet finds ground.
            let feet = t.position + vec2(0.0, 10.0);
            let grounded = ctx
                .physics
                .cast_ray(feet, Vec2::Y, 4.0, QueryFilter::default().exclude(me))
                .is_some();
            if jump && grounded {
                v.linear.y = -450.0; // y points down
            }
        });
}
// ANCHOR_END: platformer

#[test]
fn the_platformer_player_runs_jumps_from_the_ground_only_and_lands() {
    #[derive(Default)]
    struct Heights(Vec<f32>);
    let mut world = Game::new("platformer", 640, 480)
        .physics(vec2(0.0, 900.0))
        .setup(|ctx| {
            bind_move(ctx);
            ctx.world.insert_resource(Heights::default());
            ctx.world.spawn((
                Transform::at(vec2(300.0, 300.0)),
                RigidBody::fixed(),
                Collider::rect(vec2(2000.0, 20.0)),
            )); // the floor, top at y = 290
            ctx.world.spawn((
                Player,
                Transform::at(vec2(100.0, 270.0)),
                RigidBody {
                    lock_rotation: true,
                    ..RigidBody::dynamic()
                },
                Collider::rect(vec2(16.0, 20.0)),
                Velocity::default(),
            ));
        })
        .add(Phase::BeforePhysics, platformer_controls)
        .sim_system(|ctx: &mut Ctx| {
            let y = ctx
                .world
                .query::<(&Player, &Transform)>()
                .iter()
                .next()
                .unwrap()
                .1
                .position
                .y;
            ctx.world.resource_mut::<Heights>().0.push(y);
        })
        .input_scenario(
            hold(InputScenario::new(), Key::D, 10, 40)
                .at(60, InputEvent::Key(Key::Space, true))
                .at(61, InputEvent::Key(Key::Space, false)),
        )
        .run_headless(150);
    let (x, resting_y) = {
        let t = world.query::<(&Player, &Transform)>().iter().next().unwrap().1.position;
        (t.x, world.resource::<Heights>().0[50])
    };
    assert!((x - 190.0).abs() < 8.0, "30 ticks at 200 pixels a second: {x}");
    let heights = &world.resource::<Heights>().0;
    let highest = heights.iter().cloned().fold(f32::MAX, f32::min);
    assert!(
        resting_y - highest > 50.0,
        "the jump went {} pixels up",
        resting_y - highest
    );
    assert!((heights[149] - resting_y).abs() < 1.0, "and it landed again");
}
