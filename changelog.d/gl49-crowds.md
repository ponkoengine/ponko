### Added
- Crowds: thousands of animated skinned characters in a few draws.
  `CrowdAnimation::bake(skeleton, clips, fps)` samples clips (`CrowdClip`,
  looping or held) once; `Graphics::create_crowd(animation, levels,
  instances, options)` makes a `Crowd` of `CrowdInstance`s (transform, tint,
  clip and time, a second clip blended in, speed) drawn with up to 8
  `CrowdLevel`s (a skinned mesh per distance band, joints per corner,
  camera and shadow switches). The clips play on by themselves between
  updates; `update_crowd` replaces the characters, `crowd_len` counts them
  and `Ctx::free` frees a crowd. `try_create_crowd` and `try_update_crowd`
  return errors instead of panicking.
- On devices with compute culling a crowd is culled per view in a compute
  pass and drawn with one indirect draw per view, level and mesh part,
  skinned in the vertex shader from the bake, with sun and light shadows
  and motion vectors for temporal antialiasing. Other devices (WebGL2) take
  a CPU path: frames posed when the crowd is made, CPU culling, one draw
  per frame shown. Scenes without crowds make no crowd buffers, passes or
  uploads.
- `LiveCounts::crowds`.
- The `crowd` example: a plaza of 10,000 people, section after section
  cheering. A benchmark (`crowd_benchmark`, ignored) against the same
  characters as skinned models; the guide's "Crowds" chapter has the
  numbers.
- C and C++: `ponko_ctx_bake_crowd_animation`, `ponko_crowd_animation_clip`,
  `ponko_crowd_animation_free`, `ponko_ctx_create_crowd`,
  `ponko_ctx_update_crowd`, `ponko_ctx_crowd_len`, `ponko_ctx_free_crowd`,
  `ponko_crowd_instance_at` and the `PonkoCrowd*` types;
  `ctx::bake_crowd_animation`, `ctx::create_crowd` and friends and
  `ponko::crowd_animation` in `ponko.hpp`.
