### Added
- Audio mixer: buses nest (`Audio::set_bus_parent`), with volumes and mutes
  multiplying down the tree; `bus_volume`, `bus_muted`, `bus_gain`,
  `bus_parent` and `bus_name` read it back.
- Effect chains per bus (`Audio::set_bus_effects`, `BusEffect`): filter,
  reverb, compressor and delay, changed smoothly; changing the chain or a
  parent rebuilds the bus and its sounds carry on where they were.
- Sidechain ducking (`Audio::set_bus_ducking`, `Ducking`): a bus drops while
  up to four key buses are loud, with attack and release, on the audio thread.
- Mixer snapshots (`MixerSnapshot`, `Audio::add_snapshot`, `start_snapshot`,
  `stop_snapshot`, `snapshot_weight`) blending bus volumes and effect
  settings in and out over time, layered.
- Mixer files (`Audio::load_mixer`, `try_load_mixer`, `apply_mixer_json`):
  buses, effects, ducking and snapshots in JSON, hot reloaded; a broken
  file changes nothing.
- `MixerState` (`Audio::mixer_state`, `restore_mixer_state`) to save bus
  volumes, mutes and active snapshots with the game's settings.
- C and C++: `ponko_ctx_set_audio_bus_parent`, `_set_audio_bus_effects`,
  `_set_audio_bus_ducking`, `_add_audio_snapshot`, `_start_audio_snapshot`,
  `_stop_audio_snapshot`, `_load_audio_mixer`, `_apply_audio_mixer_json`,
  `_audio_mixer_state`, `_restore_audio_mixer_state` and getters, with
  `ponko::ctx` wrappers.
- The `audio_mixer` example ducks music under dialogue and switches to an
  underwater snapshot.
