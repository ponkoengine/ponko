### Changed
- Screen-space reflections assume a thing as deep as the screen shows it
  tall across the view, up to the old guess that grew with distance: far
  thin things are reflected 1.23 to 1.36 times as wide as in a true
  mirror, from 1.45 to 1.63 (rails 8 to 32 units away, 1920x1080, no
  TAA), and a thin bar near the floor 1.4 times, from 2.3. About 0.1 ms
  more for the trace at 1920x1080 on an Apple M4 Max.

### Fixed
- Temporal antialiasing with soft particles no longer fails the frame: the
  transparent pass, split to read the scene behind the particles, copies
  the lit frame, which now allows it.
