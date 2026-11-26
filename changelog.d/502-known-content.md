### Added
- Well-known content: `scripts/fetch-known-content.py` fetches the Khronos
  glTF Sample Assets and two public scenes (Sponza, Amazon Lumberyard
  Bistro) at pinned revisions into a git-ignored cache and lists each
  asset's license. A load sweep (`tests/known_content.rs`) records whether
  every sample loads, fails or needs an extension the engine doesn't read,
  an offscreen GPU test draws every loaded sample and compares it with the
  sample's own screenshot, and an ignored release benchmark measures the two
  scenes at 1080p and 1440p for each quality preset. The opt-in
  `known-content` job of `scripts/checks.yml` runs them; the guide's
  "Well-known content" chapter has the results.
- glTF: `EXT_mesh_gpu_instancing` is read: each instance draws the node's
  mesh with its own translation, rotation and scale.
- glTF: `MSFT_texture_dds` images (BCn blocks in DDS files) are read with
  the `compressed-textures` feature, as KTX2 ones are.
- glTF: materials of the retired `KHR_materials_pbrSpecularGlossiness`
  workflow load, approximated: the diffuse color and texture as the base
  color and the glossiness as roughness (a warning says so).

### Fixed
- glTF: triangle strips and fans are drawn (they were left out); points and
  lines still aren't.
- glTF: a file with `KHR_animation_pointer` channels no longer fails to
  load: those channels are left out and its node animations play.
