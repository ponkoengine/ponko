### Changed
- In a web build with threads, the engine's own parallel passes run on the
  frame's workers, through the same runner as parallel queries, instead of
  calling the thread pool from the page's thread (where they ran alone):
  spatial grid rebuilds, flow field directions, 2D skeleton drawing and its
  copy into the draw list, and 3D character moves. The browser test checks
  that the grid rebuild and the flow field went there.

### Fixed
- Safe area insets fed by a scenario step hold for as long as the scenario
  runs: a windowed web run that re-reads the page's insets on a resize no
  longer overwrites them. The page's insets apply once the scenario ends.
