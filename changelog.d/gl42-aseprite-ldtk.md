### Added
- Aseprite JSON Array sprite sheets load as one texture atlas, with trimmed
  frames restored to the original canvas, optional split layers, named slices
  and variable-duration sprite animations from tags. Edits to the JSON or PNG
  reload live playback. C and C++ can load individual frames or play tags.
- LDtk world placement is available for free, GridVania and linear layouts,
  including projects with several worlds. Games can spawn levels at their
  authored offsets and map entity fields onto registered reflected components
  in Rust, C and C++. Failed level reloads keep the previous entities.
