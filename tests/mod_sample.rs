//! The mods sample: a data-only mod for the `ember_hop` reference
//! platformer that adds an enemy type, the "frost drip" (its own prefab and
//! sprite sheet, twice as fast as a drip), and one that changes the game's
//! own drip by replacing its prefab. The game's rules drive both: a mod is
//! data, so it recombines components the game already reflects and runs.
//! No window, no GPU.
#![cfg(feature = "physics")]

#[path = "../examples/ember_hop/game.rs"]
mod game;

use std::cell::RefCell;
use std::rc::Rc;

use game::*;
use ponko::d2::Transform;
use ponko::mods::{ModPack, ModProblem, ModSet, Mods};
use ponko::prelude::*;

const FROST: &str = "ember_hop/prefabs/frost_drip.json";

fn data_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ponko-mod-sample-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn manifest(id: &str) -> String {
    format!(r#"{{"format":"ponko-mod","version":1,"id":"{id}","name":"Frost drips","mod_version":"1.0.0"}}"#)
}

/// The frost drip: the game's drip components with the mod's own art, faster.
fn frost_prefab(drip: &str) -> String {
    format!(
        r#"{{
  "format": "ponko-prefab",
  "version": 1,
  "components": {{
    "Transform2D": {{}},
    "Sprite": {{ "texture": "ember_hop/sprites/frost_drip.png", "region": {{ "min": [0.0, 0.0], "max": [16.0, 16.0] }},
                "color": {{ "r": 0.6, "g": 0.85, "b": 1.0, "a": 1.0 }}, "z": 0.3 }},
    "Animation": {{ "texture": "ember_hop/sprites/frost_drip.png", "frame_size": [16.0, 16.0], "frames": 2, "fps": 8.0 }},
    "RigidBody": {{ "kind": "Kinematic" }},
    "Collider": {{ "shape": {{ "Rect": {{ "size": [12.0, 9.0] }} }}, "sensor": true, "offset": [0.0, 3.5] }},
    {drip}
  }}
}}"#
    )
}

fn frost_mod(drip: &str) -> ModPack {
    let sheet = std::fs::read("assets/ember_hop/sprites/drip.png").expect("the game's drip sheet");
    ModPack::from_files(
        "frost-drips.pack",
        [
            ("mod.json".to_string(), manifest("frost-drips").into_bytes()),
            (FROST.to_string(), frost_prefab(drip).into_bytes()),
            ("ember_hop/sprites/frost_drip.png".to_string(), sheet),
        ],
    )
    .expect("a valid manifest")
}

// ANCHOR: mount
/// The game, with the mods `set` mounted in setup and `script` deciding,
/// each tick, what happens.
fn modded(name: &str, set: ModSet, script: impl FnMut(&mut Ctx) + 'static) -> (World, Option<ponko::mods::ModError>) {
    let dir = data_dir(name);
    let mut refused = None;
    let world = Game::new("mod sample", 960, 540)
        .data_dir(&dir)
        .seed(SEED)
        .sim_system(script)
        .plugin(EmberHop)
        .run_headless(40, |ctx| refused = set.mount(ctx).err());
    let _ = std::fs::remove_dir_all(dir);
    (world, refused)
}
// ANCHOR_END: mount

#[test]
fn a_data_only_mod_adds_a_frost_drip_that_the_games_own_rules_drive() {
    // ANCHOR: frost_spawn
    let steps = Rc::new(RefCell::new((Vec::new(), Vec::new())));
    let seen = steps.clone();
    let frost = Rc::new(RefCell::new(None));
    let spawned = frost.clone();
    let (mut world, refused) = modded(
        "frost",
        ModSet::new().with(frost_mod(r#""Drip": { "speed": 56.0, "heading": 1.0 }"#)),
        move |ctx| {
            let tick = ctx.time.sim_tick();
            if tick == 1 {
                new_run(ctx);
            }
            if tick == 2 {
                // On a drip's ledge: where the level put one.
                let (drip, at) = ctx
                    .world
                    .query::<(Entity, &Transform, &Drip)>()
                    .iter()
                    .map(|(e, t, _)| (e, t.position))
                    .next()
                    .expect("a drip");
                let e = ctx.spawn_prefab(FROST, at).expect("the mod's prefab");
                *spawned.borrow_mut() = Some((e, drip));
            }
            if let Some((frost, drip)) = *spawned.borrow() {
                let x = |e| ctx.world.get::<Transform>(e).map(|t| t.position.x);
                let mut s = seen.borrow_mut();
                s.0.extend(x(frost));
                s.1.extend(x(drip));
            }
        },
    );
    // ANCHOR_END: frost_spawn
    assert!(refused.is_none(), "{refused:?}");
    let (frost, _) = frost.borrow().expect("spawned");
    assert_eq!(world.get::<Drip>(frost).map(|d| d.speed), Some(56.0));
    let mods = world.resource::<Mods>();
    assert_eq!(mods.provider(FROST), Some("frost-drips"));
    assert_eq!(mods.provider(DRIP_PREFAB), None, "the game's drip is its own");
    // The game's crawl moves it each tick by its own speed: twice the drip's.
    let fastest = |xs: &[f32]| xs.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    let (frost_x, drip_x) = &*steps.borrow();
    assert!(frost_x.len() > 30, "{} ticks", frost_x.len());
    let (f, d) = (fastest(frost_x), fastest(drip_x));
    assert!((f / d - 2.0).abs() < 1e-3, "frost {f} px/tick, drip {d} px/tick");
    assert!(
        world.query::<&Drip>().iter().count() == 4,
        "the level's three and the frost drip"
    );
}

#[test]
fn a_mod_that_replaces_the_drip_prefab_changes_every_drip_of_the_level() {
    let faster = r#"{"format":"ponko-prefab","version":1,"components":{
        "Transform2D":{},
        "Sprite":{"texture":"ember_hop/sprites/drip.png","region":{"min":[0.0,0.0],"max":[16.0,16.0]},"z":0.3},
        "RigidBody":{"kind":"Kinematic"},
        "Collider":{"shape":{"Rect":{"size":[12.0,9.0]}},"sensor":true,"offset":[0.0,3.5]},
        "Drip":{"speed":40.0,"heading":-1.0}}}"#;
    let pack = ModPack::from_files(
        "faster",
        [
            ("mod.json".to_string(), manifest("faster-drips")),
            (DRIP_PREFAB.to_string(), faster.to_string()),
        ],
    )
    .unwrap();
    let (mut world, refused) = modded("replace", ModSet::new().with(pack), |ctx| {
        if ctx.time.sim_tick() == 1 {
            new_run(ctx);
        }
    });
    assert!(refused.is_none(), "{refused:?}");
    let speeds: Vec<f32> = world.query::<&Drip>().iter().map(|d| d.speed).collect();
    assert_eq!(speeds, [40.0; 3]);
}

#[test]
fn a_frost_drip_naming_a_component_the_game_lacks_is_refused_and_the_game_plays_unmodded() {
    let bad = frost_mod(r#""Drip": { "speed": 56.0 }, "Frozen": { "seconds": 2.0 }"#);
    let (mut world, refused) = modded("refused", ModSet::new().with(bad), |ctx| {
        if ctx.time.sim_tick() == 1 {
            new_run(ctx);
        }
    });
    let err = refused.expect("refused");
    assert_eq!((err.mod_id(), err.file()), ("frost-drips", Some(FROST)));
    assert_eq!(err.problem(), &ModProblem::UnknownComponent("Frozen".into()));
    assert!(world.get_resource::<Mods>().is_none());
    assert_eq!(screen(&world), Screen::Playing);
    let speeds: Vec<f32> = world.query::<&Drip>().iter().map(|d| d.speed).collect();
    assert_eq!(speeds, [28.0; 3], "the game's own drips");
}
