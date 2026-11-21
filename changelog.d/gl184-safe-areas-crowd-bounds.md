### Added
- `ctx.safe_area()`: the part of the window a phone's notch or camera
  hole, its home bar and its rounded corners leave, in logical pixels
  (the whole window where nothing covers it), and `ctx.safe_insets()`,
  how far in from each edge they reach (`ponko::window::SafeInsets`). In
  the browser the engine reads the page's `env(safe-area-inset-*)` (a page
  with `viewport-fit=cover`) at start and whenever the window changes
  size; `ctx.feed_safe_insets` feeds them as a phone reports them, for
  tests and platform layers. Not in C or C++ yet.
- UI roots made with `.inside_safe_area()` (`Ui::set_inside_safe_area`)
  are laid out in the safe area: their sizes, fractions, anchors and
  `auto` margins count from its edges. Roots still fill the whole window
  by default. A `Ui` of the game's own takes the area with
  `Ui::set_safe_area`.

### Changed
- On-screen touch controls are laid out in the window's safe area, a
  `TouchPlace` counts from its corners, a control the player drags stops
  at its edge, and the ready-made arrange screen's panel stays in it.
  Without insets (every desktop, and pages without `viewport-fit=cover`)
  nothing moves.
- Crowds are culled by the box of each clip's frame range, not of the
  whole clip: the bake keeps a box for every 4 frames of a clip (and the
  frame after, which skinning blends with them), and each character is
  tested with the box of the range its clip shows at the frame's time
  (and of the one its blend clip shows), on both paths. In the tests'
  scenes: 13 figures held across a jump below the camera view's bottom
  edge, 10 drawn (13 with a box per clip), the picture unchanged; 26
  jumping behind a low wall, 17 kept by occlusion culling on the GPU path
  (26); global illumination's probe views over 8 frames, 918 kept (1,047).
