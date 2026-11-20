### Added

- Threads on the web: a web build with threads (`web/build.sh --threads`,
  nightly Rust) runs work on Web Workers that share the game's memory, on
  a page served with `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp`. What runs there:
  - parallel queries (`par_for_each` and the other `par_*` calls), the
    parallel scheduler's batches and the ECS's parallel sorts;
  - `Job`s, on workers of their own, which carry asset groups' and
    streaming's reads and decodes.

  On a page without the headers the same build runs on the page's thread,
  with a console warning that names them. The ordinary web build is
  unchanged.
- `Game::web_threads(n)` chooses how many frame threads a web build
  starts: by default one fewer than the browser's cores, at most 16; 0
  or 1 means none. `ponko::web::threads()` says how many it runs on.
- `ponko package --web --threads`:
  - builds the web package with threads;
  - adds `ponko-worker.js`, and files that send the two headers on
    Netlify and Cloudflare Pages (`_headers`), Vercel (`vercel.json`) and
    Apache (`.htaccess`).
- `web/smoke/threads.mjs` checks a threaded build in a headless browser:
  - work goes to workers on an isolated page;
  - it falls back to one thread on a page that isn't isolated, with the
    same results;
  - a worker's panic stops the game.

  `--bench` times the simulation workloads with 1 and N threads.
- A guide chapter, "Threads on the web": the build recipe and what the Rust
  toolchain and wasm-bindgen need, the headers for each host, the limits
  and the numbers.
