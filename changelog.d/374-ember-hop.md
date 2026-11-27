### Added
- Ember Hop (`examples/ember_hop`, feature `physics`), a second reference
  game: a small platformer on the engine's own Tiled levels (autotiled
  ground, tile colliders, objects as prefabs, one with children), 2D physics
  with sensors and collision events as its rules, a title scene, world save
  slots at its checkpoints, sound files and streamed music, particles, a
  following camera, and keyboard and gamepad actions. `tests/ember_hop.rs`
  plays it headlessly (moving, jumping, collecting, the attack, losing,
  saving and loading, the level transition, the end of a run, the input
  mapping through headless frames) and its autopilot plays the whole game.
  The check list packages it with `ponko package` and plays the package on
  Linux, macOS and Windows; it builds for the web (`web/build.sh --features physics
  ember_hop`). The guide's chapter "Ember Hop: a platformer, start to
  finish" walks through it.
