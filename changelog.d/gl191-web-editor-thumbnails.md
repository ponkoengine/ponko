### Added
- The editor's web build shows thumbnails in the asset panel, made from
  the page's files in memory (fetched, packed or saved in the browser)
  instead of paths on disk.
- **File > Saved in this browser** in the editor's web build lists the
  files kept in the browser, with **Revert** (back to the build's file, at
  once and after a reload) or **Delete** (a file only the browser has).
  `Assets::browser_saves` and `Assets::revert_browser_save` do the same
  from code; other builds list nothing.
