### Added
- `ponko::render::RenderTarget` (with `3d`): a render target is its own handle kind, so an image can't be passed where a camera, a compute pass, a target effect or a UI panel draws, and a render target can't be passed where only a plain texture belongs. `target.texture()` reads it as the texture it draws into (free, `const`); `width`, `height` and `size` as on `Texture`; `RenderTarget::NONE`. It is a `#[repr(transparent)]` wrapper of `Texture`, with the same size and layout; `ctx.is_live`, `ctx.free`, `ctx.gfx.is_live` and `ctx.gfx.free` take it like any handle, and a target and its texture live and die together.
- Every handle kind is now listed and documented as its own type (the `Handle` docs, with compile-fail examples, and the guide's Handles chapter, "Each kind is its own type"). `tests/handle_layout.rs` asserts every kind's size and alignment, and `tests/handle_compat.rs` loads scene files with texture, mesh and material handles written before this change (`tests/fixtures/saves/handles_v1.json`, `handles3d_v1.json`).
- C: `PONKO_HANDLE_RENDER_TARGET` (13), the render target kind checked at run time. `ponko_ctx_handle_is_live` with it answers whether a texture is a live render target; `ponko_ctx_handle_free` with it frees one, and refuses a plain texture (`PONKO_INVALID_ARGUMENT`). The C ABI version is unchanged: the enum only gained a value.
- C++: `ponko::render_target`, a `PonkoTexture` that converts to `ponko::texture` and has `texture()`; `ctx.render_target_of(texture)` checks that a texture read back from the engine is a render target and throws `ponko::error` if not. `ponko::is_live`/`free` take it with the render target kind.

### Changed
- **Breaking:** `Graphics::create_render_target` and `try_create_render_target` return a `RenderTarget` instead of a `Texture`. `TargetCamera::new`, `TargetCamera::reflection`, `ComputePass::new`, `TargetEffect::new` and `d3::UiPanel::new` take a `RenderTarget`, and their `target` fields are one. Pass `target.texture()` where the target is read as a texture.
- **Breaking:** `d3::UiPanel`'s `texture: Texture` field is `target: RenderTarget`.
- **Breaking:** `ComputePass::for_buffer` sets `target` to `RenderTarget::NONE` (it was a zero-sized `Texture` with id 0).
- **Breaking:** Rust code matching on `ponko_ffi::PonkoHandleKind` needs an arm for `PONKO_HANDLE_RENDER_TARGET`.
- **Breaking:** C++: `ctx.create_render_target` and `ctx.create_ui_panel` return a `ponko::render_target`; `target_camera_of`, `compute_pass_of`, `target_effect_of` and `ctx.reflection_camera` take one and no longer accept a `ponko::texture`.
- C: `ponko_ctx_reflection_camera` refuses a texture that isn't a render target of the game (`PONKO_INVALID_ARGUMENT`), as the target camera, compute pass and target effect lists already did when set.

### Breaking changes
- Render targets have their own handle type, `RenderTarget`, instead of `Texture`.
  `create_render_target`, `try_create_render_target` and a UI panel's `target` (it was `texture`) give a `RenderTarget`; `TargetCamera::new`, `TargetCamera::reflection`, `ComputePass::new`, `TargetEffect::new` and `d3::UiPanel::new` take only that, and their `target` fields are one. Scenes and saves are unaffected (they never stored render targets, and store other handles by name).
  Instead: the migration is mechanical. (1) Keep what `create_render_target` returns as a `RenderTarget` (`use ponko::render::RenderTarget;`), and change helper functions, struct fields and collections that hold a render target from `Texture` to `RenderTarget`. (2) Where a target is read as a texture (`MaterialDesc::with_custom_textures`, `with_texture`, `with_emissive_map`, `with_normal_map`, `Sprite::new`, `ctx.gfx.draw`, `update_texture`, `is_render_target`), write `target.texture()`. (3) `panel.texture` becomes `panel.target.texture()`. (4) Comparing a pass's `target` with a `Texture`: compare `target.texture()`, or keep the `RenderTarget` and compare that. (5) Code reading `c.target.id` reads `c.target.texture().id`. The compiler names every place: each is a type mismatch between `Texture` and `RenderTarget`.
  Before:
  ```rust
  let ripples: Texture = ctx.gfx.create_render_target(256, 256);
  ctx.scene3d.passes.compute.push(ComputePass::new(waves, ripples));
  let water = MaterialDesc::default().with_custom_textures([Some(ripples), None]);
  let (panel, model) = ctx.gfx.create_ui_panel(600, 400, vec2(1.2, 0.8));
  let screen = MaterialDesc::default().with_emissive_map(panel.texture);
  ```
  After:
  ```rust
  use ponko::render::RenderTarget;
  let ripples: RenderTarget = ctx.gfx.create_render_target(256, 256);
  ctx.scene3d.passes.compute.push(ComputePass::new(waves, ripples));
  let water = MaterialDesc::default().with_custom_textures([Some(ripples.texture()), None]);
  let (panel, model) = ctx.gfx.create_ui_panel(600, 400, vec2(1.2, 0.8));
  let screen = MaterialDesc::default().with_emissive_map(panel.target.texture());
  ```
- C++: render targets are `ponko::render_target`, and the calls that draw into one take only that.
  `ctx.create_render_target` and `ctx.create_ui_panel` return a `ponko::render_target`, which converts to `ponko::texture` wherever a texture is read. `target_camera_of`, `compute_pass_of`, `target_effect_of` and `ctx.reflection_camera` no longer accept a `ponko::texture`.
  Instead: hold render targets as `ponko::render_target` (or `auto`) and take it in helpers; for a target read back from the engine (a `PonkoTargetCamera`'s, `PonkoComputePass`'s or `PonkoTargetEffect`'s `target`, which stay plain `PonkoTexture`s in C), call `ctx.render_target_of(t.target)`, which checks it and throws `ponko::error` if it isn't one. C code is unchanged, except that `ponko_ctx_reflection_camera` now refuses a texture that isn't a render target.
  Before:
  ```cpp
  ponko::texture screen = ctx.create_render_target(320, 180);
  for (auto& t : list) t = ctx.reflection_camera(ctx.camera3d(), t.target, 0);
  ```
  After:
  ```cpp
  ponko::render_target screen = ctx.create_render_target(320, 180);
  for (auto& t : list) t = ctx.reflection_camera(ctx.camera3d(), ctx.render_target_of(t.target), 0);
  ```
