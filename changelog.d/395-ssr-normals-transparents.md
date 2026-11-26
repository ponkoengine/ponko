### Added
- `Post::ssr_transparent` (C: `PonkoPostEx` version 4, `ssr_transparent`):
  screen-space reflections on glossy transparent surfaces such as water and
  glass. Each such pixel marches its reflected ray across the opaque scene
  behind it. Off by default and in every quality preset.

### Changed
- Screen-space reflections follow normal maps, decals and normals set by
  material shaders: a thin pass writes the lit normal while reflections are
  on (about 0.1 ms at 1080p on a fast GPU, 4 bytes a pixel). With `ssr` at 0
  nothing new is allocated or drawn.
