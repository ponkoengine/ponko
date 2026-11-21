### Added
- The editor's web build saves: **Save**, **Save as** and **Save selection
  as prefab** keep the file in the page's `localStorage` (named after the
  page's path and the asset), it is read from there at once, and the next
  visit puts it back over the fetched files before setup. **File >
  Download** saves the document and hands its file over as a browser
  download.
- On the web, the editor's **Open** list and asset panel are filled from
  the build's file manifest: the files `assets/index.json` names, the
  page's asset pack, and the files saved in the browser.

### Changed
- In web builds with the `editor` feature, `Assets::save_scene` and
  `Assets::save_prefab` keep the file in the browser's storage instead of
  failing with `ErrorKind::Unsupported`; without the feature they still
  report that web builds have no files.
