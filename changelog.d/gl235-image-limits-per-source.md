### Added
- An asset source can carry `ImageLimits` of its own: pictures read from it (a mod pack, a download cache, a
  folder) are decoded under them and everything else under the game's. `Assets::set_source_image_limits`,
  `clear_source_image_limits`, `source_image_limits`, `image_limit_sources`, and
  `add_source_with_image_limits` / `add_background_source_with_image_limits`. The limits travel with the decode
  job to worker threads (asset groups, streamed textures), and also cover a model's pictures, texture array
  layers, a level's tilesets, terrain weight maps and Aseprite sheets, by the source their file came from.
  The security chapter says how to give mods tight limits.
- A picture refused by the image limits says where they came from: `the game's limits`, `the limits of asset
  source "mods"`, or `limits given for this picture`.
- The limits in force are read only: the control endpoint method `assets.image_limits` (MCP tool
  `image_limits`) lists the game's and each source's, and the inspector's asset view shows them. No remote call
  changes them.
- C: `ponko_ctx_set_source_image_limits`, `ponko_ctx_clear_source_image_limits`,
  `ponko_ctx_source_image_limits`; C++: `ctx::set_source_image_limits`, `clear_source_image_limits`,
  `source_image_limits`.
