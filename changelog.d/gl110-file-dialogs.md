### Changed
- The editor's **Open** and **Save as** use the system's file picker on
  macOS, Windows and Linux (the `editor` feature, through the `rfd`
  crate). Save as asks for Scene or Prefab, then opens the save picker in
  the document's folder; the chosen file must be inside the project's
  `assets/` folder. Builds without a system picker keep the path field.
