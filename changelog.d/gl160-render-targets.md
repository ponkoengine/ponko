### Fixed
- A target camera clears its render target in a frame with nothing at all
  to draw, instead of leaving the last picture in it, and draws what it
  sees while the main camera sees nothing (that frame used to skip the 3D
  passes, the target cameras' too).
- First-person held items are drawn in a frame that has nothing else to
  draw.
- 3D debug labels that aren't depth-tested show in the render target of
  every target camera that shows debug drawing, on top there as on screen:
  drawn last, over the scene, particles and debug lines (they were left
  out of render targets).
- Held items whose material has a stencil state (`MaterialDesc::stencil`)
  test and write the stencil as in the world, in the stencil's `order`;
  cut-out ones mark it only where they are solid (the view model drew them
  without it).
