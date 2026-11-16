### Added
- Allocation-free inspection: `StringTable::iter_keys`, `Strings::iter_languages`, `SchedulePlan::iter_stages_in` and `iter_names` borrow instead of collecting.
- Reusable inspection output with replace semantics and retained capacity: `Graphics::loaded_materials_into`, `mesh_costs_into`, `streamed_assets_into`, `asset_handles_into` and `Audio::asset_handles_into`.
- `DrawSpace { World, Screen }` and `DrawParams::with_space` name the coordinates of sprite draws. It is the same type as `ponko::debug::DrawSpace` (now defined once, public at `ponko::debug::DrawSpace` and in the prelude). C: `PonkoDrawSpace` and `ponko_draw_params_with_space`; C++: `ponko::draw_space` and `ponko::with_space`. The `PonkoDrawParams` layout is unchanged.

### Changed
- `DrawParams::with_screen(bool)` is deprecated in favor of `with_space(DrawSpace)`; existing calls keep working. The drawing chapter explains world units and window pixels in one place.
