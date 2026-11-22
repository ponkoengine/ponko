//! Terrain files (`"format": "ponko-terrain"`, as the editor's terrain
//! window saves them) spawned as tiles. See `TerrainTile::spawn_from_file`
//! in Rust and the guide's editor chapter.
use std::ffi::c_char;

use ponko::terrain::TerrainTile;

use crate::ctx::{PonkoCtx, ctx_access, out, text};
use crate::three::PonkoVec3;
use crate::{PonkoStatus, boundary, engine_error, invalid};

/// Spawns the tile of terrain file `path` with its corner at `at` (Rust
/// `TerrainTile::spawn_from_file`): an entity with a `PonkoTransform3D`, a
/// `PonkoTerrainTile` and the reflected `TerrainFile` that saves it in a
/// scene (as the file and its place; loading the scene makes the tile
/// again). `PONKO_NOT_FOUND` for a file that isn't there (the terrain file
/// or an image it names), `PONKO_INVALID_ARGUMENT` for one that doesn't read; the
/// last error says why, and nothing is spawned then.
/// ```
/// let at = ponko_ffi::PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_terrain_tile_spawn_from_file(std::ptr::null_mut(), c"t.json".as_ptr(), at, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// Live ctx; `path` NUL-terminated; `out_entity` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_terrain_tile_spawn_from_file(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    at: PonkoVec3,
    out_entity: *mut u64,
) -> PonkoStatus {
    boundary(|| {
        if out_entity.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let path = text(path)?;
            ctx_access(ctx, |c| {
                let at = ponko::prelude::vec3(at.x, at.y, at.z);
                let e = TerrainTile::spawn_from_file(c, path, at).map_err(engine_error)?;
                out(out_entity, e.to_bits())
            })
        }
    })
}
