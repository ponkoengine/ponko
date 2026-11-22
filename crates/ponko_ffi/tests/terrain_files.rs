//! Terrain files spawned as tiles through the C API: the tile, its corner
//! and the `TerrainFile` that saves it in a scene, and a missing file.
mod common;
use common::{last_error, run_game};
use ponko_ffi::*;
use std::ffi::CString;

use PonkoStatus::*;

fn asset(name: &str) -> CString {
    CString::new(format!("{}/../../assets/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn a_terrain_file_spawns_as_a_tile_at_its_corner_and_a_missing_one_is_not_found() {
    let bump = asset("terrain/bump.json");
    let nowhere = asset("terrain/nowhere.json");
    unsafe {
        let game = ponko_game_new(c"terrain file".as_ptr(), 64, 64);
        run_game(game, 0, |ctx| {
            let at = PonkoVec3 {
                x: -4.0,
                y: 1.0,
                z: -4.0,
            };
            let mut hill = 0;
            assert_eq!(
                ponko_ctx_terrain_tile_spawn_from_file(ctx, bump.as_ptr(), at, &mut hill),
                PONKO_OK,
                "{}",
                last_error()
            );
            let w = ponko_ctx_world(ctx);
            let (mut tile_id, mut transform_id) = (0, 0);
            assert_eq!(ponko_terrain_tile_component(w, &mut tile_id), PONKO_OK);
            assert_eq!(ponko_transform3d_component(w, &mut transform_id), PONKO_OK);
            let mut tile = PonkoTerrainTile {
                lods: [PonkoMesh { id: 0 }; PONKO_TERRAIN_LODS],
                lod_distance: 0.0,
            };
            assert_eq!(
                ponko_get(w, hill, tile_id, (&raw mut tile).cast(), size_of::<PonkoTerrainTile>()),
                PONKO_OK
            );
            assert_eq!(tile.lod_distance, 8.0, "the file's size");
            let mut placed = ponko_transform3d_at(PonkoVec3::default());
            assert_eq!(
                ponko_get(
                    w,
                    hill,
                    transform_id,
                    (&raw mut placed).cast(),
                    size_of::<PonkoTransform3D>()
                ),
                PONKO_OK
            );
            assert_eq!(placed.position, at);

            let mut none = 0;
            assert_eq!(
                ponko_ctx_terrain_tile_spawn_from_file(ctx, nowhere.as_ptr(), at, &mut none),
                PONKO_NOT_FOUND
            );
            assert!(last_error().contains("nowhere.json"), "{}", last_error());
            assert_eq!(
                ponko_ctx_terrain_tile_spawn_from_file(ctx, bump.as_ptr(), at, std::ptr::null_mut()),
                PONKO_INVALID_ARGUMENT
            );
        });
    }
}
