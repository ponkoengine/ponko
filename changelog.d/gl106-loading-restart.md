### Added

- `ctx.assets.restart_group(id)` loads the rest of a failed or cancelled
  asset group, reading again only the assets that had not loaded: the one
  that failed and the ones never started. What loaded keeps its handle,
  untaken values stay, and the files a failed group was already reading
  finish on their workers and are taken as they are. C:
  `ponko_ctx_restart_asset_group`; C++: `ctx.restart_asset_group(id)`.
- Asset groups measure what their GPU uploads cost on the game thread:
  `GroupStats::upload_ms`, `max_upload_ms`, `total_upload_ms` and
  `total_upload_bytes`, and per group `GroupProgress::upload_ms` and
  `bytes_uploaded` (also in `PonkoAssetGroupStats` and
  `PonkoAssetGroupProgress`).
- Where there are no worker threads (a web build without threads, or a page
  that doesn't allow them), an asset group decodes a large PNG texture a few
  rows at a time and a sound a few packets at a time, checking its frame
  budget between slices: one large file is spread over several frames
  instead of taking one long frame, with the same pixels and samples as the
  one-piece decode.
- The guide's "Loading screens" chapter: "Trying again", "What uploads
  cost", and the sliced decodes under "In a browser".

### Changed

- A failed asset group keeps the assets it was already reading when it
  failed, for a restart, and `cancel_group` now cancels a failed group too,
  dropping them. `AssetGroup::with` takes an `Fn` loader instead of an
  `FnOnce`, since a restart may run it again. Asset groups have not been in
  a release yet.
