### Breaking changes
- `ponko_ffi::PonkoHandleKind` has three new values,
  `PONKO_HANDLE_SPRITE_SHADER`, `PONKO_HANDLE_SPRITE_MATERIAL` and
  `PONKO_HANDLE_RENDER_TARGET`, so a Rust `match` over every value no longer
  compiles. C and C++ code is not affected.
  Instead: add arms for the new values, or a `_` arm.
  Before:
  ```rust
  match kind {
      PonkoHandleKind::PONKO_HANDLE_TEXTURE => free_texture(id),
      // ... one arm per value up to 0.1.0
  }
  ```
  After:
  ```rust
  use ponko_ffi::PonkoHandleKind;
  # fn free_texture(id: u64) {}
  # let (kind, id): (PonkoHandleKind, u64) = todo!();
  match kind {
      PonkoHandleKind::PONKO_HANDLE_TEXTURE => free_texture(id),
      // ... the 0.1.0 arms
  #   PonkoHandleKind::PONKO_HANDLE_SOUND | PonkoHandleKind::PONKO_HANDLE_FONT
  #   | PonkoHandleKind::PONKO_HANDLE_MESH | PonkoHandleKind::PONKO_HANDLE_MATERIAL
  #   | PonkoHandleKind::PONKO_HANDLE_SHADER | PonkoHandleKind::PONKO_HANDLE_COMPUTE
  #   | PonkoHandleKind::PONKO_HANDLE_EFFECT | PonkoHandleKind::PONKO_HANDLE_GPU_BUFFER
  #   | PonkoHandleKind::PONKO_HANDLE_INSTANCE_BATCH
  #   | PonkoHandleKind::PONKO_HANDLE_TEXTURE_ARRAY => {}
      PonkoHandleKind::PONKO_HANDLE_SPRITE_SHADER
      | PonkoHandleKind::PONKO_HANDLE_SPRITE_MATERIAL
      | PonkoHandleKind::PONKO_HANDLE_RENDER_TARGET => {}
  }
  ```