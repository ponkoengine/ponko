# Video: intros and screens in the world

With the `video` feature (off by default), a game plays WebM files into a
texture: a studio logo before the menu, a cutscene, a looping clip on a
television in the world. The pictures are AV1, decoded on the CPU by
[rav1d](https://crates.io/crates/rav1d) (the Rust port of the dav1d
decoder) and copied into an ordinary texture each frame; the sound is
Vorbis, played through the mixer on a bus of your choosing.

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

The picture follows the frame clock and the sound the audio device's; both
run in real time and stay together. The frame clock is clamped to 0.25 s a
frame, so a frame that blocks for longer (loading a level under the video)
leaves the picture behind the sound: `video.seek(ctx, video.time())` after
such a load lines them up again. With `advance` at another speed, the sound
doesn't follow.

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

The `video/frame` benchmark times every step of a playing video that shows
a new frame (demuxing, decoding, converting to RGBA), as `Video::update`
does in a game:

```sh
PONKO_BENCH_VIDEO=clip.webm cargo run --release -p ponko_bench --features video -- video
```

Measured on an Apple M4 Max (14 cores), release build, rav1d without its
assembly as the engine builds it, 30 fps clips encoded with `-crf 32`. The
machine was shared with other builds (load average 130 to 170), so these
are numbers for a busy machine; the minimum is a frame decoded without
waiting for a core:

| Clip | Median | 95th percentile | Minimum |
|---|---|---|---|
| 1280 x 720 | 23.3 ms | 92 ms | 0.66 ms |
| 1920 x 1080 | 27.9 ms | 124 ms | 0.87 ms |

For comparison, the C dav1d 1.5.5 command-line decoder with the same
settings (4 threads, one frame in flight), on the same machine under the
same load, decoded the clips at about 330 and 170 frames per second, 3 and
6 ms a frame. Measure your own clips on the hardware you ship for.

Decoding takes up to four threads of its own while a frame decodes, and the
game's thread waits for the frame: a video costs that much of every frame
in which it shows a new picture. Converting to RGBA runs on rayon's threads
for frames 256 rows or taller. That suits an intro or a cutscene, where
little else runs, and a small screen in the world (a 512 x 288 clip is a
sixth of the 720p one's pixels); several large videos at once are not what
it is for, and 720p or smaller is the safe size for intros.

rav1d has hand-written assembly behind its `asm` feature, which the engine
leaves off: it needs `nasm` to build on x86-64, and the published rav1d
1.1.0 can't build it on arm64 (the package lacks `src/arm/asm-offsets.h`;
tried on macOS).

The feature adds 2.0 MB to a release build of a game (the `video_player`
example: 22.2 MB without it, 24.2 MB with it, macOS arm64). Off, nothing of
it is built.

## Why AV1 with rav1d: the decoders compared

Video was weighed for two uses, intros and cutscenes (once, full screen,
with sound) and screens in the world (looping, small, often silent). What
was compared, from each crate's own metadata and documentation (November
2026):

| Decoder | Codecs | License | Patents | Build and size | Web |
|---|---|---|---|---|---|
| **rav1d 1.1.0** (chosen) | AV1 | BSD-2-Clause | AV1 is royalty-free under the Alliance for Open Media patent license | Pure Rust without its assembly, no system library; 2.0 MB | Does not build for `wasm32-unknown-unknown` |
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
well, and rav1d decodes it in pure Rust with a permissive license, from one
code path on every desktop system, without anything to install. The
container is read by
[matroska-demuxer](https://crates.io/crates/matroska-demuxer) (Zlib, MIT or
Apache-2.0, no dependencies), the sound by the Vorbis decoder of
[Symphonia](https://crates.io/crates/symphonia), which the `audio` feature
already builds.

## In a browser

rav1d doesn't build for the web (it uses C error numbers that
`wasm32-unknown-unknown` doesn't define), and a decoder compiled to
WebAssembly would run on the page's one thread anyway. The web build
compiles the `video` API, and `Video::load` returns an `Unsupported` error.

The browser has video decoders of its own. For an intro, put a `<video>`
element over the canvas from the page and remove it when it ends: copy
`web/index.html` and add, for example,

```html
<video id="intro" src="assets/intro.webm" autoplay playsinline
       style="position:fixed; inset:0; width:100%; height:100%; background:#000"
       onended="this.remove()" onclick="this.remove()"></video>
```

(browsers start a video with sound only after a click or key press, the
same rule as the game's own sound). The game can wait for it by checking
for the element from Rust through `web-sys`, or simply start behind it.
A screen in the world isn't possible on the web this way.

## C and C++

Not bound yet: C and C++ games have no `Video` (it is listed in [C and
C++](c-cpp.md#still-rust-only)); they can decode with a library of their own
and show the frames with `ponko_ctx_update_texture`.

## Testing

Everything above runs headless: `Video` decodes without a GPU (the texture
is a headless handle, and `frame_rgba()` holds the pixels), and the sound
can be rendered with the [offline output](audio.md). The engine's own tests
(`tests/video.rs`) check the frame shown at known times, seeking both ways,
looping, the end, and the soundtrack's pitch before and after a seek, on
small files made by `tests/fixtures/video/make.sh` from generated pictures
and tones.
