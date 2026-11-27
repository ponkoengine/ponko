<!-- area: C and C++ -->
### Added
- C and C++ read and set a sprite's flips: `ponko_sprite_set_flip(world, entity, flip_x, flip_y)` and
  `ponko_sprite_flip(world, entity, &flip_x, &flip_y)` (C++: `world_view::set_sprite_flip` and
  `world_view::sprite_flip`, which returns `sprite_flips`). `PonkoSprite` keeps its fixed layout.

### Fixed
- Writing a `PonkoSprite` back with `ponko_set`, `ponko_ctx_set_later` or `world_view::set` no longer clears the
  flips of the sprite the entity already has. A sprite added to an entity that had none starts unflipped.
