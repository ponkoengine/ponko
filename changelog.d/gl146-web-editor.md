### Fixed
- The editor builds for the web with clippy clean (`--features editor`,
  and with `3d`). There, **Open** and **Save as** ask for the path in the
  editor's own field instead of a system dialog, and model thumbnails stay
  the software drawings: the offscreen renderer and its disk cache are
  left out of the web build.
- After the system file dialog closes, or the window's focus leaves and
  returns, the editor no longer keeps Ctrl (⌘), Shift or Enter held: it
  forgets every held key and modifier, so a plain key afterwards isn't
  taken for a shortcut.
