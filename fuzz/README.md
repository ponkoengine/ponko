# Fuzz targets

Fuzzing for the loaders that read data the game doesn't control: files a
player, a mod or a download brings, and packets from the network. Each
target feeds the fuzzer's bytes to one loader, through the public API the
way a game loads the file, and must never crash, hang or allocate without
bound. Errors are fine. Where the input is accepted, the target also checks
what must hold for it (a loaded scene saves and loads again, an action map
reads back).

The list of targets, with the seconds the weekly job runs each, is
`fuzz/targets.txt` (one target per line). A new target needs its file in
`fuzz_targets/`, its `[[bin]]` in `Cargo.toml`, its line there and its row
in this table; `python3 scripts/check-fuzz-list.py` fails when one is missing.

| Target | What it reads |
|---|---|
| `scene` | A scene or world save (`ctx.load_scene`, the `World::load` format), then two ticks of the engine's systems |
| `prefab` | A prefab, one entity or a tree that may include itself (`ctx.spawn_prefab`, `despawn_prefab`) |
| `gltf` | A `.glb` or `.gltf` model: mesh data, the mesh with its materials and images, skeleton and clips; with `gltf-meshopt`, compressed geometry too |
| `unity_yaml` | A Unity text asset (`UnityFile::parse`, `AnimationClip::parse`) and every reader of its objects: meshes, textures, materials, terrains, the prefab hierarchy |
| `unity_binary` | A Unity binary serialized file (`UnityFile::parse_binary`, `AnimationClip::parse_binary`) and the same readers |
| `action_map` | Saved action bindings (`ActionMap::from_json`), installed and queried |
| `asset_pack` | An asset pack (`PackSource::from_bytes`) and every file in it |
| `lockstep` | Datagrams to a running two-player lockstep session (`Net`) |
| `messages` | Datagrams to a connected message host and client (`NetMessages`) |
| `replication` | Snapshots, inputs and events to a replication server and a client that joined it (`Replication`) |
| `lobby` | Lobby protocol frames from a hostile client to a lobby server (`ponko_lobby`), and from a hostile server to a `LobbyClient` |
| `control` | Control endpoint request lines: refused without the token, then run by the game's dispatcher |
| `aseprite` | An Aseprite file (`aseprite-source`) and a cooked Aseprite sheet, every frame, layer and animation |
| `levels` | An LDtk project and a Tiled map (JSON and XML), loaded, merged into solid rectangles and spawned |
| `cooked_texture` | A texture as `ponko cook` ships it (KTX2, DDS) or any image file, decoded for GPUs with and without block formats, and streamed |
| `sound` | A sound file in any compiled format (WAV, AIFF, ADPCM, OGG Vorbis, FLAC, MP3): decoded whole, decoded in slices, streamed from memory, run through the desktop's streaming decoder (decoding and seeking, as Kira's decoder thread does) and played as the web streams it, which must refuse absurd headers and stop at the length limit |
| `webm_audio` | The sound of a WebM file (feature `video`): the tracks a video reads first, then its soundtrack's header and a little of it played, with seeks and a loop, as a video plays it |

The network targets reach the packet parsers behind a connected session
through `ponko::__fuzz`, which only exists in builds with `--cfg fuzzing`
(as `cargo fuzz` builds); the unit tests in `src/net/fuzz.rs` run the same
entry points on every `cargo test`. Their input is a list of datagrams,
each `[flags][length: u16 LE][bytes]`; the flags pick the receiving peer,
the sender or channel, and whether to write a valid header first (see that
file). `control` and `cooked_texture` use `ponko::__fuzz` too (the
dispatcher and the texture decoders are internal), with unit tests in
`src/control.rs` and `src/assets.rs`. The `control` target runs in a
temporary folder of its own, since requests may write files below the
folder the game runs in.

The `sound` target reaches the sound decoders through `ponko::__fuzz` too
(a whole decode, the slice-by-slice decode asset groups use, a stream opened
from memory, the decoder a desktop stream plays through and the web's
packet-at-a-time stream). None of them opens an audio device: the desktop's
streaming decoder is the engine's own (`src/audio/stream_decoder.rs`),
which Kira's decoder thread runs, so the target calls it directly. The
`webm_audio` target does the same for a video's soundtrack, which the
`video` feature reads from the WebM file with the Matroska reader; it runs
the soundtrack even when the file's picture track would be refused, so the
fuzzer reaches the audio without building a valid AV1 track first. Its
seeds in `fuzz/seeds/webm_audio/` are the video fixtures cut down to their
headers and the sound of their first cluster; `seed-corpus.sh` adds the
whole fixtures too.

The first two-minute run of `webm_audio` found a hang in the engine: a
looping soundtrack whose packets decode to no frames went back to its start
forever. It is fixed in `src/audio/chunked.rs`, and the input is the
regression test's `tests/fixtures/video/no-frames-soundtrack.webm`. The
second run found a panic in the Matroska reader (`matroska-demuxer`: a
block group's size overflows its position arithmetic), which the engine now
turns into an error at its boundary (`src/video/demux.rs`; input
`tests/fixtures/video/huge-block-group.webm`).

Both targets catch the decoders' panics the way the engine does (a panic
inside a sound decoder is an error) and print the first few: the run goes
on. On Linux the fuzz crate's build needs the audio output library's headers
(`libasound2-dev`) and, for the AV1 decoder's x86 assembly that `video`
brings, `nasm`.

FBX models are not fuzzed here: the importer hands them to ufbx, a C library
fuzzed upstream, which `cargo fuzz` can't instrument.

## Running

Needs nightly Rust and [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz):

```sh
rustup toolchain install nightly
cargo install cargo-fuzz --locked
cp Cargo.lock fuzz/Cargo.lock   # the engine's versions (git-ignored here)
fuzz/seed-corpus.sh             # fuzz/corpus/<target>: seeds plus fixtures
cd fuzz
cargo +nightly fuzz run gltf corpus/gltf -- -max_total_time=600 -rss_limit_mb=2048 -timeout=10
```

Or every target in turn, with the flags the weekly job uses:

```sh
fuzz/run-all.sh            # as long as targets.txt says; findings in fuzz/findings.tsv
fuzz/run-all.sh 600        # ten minutes each
fuzz/run-all.sh 3600 scene # an hour of one target
```

`-rss_limit_mb=2048` makes an input that drives a loader past 2 GB a
finding, and `-timeout=10` one that takes ten seconds (a hang). A finding is
saved in `fuzz/artifacts/<target>/`; run it again with
`cargo +nightly fuzz run <target> <file>` (add `RUST_BACKTRACE=1` for where
it panicked), and `cargo +nightly fuzz tmin <target> <file>` shrinks it.

Build output goes to `fuzz/target/` (a few GB with the address sanitizer):
delete it when you are done. `corpus/`, `artifacts/`, `logs/` and
`Cargo.lock` are git-ignored.

## Fixing a finding

Fix the loader, not the target, and add a regression test next to the
loader's other tests, named after what must hold (for example
`a_pack_whose_header_sizes_overflow_is_a_decode_error`), so the plain
`cargo test` run keeps it fixed. Commit a new seed to `fuzz/seeds/<target>/`
only when it is small (a few kilobytes) and reaches code the existing seeds
don't.

## The weekly job

`.github/workflows/fuzz.yml` runs every target for three minutes each Monday
on `main` (or by hand, with the seconds per target), from the committed seeds
and the fixtures. A finding fails the run and opens an issue,
"Fuzzing: `<target>` (`<kind>`)", or comments on the open one; the inputs
are in the run's `fuzz-findings` artifact. Nothing runs on pull requests:
run the target of a loader you change for a few minutes before you open
one.
