//! Cookbook, "Levels and objects": loading a level, spawning a prefab,
//! attaching one entity to another and drawing debug shapes. Every recipe
//! between `ANCHOR` lines is what `book/src/cookbook/world.md` shows, and each
//! test runs it headless for a few ticks and checks that it did its job.
#![allow(clippy::type_complexity)]

use ponko::d2::*;
use ponko::prelude::*;
use ponko::scenario::InputScenario;
use serde::{Deserialize, Serialize};

// ANCHOR: load_level
use ponko::tilemap::LevelObject;

#[derive(Component)]
struct Hero;

#[derive(Component)]
struct Coin;

/// Builds the level from a Tiled map and turns its objects into game entities:
/// the "Spawn" object is where the hero starts, each "Coin" object becomes a coin.
fn load_level(ctx: &mut Ctx) -> Result<Entity, Error> {
    let level = ctx.spawn_level("tutorial_2d/cove.tmx", Vec2::ZERO)?; // tiles, colliders, objects
    let objects: Vec<(String, Vec2)> = ctx
        .world
        .query::<&LevelObject>()
        .iter()
        .map(|o| (o.class.clone(), o.position))
        .collect();
    for (class, at) in objects {
        match class.as_str() {
            "Spawn" => drop(ctx.world.spawn((Hero, Transform::at(at)))),
            "Coin" => drop(ctx.world.spawn((Coin, Transform::at(at)))),
            _ => {}
        }
    }
    Ok(level) // keep it: `ctx.despawn_level(level)` removes it all again
}
// ANCHOR_END: load_level

#[test]
fn the_level_puts_the_hero_at_its_spawn_object_and_a_coin_at_each_coin_object() {
    let mut world = Game::new("level", 800, 600)
        .setup(|ctx| {
            let level = load_level(ctx).expect("the level loads");
            ctx.world.insert_resource(level);
        })
        .run_headless(2);
    let hero = world
        .query::<(&Hero, &Transform)>()
        .iter()
        .map(|(_, t)| t.position)
        .collect::<Vec<_>>();
    assert_eq!(hero.len(), 1);
    assert_eq!(hero[0], vec2(40.0, 180.0), "the Spawn object of cove.tmx");
    assert_eq!(world.query::<&Coin>().count(), 12, "one per Coin object");
}

#[test]
fn removing_the_level_removes_what_it_spawned_so_a_restart_can_load_it_again() {
    let mut world = Game::new("level", 800, 600)
        .setup(|ctx| {
            let level = load_level(ctx).unwrap();
            let objects = ctx.world.query::<&LevelObject>().count();
            assert!(objects > 0);
            assert!(ctx.despawn_level(level));
            assert_eq!(
                ctx.world.query::<&LevelObject>().count(),
                0,
                "the level's own entities are gone"
            );
            load_level(ctx).unwrap();
        })
        .run_headless(1);
    assert!(world.query::<&LevelObject>().count() > 0, "loaded again");
}

// ANCHOR: prefab
/// What the prefab file `prefabs/crab.json` says about a crab.
#[derive(Component, Serialize, Deserialize)]
struct Walker {
    speed: f32,
    heading: f32,
}

/// Three crabs from one file, each turned its own way.
fn spawn_crabs(ctx: &mut Ctx) -> Result<(), Error> {
    ctx.world.reflect::<Walker>(); // components in files must be reflected first
    let mut random = ctx.random().stream(1);
    for i in 0..3 {
        let crab = ctx.spawn_prefab("prefabs/crab.json", vec2(100.0 + 80.0 * i as f32, 200.0))?;
        ctx.world.get_mut::<Walker>(crab).unwrap().heading = random.angle(); // this copy only
    }
    Ok(())
}
// ANCHOR_END: prefab

#[test]
fn three_crabs_from_one_prefab_file_share_a_speed_and_differ_in_heading() {
    let mut world = Game::new("prefab", 800, 600).run_headless(0, |ctx| spawn_crabs(ctx).unwrap());
    let crabs: Vec<(f32, f32, f32)> = world
        .query::<(&Walker, &Transform)>()
        .iter()
        .map(|(w, t)| (w.speed, w.heading, t.position.x))
        .collect();
    assert_eq!(crabs.len(), 3);
    assert!(crabs.iter().all(|c| c.0 == 50.0), "the file's speed");
    let mut headings: Vec<u32> = crabs.iter().map(|c| c.1.to_bits()).collect();
    headings.dedup();
    assert_eq!(headings.len(), 3, "each its own heading: {crabs:?}");
    let mut xs: Vec<f32> = crabs.iter().map(|c| c.2).collect();
    xs.sort_by(f32::total_cmp);
    assert_eq!(xs, [100.0, 180.0, 260.0]);
}

// ANCHOR: attach
use ponko::attach::Attached;

#[derive(Component)]
struct Sword;

fn give_a_sword(ctx: &mut Ctx, hero: Entity) -> Entity {
    // The sword hangs 14 pixels to the hero's right, and follows wherever the hero goes.
    ctx.world
        .spawn((Sword, Transform::default(), Attached::to(hero).offset(vec2(14.0, 0.0))))
}

fn add_attachments(game: ConfiguredGame) -> ConfiguredGame {
    // Moves everything attached to where its parent is now: after the systems that move things.
    game.sim_system(systems::follow_attachments)
}
// ANCHOR_END: attach

#[test]
fn the_sword_follows_the_hero_and_goes_when_the_hero_does() {
    let mut world = add_attachments(Game::new("sword", 800, 600).setup(|ctx| {
        let hero = ctx.world.spawn((Hero, Transform::at(vec2(100.0, 100.0))));
        give_a_sword(ctx, hero);
    }))
    .sim_system(|ctx: &mut Ctx| {
        // The hero walks right one pixel a tick, and is gone at tick 40.
        let hero = ctx.world.query::<(Entity, &Hero)>().iter().next().map(|(e, _)| e);
        if let Some(hero) = hero {
            if ctx.time.sim_tick() < 30 {
                ctx.world.get_mut::<Transform>(hero).unwrap().position.x += 1.0;
            }
            if ctx.time.sim_tick() == 40 {
                ctx.world.despawn(hero);
            }
        }
    })
    .sim_system(systems::follow_attachments)
    .run_headless(30);
    let sword = world.query::<(&Sword, &Transform)>().iter().next().unwrap().1.position;
    assert_eq!(
        sword,
        vec2(100.0 + 30.0 + 14.0, 100.0),
        "14 pixels to the right of the hero"
    );
    let mut world = Game::new("sword", 800, 600)
        .setup(|ctx| {
            let hero = ctx.world.spawn((Hero, Transform::at(vec2(100.0, 100.0))));
            give_a_sword(ctx, hero);
            ctx.world.despawn(hero);
        })
        .sim_system(systems::follow_attachments)
        .run_headless(2);
    assert_eq!(world.query::<&Sword>().count(), 0, "no hero, no sword");
}

// ANCHOR: debug_draw
#[derive(Component)]
struct Watcher {
    sees: Option<Vec2>, // what it is aiming at, if anything
}

/// Shapes last until this system runs again. They only show in debug builds
/// of the engine, so there is nothing to remove for the release.
fn show_what_the_ai_thinks(ctx: &mut Ctx) {
    ctx.world.query::<(&Transform, &Watcher)>().for_each(|(t, watcher)| {
        ctx.debug.circle(t.position, 120.0, Color::YELLOW); // how far it can see
        match watcher.sees {
            Some(target) => ctx.debug.arrow(t.position, target, Color::RED),
            None => ctx.debug.text(t.position, "idle", Color::WHITE),
        };
    });
}
// ANCHOR_END: debug_draw

#[test]
fn the_ai_draws_its_reach_its_target_and_its_mood() {
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = seen.clone();
    Game::new("debug", 800, 600)
        .debug_draw(true)
        .headless_frames(true)
        .setup(|ctx| {
            ctx.world.spawn((
                Transform::at(vec2(100.0, 100.0)),
                Watcher {
                    sees: Some(vec2(200.0, 100.0)),
                },
            ));
            ctx.world
                .spawn((Transform::at(vec2(400.0, 100.0)), Watcher { sees: None }));
        })
        .system(show_what_the_ai_thinks)
        .add(Phase::Draw, move |ctx: &mut Ctx| {
            log.borrow_mut().push(ctx.debug.stats())
        })
        .run_headless_scenario(InputScenario::new().duration(3))
        .1
        .assert_passed();
    let stats = seen.borrow()[1];
    assert_eq!(stats.labels, 1, "one idle watcher");
    assert!(
        stats.lines_2d > 32 * 2,
        "two circles of 32 segments and an arrow: {}",
        stats.lines_2d
    );
}
