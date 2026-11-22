### Changed
- The see-through list groups more draws: a model's screen rectangle is
  where its mesh's box, turned and placed by the model, and its bounding
  sphere both land, so thin and flat models near each other or near the
  edge of the view share draws (the `alpha-heavy` benchmark scene: 2,150
  draws a frame instead of 2,480, the same picture to the byte).
- Particles, ribbons and lines are no longer barriers in the see-through
  list: a model moves past one it doesn't overlap on screen, the next
  particle, ribbon or line joins the draw before it past what it doesn't
  overlap, and additive ones pass each other even where they overlap.
  Order that changes the blended picture is kept.
