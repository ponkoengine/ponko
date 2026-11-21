### Added
- Editor authoring tools, all through the editor's undo history and saved in
  the formats the runtime loads:
  - a **Particles** section in the inspector for 2D and 3D `ParticleEmitter`s:
    every setting, a size curve and a colour gradient, and a live preview
    while the game is paused;
  - **Author → Tilemap…**: paint, erase and autotile Tiled `.tmj` maps from
    a tileset palette, with the autotile rule kept as a layer property;
  - **Author → Animation graph…**: a 2D skeleton's states and transitions as
    a graph, edited in place, with a preview driven by test parameter values;
  - **Author → Materials…** and **Shader…**: glTF material values and
    textures edited live, material shaders edited with their errors shown at
    the line, custom values edited live with **Copy as Rust**;
  - **Author → Terrain…**: raise, lower and smooth brushes and painted
    ground layers.
- A terrain file format (`"format": "ponko-terrain"`, JSON with a 16-bit
  height PNG and RGBA layer-weight PNGs) and `Ctx::spawn_terrain(path, at)`,
  which spawns it as a `TerrainTile`.
