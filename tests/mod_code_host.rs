//! The host functions that let code mods (`ponko::mods`, feature
//! `mods-wasm`) read and change the game: fields of reflected components of
//! the entities the game exposed, spawning the prefabs it exposed, and a
//! random number from a seed it sets. Each with its refusals. No window.
#![cfg(feature = "mods-wasm")]

use ponko::mods::{ModPack, ModSet, ModValue, REFUSED_ENTITY, REFUSED_FIELD, REFUSED_VALUE, with_code};
use ponko::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
struct Health {
    current: f32,
    max: u32,
    shielded: bool,
}

/// A module passing the game's arguments to the host functions, with the
/// text the tests name at fixed places in its memory.
const HOST: &str = r#"(module
    (import "ponko" "get_field" (func $get (param i64 i32 i32) (result f64)))
    (import "ponko" "set_field" (func $set (param i64 i32 i32 f64) (result i32)))
    (import "ponko" "spawn_prefab" (func $spawn (param i32 i32 f32 f32) (result i64)))
    (import "ponko" "random" (func $random (result f32)))
    (memory (export "memory") 1)
    (data (i32.const 0) "Health.current")
    (data (i32.const 32) "Health.max")
    (data (i32.const 64) "Health.shielded")
    (data (i32.const 96) "Transform2D.position.1")
    (data (i32.const 128) "Health")
    (data (i32.const 160) "Health.armor")
    (data (i32.const 192) "Armor.value")
    (data (i32.const 224) "prefabs/drip.json")
    (data (i32.const 256) "prefabs/boss.json")
    (data (i32.const 288) "prefabs/none.json")
    (data (i32.const 320) "\ff\fe")
    (func (export "get") (param i64 i32 i32) (result f64)
        local.get 0 local.get 1 local.get 2 call $get)
    (func (export "set") (param i64 i32 i32 f64) (result i32)
        local.get 0 local.get 1 local.get 2 local.get 3 call $set)
    (func (export "spawn") (param i32 i32 f32 f32) (result i64)
        local.get 0 local.get 1 local.get 2 local.get 3 call $spawn)
    (func (export "random") (result f32) call $random)
    ;; Reads a field until the fuel runs out.
    (func (export "poll") (param i64)
        (loop $l (drop (call $get (local.get 0) (i32.const 0) (i32.const 14))) br $l)))"#;

/// Where each text is in [`HOST`]'s memory, and its length.
const CURRENT: (i32, i32) = (0, 14);
const MAX: (i32, i32) = (32, 10);
const SHIELDED: (i32, i32) = (64, 15);
const POSITION_Y: (i32, i32) = (96, 22);
const NO_FIELD: (i32, i32) = (128, 6);
const UNKNOWN_FIELD: (i32, i32) = (160, 12);
const UNKNOWN_COMPONENT: (i32, i32) = (192, 11);
const DRIP: (i32, i32) = (224, 17);
const BOSS: (i32, i32) = (256, 17);
const MISSING: (i32, i32) = (288, 17);
const NOT_UTF8: (i32, i32) = (320, 2);

const DRIP_PREFAB: &str = r#"{"format":"ponko-prefab","version":1,"components":{
    "Health":{"current":2.0,"max":2,"shielded":false},
    "Transform2D":{"position":[0.0,0.0],"rotation":0.0,"scale":[1.0,1.0]}}}"#;

/// A mod with id `id` running [`HOST`], and a prefab of its own.
fn host_mod(id: &str) -> ModPack {
    let manifest = format!(r#"{{"format":"ponko-mod","version":1,"id":"{id}","code":"code.wasm"}}"#);
    let wasm = wat::parse_str(HOST).expect("valid WebAssembly text");
    ModPack::from_files(
        id,
        [
            ("mod.json".to_string(), manifest.into_bytes()),
            ("code.wasm".to_string(), wasm),
            ("prefabs/drip.json".to_string(), DRIP_PREFAB.as_bytes().to_vec()),
            ("prefabs/boss.json".to_string(), DRIP_PREFAB.as_bytes().to_vec()),
        ],
    )
    .expect("a valid manifest")
}

fn run(mods: &[&str], body: impl FnOnce(&mut Ctx)) {
    Game::new("mod host", 1, 1).run_headless(0, |ctx| {
        ctx.world.reflect::<Health>();
        let mut set = ModSet::new();
        for id in mods {
            set.add(host_mod(id));
        }
        set.mount(ctx).expect("the mods mount");
        body(ctx);
    });
}

fn bits(e: Entity) -> ModValue {
    ModValue::I64(e.to_bits() as i64)
}

fn text((at, len): (i32, i32)) -> [ModValue; 2] {
    [ModValue::I32(at), ModValue::I32(len)]
}

fn get(code: &mut ponko::mods::ModCode, e: Entity, path: (i32, i32)) -> f64 {
    let f = code.function("host", "get").expect("exported");
    let [p, l] = text(path);
    match code.call(f, &[bits(e), p, l]) {
        Ok(Some(ModValue::F64(v))) => v,
        other => panic!("get returned {other:?}"),
    }
}

fn set(code: &mut ponko::mods::ModCode, e: Entity, path: (i32, i32), value: f64) -> i32 {
    let f = code.function("host", "set").expect("exported");
    let [p, l] = text(path);
    match code.call(f, &[bits(e), p, l, ModValue::F64(value)]) {
        Ok(Some(ModValue::I32(v))) => v,
        other => panic!("set returned {other:?}"),
    }
}

fn spawn(code: &mut ponko::mods::ModCode, path: (i32, i32), at: Vec2) -> i64 {
    let f = code.function("host", "spawn").expect("exported");
    let [p, l] = text(path);
    match code.call(f, &[p, l, ModValue::F32(at.x), ModValue::F32(at.y)]) {
        Ok(Some(ModValue::I64(v))) => v,
        other => panic!("spawn returned {other:?}"),
    }
}

fn refusals(code: &mut ponko::mods::ModCode) -> Vec<String> {
    code.take_log()
        .into_iter()
        .map(|(id, line)| {
            assert_eq!(id, "host");
            line
        })
        .filter(|line| line.starts_with("refused:"))
        .collect()
}

fn health() -> Health {
    Health {
        current: 7.5,
        max: 10,
        shielded: true,
    }
}

#[test]
fn a_mod_reads_and_writes_number_and_bool_fields_of_an_exposed_entity() {
    run(&["host"], |ctx| {
        let e = ctx.world.spawn((health(), d2::Transform::at(vec2(3.0, 4.0))));
        with_code(ctx, |code| {
            code.expose(e);
            assert_eq!(get(code, e, CURRENT), 7.5);
            assert_eq!(get(code, e, MAX), 10.0);
            assert_eq!(get(code, e, SHIELDED), 1.0);
            assert_eq!(get(code, e, POSITION_Y), 4.0);
            assert_eq!(set(code, e, CURRENT, 2.25), 0);
            assert_eq!(set(code, e, MAX, 12.0), 0);
            assert_eq!(set(code, e, SHIELDED, 0.0), 0);
            assert_eq!(set(code, e, POSITION_Y, -8.0), 0);
            assert!(refusals(code).is_empty());
        })
        .expect("a mod has code");
        let h = *ctx.world.get::<Health>(e).unwrap();
        assert_eq!(
            h,
            Health {
                current: 2.25,
                max: 12,
                shielded: false
            }
        );
        let t = ctx.world.get::<d2::Transform>(e).unwrap();
        assert_eq!(t.position, vec2(3.0, -8.0), "the other coordinate is kept");
    });
}

#[test]
fn a_mod_cannot_reach_an_entity_the_game_did_not_expose() {
    run(&["host"], |ctx| {
        let hidden = ctx.world.spawn((health(),));
        let shown = ctx.world.spawn((health(),));
        let gone = ctx.world.spawn((health(),));
        with_code(ctx, |code| {
            code.expose(shown);
            code.expose(gone);
            assert!(get(code, hidden, CURRENT).is_nan());
            assert_eq!(set(code, hidden, CURRENT, 0.0), REFUSED_ENTITY);
            // A guessed id: the exposed entity's index with another generation.
            let forged = Entity::from_bits(shown.to_bits() + (1 << 32));
            assert_eq!(set(code, forged, CURRENT, 0.0), REFUSED_ENTITY);
            // Taken back.
            assert_eq!(set(code, shown, CURRENT, 1.0), 0);
            assert!(code.hide(shown) && !code.is_exposed(shown));
            assert_eq!(set(code, shown, CURRENT, 0.0), REFUSED_ENTITY);
            let lines = refusals(code);
            assert_eq!(lines.len(), 4, "{lines:?}");
            assert!(lines[0].contains("get_field") && lines[0].contains("isn't one the game exposed"), "{lines:?}");
        })
        .expect("a mod has code");
        ctx.world.despawn(gone);
        with_code(ctx, |code| {
            assert_eq!(set(code, gone, CURRENT, 0.0), REFUSED_ENTITY, "a dead entity");
        });
        assert_eq!(ctx.world.get::<Health>(hidden).unwrap().current, 7.5);
        assert_eq!(ctx.world.get::<Health>(shown).unwrap().current, 1.0);
    });
}

#[test]
fn fields_that_are_missing_or_not_numbers_are_refused_and_nothing_changes() {
    run(&["host"], |ctx| {
        let e = ctx.world.spawn((health(),));
        with_code(ctx, |code| {
            code.expose(e);
            assert_eq!(set(code, e, NO_FIELD, 1.0), REFUSED_FIELD, "a component, not a field");
            assert_eq!(set(code, e, UNKNOWN_FIELD, 1.0), REFUSED_FIELD);
            assert_eq!(set(code, e, UNKNOWN_COMPONENT, 1.0), REFUSED_FIELD);
            assert_eq!(set(code, e, POSITION_Y, 1.0), REFUSED_FIELD, "no Transform2D");
            assert!(get(code, e, UNKNOWN_FIELD).is_nan());
            assert_eq!(set(code, e, MAX, 2.5), REFUSED_VALUE, "a fraction for an integer");
            assert_eq!(set(code, e, MAX, -1.0), REFUSED_VALUE, "out of an unsigned field's range");
            assert_eq!(set(code, e, CURRENT, f64::NAN), REFUSED_VALUE);
            assert_eq!(set(code, e, NOT_UTF8, 1.0), REFUSED_VALUE, "text that isn't UTF-8");
            let lines = refusals(code);
            assert_eq!(lines.len(), 9, "{lines:?}");
        })
        .expect("a mod has code");
        assert_eq!(*ctx.world.get::<Health>(e).unwrap(), health());
    });
}

#[test]
fn a_mod_spawns_only_the_prefabs_the_game_exposed_and_can_change_what_it_spawned() {
    run(&["host"], |ctx| {
        let before = ctx.world.len();
        with_code(ctx, |code| {
            assert_eq!(spawn(code, DRIP, vec2(5.0, 6.0)), i64::from(REFUSED_ENTITY));
            code.expose_prefab("prefabs/drip.json");
            code.expose_prefab("prefabs/none.json");
            assert_eq!(spawn(code, BOSS, vec2(0.0, 0.0)), i64::from(REFUSED_ENTITY), "boss isn't exposed");
            assert_eq!(spawn(code, MISSING, vec2(0.0, 0.0)), i64::from(REFUSED_FIELD), "exposed, but no file");
            let root = spawn(code, DRIP, vec2(5.0, 6.0));
            assert!(root >= 0);
            let root = Entity::from_bits(root as u64);
            assert!(code.is_exposed(root), "the mod may change what it spawned");
            assert_eq!(set(code, root, CURRENT, 9.0), 0);
            let lines = refusals(code);
            assert_eq!(lines.len(), 3, "{lines:?}");
            assert!(lines[2].contains("prefabs/none.json"), "{lines:?}");
        })
        .expect("a mod has code");
        assert_eq!(ctx.world.len(), before + 1, "one drip, nothing from the refusals");
        let (t, h) = ctx
            .world
            .query::<(&d2::Transform, &Health)>()
            .iter()
            .map(|(t, h)| (*t, *h))
            .next()
            .expect("the drip");
        assert_eq!((t.position, h.current), (vec2(5.0, 6.0), 9.0));
    });
}

fn draws(code: &mut ponko::mods::ModCode, id: &str, n: usize) -> Vec<f32> {
    let f = code.function(id, "random").expect("exported");
    (0..n)
        .map(|_| match code.call(f, &[]) {
            Ok(Some(ModValue::F32(v))) => v,
            other => panic!("random returned {other:?}"),
        })
        .collect()
}

#[test]
fn random_numbers_follow_the_seed_the_game_sets_and_each_mod_has_its_own() {
    let mut runs = Vec::new();
    for _ in 0..2 {
        run(&["host", "other"], |ctx| {
            with_code(ctx, |code| {
                code.seed_random(42);
                let first = draws(code, "host", 64);
                assert!(first.iter().all(|v| (0.0..1.0).contains(v)), "{first:?}");
                let other = draws(code, "other", 64);
                assert_ne!(first, other, "mods don't share a sequence");
                code.seed_random(42);
                assert_eq!(draws(code, "host", 64), first, "same seed, same sequence");
                code.seed_random(43);
                assert_ne!(draws(code, "host", 64), first, "another seed, another sequence");
                runs.push(first);
            });
        });
    }
    assert_eq!(runs[0], runs[1], "the same in every run");
}

#[test]
fn host_calls_spend_the_callers_fuel_and_a_polling_mod_is_stopped() {
    run(&["host"], |ctx| {
        let e = ctx.world.spawn((health(),));
        with_code(ctx, |code| {
            code.expose(e);
            let poll = code.function("host", "poll").expect("exported");
            let err = code.call(poll, &[bits(e)]).expect_err("out of fuel");
            assert!(err.to_string().contains("fuel"), "{err}");
            assert_eq!(get(code, e, CURRENT), 7.5, "the mod stays callable");
        })
        .expect("a mod has code");
    });
}

#[test]
fn a_module_importing_the_new_host_functions_mounts_and_an_unknown_one_is_named() {
    let manifest = r#"{"format":"ponko-mod","version":1,"id":"bad","code":"code.wasm"}"#;
    let wasm = wat::parse_str(r#"(module (import "ponko" "despawn" (func (param i64))))"#).unwrap();
    let pack = ModPack::from_files("bad", [("mod.json", manifest.as_bytes().to_vec()), ("code.wasm", wasm)]).unwrap();
    Game::new("mod host", 1, 1).run_headless(0, |ctx| {
        let err = ModSet::new().with(pack).mount(ctx).expect_err("refused");
        let text = err.to_string();
        assert!(text.contains("\"despawn\"") && text.contains("spawn_prefab"), "{text}");
    });
}
