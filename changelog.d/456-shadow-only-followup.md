### Fixed
- `ShadowOnly` is kept by world saves, scene files and prefabs, and shows in
  the inspector: it is reflected by default and written as
  `"ShadowOnly": null`.
- Plain shadow-only models hidden by occlusion culling are no longer counted
  in the frame's occluded models, as skinned ones already weren't: no camera
  draws them.

### Added
- C and C++: `ponko_world_set_shadow_only` and `ponko_world_is_shadow_only`
  (`world::set_shadow_only` and `world::shadow_only` in C++).
