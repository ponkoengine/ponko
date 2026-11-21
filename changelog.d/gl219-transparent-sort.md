### Changed
- Particles are drawn among the see-through models, far to near, instead
  of after all of them: a particle behind a pane of glass takes its tint
  (additive emitters too, whole, at their centre's depth), one in front
  is drawn over it as before. In the order-independent transparency
  evaluation's particles-through-glass scene the mean error falls from 4.7
  to 1.3 levels. Each stretch of particles between two see-through models
  costs one more draw, and models of one mesh on either side of it are
  drawn apart; ribbons, lines, 3D text, see-through instance batches and
  crowds keep their place after the see-through models.

### Added
- The order-independent transparency evaluation measures two ways of
  ordering a see-through model's own triangles (crossed cards, multi-pane
  glass in one mesh): sorting them by centroid every frame halves the
  error (multi-pane glass 10.3 to 6.4 levels) for 11.6 ns a triangle a
  frame; splitting them where they cross at build time and walking the
  BSP tree is exact (0.02) but cut dense card clusters into 37 times as
  many triangles. Neither is shipped; the numbers are in the 3D guide, "A
  model's own triangles: measured, not shipped".
