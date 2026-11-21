### Added
- `perf::thread_alloc_counts()`: the allocations made on the calling thread
  while profiling is on. It costs nothing while profiling is off and never
  allocates itself.
- Profiling scopes entered with `profile_scope!` or `ProfileScope::enter`
  count the allocations made on their own thread while they are open
  (`ScopeReport::allocs_per_frame`): a scope inside a parallel loop counts the
  loop's work on every thread, and nothing the thread pool allocates for
  itself. Drawing 2D skeletons in parallel has one, `skeleton2d/draw_chunk`.
- `NavMesh::crowd_stats()` (`nav::NavCrowdStats`): the crowd steps run, the
  agents they moved and the backend's work units, since the mesh was baked.

### Changed
- The 3D crowd test asserts counted steps and work per agent instead of a
  30 s wall-clock guard, the 2D skeleton allocation test counts per thread
  and no longer allows for rayon's workers, and the ECS test that a slow
  query reaches several threads counts the rows run on the calling thread
  instead of relying on idle workers waking in time.

### Breaking changes
- `ScopeReport` has a new field, `allocs_per_frame`.
  A struct literal of it no longer compiles; reports read from JSON written
  before it get 0.
  Instead: end the literal with `..Default::default()`.
  Before:
  ```rust
  let s = ScopeReport { name, avg_ms, max_ms, calls_per_frame };
  ```
  After:
  ```rust
  let s = ScopeReport { name, avg_ms, max_ms, calls_per_frame, ..Default::default() };
  ```
