### Added

- `LodFadeTime`: a `LodGroup` with it fades every change of level over that many seconds, on each camera's frame clock, instead of over a band of screen heights; fades turn back with the camera and keep `LodHysteresis`. In scene files `"LodFadeTime": 0.5`; C `ponko_ctx_set_lod_fade_time` / `ponko_ctx_lod_fade_time`, C++ `set_lod_fade_time` / `lod_fade_time`.
- Unity importer: humanoid clips import. Their muscle and body curves become bone rotations and hips movement for the skeleton they're exported onto, through a `HumanoidAvatar`: the one the character's Animator or the mesh's model names (its `.meta` `humanDescription`, with changed limits and reference pose), one given with `Importer::humanoid_avatar`, or one found by the usual bone names (`HumanoidAvatar::detect`). `AnimationClip::append_to_skin_with_avatar` and `is_humanoid` do the same for skins of your own.

### Changed

- Unity importer: LOD groups with Animate Cross-fading get a `LodFadeTime` (`Importer::lod_fade_time`, Unity's 0.5 s by default) instead of a band crossfade and a report entry.
- Unity importer: a clip whose humanoid curves are only in baked runtime data says to resave it from the editor (those name muscles by index).
