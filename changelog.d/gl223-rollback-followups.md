### Added
- Rollback from C and C++: `ponko_ctx_rollback_start` (with a
  `PonkoRollbackConfig` from `ponko_rollback_config_default`),
  `ponko_ctx_rollback_set_input`, `_packet`, `_receive`, `_input`,
  `_resimulating`, `_frames`, `_stats`, `_desync` and `_stop` run a
  `GameRollback` whose inputs are plain structs of up to
  `PONKO_ROLLBACK_INPUT_MAX` (32) bytes; the C++ wrapper has them as
  `ctx::rollback_*`. World snapshots too: `ponko_world_snapshot_new`,
  `ponko_world_save_snapshot`, `ponko_world_restore_snapshot`,
  `ponko_world_snapshot_len` and `ponko_world_snapshot_free` (C++
  `ponko::world_snapshot`, `world.save_snapshot`, `world.restore_snapshot`).
  See the guide's "Rollback", "From C and C++".

### Changed
- `World::restore_snapshot` puts back the change stamps and removal logs
  saved with the snapshot, each as many ticks old as it was at the save,
  instead of counting every restored value as added and changed now. In a
  `GameRollback`, the engine also moves what each simulation system has
  seen by the same amount, so `Added` and `Changed` filters and `removed`
  show a tick that runs again exactly what they showed it the first time.
