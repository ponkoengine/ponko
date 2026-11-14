### Added
- Reflection probes that refresh by themselves: a `d3::ReflectionRefresh` on
  a probe's entity captures it again `Never`, `OnDemand`
  (`Graphics::refresh_reflection_probe`), `Every(seconds)` or when the sun or
  sky changed past a threshold (`ProbeRefresh::LightingChanged`). A refresh
  is spread over 14 frames, at most one step a frame for all probes (one
  128² face rendered or filtered, the harmonics, or the diffuse light; at
  most 0.7 ms each on the GPU tested), into a copy that then eases in over
  `blend_seconds`, so a changing sky never pops. Each refreshing probe's copy
  (2.5 MiB) counts under `probes` in the GPU memory report. Rust only for
  now: C and C++ games bake again with `ponko_ctx_bake_reflection_probe`.
