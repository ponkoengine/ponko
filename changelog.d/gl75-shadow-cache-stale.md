### Fixed
- Far sun cascades kept between frames (`Lighting::shadow_refresh` above 1)
  no longer lag behind their casters: a model moving, spawned or despawned,
  a level-of-detail switch, an instance batch or cell made, changed or freed,
  an instance level crossing its shadow distance band, or a mesh's data made
  again under a held cascade redraws it on that frame, so kept and redrawn
  shadows match pixel for pixel. Still scenes keep every shadow map and draw
  no shadow pass, as before.

### Changed
- `Lighting::shadow_refresh` now sets how many frames the far cascades hold
  the box they are drawn in; whether a map is drawn again depends only on
  whether what it holds changed (the rules are in its API docs). A shadow
  view is kept without comparing its casters while the collector's numbered
  caster list for it is unchanged, so an unchanged frame costs no instance
  comparison.
- An instance batch or instance cell made, changed or freed draws again only
  the shadow maps its box can be seen in, instead of every shadow map: a
  streamed cell no longer redraws the lights' views and cascades it doesn't
  reach.
