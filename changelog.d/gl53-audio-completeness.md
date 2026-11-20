### Breaking changes
- `ponko_ffi::PonkoBusEffectKind` has a new value,
  `PONKO_BUS_EFFECT_PITCH_SHIFT`, so a Rust `match` over every value no
  longer compiles. C and C++ code is not affected.
  Instead: add an arm for the new value, or a `_` arm.
  Before:
  ```rust
  match effect.kind {
      PonkoBusEffectKind::PONKO_BUS_EFFECT_FILTER => "filter",
      PonkoBusEffectKind::PONKO_BUS_EFFECT_REVERB => "reverb",
      PonkoBusEffectKind::PONKO_BUS_EFFECT_COMPRESSOR => "compressor",
      PonkoBusEffectKind::PONKO_BUS_EFFECT_DELAY => "delay",
  }
  ```
  After:
  ```rust
  match effect.kind {
      PonkoBusEffectKind::PONKO_BUS_EFFECT_FILTER => "filter",
      PonkoBusEffectKind::PONKO_BUS_EFFECT_REVERB => "reverb",
      PonkoBusEffectKind::PONKO_BUS_EFFECT_COMPRESSOR => "compressor",
      PonkoBusEffectKind::PONKO_BUS_EFFECT_DELAY => "delay",
      PonkoBusEffectKind::PONKO_BUS_EFFECT_PITCH_SHIFT => "pitch shift",
  }
  ```

### Added
- Loop points: `Audio::set_loop(sound, start, end)` plays an intro once and
  loops the rest; `sound_loop` reads them back.
- Fades on a playing sound: `Audio::set_volume(playing, volume, seconds)`,
  `set_speed` and `fade_out`, linear in amplitude so crossfades add up.
- Music: `Audio::play_music` plays a `Song` of layers that stay in sync,
  with changes on the next beat or bar line (`Quantize`):
  `set_music_layer`, `play_stinger`, `stop_music`, `set_music_speed`,
  `music_beat`, `music_layer_volume` and `set_music_bus`.
  `Song::from_layers` builds a song from data.
- `BusEffect::PitchShift` (`PitchShiftEffect`): pitch without tempo, with
  `semitones` and `undo_speed`.
- Pause groups: `Audio::pause_bus` and `bus_paused` hold a bus's sounds
  where they are; `Game::pause_audio_in(state, buses)` ties it to a game
  state.
- Voice limits: 64 sounds heard at once by default
  (`Audio::set_voice_limit`, up to 256), per-bus limits
  (`set_bus_voice_limit`), priorities (`set_priority`,
  `d3::AudioSource::with_priority`, `DEFAULT_PRIORITY`) and virtual voices
  that are followed while not heard (`is_virtual`, `voice_count`).
- Output devices: `Audio::output_devices`, `set_output_device`,
  `chosen_output_device`, `output_device` and `device_events`. A lost
  device moves sound to the default one, and back when a chosen device
  returns.
- `Audio::open_offline` and `render_offline` mix into memory, also in
  headless runs, so tests can check what a game sounds like.
- IMA ADPCM WAV and AIFF files decode.
- The web's streamed sounds honour loop points, start positions, fades
  and music timing.
- C and C++: all of the above (`ponko_ctx_set_sound_loop`,
  `ponko_ctx_play_music`, `PonkoQuantize`, `ponko_ctx_pause_audio_bus`,
  `ponko_game_pause_audio_in_state`, `ponko_ctx_set_audio_voice_limit`,
  `ponko_ctx_audio_output_devices`, `ponko_ctx_render_audio_offline`, ...),
  and `PONKO_BUS_EFFECT_PITCH_SHIFT`.
- `ponko_bench --features audio -- audio3d`: 1,000 positional sources,
  the game side of a frame and the mix.

### Changed
- **Breaking:** a new `PonkoBusEffectKind` value (see Breaking changes).
- At most 64 sounds are heard at once unless `Audio::set_voice_limit`
  raises it; past that the quietest, lowest-priority sounds go virtual
  instead of each bus failing to start sounds past 128.
