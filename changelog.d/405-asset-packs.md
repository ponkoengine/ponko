### Added
- Asset packs: many asset files in one archive with an index, each file
  stored or deflated (when that saves a tenth) with its CRC-32, identical
  files stored once, names sorted so the same files give the same bytes.
  `PackSource` (`ponko::assets`, and the prelude) reads one as an asset
  source: `PackSource::open(path)` reads only the index and then a file at a
  time, `PackSource::from_bytes(id, bytes)` takes one in memory; a damaged
  file is an I/O error naming it.
- A game reads `assets.pack` where its `assets/` folder would be (next to the
  program, in a macOS app's `Resources`, next to the web page) when a name
  isn't a loose file there, with no code; loose files win, so development
  keeps hot reload. A web page names its pack on the canvas
  (`data-pack="assets.pack"`) and the engine fetches it before setup with at
  most two HTTP range requests (the head with the index, then the rest), or
  whole from a server without ranges.
- `ponko pack` (`ponko_cli`): packs a folder (`--out`, `--exclude <pattern>`,
  `--no-compress`), converts images to KTX2 into the pack per `textures.json`
  with `--textures`, and lists and checks a pack with `--list`.
- `web/smoke/first-load.mjs`: the requests, bytes, asset phase and first frame
  of web builds in headless Chromium on an emulated network. On Ember Hop's
  web package the pack takes the asset requests from 27 to 2 and the asset
  phase from 292 to 167 ms (40 ms, 20 Mbit/s) and from 1055 to 591 ms
  (150 ms, 5 Mbit/s).

### Changed
- `ponko package` ships the files under `assets/` as one `assets.pack` (the
  reference games Coin Dash, Ember Hop and Shard Grove included); `--no-pack`
  or `pack = false` in `[package.metadata.ponko]` keeps them loose, and
  `pack = { exclude = [...], textures = true }` sets packing up. A web
  package has no `assets/index.json` when it is packed.
- `scripts/package-coin-dash.py smoke` checks the files in Coin Dash's
  `assets.pack` against its content notices, and that a package without the
  pack fails naming the missing file.
