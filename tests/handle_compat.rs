//! Handles in old scene and save files: components that hold a handle
//! (`Sprite`, `Animation` and `ParticleEmitter2D` a texture; with `3d`,
//! `Model` and `ShadowMesh` a mesh, `ModelMaterial` a material) are saved
//! by the asset's name, never by the handle's number. Files written before
//! render targets got their own handle type (`RenderTarget`, which is never
//! saved) must load into the same typed handles as before.
//!
//! The fixtures are `tests/fixtures/saves/handles_v1.json` (2D) and
//! `handles3d_v1.json` (3D), written by the `#[ignore]`d generators below
//! at the revision named in their provenance files. These tests only read
//! checked-in bytes; never regenerate a fixture to make a test pass.

use ponko::assets::MemorySource;
use ponko::prelude::*;
use serde_json::Value;

fn fixture_text(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/saves")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("can't read {}: {e}", path.display()))
}

/// Writes the world `build` makes to `PONKO_SAVE_FIXTURE_OUT`, refusing to
/// overwrite a file.
fn write_fixture(build: impl FnOnce(&mut Ctx) + 'static) {
    let out = std::env::var("PONKO_SAVE_FIXTURE_OUT").expect("set PONKO_SAVE_FIXTURE_OUT to the file to write");
    assert!(
        !std::path::Path::new(&out).exists(),
        "{out} exists; fixtures are never overwritten"
    );
    Game::new("fixture", 1, 1).run_headless(0, move |ctx| {
        build(ctx);
        let text = serde_json::to_string_pretty(&ctx.world.save()).unwrap() + "\n";
        std::fs::write(&out, text).unwrap();
    });
}

/// The 2D fixture: a sprite, a sprite-sheet animation and a particle
/// emitter, each showing `crab.png` from `assets/`.
#[test]
#[ignore = "writes a retained fixture: run by hand at a recorded revision"]
fn write_handles_v1_fixture() {
    write_fixture(|ctx| {
        let crab = ctx.gfx.load_texture("crab.png");
        ctx.world.spawn((
            ponko::d2::Transform::at(vec2(10.0, 20.0)),
            ponko::d2::Sprite::new(crab).with_z(2.0),
        ));
        let sheet = ponko::sheet::SpriteSheet::new(crab, 2, 2);
        ctx.world.spawn((
            ponko::d2::Transform::at(vec2(-5.0, 0.0)),
            ponko::sheet::Animation::new(sheet, 0..2, 8.0),
        ));
        let settings = ponko::d2::ParticleSettings::default().with_texture(crab);
        ctx.world.spawn((
            ponko::d2::Transform::default(),
            ponko::d2::ParticleEmitter::new(settings),
        ));
    });
}

/// Loads fixture `name` as a scene through the game-facing loader, then
/// hands the context to `check`.
fn load_scene_fixture(name: &'static str, check: impl FnOnce(&mut Ctx) + 'static) {
    Game::new("t", 1, 1).run_headless(0, move |ctx| {
        ctx.assets
            .add_source(MemorySource::new("fixtures").with(name, fixture_text(name)));
        ctx.load_scene(name)
            .unwrap_or_else(|e| panic!("{name} no longer loads: {e}"));
        check(ctx);
    });
}

/// The saved component `component` of every entity in a saved world that
/// has one, in the order they are saved.
fn saved(world: &Value, component: &str) -> Vec<Value> {
    world["entities"]
        .as_array()
        .expect("a saved world has an entities list")
        .iter()
        .filter_map(|entity| entity["components"].get(component).cloned())
        .collect()
}

#[test]
fn old_2d_scene_files_load_their_texture_handles() {
    load_scene_fixture("handles_v1.json", |ctx| {
        // The scene loaded the image first; loading it again is the cached handle.
        let crab: Texture = ctx.gfx.load_texture("crab.png");
        assert!(ctx.is_live(crab));

        let sprites: Vec<ponko::d2::Sprite> = ctx.world.query::<&ponko::d2::Sprite>().iter().copied().collect();
        assert_eq!(sprites.len(), 1);
        assert_eq!(sprites[0].texture, crab);
        assert_eq!(sprites[0].z, 2.0);

        let sheets: Vec<Texture> = ctx
            .world
            .query::<&ponko::sheet::Animation>()
            .iter()
            .map(|a| a.sheet.texture)
            .collect();
        assert_eq!(sheets, [crab]);

        let emitters: Vec<Option<Texture>> = ctx
            .world
            .query::<&ponko::d2::ParticleEmitter>()
            .iter()
            .map(|e| e.settings().texture)
            .collect();
        assert_eq!(emitters, [Some(crab)]);
    });
}

#[test]
fn old_2d_scene_files_save_their_handles_as_they_were_written() {
    let old: Value = serde_json::from_str(&fixture_text("handles_v1.json")).unwrap();
    load_scene_fixture("handles_v1.json", move |ctx| {
        let new = serde_json::to_value(ctx.world.save()).unwrap();
        for component in ["Sprite", "Animation", "ParticleEmitter2D"] {
            let (was, is) = (saved(&old, component), saved(&new, component));
            assert!(!was.is_empty(), "the fixture has a {component}");
            assert_eq!(is, was, "{component} saves as the old file wrote it");
        }
    });
}

#[cfg(feature = "3d")]
mod three {
    use super::*;

    /// The 3D fixture: a model of `models/house.glb` from `assets/` with a
    /// shadow mesh and its "roof" material as an override.
    #[test]
    #[ignore = "writes a retained fixture: run by hand at a recorded revision"]
    fn write_handles3d_v1_fixture() {
        write_fixture(|ctx| {
            let house = ctx.gfx.load_mesh("models/house.glb");
            let material = ctx.gfx.loaded_materials("roof")[0].0;
            ctx.world.spawn((
                ponko::d3::Transform::at(vec3(1.0, 0.0, -2.0)),
                ponko::render::Model::new(house),
                ponko::render::ShadowMesh(Some(house)),
                ponko::render::ModelMaterial(material),
            ));
        });
    }

    #[test]
    fn old_3d_scene_files_load_their_mesh_and_material_handles() {
        load_scene_fixture("handles3d_v1.json", |ctx| {
            let house: ponko::render::Mesh = ctx.gfx.load_mesh("models/house.glb");
            let material: ponko::render::Material = ctx.gfx.loaded_materials("roof")[0].0;
            assert!(ctx.is_live(house) && ctx.is_live(material));

            let loaded: Vec<(
                ponko::render::Model,
                ponko::render::ShadowMesh,
                ponko::render::ModelMaterial,
            )> = ctx
                .world
                .query::<(
                    &ponko::render::Model,
                    &ponko::render::ShadowMesh,
                    &ponko::render::ModelMaterial,
                )>()
                .iter()
                .map(|(m, s, mm)| (*m, *s, *mm))
                .collect();
            assert_eq!(loaded.len(), 1);
            let (model, shadow, model_material) = loaded[0];
            assert_eq!(model.mesh, house);
            assert_eq!(shadow.0, Some(house));
            assert_eq!(model_material.0, material);
        });
    }

    #[test]
    fn old_3d_scene_files_save_their_handles_as_they_were_written() {
        let old: Value = serde_json::from_str(&fixture_text("handles3d_v1.json")).unwrap();
        load_scene_fixture("handles3d_v1.json", move |ctx| {
            let new = serde_json::to_value(ctx.world.save()).unwrap();
            for component in ["Model", "ShadowMesh", "ModelMaterial"] {
                let (was, is) = (saved(&old, component), saved(&new, component));
                assert!(!was.is_empty(), "the fixture has a {component}");
                assert_eq!(is, was, "{component} saves as the old file wrote it");
            }
        });
    }
}
