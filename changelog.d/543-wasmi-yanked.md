### Fixed
- The `mods-wasm` feature's lockfile moves `wasmi` (and `wasmi_core`,
  `wasmi_ir`, `wasmi_collections`) from the yanked 2.0.2 to 2.0.3, so
  `cargo deny check` passes again.
