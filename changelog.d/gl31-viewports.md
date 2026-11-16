### Added
- Several cameras in one window: split screen, minimaps, picture in
  picture. Every 2D and 3D camera has a `rect` (`ponko::camera::Viewport`,
  fractions of the window; `Viewport::split(players, index)` for split
  screen) and `layers` (`RenderLayers`). More cameras go in
  `ctx.views2d` (`d2::View`) and `ctx.scene3d.views` (`d3::View`) and draw
  after the main one, in order, each into its rectangle with its own
  clear colour and, in 3D, its own `Post`.
- 3D views are whole frames of their own camera: their own culling and
  temporal history (TAA, temporal AO, reflections), sharing the frame's
  skinning, compute passes, global illumination and shadow maps. The
  sun's cascades are fitted around every view; point and spot light
  shadows go to the lights nearest any camera, in one atlas drawn once a
  frame. Views draw without occlusion culling (never too little), and the
  main camera's `Post::msaa` applies to all of them.
- 2D lights, shadow casters, ambient light and normal maps light every 2D
  view, each in its own rectangle.
- `ctx.debug` draws on layer 0: its lines and labels (and the built-in
  views) show in every 2D and 3D camera that shows layer 0; window-pixel
  shapes are drawn once.
- `RenderLayers`, a component (saved as its bits) choosing which cameras
  draw a 3D model or a 2D sprite entity; `Gfx::set_layers` for what is
  drawn with `ctx.gfx`. A model off a camera's layers still casts its
  shadow there.
- `d2::View::camera_at` and `SceneSettings::camera_at`: the camera drawing
  at a window point, for picking.
- C and C++: `PonkoView2D`, `PonkoView3D` (versioned) and `PonkoViewport`,
  with `ponko_ctx_set_view2d`, `ponko_ctx_set_view3d`, `ponko_ctx_camera_at`,
  `ponko_ctx_screen_to_world_at`, `ponko_ctx_screen_to_ground_at`,
  `ponko_world_set_render_layers` and the rest; `ponko::view2d`,
  `ponko::view3d` and `ponko::split_viewport` in C++.
- Guide chapter "Several cameras: split screen and minimaps" and the
  `split_screen` example.
- Cost, from the scene benchmark (`PONKO_RENDER_VIEWS`, Apple M4 Max,
  1280 x 720, high quality, load average 62–78): GPU median 3.1–3.5 /
  2.3–3.7 / 3.4–3.9 ms and CPU median 1.2–1.5 / 1.9 / 3.1–3.3 ms for 1, 2
  and 4 cameras. Allocations per frame 1222 / 1843 / 2791 (1222 / 1847 /
  2832 before the views stopped building lists of scenes each frame).
  Collecting a view allocates nothing once warm (a counting-allocator
  test); what a view adds is wgpu recording its render passes.

### Changed
- A camera's `viewport()` is the size of its `rect` (the window's size, as
  before, unless the rect is changed), and its screen and world
  conversions work in window pixels inside that rect.
- `Ctx::mouse_world` and the inspector's picking go through the camera
  under the mouse.
