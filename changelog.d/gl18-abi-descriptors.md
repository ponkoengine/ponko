### Breaking changes
- C: `ponko_game_run` returns `PONKO_UNSUPPORTED` (was `PONKO_INVALID_ARGUMENT`) when the game can't start on this machine.
  That is: no usable GPU or graphics device, no window or event loop, or a
  library built without the `window` feature. Other statuses are unchanged,
  and C++'s `game::run` still throws a `ponko::error` of kind
  `PONKO_ERROR_UNSUPPORTED`.
  Instead: code that checks `!= PONKO_OK` needs nothing; code that compared
  the result with `PONKO_INVALID_ARGUMENT` to detect a missing GPU compares
  it with `PONKO_UNSUPPORTED`.
  Before:
  ```c
  if (ponko_game_run(game, setup, NULL) == PONKO_INVALID_ARGUMENT)
      fprintf(stderr, "this game needs a GPU\n");
  ```
  After:
  ```c
  if (ponko_game_run(game, setup, NULL) == PONKO_UNSUPPORTED)
      fprintf(stderr, "this game needs a GPU\n");
  ```

### Added
- Versioned C descriptors: settings structs that can gain fields within an
  ABI version. Each starts with `struct_size` and `version`, holds its ABI 2
  struct, and has its own entry points next to the ABI 2 ones:
  `PonkoMaterialDescEx` (`ponko_ctx_create_material_ex`,
  `ponko_ctx_update_material_ex`, with the stencil state),
  `PonkoNavBakeSettingsEx` (`ponko_ctx_bake_navmesh_ex`), `PonkoLightingEx`
  (`ponko_ctx_lighting_ex`, `ponko_ctx_set_lighting_ex`, with
  `shadow_sun_size` for contact-hardening shadows and `shadow_sun_step`),
  `PonkoColliderEx` (`ponko_set_collider_ex`, `ponko_get_collider_ex`: a 2D
  collider with its collision groups and polygon corners in one call) and
  `PonkoQueryFilterEx` (a `ponko_ctx_*_ex` form of every 2D and 3D query,
  collision groups included). STABILITY.md's "Versioned descriptors" has the
  rule; C++ has `ponko::material_desc_ex` and friends with helpers that fill
  the header.
- Humanoid retargeting from C and C++: `PonkoHumanoidMap`
  (`ponko_ctx_detect_humanoid_map`, `ponko_humanoid_map_get`, `_set`,
  `_missing`), `PonkoRetargeter` (`ponko_ctx_create_retargeter`,
  `ponko_retarget_clip`, `ponko_ctx_retarget_pose`), `PonkoHumanBone`, and
  the C++ `ponko::humanoid_map` and `ponko::retargeter`.
- Client prediction for native games: `PonkoPrediction` and
  `PonkoInputQueue` over fixed-size byte states and inputs, with a step
  callback, and `ponko::prediction<State, Input>` and
  `ponko::input_queue<Input>` in C++ for trivially copyable types. Replay
  is deterministic; a step that throws stops the call and the exception
  comes out of it. A steady predict-and-reconcile loop allocates nothing.
- `PONKO_UNSUPPORTED`, a `PonkoStatus` for "this machine or build can't do
  it".
