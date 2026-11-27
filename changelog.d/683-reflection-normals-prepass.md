<!-- area: Rendering -->
### Changed
- Screen-space reflections get their normals from the depth prepass, which
  writes them into a second color target as it draws each solid and
  cut-out surface, instead of from a separate pass that drew every solid
  surface again. The images are the same. On an Apple M4 Max (1280×720,
  4× MSAA, `high`) the render benchmark's frames got up to 0.6 ms shorter
  (`static-world` 5.17 → 4.53 ms) and `multi-material` makes 513 fewer
  draw calls (CPU 2.28 → 1.85 ms). The render baseline is recorded again.

### Fixed
- `ponko_bench` no longer keeps 128 MiB resident in every run: the bloom
  filter membership benchmark filled its cache-flush buffer before
  checking whether it would run. The render benchmark's peak memory is
  back to 236 MB (it was 363).
