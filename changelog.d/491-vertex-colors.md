### Added
- `MaterialDesc::masked_tint` (`with_masked_tint`): the model's colour
  (`Model::color`) tints only as far as the vertex colour's alpha says, so
  one mesh and one material take a colour per model on part of it (shirts,
  car paint), skinned meshes included. C and C++: `masked_tint` in
  `PonkoMaterialDescEx` version 2.

### Changed
- Vertex colours (`MeshData::colors`, glTF `COLOR_0`) are kept as half
  floats instead of 8 bits per channel: red, green and blue above 1 stay
  brighter than white (alpha stays 0 to 1). A mesh corner on the GPU takes
  48 bytes instead of 44 (about 9% more vertex memory); drawing time
  measured the same.
