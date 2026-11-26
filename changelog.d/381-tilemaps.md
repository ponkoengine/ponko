### Breaking changes
- `TileMap` is `#[non_exhaustive]` and has new fields (`parallax`,
  `parallax_origin`, `animations`, `flips`), so it can't be built with a
  struct literal outside the engine any more.
  Instead: build it with `TileMap::new(sheet, width, height)` and set the
  fields you need afterwards.
  Before:
  ```rust
  let map = TileMap { sheet, size: uvec2(64, 64), tile_size: vec2(16.0, 16.0), z: 0.0, color: Color::WHITE, tiles };
  ```
  After:
  ```rust
  use ponko::tilemap::TileMap;
  # let (sheet, tiles) = todo!();
  let mut map = TileMap::new(sheet, 64, 64);
  map.tiles = tiles;
  ```

### Added
- Levels from the Tiled map editor (`.tmx`, `.tmj`, external `.tsx` /
  `.tsj` tilesets, `.tx` templates, groups, infinite maps, CSV, base64,
  zlib and gzip data) and LDtk (`.ldtk` projects, worlds, separate level
  files, tile, auto and IntGrid layers, entities and their fields):
  `Ctx::spawn_level` / `spawn_level_with` spawn every layer as tilemaps,
  every object as an entity with a `LevelObject` (or a prefab instance,
  `LevelOptions::prefab`), custom properties included (`Properties`), and
  with `physics` fixed colliders for the solid tiles. `Ctx::despawn_level`,
  `Assets::load_level` (the level as plain data, `Level`). Saving the level
  (or a tileset or template) with hot reload on spawns it again in place.
  C: `ponko_ctx_spawn_level`, `ponko_level_*`; C++: `ponko::spawn_level`,
  `level_options`, `level_entities`, `level_object_of`.
- Animated tiles (`TileMap::animate`), flipped and rotated tiles
  (`TileMap::set_flipped`, `FLIP_X`, `FLIP_Y`, `FLIP_DIAGONAL`) and parallax
  layers (`TileMap::parallax`, `parallax_origin`). Maps without them draw
  as before. C: `ponko_tilemap_animate`, `ponko_tilemap_set_flipped`,
  `ponko_tilemap_set_parallax`; C++: `ponko::tilemap_animate`,
  `tilemap_set_flipped`, `tilemap_set_parallax`.
- Autotiling for maps built in code: `AutoTile` rules with 4-neighbour (16
  frames) or 8-neighbour (47 "blob" frames, or any mapping) bitmasks,
  `TileMap::autotile` and `TileMap::paint_autotile`. C:
  `ponko_tilemap_autotile`, `ponko_tilemap_paint_autotile`; C++:
  `ponko::autotile_rule`, `tilemap_autotile`, `tilemap_paint_autotile`.
- Tile colliders: `TileMap::solid_rects` merges solid cells into few
  rectangles, and with `physics` `TileMap::colliders` makes a fixed body for
  each. C: `ponko_tilemap_solid_rects`, `ponko_tilemap_spawn_colliders`;
  C++: `ponko::tilemap_solid_rects`, `tilemap_spawn_colliders`.
- The `level` example (a Tiled level with parallax clouds, animated water,
  coins and walls built with an autotile rule), sample level files in
  `assets/levels/`, and the benchmarks `tilemap/draw-1m` and
  `tilemap/draw-1m-animated`.
- New dependencies: `quick-xml` (reads Tiled's XML files) and `flate2`
  (zlib and gzip tile data; already in every build through PNG decoding).
