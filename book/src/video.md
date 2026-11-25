# Video: intros and screens in the world

> **Maturity: [tested](maturity.md).** Game: none. Tests: `tests/video.rs`,
> `video_render_test.rs`, with the `video` feature. Platforms: Linux, macOS
> and Windows (the `video` lines of the list of checks); desktop only, the web
> reports `Unsupported`. Graphics: offscreen render tests, required on Windows
> (DirectX 12 on the WARP software adapter) and run on Linux and macOS only
> where a GPU is found. Missing: a reference game that uses it.

With the `video` feature (off by default), a game plays WebM files into a
texture: a studio logo before the menu, a cutscene, a looping clip on a
television in the world. The pictures are AV1, decoded on the desktop by
[rav1d](https://crates.io/crates/rav1d) (the Rust port of the dav1d
decoder) and converted to RGBA on the GPU into an ordinary texture, and in
the browser by the browser's own decoder, behind the same API; the sound
is Vorbis, played through the mixer on a bus of your choosing, and the
picture keeps to it.

```toml
[dependencies]
ponko = { version = "0.1", features = ["video"] }
```

```rust,no_run
# use ponko::prelude::*;
use ponko::video::Video;

struct Intro(Video);

fn play_intro(ctx: &mut Ctx) {
    let Some(mut intro) = ctx.world.remove_resource::<Intro>() else { return };
    if intro.0.update(ctx).is_err() || intro.0.finished() {
        intro.0.free(ctx); // on to the menu
        return;
    }
    let center = intro.0.size().as_vec2() / 2.0;
    ctx.gfx.draw_ex(intro.0.texture(), center, DrawParams::default().with_space(DrawSpace::Screen));
    ctx.world.insert_resource(intro);
}

Game::new("Intro", 1280, 720)
    .setup(|ctx| {
        let mut intro = Video::load(ctx, "intro.webm").expect("the intro video");
        intro.play(ctx);
        ctx.world.insert_resource(Intro(intro));
    })
    .system(play_intro)
    .run();
```

- `Video::load(ctx, "intro.webm")` reads the file from `assets/` (or an
  [asset source](asset-sources.md)) and shows its first frame, paused.
  `Video::from_bytes` takes the bytes instead. The file stays in memory,
  encoded; one frame is decoded at a time.
- `play`, `pause`, `seek(ctx, seconds)` and `set_looping(true)`. A seek
  decodes from the keyframe before the time asked for, so it costs up to
  one keyframe interval of decoding.
- `update(ctx)` once a frame moves the video on by the frame's time and
  puts the frame for the new time in the texture. Frames between two
  updates are decoded but not converted to pixels; a step past a keyframe
  skips straight to it. `advance(ctx, seconds)` steps by a given time
  instead (tests, slow motion).
- `texture()` is a plain texture: draw it as a sprite, stretch it over the
  screen, or give it to a [material](3d.md) as its base color for a screen
  in the world. `frame_rgba()` has the same pixels on the CPU.
- `finished()` turns true at the end of a video that doesn't loop; `play`
  then starts it over.
- `free(ctx)` frees its texture and its sound.

## Sound

The soundtrack is a sound like any other: `set_bus(ctx, bus)` puts it on a
[bus](audio.md) (the default bus, `sfx`, until you do), so a "cutscene"
slider in the settings and the mixer's ducking apply to it, and
`set_volume(ctx, v)` scales it. It starts and stops with the picture, seeks
with it, and is never made virtual by the voice limit, like music. It is
decoded a packet at a time as it plays, so a long video doesn't hold its
whole sound decoded in memory. `ctx.audio.started()` lists it when it
starts, as it lists every sound, which is how tests check it.

The picture follows the frame clock and the sound the audio device's;
both run in real time and stay together, and `update` keeps them so: when
they drift more than a tenth of a second apart, the one behind catches up.
Frames that didn't run while the sound played on (a level loading under
the video, a game stopped in the debugger, a browser tab in the
background) leave the picture behind, and the next `update` skips it
ahead to the sound; a sound that started late (a browser waiting for the
first click) starts again where the picture is. A seek and a pause keep
them together to the millisecond: the sound goes on from the picture's
time exactly. Pausing the game's simulation (`ctx.time.pause()`) doesn't
pause a video, which runs on the frame clock: call `video.pause(ctx)` in
your pause menu. With `advance` at another speed, the sound doesn't
follow.

## Encoding videos for the engine

The engine plays one kind of file: WebM with AV1 pictures (8 bits, any
chroma layout; 4:2:0 is usual) and either Vorbis sound or none. With
[ffmpeg](https://ffmpeg.org) and its SVT-AV1 encoder:

```sh
ffmpeg -i intro.mov \
  -c:v libsvtav1 -preset 6 -crf 32 -g 60 -pix_fmt yuv420p \
  -c:a libvorbis -q:a 5 \
  intro.webm
```

- `-crf` sets the quality (lower is better and larger; 25 to 35 suits
  most games), `-preset` the encoding speed (lower is slower and smaller).
- `-g 60` puts a keyframe every 60 frames (2 seconds at 30 fps). Seeking
  and looping decode from the keyframe before; a short interval makes them
  cheaper and the file a little larger.
- `-an` instead of the `-c:a` options leaves the sound out (a silent loop
  on a screen in the world).
- Opus sound is refused with a message saying to use Vorbis: the Rust
  Vorbis decoder is the one the engine already ships for `.ogg` sounds.
  ffmpeg builds without `libvorbis` have `-c:a vorbis -strict -2`.
- A file in another format (VP9, H.264, 10-bit AV1) fails to load with an
  `Unsupported` error that names the option to change.

Pictures are converted with the BT.709 matrix unless the file says BT.601
or BT.2020, in TV range unless it says full range.

## What it costs

On the desktop a playing video costs, in every frame in which it shows a
new picture, the decoding of that picture (rav1d's own threads, up to
four, while the game's thread waits) and a copy of its planes; the GPU
then converts them to RGBA in a small pass (1.5 bytes a pixel uploaded
for 4:2:0 instead of 4). `frame_rgba()` converts the frame on the CPU
only when it is asked for. Two benchmarks time these:

```sh
PONKO_BENCH_VIDEO=clip.webm cargo run --release -p ponko_bench --features video -- video
```

`video/frame` times every step of a playing video that shows a new frame,
as `Video::update` does in a game; `video/convert` the CPU conversion
`frame_rgba()` does. Measured on an Apple M4 Max (14 cores), release
build, a 1920 x 1080 clip at 30 fps encoded with `-crf 32 -g 60` (240
frames, 7.8 Mbit/s; made with ffmpeg's SVT-AV1 from its `testsrc2`
pattern with light noise), on an idle machine (2026-11-22, revision
`9235ea05`, load average 1.5), three runs whose medians agreed within 3%:

| 1920 x 1080, per new frame | Median | 95th percentile |
|---|---|---|
| A new frame shown (`video/frame`): assembly on arm64, converted on the GPU | 2.2 ms | 11.7 ms |
| The CPU conversion alone (`video/convert`) | 0.65 ms | 0.78 to 0.87 ms |

One frame in twenty takes about five times the median: decoding cost
differs a lot between the pictures of a clip. When the assembly and the GPU conversion
were added they were measured before and after on a machine shared with
other builds (load average 120 to 143): 18.2 and 22.1 ms a frame without
them, 4.5 and 3.5 ms with them, and 6.5 to 7.2 ms for the CPU conversion,
all several times the idle machine's figures.

Decoding takes up to four threads of its own while a frame decodes, and
the game's thread waits for the frame. That suits an intro or a cutscene,
where little else runs, and a screen in the world; several large videos
at once are not what it is for. Measure your own clips on the hardware
you ship for.

### The decoder's assembly

rav1d carries dav1d's hand-written assembly. The engine uses rav1d as
[Rerun](https://crates.io/crates/re_rav1d) publishes it (`re_rav1d`, the
same code and license), because the published rav1d 1.1.0 lacks a header
its arm64 assembly includes. On arm64 (Apple silicon, ARM Linux and
Windows) the assembly is on: the system's C compiler, which links every
build anyway, builds it, and the CPU's dot-product and i8mm variants are
picked at run time. On x86-64 it needs [nasm](https://nasm.us), which the
engine doesn't require; a game whose build machines have nasm turns it on
by naming the decoder with its `asm` feature next to the engine:

```toml
[dependencies]
ponko = { version = "0.1", features = ["video"] }
re_rav1d = { version = "0.1.3", default-features = false, features = ["bitdepth_8", "asm"] }
```

Without it, x86-64 decodes with rav1d's Rust code, several times slower
than with the assembly.

Without the assembly, the feature added 2.0 MB to a release build of a
game (the `video_player` example, macOS arm64); the assembly and the 16-bit
code its arm64 build needs add 0.4 MB more (measured on the benchmark
binary: 29.03 MB before, 29.42 MB after). Off, nothing of it is built.

## Why AV1 with rav1d: the decoders compared

Video was weighed for two uses, intros and cutscenes (once, full screen,
with sound) and screens in the world (looping, small, often silent). What
was compared, from each crate's own metadata and documentation (November
2026):

| Decoder | Codecs | License | Patents | Build and size | Web |
|---|---|---|---|---|---|
| **rav1d 1.1.0** (chosen, as `re_rav1d` 0.1.3) | AV1 | BSD-2-Clause | AV1 is royalty-free under the Alliance for Open Media patent license | Rust, with dav1d's assembly built by the system's C compiler on arm64 (x86-64: needs nasm, off by default), no system library; 2.4 MB | Does not build for `wasm32-unknown-unknown`: the browser's own decoder plays the same files there |
| dav1d 0.12.0 (bindings) | AV1 | MIT bindings, BSD-2-Clause library | As above | Needs libdav1d from the system or built with meson and nasm | No |
| openh264 0.9.10 | H.264 | BSD-2-Clause | H.264 patents are licensed through a pool (Via LA); Cisco's royalty coverage applies only to its own prebuilt binary, not to a copy built from source as the crate does | Builds Cisco's C++ with the system compiler | No |
| ffmpeg-next 9.1.0 | Nearly all | WTFPL bindings; FFmpeg is LGPL-2.1+ (GPL with some options) | Depends on the codecs enabled | Needs FFmpeg's shared libraries next to the game (tens of MB), dynamically linked to stay LGPL | No |
| gstreamer 0.25.4 | Whatever plugins are installed | MIT/Apache bindings; GStreamer is LGPL | Depends on the plugins | A GStreamer runtime on the system (Linux) or shipped with the game | No |
| AVFoundation (objc2-av-foundation 0.3.2) | H.264, HEVC, ProRes | Zlib/Apache/MIT bindings; system framework | Licensed by the OS vendor | Nothing shipped; macOS and iOS only, its own code path | No |
| Media Foundation (the `windows` crate) | H.264, HEVC (with the extension), VP9 | MIT/Apache bindings; system API | Licensed by the OS vendor; the "N" editions of Windows lack it | Nothing shipped; Windows only, its own code path | No |
| The browser's `<video>` element | H.264, VP9, AV1 (most browsers) | Browser | Licensed by the browser vendor | Nothing shipped | Web only |
| oxideav-mpeg12video 0.0.14 | MPEG-1, MPEG-2 | MIT | Expired | Pure Rust; an early release (0.0.x), and MPEG-1 needs several times AV1's size for the same picture | Builds |
| rav1d-safe 0.6.0 | AV1 | AGPL-3.0 or commercial | As AV1 | Pure Rust | Not considered: the license |
| southstar-codecs 1.0.10 | MPEG-1 (a pl_mpeg port) | GPL-3.0 or a custom license, with MIT | Expired | Pure Rust | Not considered: the license |

The platform APIs need a different code path on every system and leave
Linux to GStreamer; FFmpeg and GStreamer mean shipping and licensing large
native libraries; H.264 brings a patent pool. AV1 is royalty-free, compresses
well, and rav1d decodes it in Rust (with dav1d's assembly where the system's
C compiler builds it) with a permissive license, from one code path on every
desktop system, without anything to install; browsers decode it themselves. The
container is read by
[matroska-demuxer](https://crates.io/crates/matroska-demuxer) (Zlib, MIT or
Apache-2.0, no dependencies), the sound by the Vorbis decoder of
[Symphonia](https://crates.io/crates/symphonia), which the `audio` feature
already builds.

## In a browser

In the web build, `Video` takes its pictures from the browser's own AV1
decoder ([WebCodecs](https://www.w3.org/TR/webcodecs/)' `VideoDecoder`):
the same calls, the same file, the same sound through the mixer. The
engine reads the file and keeps the clock as on the desktop; the browser
decodes on its own threads and hands each frame back a moment later, so
the engine sends frames a few ahead of the one due and shows each once it
is decoded. What differs:

- A frame shows once the browser has decoded it: usually by the next
  `update`, so a video starts on black for a frame or two, and a seek
  shows its frame a few updates later. `update` keeps showing what
  arrives also while paused.
- `frame_rgba()` holds the pixels the browser converted (its color
  conversion, not the engine's), read back through an `OffscreenCanvas`.
- A browser without WebCodecs fails `Video::load` with `Unsupported`; one
  that turns AV1 down (the codec is checked when the browser gets to it)
  makes `update` return a `Decode` error naming the browser's reason.
  Check on the browsers you ship for; keep the video optional (an intro
  that can be skipped).
- As with all sound on the web, the soundtrack waits for the first click
  or key; the picture plays meanwhile and the sound starts where the
  picture is.

`web/smoke/video.mjs` plays the engine's test clip this way in headless
Chromium and checks every frame shown against its time (the
[browser smoke test](https://github.com/ponkoengine/ponko/tree/main/web/smoke)'s
"Video").

## C and C++

Build the C library with the feature (`cargo build -p ponko_ffi --features
video`); without it, loading a video reports `PONKO_UNSUPPORTED`. The calls
follow Rust's: `ponko_ctx_load_video` or `ponko_ctx_video_from_bytes` make
a `PonkoVideo`, `ponko_ctx_video_play`, `_pause`, `_seek`, `_update` and
`_advance` move it, `ponko_video_state` gives its texture, size, time and
whether it finished, and `ponko_ctx_free_video` frees it with its texture
and sound. In C++ it is `ponko::video`:

```cpp
{{#include ../../examples/cpp/guide.cpp:video_intro}}
```

`ponko_video_frame_rgba` hands the frame's pixels on the CPU, valid until
the video changes. A file in another codec is `PONKO_UNSUPPORTED`, with
`ponko_last_error` naming the encoder settings to use instead.

## Testing

Everything above runs headless: `Video` decodes without a GPU (the texture
is a headless handle, and `frame_rgba()` holds the pixels), and the sound
can be rendered with the [offline output](audio.md). The engine's own tests
(`tests/video.rs`) check the frame shown at known times, seeking both ways,
looping, the end, and the soundtrack's pitch before and after a seek, on
small files made by `tests/fixtures/video/make.sh` from generated pictures
and tones. With the clip's tone, which turns from 440 Hz to 880 Hz at one
second, they check that the sound stays with the picture after a seek, a
pause, frames that stopped while the sound played on, and a sound that
started late. The offscreen GPU tests (`src/render/video_render_test.rs`)
check the GPU conversion against the CPU one for every chroma layout and
matrix.
