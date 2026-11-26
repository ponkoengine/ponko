### Added
- `Sprite::flip_x` and `Sprite::flip_y` (with `with_flip_x` / `with_flip_y`)
  mirror a sprite's image in place: it covers the same rectangle around the
  same pivot, and attached children and their offsets are left alone, unlike
  a negative `Transform` scale. Works with regions, sheets, blend modes,
  materials, render layers and normal-mapped 2D lighting. Prefabs, scenes,
  saves and the world hash keep the flips (saved as `"flip_x": true` and
  `"flip_y": true`, only when set); an unflipped sprite saves and hashes
  exactly as before, and saves and prefabs written before load unflipped.
- Known gap: the C ABI's `PonkoSprite` has no flip fields yet (its layout is
  fixed), so a sprite read and written back through the C API comes back
  unflipped.

### Breaking changes
- `Sprite` has two new public fields, `flip_x` and `flip_y`.
  Code that builds a `Sprite` with a struct literal naming every field no
  longer compiles.
  Instead: start from `Sprite::new` and the `with_*` builders, or add the two
  fields (`false` keeps the old look).
  Before:
  ```rust
  let s = Sprite { texture, color, z: 0.0, pivot: Vec2::splat(0.5), region: None };
  ```
  After:
  ```rust
  # let (texture, color): (Texture, Color) = todo!();
  let s = Sprite::new(texture).with_color(color);
  ```
