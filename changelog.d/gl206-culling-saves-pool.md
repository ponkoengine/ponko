### Changed
- In a web build with threads, the 3D renderer's culling (each model
  compared with its last frame and culled again) runs on the frame's
  workers: it sizes its work by the frame pool's threads instead of the
  page's thread pool, which has one thread and made it take its one-pass
  path. Saving a world as JSON (`World::save`, `save_world`,
  `save_world_in_background`) builds its entities' JSON there too; the
  text is byte-identical to a one-thread save. The browser test checks
  that both went there and give the same result as on one thread. On the
  desktop nothing changes.
