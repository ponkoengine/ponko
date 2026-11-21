### Fixed
- `ponko textures`, `ponko cook` and `ponko package` no longer risk hanging
  forever after encoding a texture on a busy machine. Each image used to get
  its own Basis Universal encoder, with a thread per core, dropped right
  after; that encoder's job pool can miss waking a worker as it shuts down,
  and then waits for it forever. Encoders are now kept and reused until the
  command exits, and never dropped. Cooks take the same time and peak memory
  as before.
