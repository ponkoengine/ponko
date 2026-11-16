### Added
- A web gallery of the examples, playable in the browser: each release
  publishes the examples that run on the web
  as a gallery, a page each with the demo, what it
  shows, its controls, whether it needs WebGPU and its source at the release.
  A demo that needs WebGPU says so, and what to try, on a browser without it;
  the others fall back to WebGL2. `node web/gallery/build.mjs` builds it
  locally, `web/gallery/examples.json` lists the examples (one line each), and
  `node web/smoke/gallery.mjs` checks every page in a headless browser. Linked
  from the README, the guide's web chapter and the release notes.
