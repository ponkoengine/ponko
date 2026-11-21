### Changed
- `GraphicsQuality::High` now turns on `Post::ssr_thin_objects` (thin
  things stay whole in screen-space reflections without TAA), as `Ultra`
  does; the C presets follow. Games on the High preset get it with no
  change: about 0.2 ms more GPU time at 2560x1440 on an Apple M4 Max (a
  floor and a box; over thin rails it costs less than reflections without
  it). Set `ctx.scene3d.post.ssr_thin_objects = false` after the preset to
  keep the old picture. Render benchmark baselines recorded on High need
  recording again: their scenes now draw it.
- The thin-objects reflection trace walks the blocks of its coarse depth
  (a hierarchical, "Hi-Z", march) instead of the trace's long steps: about
  a third of its cost before at 2560x1440 (+0.19 ms over off in High, from
  about +0.6 ms), with the same results in the thin-object tests (4.7% of
  the reflected light differs on the rails, 0.04% on the bars). A smaller
  trace target was measured and rejected (see the 3D guide).

### Fixed
- On Metal, a GPU timer's timestamp query set is counted against the
  process's 32 until its device has really freed it, not only until the
  timer is dropped: dropping and making timers again while their frames
  were still on the GPU could hold more sets than counted, and a set past
  32 loses the device.
