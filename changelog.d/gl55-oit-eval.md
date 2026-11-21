### Added
- An offscreen evaluation of order-independent transparency
  (`src/render/oit_render_test.rs`): crossing glass panes, particles
  through glass and crossed foliage cards drawn sorted, with weighted
  blended OIT and with depth peeling, each against an exact depth-peeled
  reference, with a GPU cost benchmark on the engine's pass timer and a
  run on WebGL2 limits. Weighted blended OIT was turned down: it trades
  sorting's wrong-order pixels for wider tint errors (crossing panes: mean
  error 7.1 against 8.6 levels, but 65% of pixels visibly off against 35%;
  foliage: 6.9 against 4.6), and costs 0.13 to 0.25 ms more at 1920x1080 and
  10 bytes a pixel (83 MB with 4x multisampling). The numbers are in the
  3D guide, "Order-independent transparency: measured, not shipped".

### Fixed
- The 3D guide said see-through models aren't sorted; they are drawn far
  to near by each model's distance, with the alpha particles after them.
