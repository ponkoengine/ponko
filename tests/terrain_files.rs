//! Terrain tiles from terrain files (`"format": "ponko-terrain"`): an
//! entity with a `TerrainFile` gets its tile however it arrives (a prefab,
//! JSON spawned while the game runs), a missing file is an error naming it
//! with nothing left half made, and a moved tile's collision moves with it.
#![cfg(feature = "3d")]

use std::cell::RefCell;
use std::rc::Rc;

use ponko::assets::MemorySource;
use ponko::d3::Transform;
use ponko::prelude::*;
use ponko::reflect::WorldReflect;
use ponko::terrain::{TerrainFile, TerrainTile};
use serde_json::json;

/// A 16-bit greyscale PNG of `n` × `n` heights, all `level`.
fn flat_heights(n: u32, level: u16) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, n, n);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Sixteen);
    let bytes: Vec<u8> = (0..n * n).flat_map(|_| level.to_be_bytes()).collect();
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&bytes))
        .unwrap();
    out
}

/// A flat 16-unit tile at height 2 (`terrain/flat.json`) and a prefab of
/// it (`prefabs/flat.json`), plus a prefab naming a file that isn't there.
fn assets(ctx: &mut Ctx) {
    let file = json!({"format": "ponko-terrain", "size": 16.0, "heights": "flat-heights.png", "low": 0.0, "high": 4.0});
    let prefab = |path: &str| {
        serde_json::to_vec(&json!({"format": "ponko-prefab", "version": 1, "components": {
            "Transform3D": {"position": [0.0, 0.0, 0.0]},
            "TerrainFile": {"path": path},
        }}))
        .unwrap()
    };
    ctx.assets.add_source(
        MemorySource::new("mem")
            .with("terrain/flat.json", serde_json::to_vec(&file).unwrap())
            .with("terrain/flat-heights.png", flat_heights(5, 32768))
            .with("prefabs/flat.json", prefab("terrain/flat.json"))
            .with("prefabs/lost.json", prefab("terrain/lost.json")),
    );
}

fn game() -> Game {
    Game::new("terrain files", 1, 1).hot_reload(false)
}

#[test]
fn a_prefab_naming_a_terrain_file_spawns_with_its_tile_where_it_is_placed() {
    game().run_headless(0, |ctx| {
        assets(ctx);
        let e = ctx.spawn_prefab("prefabs/flat.json", vec3(10.0, 0.0, -4.0)).unwrap();
        let tile = *ctx
            .world
            .get::<TerrainTile>(e)
            .expect("the prefab has its tile at once");
        assert!(ctx.is_live(tile));
        assert_eq!(ctx.world.get::<Transform>(e).unwrap().position, vec3(10.0, 0.0, -4.0));
        // A second copy gets a tile of its own.
        let other = ctx.spawn_prefab("prefabs/flat.json", vec3(30.0, 0.0, -4.0)).unwrap();
        assert_ne!(*ctx.world.get::<TerrainTile>(other).unwrap(), tile);
    });
}

#[test]
fn a_prefab_naming_a_missing_terrain_file_is_an_error_naming_it_and_spawns_nothing() {
    game().run_headless(0, |ctx| {
        assets(ctx);
        let before = ctx.world.len();
        let err = ctx.spawn_prefab("prefabs/lost.json", Vec3::ZERO).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound, "{err}");
        assert!(err.to_string().contains("terrain/lost.json"), "{err}");
        assert_eq!(ctx.world.len(), before, "nothing is left half made");
    });
}

#[test]
fn an_entity_spawned_from_json_while_the_game_runs_gets_its_tile_and_a_missing_file_despawns_it() {
    let spawned = Rc::new(RefCell::new(Vec::new()));
    let seen = spawned.clone();
    let world = game()
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 0 {
                assets(ctx);
                for path in ["terrain/flat.json", "terrain/lost.json"] {
                    let e = ctx
                        .world
                        .spawn_json(
                            json!({"Transform3D": {"position": [4.0, 0.0, 4.0]}, "TerrainFile": {"path": path}}),
                        )
                        .unwrap();
                    seen.borrow_mut().push(e);
                }
            }
        })
        .run_headless(2, |_| {});
    let [found, lost] = spawned.borrow()[..] else {
        panic!("two spawned")
    };
    assert!(
        world.get::<TerrainTile>(found).is_some(),
        "the tile is made after the spawn"
    );
    assert_eq!(world.get::<TerrainFile>(found).unwrap().path, "terrain/flat.json");
    assert!(
        !world.is_alive(lost),
        "an entity whose file isn't there doesn't stay without its ground"
    );
}

#[cfg(feature = "physics3d")]
#[test]
fn a_moved_terrain_tile_is_solid_where_it_is_drawn_and_not_where_it_was() {
    use ponko::d3::{QueryFilter, TerrainCollider};
    let hits = Rc::new(RefCell::new(Vec::new()));
    let log = hits.clone();
    let tile = Rc::new(RefCell::new(None));
    let made = tile.clone();
    game()
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            let down = |ctx: &Ctx, x: f32| {
                ctx.physics3d
                    .cast_ray(vec3(x, 10.0, 8.0), Vec3::NEG_Y, 50.0, QueryFilter::default())
                    .map(|h| h.point.y)
            };
            match ctx.time.sim_tick() {
                0 => {
                    assets(ctx);
                    // As a scene or a prefab names them: the collider's mesh
                    // is made from the file with the tile.
                    let collider = serde_json::to_value(TerrainCollider::new(Handle::NONE)).unwrap();
                    let e = ctx
                        .world
                        .spawn_json(json!({
                            "Transform3D": {"position": [0.0, 0.0, 0.0]},
                            "TerrainFile": {"path": "terrain/flat.json"},
                            "TerrainCollider": collider,
                        }))
                        .unwrap();
                    *made.borrow_mut() = Some(e);
                }
                2 => {
                    log.borrow_mut().push((down(ctx, 8.0), down(ctx, 108.0)));
                    // Moved by code: 100 units along X.
                    let e = made.borrow().unwrap();
                    ctx.world.get_mut::<Transform>(e).unwrap().position.x = 100.0;
                }
                4 => log.borrow_mut().push((down(ctx, 8.0), down(ctx, 108.0))),
                _ => {}
            }
        })
        .run_headless(5, |_| {});
    let hits = hits.borrow();
    let near = |h: Option<f32>| h.is_some_and(|y| (y - 2.0).abs() < 0.01);
    assert!(near(hits[0].0) && hits[0].1.is_none(), "before the move: {:?}", hits[0]);
    assert!(hits[1].0.is_none() && near(hits[1].1), "after the move: {:?}", hits[1]);
}
