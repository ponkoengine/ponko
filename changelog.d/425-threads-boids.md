### Added

- The web threads fixture's benchmark has a `boids` workload: one tick of
  the boids example's flocking system over its 20,000 boids
  (`node web/smoke/threads.mjs --bench --workloads boids`). The guide's
  "Threads on the web" gives its tick time with threads and without: 1.4 ms
  against 9.7 to 9.9 ms in headless Chromium on a 14-core Apple M4 Max.
