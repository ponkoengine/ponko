### Changed
- The guide's cost table for `Post::ssr_thin_objects` has the coarse depth
  pass's own GPU time (about 0.03 ms at 1920x1080 on an Apple M4 Max; it
  was counted in the frame only), and an offscreen test checks that the
  pass is timed. Three ways to make the thin-objects trace cheaper (two
  coarse levels instead of four, at most four depth reads in a block a
  step, one coarse-depth look for up to four steps ahead) were measured
  and none came near the 0.2 ms over off that would put it in the High
  preset, so it stays on in Ultra only; the guide has the numbers.
