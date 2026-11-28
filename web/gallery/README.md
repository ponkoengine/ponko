# Web gallery

The examples that run in a browser, as one static site: an index page and a
page per example with the demo, what it shows, its controls, whether it needs
WebGPU, and a link to its source at the release. The release steps publish
the gallery of the release's version (see [Publishing](#publishing)); no
gallery is online yet, so build and serve it locally:

```sh
node web/gallery/build.mjs                  # every example -> target/web/gallery/
node web/gallery/build.mjs --only battle,3d # some of them
python3 -m http.server -d target/web/gallery 8000
```

It needs what [`web/build.sh`](../build.sh) needs (the `wasm32-unknown-unknown`
target and `wasm-bindgen-cli` at the version in `Cargo.lock`, see the guide's
[web chapter](../../book/src/web.md)) and Node. Options:

| Option | Meaning |
|---|---|
| `--out <dir>` | Where the site goes (default `target/web/gallery`). |
| `--ref <tag or branch>` | What the source links point at, and the version the pages name (`v0.2.0` reads "Ponko 0.2.0"). Default `main`. |
| `--only <a,b>` | Build only these examples. |
| `--pages-only` | Rewrite the pages without building (after editing `examples.json` or the page code). |

The site is about 20 MB per example uncompressed (the game's `.wasm` and a
copy of `assets/`); the 49 examples come to about 1.1 GB.

## Adding an example: one line

Add a line to [`examples.json`](examples.json), where the index should show it:

```json
  {"example": "my_example", "title": "My example", "kind": "2d", "webgpu": "optional", "shows": "What it demonstrates, in a sentence.", "controls": "Keys and mouse."},
```

| Field | Meaning |
|---|---|
| `example` | The Cargo example: `examples/<name>.rs` or `examples/<name>/main.rs`. |
| `title` | The name on the index and the page. |
| `kind` | `2d` or `3d`: the index groups by it. |
| `features` | The Cargo features it is built with (optional). At least its `required-features` from `Cargo.toml`; the build and the tests refuse fewer. |
| `webgpu` | `optional`: runs with WebGPU, or WebGL2 where the browser has no WebGPU. `required`: needs WebGPU; without it the page says so and what to try, instead of starting the game. |
| `shows` | What it demonstrates, one or two sentences. |
| `controls` | How to play with it. |

Then build it (`--only my_example`) and check it in a headless browser (below).
`node --test web/gallery` checks the manifest: every example exists, asks for
its required features, and sits on one line.

### Which examples are in, and why the rest aren't

Every example with a window that shows something on its own or with a
mouse and keyboard is listed. Left out: those without a picture (`headless`,
`ecs`, `action_map`, `net_messages`, `net_replication`, `net_prediction`,
`navmesh3d`, `ffi_bench`, `validation_bundle`; `audio3d` and `audio_mixer`
are sounds with an empty scene; `window` draws nothing), those that need
something a gallery page doesn't give them (`lockstep`: a second player over
the network; `net_arena`: a server; `net_webrtc`: a peer; `hotpatch`: a
running `dx`; `scene_viewer`: a scene file named in the environment;
`video_player`: a video file named on the command line; `bake_lighting`: a
file of baked light it writes first; `gamepad`: a controller, which the
headless check can't plug in; `phone`: a phone's insets and keyboard), the
smoke-test fixtures (`web_smoke`, `web_smoke_3d`, `web_editor_save`,
`web_editor_unloaded`, `web_threads`, `web_threads_warning`, `web_spectrum`,
`web_video`) and `bunnymark` (the same picture as `bunnymark_ecs`, which is
listed). `lockstep`, `net_arena` and the fixtures still run in a browser:
`web/smoke` checks each (its [README](../smoke/README.md)).

`webgpu: required` is set where WebGL2 can't show the demo:
`compute` and `scene_compute` are about compute passes, which WebGL2 lacks.
The demos that read the scene depth (`terrain`, `shaders`, `target_effect`)
run on WebGL2 through the renderer's GL path for it (see the book's shader
section), and the gallery check runs them there.

## The pages

[`lib.mjs`](lib.mjs) writes the pages, [`gallery.css`](gallery.css) styles
them and [`play.js`](play.js) starts the demo on each. The demo draws into the
page's `<canvas id="ponko">` (the engine uses it, as in `web/index.html`),
16:10 in the page, or the whole window with "Fill window" or `?full` in the
address. Before loading the game, `play.js` asks the browser for a WebGPU
adapter: a `required` demo without one shows the reason and stops there; an
`optional` one says it draws with WebGL2. It reports how the page went in
`<html data-gallery>` (`loading`, `running`, `unsupported`, `error`) for the
check below.

## Checking it in a browser

[`web/smoke/gallery.mjs`](../smoke/gallery.mjs) opens the built gallery in a
headless browser (no window): the index must link every example, and each
page must start its game, keep drawing (draw calls or WebGPU submissions go
on), survive a click and show a canvas that isn't blank, with no panic, page
error, WebGL error the browser logged (a refused call: the draw drew nothing,
even where the canvas isn't quite blank) or failed request; a `required` demo
without WebGPU must show its message instead. With the runner set up as in
[`web/smoke/README.md`](../smoke/README.md):

```sh
node web/smoke/gallery.mjs --backend webgl2                    # WebGPU hidden: the fallback path
node web/smoke/gallery.mjs --backend webgpu --chromium-mode new  # WebGPU (a Mac's Metal adapter)
node web/smoke/gallery.mjs --only battle,3d --browser webkit
```

Each page leaves `report.json`, `canvas.png` and `console.log` in
`target/web-smoke/gallery-<browser>-<backend>/<example>/`, and
`node web/smoke/summary.mjs target/web-smoke` tables them. Exit codes as for
`smoke.mjs`: 0 passed, 1 a page failed, 2 the browser lacks the backend asked
for, 3 bad arguments or no build. The headless browser can't lock the
pointer, so the check makes `requestPointerLock` succeed without locking.

## Publishing

**Decision (#481): the gallery lives on GitHub Pages, at the address the
release notes already link, `https://<owner>.github.io/<repository>/` (for this
repository <https://ponkoengine.github.io/ponko/>), and a maintainer publishes
it by hand by pushing the `gh-pages` branch.** There is no CI to do it (see
RELEASING.md), and there is no separate domain for the gallery. The address
is not written in the tooling: `web/gallery/lib.mjs` and
`scripts/release-check.py` derive it from the project's URL in
[`PROJECT_URL`](../../PROJECT_URL), and `scripts/project-url.py check` covers
both. If the project moves to another host, `project-url.py set` moves the
repository links and the gallery's address needs a new decision (both refuse a
host other than github.com until it is made).

The maintainer who makes a release builds the gallery for the tag
(`--ref vX.Y.Z`) and checks every page in headless Chromium on WebGL2 (the
commands are in RELEASING.md, step 5.7). After the release is
published, they put the built folder on the `gh-pages` branch, which GitHub
Pages serves, for a release (not a candidate) that is the newest version, so
a patch release of an older minor doesn't replace the newer gallery. GitHub
Pages is set up once by a maintainer (RELEASING.md, "One-time setup").

The branch holds one commit, the newest gallery (a site is hundreds of
megabytes, so old galleries must not pile up in the history). From the
repository root, with the site built into `target/web/gallery`:

```sh
git branch -D gh-pages 2>/dev/null           # the last publish, if still here
git worktree add --orphan -b gh-pages target/gh-pages
cp -R target/web/gallery/. target/gh-pages/  # includes .nojekyll
git -C target/gh-pages add -A
git -C target/gh-pages commit -m "Web gallery of vX.Y.Z"
git -C target/gh-pages push --force origin gh-pages
git worktree remove --force target/gh-pages && git branch -D gh-pages
```

Open the address afterwards and play one example.
