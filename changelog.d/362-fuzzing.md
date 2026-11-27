### Added
- Fuzz targets (`fuzz/`, run with `cargo fuzz`) for the loaders that read
  untrusted data: scenes and world saves, prefabs, glTF models, the Unity
  importer's text and binary readers, action maps, asset packs, and lockstep
  and message packets. `fuzz/run-all.sh` runs each for the time
  `fuzz/targets.txt` gives it. See
  `fuzz/README.md` and CONTRIBUTING.md, "Fuzzing".

### Fixed
- A glTF animation channel whose target names a missing node is a decode
  error instead of a panic, and channels with a path this loader doesn't
  read (another extension's, such as `pointer`) are skipped.
- A glTF accessor of a type glTF doesn't allow for its use (a `NORMAL` that
  isn't `VEC3`, float indices, colors of signed bytes), with no elements, a
  stride shorter than an element or an end past 2^64 is a decode error; such
  files panicked the loader, some in release builds too. Positions, normals,
  texture coordinates and morph targets must also fit their buffer view and
  have as many elements as `POSITION`, so a file can't claim a billion
  corners and make the loader set aside room for them.
- A glTF primitive whose `POSITION` names a missing accessor is a decode
  error instead of a panic in glTF validation.
- A glTF file whose nodes don't form trees (a node that is its own
  ancestor, has two parents, or is a scene root and a child) is a decode
  error; a cycle made the loader walk the scene forever.
- A `.glb` whose header gives a length under 12 bytes is a decode error
  instead of a panic in builds with overflow checks.
- A lockstep peer ignores a forged input packet whose ticks run past
  `u64::MAX` instead of panicking (debug builds) or wrapping (release).
