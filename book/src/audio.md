# Sound

<div class="lang-pair">
<div>

```rust
let hit = ctx.audio.load("hit.ogg");   // OGG Vorbis, WAV, MP3 or FLAC from assets/
ctx.audio.play(hit);

let music = ctx.audio.load("theme.ogg");
let song = ctx.audio.play_ex(music, SoundParams { volume: 0.5, looping: true, ..Default::default() });
ctx.audio.stop(song);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:sound}}
```

</div>
</div>

- `SoundParams { volume, speed, looping }`: `speed` also changes the pitch,
  and a little random variation keeps repeated effects from sounding
  mechanical.
- `ctx.audio.create(sample_rate, &samples)` makes a sound from generated
  mono samples (-1..1). The `ui` example's click is made this way:

<div class="lang-pair">
<div>

```rust
let samples: Vec<f32> = (0..4410) // 0.1 s at 44.1 kHz
    .map(|i| 0.3 * (i as f32 * 880.0 * std::f32::consts::TAU / 44100.0).sin())
    .collect();
let click = ctx.audio.create(44100, &samples);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:sound_samples}}
```

</div>
</div>
- `Sound` is a cheap `Copy` handle, like `Texture` (C++: `ponko::sound`,
  with `play_sound(s, params)` for `play_sound_ex`).

The audio device opens the first time a sound plays (in a browser, at the
player's first click or key press: see [In a browser](#in-a-browser)). Headless runs, and
machines without audio, are silent but run the same code. For servers you
can drop the audio library entirely:

```toml
ponko = { version = "0.1", default-features = false }
```

The sound API stays; it just makes no noise. The C library has the same
switch: build `ponko_ffi` with `--no-default-features`. Linux builds with audio need
the ALSA headers (`libasound2-dev` on Debian and Ubuntu).

## Sound in 3D

With the `3d` feature, pair `d3::AudioSource` with `d3::Transform`, or play a
one-off sound at a point with `ctx.audio.play_at`. Every frame, after your
systems, the engine moves each sound to its entity and sets its volume,
panning and doppler from where the listener is. The listener follows
`ctx.scene3d.camera`; to hear from somewhere else, such as the player in a
third-person game, call `ctx.audio.set_listener(Some(..))`.
Positions use world units; doppler assumes metres and takes velocities
from frame to frame. Set `doppler: 0.0` to turn it off, for example for
something that teleports.

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/audio3d.rs:sound3d}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:sound3d}}
```

</div>
</div>

Build this example with `cargo build --example audio3d --features 3d`.
Only running it opens a window and plays audio. Procedural clips already use
`create_sound`; file-backed clips use `load_sound`.

`d3::AudioSettings` controls volume, pitch (`sound.speed`), looping, minimum and
maximum distance, and logarithmic, linear or keyed rolloff. Keyed curves use
sorted `(normalized_distance, gain)` pairs in a static slice. All curves are
silent at the maximum distance and full volume inside the minimum distance.
From C, `ponko_ctx_play_sound_at_keyed` and
`ponko_ctx_set_audio_source3d_keyed` take the keys as an array of
`PonkoAudioKey` (`distance`, `gain`); in C++ `play_sound_at` and
`set_audio_source3d` take a `std::vector<ponko::audio_key>` after the
settings. Keys out of order are refused. The engine keeps one copy of each
distinct curve for the rest of the run, so build curves once rather than
per frame.
Stereo panning follows the listener's right axis. Kira applies gain, panning
and playback-rate changes with smoothing; this is stereo positional audio,
not an HRTF simulation. Prefer mono source clips for localized effects.

`ctx.audio.play_at` starts a voice at a fixed world position. Save its handle
to stop it; removing an entity source stops its voice on the next audio update.
Headless execution never opens an audio device, and disabling `audio` keeps
the same APIs silent. No allocation occurs during updates unless a new voice
starts. `load_sound` decodes clips into memory; `load_stream` keeps long files streamed.

### Buses, low-pass and reverb

Ordinary playback uses the `sfx` bus. Create other named buses once with
`ctx.audio.bus`, save their handles, and pass one to `ctx.audio.play_on` or to
`d3::AudioSettings::bus`. Handles belong to the context that created them.
Volume is a linear gain; muting preserves the configured gain. Bus changes
apply to active voices as well as future playback.

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/audio3d.rs:audio_mixer}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:audio_mixer}}
```

</div>
</div>

Each voice uses a Kira low-pass filter on its own child track. The cutoff
is clamped to 20–20,000 Hz. `ctx.audio.set_effects` changes cutoff and reverb
send while a voice is playing, including positional voices. Reverb is a
single shared, fully wet Kira send track; the send amount is linear 0–1.
The send follows voice attenuation, filtering and bus gain. Muting a bus
stops its new reverb input; existing shared reverb tails decay naturally.
Nonfinite cutoff/send values use defaults. Starting
a voice allocates its track and decoder; changing parameters and updating
existing spatial voices does not allocate engine-side buffers each frame.

## Mixer

Buses form a tree, each bus can run a chain of effects over everything it
plays, one bus can duck another, and named snapshots blend the whole mix to
another state (underwater, paused) over time. All of it can live in a JSON
file that hot reloads, and the player's part of it (volumes, mutes, which
snapshots are on) saves with the settings.

The `audio_mixer` example loads a mixer file, ducks its music under a line
of dialogue (D) and blends to an underwater snapshot (U):

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/audio_mixer.rs:mixer_setup}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:mixer_tree}}
```

</div>
</div>

The C++ side builds the same mix in code; Rust can too, with
`set_bus_parent`, `set_bus_effects`, `set_bus_ducking` and `add_snapshot`.
Each frame the example plays, ducks and switches:

```rust
{{#include ../../examples/audio_mixer.rs:mixer_frame}}
```

Build it with `cargo build --example audio_mixer`; running it opens a window
and plays sound.

### The bus tree

`ctx.audio.set_bus_parent(bus, Some(parent))` puts one bus under another
(`None`: straight to the output). A bus is heard at its own volume times
every parent's, a muted parent silences everything under it, and a parent's
effects and ducking act on its children's sound too. A loop is refused with
an `Invalid` error. `bus_volume` reads back what was set, and
`bus_gain` what is heard: snapshots, parents and mutes included. Moving a bus
while sounds play on it carries them over; they continue from where they were.

### Effects on a bus

`ctx.audio.set_bus_effects(bus, &[...])` sets a chain of `BusEffect`s, run in
order on the bus's mix before its volume:

| Effect | Settings (defaults) |
|---|---|
| `Filter(FilterEffect)` | `mode` low-pass, band-pass, high-pass or notch (low-pass); `cutoff_hz` (20,000); `resonance` 0–1 (0); `mix` (1) |
| `Reverb(ReverbEffect)` | `feedback` (0.9), `damping` (0.1), `stereo_width` (1), `mix` (0.5) |
| `Compressor(CompressorEffect)` | `threshold_db` (-18), `ratio` (4), `attack` (0.01 s), `release` (0.1 s), `makeup_db` (0), `mix` (1) |
| `Delay(DelayEffect)` | `delay` (0.25 s), `feedback_db` (-6; the first echo too), `mix` (0.3) |

Values out of range are clamped. Changing settings is smooth. Adding,
removing or reordering effects, or changing a delay's time, rebuilds the bus
(and the buses under it); the sounds playing there carry on from where they
were. A low-pass at 20 kHz changes nothing, which makes it the usual
placeholder for a snapshot to lower. Every bus also runs two small effects
of the engine's own, after its chain: the level meter that ducking listens
to and the ducker.

### Ducking

`ctx.audio.set_bus_ducking(music, &[Ducking { gain: 0.25, ..Ducking::new(dialogue) }])`
lowers `music` to `gain` while `dialogue` is louder than `threshold`
(linear peak, default 0.01), over `attack` seconds, and lets it back up over
`release` seconds once dialogue is quiet. It runs on the audio thread, on
the level the key bus is really heard at (after its effects, and its own
and its parents' volumes and mutes), so gaps between lines bring the
music back. A muted key ducks
nothing. A bus takes up to four keys (`Ducking::MAX_KEYS`); while several
are loud, the lowest gain wins.

### Snapshots

A `MixerSnapshot` lists, per bus, a volume and new settings for effects in
its chain (`EffectOverride { index, effect }`, same kind as the effect at
`index`). `ctx.audio.start_snapshot("underwater", 1.0)` blends to it over one
second, `stop_snapshot` back out, and `snapshot_weight` says how far it is.
Several can be on: one started later wins where they overlap, and starting
one again moves it last. Volumes blend linearly and filter cutoffs evenly in
pitch. Fades run on the frame clock, so a "paused" snapshot still fades in
while the game is paused; headless runs advance them each tick too.

### The mixer file

`ctx.audio.load_mixer("mixer.json")` reads buses, parents, volumes, effects,
ducking and snapshots from a file in the assets, making buses by name (their
handles stay the same when the file is applied again). With hot reload on,
saving the file applies it to the running game. A file with a mistake (an
unknown bus, a loop, an override of the wrong kind) changes nothing and the
reload says why. `apply_mixer_json` takes the same text from a string.

```json
{{#include ../../assets/audio_mixer.json}}
```

Every list and field may be left out; effects take only the settings they
change (`{"filter": {}}` is a low-pass at 20 kHz). A bus's `volume` in the
file replaces what was set, so restore saved settings after loading it.

### Saving the mix with the settings

`ctx.audio.mixer_state()` is a `MixerState`: every bus's volume and mute by
name, and the snapshots that are on. It is serde, so it goes into the
game's settings file as it is; `restore_mixer_state` puts it back (load the
mixer file first, so its snapshots are known):

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
# use ponko::audio::MixerState;
# fn save(ctx: &mut Ctx) {
let _ = ctx.storage.write_json("mixer.json", &ctx.audio.mixer_state());
# }
# fn load(ctx: &mut Ctx) {
if let Ok(saved) = ctx.storage.read_json::<MixerState>("mixer.json") {
    ctx.audio.restore_mixer_state(&saved);
}
# }
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:mixer_settings}}
```

</div>
</div>

From C, the same calls are `ponko_ctx_set_audio_bus_parent`,
`ponko_ctx_set_audio_bus_effects` (an array of `PonkoBusEffect`, from
`ponko_bus_effect_default(kind)`), `ponko_ctx_set_audio_bus_ducking`
(`PonkoDucking`, from `ponko_ducking_default`), `ponko_ctx_add_audio_snapshot`
(`PonkoBusSnapshot`), `ponko_ctx_start_audio_snapshot` (an unknown name is
`PONKO_NOT_FOUND`), `ponko_ctx_stop_audio_snapshot`,
`ponko_ctx_load_audio_mixer`, `ponko_ctx_apply_audio_mixer_json`, and
`ponko_ctx_audio_mixer_state` / `ponko_ctx_restore_audio_mixer_state` with
the state as JSON (the same format as Rust's serde), written with the usual
two-call buffer convention. Getters: `ponko_ctx_audio_bus_parent`, `_volume`,
`_gain`, `_muted`, `_effects`, `_ducking` and `ponko_ctx_audio_snapshot_weight`.

### Streaming music and ambience

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/audio3d.rs:audio_stream}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:audio_stream}}
```

</div>
</div>

`load_stream` reads the header and keeps the file's path (or, for music from
an [asset source](asset-sources.md), its still-encoded bytes in memory)
rather than decoding the entire clip. Each playback opens a separate decoder;
Kira performs buffered decoding on its worker thread. The returned `Sound`
works with 2D playback, positional playback, buses, effects and looping.
A file must remain available until playback ends. Streams are not hot
reloaded. In a browser, which has neither files nor a decoder thread,
`load_stream` keeps the fetched (or source) bytes encoded and decodes them a
packet at a time as they play (see [In a browser](#in-a-browser)).
`try_load_stream` returns errors instead of panicking. With audio disabled,
stream handles are silent and report zero duration. Headless playback never
creates tracks, decoder workers or an audio device.

## In a browser

Browsers let a page make sound only after the player has clicked, tapped or
pressed a key. The engine opens its audio output at that first gesture, and
until then `ctx.audio.unlocked()` is `false` (on the desktop it is always
`true`). What the game plays before the gesture is not simply lost:

- Looping sounds (music, ambience) wait and start at the gesture, with the
  volume, bus and effects they were given. Their handles work meanwhile:
  `stop` cancels one, `set_effects` changes it.
- One-shots are dropped: a click or a footstep heard seconds late would be
  out of place.

So a game starts its music in setup as it does on the desktop. If the
player should hear the opening, show a prompt until sound is allowed:

```rust
# use ponko::prelude::*;
# fn title(ctx: &mut Ctx) {
if !ctx.audio.unlocked() {
    ctx.gfx.draw_text("Click to start", vec2(40.0, 40.0), 32.0, Color::WHITE);
}
# }
```

Streamed sounds (`load_stream`) keep the fetched bytes encoded and decode a
packet at a time as they play, on the page's thread. A three-minute stereo
track at 44.1 kHz takes about 64 MB decoded whole, as `load_sound` holds it
(twice that for a moment while it decodes). Streamed, it takes its encoded
size (3.6 MB as a 160 kbit/s MP3) and at most 1.6 MB more while it plays.
Use `load_stream` for music and long ambience, and `load_sound` for short
effects, which start without any decoding work.

From C, `ponko_ctx_audio_unlocked` answers the same (C++:
`ctx.audio_unlocked()`); native games run on the desktop, where it is
always `true`.
