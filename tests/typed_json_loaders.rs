//! `load_reflected_component_json::<T>` / `load_reflected_resource_json::<T>`: the name comes from the type.
mod common;

use common::TempAsset;
use ponko::prelude::*;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, Component)]
struct Stats {
    hp: f32,
    armor: f32,
}
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
struct Tuning {
    speed: f32,
    gravity: f32,
}

#[test]
fn typed_component_and_resource_loaders_match_the_string_forms() {
    let stats = TempAsset::with_contents("test-typed-stats.json", r#"{"hp": 7.0}"#);
    let tuning = TempAsset::with_contents("test-typed-tuning.json", r#"{"speed": 3.0}"#);
    Game::new("typed", 1, 1).run_headless(0, |ctx| {
        ctx.world.reflect::<Stats>();
        ctx.world.reflect_resource::<Tuning>();
        ctx.world.insert_resource(Tuning {
            speed: 1.0,
            gravity: 9.0,
        });
        let e = ctx.world.spawn((Stats { hp: 1.0, armor: 5.0 },));
        let e2 = ctx.world.spawn((Stats { hp: 1.0, armor: 5.0 },));

        ctx.assets
            .load_reflected_component_json::<Stats>(&mut ctx.world, stats.name(), e)
            .unwrap();
        ctx.assets
            .load_reflected_component_json_named(&mut ctx.world, stats.name(), e2, "Stats")
            .unwrap();
        // Object fields merge: hp from the file, armor kept.
        assert_eq!(ctx.world.get::<Stats>(e), Some(&Stats { hp: 7.0, armor: 5.0 }));
        assert_eq!(ctx.world.get::<Stats>(e2), ctx.world.get::<Stats>(e));

        ctx.assets
            .load_reflected_resource_json::<Tuning>(&mut ctx.world, tuning.name())
            .unwrap();
        assert_eq!(
            *ctx.world.resource::<Tuning>(),
            Tuning {
                speed: 3.0,
                gravity: 9.0
            }
        );
    });
}

#[test]
fn a_type_that_was_not_reflected_is_an_error_not_a_panic() {
    let stats = TempAsset::with_contents("test-typed-unreflected.json", r#"{"hp": 7.0}"#);
    Game::new("typed", 1, 1).run_headless(0, |ctx| {
        let e = ctx.world.spawn((Stats::default(),));
        let error = ctx
            .assets
            .load_reflected_component_json::<Stats>(&mut ctx.world, stats.name(), e)
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NotFound);
        assert!(error.message().contains("Stats"), "{error}");
        let error = ctx
            .assets
            .load_reflected_resource_json::<Tuning>(&mut ctx.world, stats.name())
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NotFound);
        assert!(error.message().contains("Tuning"), "{error}");
    });
}
