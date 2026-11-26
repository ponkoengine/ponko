# Sound

> **Maturity: [tested](maturity.md).** Game: Coin Dash, Ember Hop and Shard
> Grove play sounds and music. Tests: `tests/audio.rs`,
> `tests/audio_mixer.rs`. Platforms: Linux, macOS and Windows, headless (the
> workspace tests in the list of checks). Graphics: not involved. Missing:
> sound out of a real device: the runs in the list of checks hear nothing.

<div class="lang-pair">
<div>

```rust
let hit = ctx.audio.load("hit.ogg");   // OGG, MP3, FLAC, WAV or AIFF from assets/
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
- File formats, told apart by their first bytes: OGG Vorbis, MP3, FLAC,
  WAV (integer or float PCM, and IMA ADPCM, the compressed WAV many sound
  tools export) and AIFF. Opus and AAC (`.m4a`) don't decode: no light,
  pure-Rust Opus decoder exists, and AAC needs an MP4 reader several times
  the size of the other decoders. Convert them to OGG Vorbis.

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

A headless run plays nothing, but still hands each sound a handle of its
own and lists what was started: `ctx.audio.started()` (C++
`ctx::sounds_started()`) gives the sounds started this tick, with their
bus, volume, speed and looping, so a test can check what the game played
(see [Headless runs](headless.md#the-pointer-sounds-and-quitting)).

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
the same APIs silent. No allocation occurs during updates unless a voice
starts or moves between heard and [virtual](#voice-limits). `load_sound` decodes clips into memory; `load_stream` keeps long files streamed.

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

The example looks names up once, in setup, and keeps handles for the frame.
`ctx.audio.bus(name)` makes a bus the first time it is asked for, so a typo
there makes a new, unheard bus; for buses a mixer file declares,
`ctx.audio.find_bus(name)` finds only an existing one, and a name no bus has
is an error that lists the buses there are. Snapshots work the same way:
`ctx.audio.snapshot_id(name)` resolves one into a `SnapshotId` (an error
listing the snapshots for a typo), and `start_snapshot`, `stop_snapshot` and
`snapshot_weight` take that id, with no lookup, as well as the name, for
tools and one-off calls. An id stays valid when the snapshot is replaced by
a reloaded mixer file. In C++, `ctx.find_audio_bus(name)` and
`ctx.audio_snapshot_id(name)` return a `ponko::result`; the snapshot calls
take the id too.

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

## Loops, fades and speed

A looping sound loops all of itself unless it has loop points: with
`set_loop(sound, start, end)` a playback plays from the beginning to `end`
(`None`: the end of the sound), then goes back to `start`, so an intro plays
once and the rest repeats without a seam. Playing sounds fade with
`set_volume(playing, volume, seconds)` (1 is the volume it was played at)
and `fade_out(playing, seconds)`, which stops it at the end; both are
linear in amplitude, so a crossfade's two halves add up to the whole:

<div class="lang-pair">
<div>

```rust
let theme = ctx.audio.load("theme.ogg");
let night = ctx.audio.load("night.ogg");
ctx.audio.set_loop(theme, 2.0, None); // two seconds of intro, then the rest loops
let looping = SoundParams { looping: true, ..Default::default() };
let day = ctx.audio.play_ex(theme, looping);
// Later: hand over to the night theme over two seconds.
ctx.audio.fade_out(day, 2.0);
let next = ctx.audio.play_ex(night, looping);
ctx.audio.set_volume(next, 0.0, 0.0);
ctx.audio.set_volume(next, 1.0, 2.0);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:audio_loops}}
```

</div>
</div>

`set_speed(playing, speed, seconds)` changes speed and pitch together, as a
tape does: an engine revving, a slow-motion moment. For pitch without
tempo, put a `BusEffect::PitchShift` on the bus (a voice's own bus, if it
should apply to one sound): it plays the bus `ratio` higher (2 is an
octave; `PitchShiftEffect::semitones(n)` makes one) at the same speed, with
a slight chorus-like smear on sustained notes, the usual price of a
real-time shifter. `PitchShiftEffect::undo_speed(speed)` cancels the pitch
change of `set_speed`, so time can slow without everything dropping in
pitch:

<div class="lang-pair">
<div>

```rust
# use ponko::audio::{BusEffect, PitchShiftEffect};
let voices = ctx.audio.bus("voices");
ctx.audio.set_bus_effects(voices, &[BusEffect::PitchShift(PitchShiftEffect::semitones(3.0))]);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:audio_pitch}}
```

</div>
</div>

## Music

`play_music` plays a `Song`: one or more layers (stems of the same length)
that start together and stay in sync sample for sample, each at its own
volume. Give the song a tempo and changes can wait for the music: the
layers come in or out (`set_music_layer`), a new song takes over
(`play_music` again, crossfading the old one out) or a stinger plays over
it (`play_stinger`) on the next beat (`Quantize::Beat`) or bar line
(`Quantize::Bar`) instead of at once (`Quantize::Now`). The changes are
scheduled on the audio thread's clock, so they land on the same sample in
every layer, whatever the frame rate.

<div class="lang-pair">
<div>

```rust
# use ponko::audio::{Quantize, Song};
let (pads, drums, sting) = (ctx.audio.load("pads.ogg"), ctx.audio.load("drums.ogg"), ctx.audio.load("sting.ogg"));
// The drums wait silent, in sync.
ctx.audio.play_music(Song::new(pads).layer(drums, 0.0).tempo(120.0, 4), 0.0, Quantize::Now);
// Combat starts: the drums come in on the next bar line, over half a second.
ctx.audio.set_music_layer(1, 1.0, 0.5, Quantize::Bar);
ctx.audio.play_stinger(sting, Quantize::Beat);
ctx.audio.set_music_speed(1.1, 4.0); // faster as the timer runs out
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:audio_music}}
```

</div>
</div>

- Music plays on the `music` bus unless `set_music_bus` says otherwise, so
  the mixer's volume, ducking and snapshots apply to it.
- `set_music_speed` changes tempo and pitch together, and the beat grid
  moves with it; a new song's tempo starts at its first beat.
- `music_beat()` is the beats played since the song started (it stops
  while the music's bus is paused), for gameplay on the beat;
  `music_layer_volume(i)` is a layer's volume once its change came.
- Layers loop, and their loop points (set on each layer's sound with
  `set_loop`) give a song an intro.
- Music voices never go virtual and don't count against the voice limits.

## Pausing sound with the game

`pause_bus(bus, true)` holds every sound on a bus, and on the buses under
it, where it is; `pause_bus(bus, false)` carries on from there, and sounds
started meanwhile wait. Put the world's sounds on one bus and the menu's on
another, and a pause menu can stop one while the other plays.
`Game::pause_audio_in(state, buses)` does it for a [game state](states.md):
the buses are paused from entering the state to leaving it, including while
other states are pushed over it.

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
# use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Mode { Playing, Paused }

# fn main() {
// While the pause menu is up, the world's sound and the music hold where they are.
Game::new("game", 1280, 720)
    .state(Mode::Playing)
    .pause_audio_in(Mode::Paused, &["world", "music"])
    .run(|_| {});
# }
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:audio_pause}}
```

</div>
</div>

A paused positional sound keeps its place in a one-shot too: it doesn't
end while it is held.

## Voice limits

At most 64 sounds are heard at once unless `set_voice_limit` says otherwise
(1 to 256), and a bus can have a limit of its own
(`set_bus_voice_limit(bus, Some(8))`: eight footsteps at most). Past the
limits, sounds are ranked by priority (`set_priority`, 0–255, 128 unless
set; `d3::AudioSource::with_priority` for one on an entity), then by how
loud they are heard (their volume, distance and buses), then by age. The
ones ranked out are **virtual**: not mixed, but followed, so when a slot
frees up (a louder sound ends, the player walks closer) they are heard
again from where they would be by then. A sound nobody can hear (beyond its
maximum distance, on a muted bus, at volume 0) is always virtual. A heard
sound keeps its place until a virtual one is clearly louder, so two sounds
of about the same loudness don't take turns.

A positional source's `priority` can change while it plays: the frame's
positional update passes the new value on, so a sound that becomes
important (an enemy that starts chasing the player) can take a voice from a
less important one without restarting.

```rust
# use ponko::d3::AudioSource;
# #[derive(Component)]
# struct Chasing;
fn raise_chasers(ctx: &mut Ctx) {
    ctx.world
        .query::<&mut AudioSource>()
        .filter::<With<Chasing>>()
        .for_each(|source| source.priority = 220);
}
```

<div class="lang-pair">
<div>

```rust
let line = ctx.audio.load("line.ogg");
ctx.audio.set_voice_limit(48);
let steps = ctx.audio.bus("footsteps");
ctx.audio.set_bus_voice_limit(steps, Some(8));
let dialogue = ctx.audio.play(line);
ctx.audio.set_priority(dialogue, 250); // never lost in a crowd
let count = ctx.audio.voice_count();
println!("{} heard, {} virtual", count.real, count.virtualized);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:audio_voices}}
```

</div>
</div>

So a level can scatter a thousand looping sources (torches, machines,
water) and pay for the few near the player. Measured with `cargo run
--release -p ponko_bench --features audio -- audio3d`: 1,000 looping
positional sources over a 200 m square, 30 m audible range, a listener
walking through them, mixed offline at 48 kHz. On an Apple M4 Max, one run
on an idle machine (2026-11-20), medians:

| Limit | Heard / virtual | Game side of a frame (median) | Mixing a frame's 1/60 s (median) |
|---|---|---|---|
| 64 (default) | 64 / 936 | 0.062 ms | 0.92 ms (5.5% of the frame) |
| 256 | 88 / 912 | 0.062 ms | 1.29 ms (7.7%) |

The game side is moving every source to the listener and ranking the
voices; the mix runs on the audio thread. The 95th percentiles are close to
the medians (0.93 and 1.36 ms for the mix).

## Output devices

The engine plays on the system's default output unless the game chooses
another: `output_devices()` lists them (`AudioDevice { id, name, default }`)
for a settings menu, and `set_output_device(Some(id))` plays on one from
then on; save the id with the settings. When the device playing goes away
(headphones unplugged, a USB headset off), the output opens again on the
default device and every sound carries on from where it was, music on its
beat; when a device the player chose comes back, sound goes back to it.
Without any device, the engine tries again every two seconds.
`device_events()` says what happened this frame (`Lost(name)`, then
`Switched(name)`), for a message to the player:

<div class="lang-pair">
<div>

```rust
# use ponko::audio::AudioDeviceEvent;
# let saved: Option<String> = None;
let devices = ctx.audio.output_devices(); // for the settings menu
if let Some(id) = &saved
    && ctx.audio.set_output_device(Some(id.as_str())).is_err()
{
    ctx.audio.set_output_device(None)?; // gone: follow the system's default
}
for event in ctx.audio.device_events() {
    if let AudioDeviceEvent::Switched(name) = event {
        println!("sound now plays on {name}");
    }
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:audio_devices}}
```

</div>
</div>

## Testing the mix

A headless run plays nothing, but `open_offline(sample_rate)` gives it an
output that mixes into memory: `render_offline(seconds)` returns the next
stretch of the mix as (left, right) frames, so a test can check what the
game sounds like (that the music ducks under dialogue, that a layer comes
in on the bar) without a sound card or a speaker. The engine's own tests do
exactly this, and test device loss and return through a fake set of
devices.

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
# fn main() {
Game::new("t", 1, 1).run_headless(0, |ctx| {
    ctx.audio.open_offline(48_000);
    let tone: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.0576).sin() * 0.4).collect();
    let tone = ctx.audio.create(48_000, &tone);
    ctx.audio.play(tone);
    let mix = ctx.audio.render_offline(0.1); // 4,800 frames
    assert!(mix.iter().any(|[left, _]| left.abs() > 0.1));
});
# }
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:audio_offline}}
```

</div>
</div>

The engine moves fades, music and voice limits once a frame; render a
frame's time between frames, as a game's frames would.

From C, all of the above is in `ponko.h` under the same names:
`ponko_ctx_set_sound_loop`, `ponko_ctx_set_sound_volume`,
`ponko_ctx_set_sound_speed`, `ponko_ctx_fade_out_sound`,
`PONKO_BUS_EFFECT_PITCH_SHIFT` (its `ratio` and `mix`),
`ponko_ctx_play_music` (layers and their volumes as two arrays),
`ponko_ctx_set_music_layer`, `ponko_ctx_play_stinger` (`PonkoQuantize`),
`ponko_ctx_pause_audio_bus`, `ponko_game_pause_audio_in_state`,
`ponko_ctx_set_audio_voice_limit`, `ponko_ctx_set_audio_bus_voice_limit` (0
for none), `ponko_ctx_set_sound_priority`, `ponko_ctx_audio_output_devices`
and `ponko_ctx_audio_device_events` (JSON, two-call buffer convention),
`ponko_ctx_set_audio_output_device`, `ponko_ctx_open_audio_offline` and
`ponko_ctx_render_audio_offline` (interleaved floats).

## In a browser

Browsers let a page make sound only after the player has clicked, tapped or
pressed a key. The engine opens its audio output at that first gesture, and
`ctx.audio.unlocked()` becomes `true` when that output starts (on the desktop
it is always `true`). Before the gesture:

- Looping sounds (music, ambience) wait and start at the gesture, with the
  volume, bus and effects they were given. Their handles work meanwhile:
  `stop` cancels one, `set_effects` changes it.
- One-shots are dropped: a click or a footstep heard seconds late would be
  out of place.

So a game starts its music in setup as it does on the desktop. If the
player should hear the opening, show a prompt for the first gesture:

```rust
# use ponko::prelude::*;
# fn title(ctx: &mut Ctx) {
if !ctx.audio.unlocked() {
    ctx.gfx.draw_text("Click to start", vec2(40.0, 40.0), 32.0, Color::WHITE);
}
# }
```

Some WebKit builds on Linux can report zero output channels while the
sound device list is still being detected. The engine logs that once and
tries stereo through the browser's default destination. If the browser
cannot open or start the output, `ctx.audio.unlocked()` stays `false` and
the engine logs one warning with the reason.

Streamed sounds (`load_stream`) keep the fetched bytes encoded and decode a
packet at a time as they play, on the page's thread. A three-minute stereo
track at 44.1 kHz takes about 64 MB decoded whole, as `load_sound` holds it
(twice that for a moment while it decodes). Streamed, it takes its encoded
size (3.6 MB as a 160 kbit/s MP3) and at most 1.6 MB more while it plays.
Use `load_stream` for music and long ambience, and `load_sound` for short
effects, which start without any decoding work.

Everything above works in a browser, with these differences:

- There is one output device, `Browser`: the browser picks the speakers,
  so `set_output_device` can only choose it, and `device_events` stays
  empty.
- Streamed sounds honour loop points, start positions, fades and music's
  beat and bar timing as on the desktop; a change waits for the audio
  block it falls in (128 frames, under 3 ms at 48 kHz). Going back to a
  loop point seeks in the file where its format allows (WAV, FLAC, OGG)
  and otherwise decodes from the start up to it.
- `open_offline` works as on the desktop, for tests compiled to the web.

From C, `ponko_ctx_audio_unlocked` answers the same (C++:
`ctx.audio_unlocked()`); native games run on the desktop, where it is
always `true`.
