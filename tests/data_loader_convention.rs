use ponko::assets::MemorySource;
use ponko::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize, Component)]
struct Rules {
    speed: u32,
    #[serde(default)]
    health: u32,
}

#[test]
fn fallible_data_loaders_preserve_values_on_failure_and_replace_or_merge_on_success() {
    Game::new("loaders", 1, 1).run_headless(0, |ctx| {
        ctx.assets.add_source(
            MemorySource::new("built-in")
                .with("rules.json", r#"{"speed":7}"#)
                .with("bad.json", "{")
                .with("en.json", r#"{"language":"en","strings":{"play":"Play"}}"#),
        );
        ctx.world.insert_resource(Rules { speed: 1, health: 9 });
        let missing = ctx
            .assets
            .try_load_resource::<Rules>(&mut ctx.world, "absent.json")
            .unwrap_err();
        assert_eq!(missing.kind(), ErrorKind::NotFound);
        let bad = ctx
            .assets
            .try_load_resource::<Rules>(&mut ctx.world, "bad.json")
            .unwrap_err();
        assert_eq!(bad.kind(), ErrorKind::Json);
        assert_eq!(bad.path(), Some("bad.json"));
        assert_eq!(ctx.world.resource::<Rules>().health, 9);
        ctx.assets
            .try_load_resource::<Rules>(&mut ctx.world, "rules.json")
            .unwrap();
        assert_eq!(*ctx.world.resource::<Rules>(), Rules { speed: 7, health: 0 });
        ctx.world.reflect_resource::<Rules>();
        ctx.world.reflect::<Rules>();
        ctx.world.resource_mut::<Rules>().health = 9;
        let entity = ctx.world.spawn((Rules { speed: 1, health: 4 },));
        ctx.assets
            .load_reflected_resource_json::<Rules>(&mut ctx.world, "rules.json")
            .unwrap();
        ctx.assets
            .load_reflected_component_json::<Rules>(&mut ctx.world, "rules.json", entity)
            .unwrap();
        assert_eq!(ctx.world.resource::<Rules>().health, 9);
        assert_eq!(*ctx.world.get::<Rules>(entity).unwrap(), Rules { speed: 7, health: 4 });
        let dead = ctx.world.spawn((Rules::default(),));
        ctx.world.despawn(dead);
        assert_eq!(
            ctx.assets
                .load_reflected_component_json::<Rules>(&mut ctx.world, "rules.json", dead)
                .unwrap_err()
                .kind(),
            ErrorKind::BadHandle
        );
        ctx.assets.try_load_strings(&mut ctx.world, "en.json").unwrap();
        assert_eq!(
            ctx.assets
                .try_load_strings(&mut ctx.world, "bad.json")
                .unwrap_err()
                .kind(),
            ErrorKind::Json
        );
        assert_eq!(ctx.world.resource::<ponko::locale::Strings>().get("play"), "Play");
    });
}

#[test]
#[should_panic(expected = "ponko:")]
fn plain_resource_loader_panics_with_the_engine_error() {
    Game::new("loaders", 1, 1).run_headless(0, |ctx| {
        ctx.assets
            .load_resource::<Rules>(&mut ctx.world, "missing-loader-rules.json");
    });
}

// ANCHOR: loader_failure
fn load_tables(ctx: &mut Ctx) {
    // Setup code: a missing file is a bug, so this panics with a `ponko:` message.
    ctx.assets.load_strings(&mut ctx.world, "lang/en.json");
    // A file that may be absent (a mod's language): ask, and carry on without it.
    match ctx.assets.try_load_strings(&mut ctx.world, "lang/fr.json") {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => eprintln!("{e}"),
    }
}
// ANCHOR_END: loader_failure

#[test]
fn the_guide_loader_example_loads_the_required_table_and_skips_the_missing_one() {
    Game::new("loaders", 1, 1).run_headless(0, |ctx| {
        ctx.assets.add_source(
            MemorySource::new("built-in").with("lang/en.json", r#"{"language":"en","strings":{"play":"Play"}}"#),
        );
        load_tables(ctx);
        assert_eq!(ctx.world.resource::<ponko::locale::Strings>().languages(), ["en"]);
    });
}
