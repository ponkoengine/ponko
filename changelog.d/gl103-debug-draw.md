### Added
- The inspector has a **Debug drawing** section that shows and flips debug
  drawing, each built-in view, the colliders view's distance and every
  toggle added with `Game::debug_toggle` (open by default when the game has
  toggles).
- Target cameras that draw particles and show the default layer draw
  `ctx.debug`'s 3D lines and depth-tested labels into their render target,
  against their own depth, from the same line buffer as the main camera.

### Changed
- See-through debug lines (alpha below 1) are drawn with alpha to coverage
  instead of blending when the 3D frame has several samples a pixel (the
  default 4x MSAA): their alpha is the share of a pixel's samples they
  cover, in steps of a quarter, and overlapping see-through lines no longer
  add up. 100,000 see-through lines at 1280x720 with 4 samples (Apple M4
  Max, busy machine, four runs): 3.0 to 3.06 ms opaque pass instead of 3.7
  to 4.8 ms. With one sample a pixel they are still blended (unchanged,
  1.67 ms).

### Fixed
- Depth-tested debug labels show in every 3D camera that shows the default
  layer, each laid out facing that camera: split-screen views and target
  cameras, not only the main camera. A main camera that leaves the default
  layer out no longer draws them.
- `d3::Text` shows in split-screen views and in target cameras that draw
  particles (it was drawn by the main camera only).
