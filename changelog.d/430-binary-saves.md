### Added
- Binary world saves: `ponko::reflect::SaveFormat::Binary` keeps a world
  save as MessagePack (`rmp-serde`) compressed in an LZ4 frame (`lz4_flex`)
  with a checksum: the same save as the JSON, about 15 times smaller at a
  million entities, and quicker to write and read. `SaveFormat::encode`,
  `decode` (either format, told apart by the first bytes) and `detect`.
- `Storage::set_save_format`: save slots in binary as `saves/<slot>.save`.
  `Ctx::load_world`, `world_slots`, `delete_world` and
  `Ctx::check_old_saves` read both formats, so JSON slots and saves from
  before keep loading, migrations included.
- `Storage::save_world_in_background`: the world's save is taken on the
  game's thread and encoded and written on a job, in order; `finish_saves`
  waits for them, and the game does when it ends. `is_saving` is `true`
  until they are written.
- A retained binary save fixture (`tests/fixtures/saves/world_v1.save`) and
  the save benchmark `cargo run --release -p ponko_bench --bin saves`.

### Changed
- `World::save` builds each entity's JSON in parallel.
