### Added
- `ShadowFilter::ContactHardening` and `Lighting::shadow_sun_size`:
  contact-hardening sun shadows (as "PCSS" elsewhere), sharp where a shadow
  meets its caster and softer the further it falls, as wide as the sun's
  disc (0.53 degrees by default, the real sun) seen from the caster. Each lit
  pixel searches for casters (16 reads), then filters across the width their
  distance gives (16 lookups). The width is in world units, so an edge keeps
  it where two cascades meet; it works with cascade caching, the held sun
  direction, cut-out casters and temporal antialiasing (the lookup pattern
  turns every frame only with TAA on). Off by default; the C API takes it as
  `shadow_filter` 4 (`shadow_sun_size` is Rust-only for now). On the GL
  backend (WebGL2) it draws as `Softest`, and `fit_quality_to` says so. About
  0.3 ms more GPU time than `Softest` at 1080p in the town benchmark scene
  on an Apple M4 Max. The benchmark's `PONKO_RENDER_SHADOW_FILTER` picks the
  filter for comparisons.

### Changed
- The `High` and `Ultra` graphics quality presets use
  `ShadowFilter::ContactHardening` instead of `Softest` (a game that applies
  them gets contact-hardening sun shadows; C reads `shadow_filter` 4 back).
