### Added
- `web/ponko-isolate.js`: a service worker that makes a page cross-origin
  isolated on hosts that can't send the headers (GitHub Pages, GitLab
  Pages), so a web build with threads starts its workers there. It adds
  the two headers to the game's own responses and reloads the page once on
  the first visit; where it can't work (no secure context, no service
  workers, inside another site's frame) it says why and the game runs on
  one thread. `web/build.sh --threads --isolation-worker` and
  `ponko package --web --threads --isolation-worker` add it. The guide's
  "Threads on the web" says when to use it and what it costs, and its
  hosting table now says which common hosts work with headers and which
  with the service worker. `web/smoke/threads.mjs --case service-worker`
  checks it in a browser served without the headers.
- "Threads on the web" lists every parallel loop of the engine, where it
  runs in a web build with threads, and its time in the browser fixture
  with 13 threads and on one; `threads.mjs` prints that table, and its
  fixture now also draws 2D skeletons, walks 3D characters and bakes light.

### Changed
- In a web build with threads, light baking (`bake_lighting`) runs on the
  frame's workers instead of the page's thread alone: 18 ms instead of
  52 ms for the fixture's scene in headless Chromium.
- In a web build with threads, a world save (`World::save`, `save_world`,
  `save_world_in_background`) builds its JSON on the page's thread again:
  on the workers it took 223 ms instead of 22 ms for the fixture's world,
  because the browser's threads share one allocator lock. The text is the
  same; the desktop keeps building it in parallel.

### Fixed
- `bake_lighting` no longer panics in a browser: it timed itself with
  `std::time::Instant`, which isn't available there.
