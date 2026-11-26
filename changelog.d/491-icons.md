### Added
- Item icons: `ctx.gfx.render_icon(mesh, &Icon)` (and `try_render_icon`)
  draws a model once into a render target of its own, framed to fill it,
  lit by the scene's sun and sky light, tone mapped, on a background that
  is transparent by default. It is drawn from a world of its own, so the
  game's world neither shows in it nor gets anything added, and it costs
  nothing after its first two frames.
- C and C++: `PonkoIcon`, `ponko_icon_defaults` and
  `ponko_ctx_render_icon` (`ponko::icon_of`, `ctx.render_icon`).

### Fixed
- A target camera's screen effects keep the alpha of its picture, so a
  transparent background stays transparent after them.
