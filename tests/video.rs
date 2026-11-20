//! Video playback (feature `video`), headless: frames at known times,
//! seeking, looping, the end, the soundtrack through the offline mixer, and
//! the files that are refused. The files in `tests/fixtures/video` are made
//! by `make.sh` next to them: 64 x 64, 10 frames a second, 2 seconds, a
//! keyframe every half second; frame `k` has red, green and blue quarters
//! and a gray quarter at level `20 + 10 k`; the sound is 440 Hz for the
//! first second and 880 Hz for the second.
#![cfg(feature = "video")]

use ponko::assets::MemorySource;
use ponko::prelude::*;
use ponko::video::Video;

const CLIP: &[u8] = include_bytes!("fixtures/video/clip.webm");
const SILENT: &[u8] = include_bytes!("fixtures/video/silent.webm");
const OPUS: &[u8] = include_bytes!("fixtures/video/opus.webm");
const VP9: &[u8] = include_bytes!("fixtures/video/vp9.webm");

const RATE: u32 = 48_000;

fn with_ctx(f: impl FnOnce(&mut Ctx) + 'static) {
    let mut f = Some(f);
    Game::new("video", 64, 64).run_headless(0, move |ctx| {
        if let Some(f) = f.take() {
            f(ctx);
        }
    });
}

fn clip(ctx: &mut Ctx) -> Video {
    Video::from_bytes(ctx, CLIP.to_vec()).expect("the test clip loads")
}

/// The pixel at `(x, y)` of the frame shown.
fn pixel(video: &Video, x: usize, y: usize) -> [u8; 3] {
    let i = (y * 64 + x) * 4;
    let p = &video.frame_rgba()[i..i + 3];
    [p[0], p[1], p[2]]
}

fn near(a: [u8; 3], b: [u8; 3], tolerance: u8) -> bool {
    a.iter().zip(b).all(|(&x, y)| x.abs_diff(y) <= tolerance)
}

/// Which frame is shown, read from the gray quarter's level.
fn frame_number(video: &Video) -> i32 {
    let level = i32::from(pixel(video, 48, 48)[0]);
    (level - 20 + 5).div_euclid(10)
}

#[test]
fn a_loaded_video_is_paused_on_its_first_frame() {
    with_ctx(|ctx| {
        let video = clip(ctx);
        assert_eq!(video.size(), uvec2(64, 64));
        assert_eq!((video.texture().width(), video.texture().height()), (64, 64));
        assert!((video.seconds() - 2.0).abs() < 0.05, "{}", video.seconds());
        assert!(!video.playing() && !video.finished());
        assert_eq!((video.time(), video.frame_time()), (0.0, 0.0));
        // The quarters, in the right places and colors (AV1 is lossy: near).
        assert!(
            near(pixel(&video, 16, 16), [255, 0, 0], 12),
            "{:?}",
            pixel(&video, 16, 16)
        );
        assert!(
            near(pixel(&video, 48, 16), [0, 255, 0], 12),
            "{:?}",
            pixel(&video, 48, 16)
        );
        assert!(
            near(pixel(&video, 16, 48), [0, 0, 255], 12),
            "{:?}",
            pixel(&video, 16, 48)
        );
        assert!(
            near(pixel(&video, 48, 48), [20, 20, 20], 6),
            "{:?}",
            pixel(&video, 48, 48)
        );
        assert!(video.frame_rgba().chunks(4).all(|p| p[3] == 255));
        assert_eq!(frame_number(&video), 0);
    });
}

#[test]
fn a_video_loads_from_an_asset_source() {
    with_ctx(|ctx| {
        ctx.assets
            .add_source(MemorySource::new("films").with("intro.webm", CLIP));
        let video = Video::load(ctx, "intro.webm").unwrap();
        assert_eq!(video.size(), uvec2(64, 64));
        let missing = Video::load(ctx, "missing.webm").unwrap_err();
        assert_eq!(missing.kind(), ErrorKind::NotFound);
    });
}

#[test]
fn playing_shows_the_frame_of_each_time() {
    with_ctx(|ctx| {
        let mut video = clip(ctx);
        // Paused: time doesn't move.
        video.advance(ctx, 0.5).unwrap();
        assert_eq!((video.time(), frame_number(&video)), (0.0, 0));
        video.play(ctx);
        let mut time = 0.0f32;
        for _ in 0..19 {
            video.advance(ctx, 0.1).unwrap();
            time += 0.1;
            let expected = (time * 10.0 + 1e-3).floor() as i32;
            assert_eq!(frame_number(&video), expected, "at {time} s");
            assert!((video.frame_time() - expected as f32 / 10.0).abs() < 1e-3);
            // The other quarters stay put.
            assert!(near(pixel(&video, 16, 16), [255, 0, 0], 12));
        }
        // Between frames, the earlier one stays up.
        video.advance(ctx, 0.04).unwrap();
        assert_eq!(frame_number(&video), 19);
    });
}

#[test]
fn a_long_step_skips_ahead_through_the_keyframes() {
    with_ctx(|ctx| {
        let mut video = clip(ctx);
        video.play(ctx);
        video.advance(ctx, 1.62).unwrap();
        assert_eq!(frame_number(&video), 16);
        video.advance(ctx, 0.1).unwrap();
        assert_eq!(frame_number(&video), 17);
    });
}

#[test]
fn seeking_shows_the_frame_at_that_time_forward_and_back() {
    with_ctx(|ctx| {
        let mut video = clip(ctx);
        for (to, frame) in [(1.37, 13), (0.2, 2), (0.95, 9), (1.0, 10), (0.0, 0), (1.99, 19)] {
            video.seek(ctx, to).unwrap();
            assert_eq!(frame_number(&video), frame, "after seeking to {to} s");
            assert!((video.time() - to).abs() < 1e-6);
        }
        // Past either end: clamped.
        video.seek(ctx, 99.0).unwrap();
        assert_eq!((frame_number(&video), video.time()), (19, video.seconds()));
        video.seek(ctx, -5.0).unwrap();
        assert_eq!((frame_number(&video), video.time()), (0, 0.0));
        // Seeking while playing keeps playing from there.
        video.play(ctx);
        video.seek(ctx, 0.5).unwrap();
        video.advance(ctx, 0.3).unwrap();
        assert_eq!(frame_number(&video), 8);
        assert!(video.playing());
    });
}

#[test]
fn a_video_stops_on_its_last_frame_and_play_starts_it_over() {
    with_ctx(|ctx| {
        let mut video = clip(ctx);
        video.play(ctx);
        video.advance(ctx, 1.5).unwrap();
        video.advance(ctx, 1.5).unwrap();
        assert!(video.finished() && !video.playing());
        assert_eq!((video.time(), frame_number(&video)), (video.seconds(), 19));
        video.advance(ctx, 1.0).unwrap();
        assert_eq!(frame_number(&video), 19, "a finished video stays on its last frame");
        video.play(ctx);
        assert!(video.playing() && !video.finished());
        assert_eq!((video.time(), frame_number(&video)), (0.0, 0));
        video.advance(ctx, 0.3).unwrap();
        assert_eq!(frame_number(&video), 3);
    });
}

#[test]
fn a_looping_video_goes_back_to_the_start_and_keeps_playing() {
    with_ctx(|ctx| {
        let mut video = clip(ctx);
        video.set_looping(true);
        video.play(ctx);
        video.advance(ctx, 1.8).unwrap();
        video.advance(ctx, 0.5).unwrap();
        assert!(video.playing() && !video.finished());
        assert!((video.time() - 0.3).abs() < 1e-3, "{}", video.time());
        assert_eq!(frame_number(&video), 3);
        // Many loops in one step.
        video.advance(ctx, 2.0 * 3.0 + 0.45).unwrap();
        assert_eq!(frame_number(&video), 7);
    });
}

#[test]
fn update_follows_the_frame_clock() {
    let world = Game::new("video", 64, 64)
        .fixed_hz(10)
        .headless_frames(true)
        .setup(|ctx| {
            let mut video = clip(ctx);
            video.play(ctx);
            ctx.world.insert_resource(video);
        })
        .system(|ctx: &mut Ctx| {
            let mut video = ctx.world.remove_resource::<Video>().unwrap();
            video.update(ctx).unwrap();
            ctx.world.insert_resource(video);
        })
        .run_headless(5);
    let video = world.resource::<Video>();
    // Five frames of a tenth of a second each.
    assert!((video.time() - 0.5).abs() < 1e-3, "{}", video.time());
    assert_eq!(frame_number(video), 5);
}

/// How many times the left channel crosses zero going up, per second.
fn pitch(mix: &[[f32; 2]]) -> f32 {
    let ups = mix.windows(2).filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0).count();
    ups as f32 * RATE as f32 / mix.len() as f32
}

fn loudness(mix: &[[f32; 2]]) -> f32 {
    (mix.iter().map(|f| f[0] * f[0]).sum::<f32>() / mix.len().max(1) as f32).sqrt()
}

#[test]
fn the_soundtrack_plays_through_the_mixer_from_the_video_time() {
    with_ctx(|ctx| {
        ctx.audio.open_offline(RATE);
        let mut video = clip(ctx);
        let sound = video.sound().expect("the clip has sound");
        assert!((sound.seconds() - video.seconds()).abs() < 0.05);
        video.play(ctx);
        let started = ctx.audio.started().to_vec();
        assert_eq!(started.len(), 1);
        assert_eq!((started[0].sound, started[0].bus), (sound, ctx.audio.bus("sfx")));
        let mix = ctx.audio.render_offline(0.5);
        if cfg!(feature = "audio") {
            assert!(loudness(&mix[2400..]) > 0.1, "{}", loudness(&mix));
            let hz = pitch(&mix[2400..]);
            assert!((hz - 440.0).abs() < 20.0, "the first second is 440 Hz, heard {hz}");
        }
        // Seeking into the second second moves the sound with the picture.
        video.seek(ctx, 1.25).unwrap();
        let mix = ctx.audio.render_offline(0.4);
        if cfg!(feature = "audio") {
            let hz = pitch(&mix[2400..]);
            assert!((hz - 880.0).abs() < 30.0, "the second second is 880 Hz, heard {hz}");
        }
        // Paused: silence (after the short fade).
        video.pause(ctx);
        let mix = ctx.audio.render_offline(0.2);
        assert!(loudness(&mix[2400..]) < 1e-3, "{}", loudness(&mix[2400..]));
        // And on again, from where it was.
        video.play(ctx);
        let mix = ctx.audio.render_offline(0.3);
        if cfg!(feature = "audio") {
            assert!(loudness(&mix[2400..]) > 0.1);
        }
    });
}

#[test]
fn the_soundtrack_takes_the_bus_and_volume_it_is_given() {
    with_ctx(|ctx| {
        ctx.audio.open_offline(RATE);
        let mut video = clip(ctx);
        let cutscene = ctx.audio.bus("cutscene");
        video.set_bus(ctx, cutscene);
        video.set_volume(ctx, 0.0);
        video.play(ctx);
        assert_eq!(ctx.audio.started()[0].bus, cutscene);
        assert_eq!((video.bus(), video.volume()), (cutscene, 0.0));
        let mix = ctx.audio.render_offline(0.3);
        assert!(loudness(&mix[2400..]) < 1e-3, "volume 0 is silent: {}", loudness(&mix));
        video.set_volume(ctx, 1.0);
        let mix = ctx.audio.render_offline(0.3);
        if cfg!(feature = "audio") {
            assert!(loudness(&mix[2400..]) > 0.1, "volume 1 is heard: {}", loudness(&mix));
        }
        // Muting the bus mutes the video.
        ctx.audio.mute_bus(cutscene, true);
        let _ = ctx.audio.render_offline(0.1);
        let mix = ctx.audio.render_offline(0.2);
        assert!(loudness(&mix) < 1e-3, "{}", loudness(&mix));
    });
}

#[test]
fn a_video_without_sound_plays_silently() {
    with_ctx(|ctx| {
        let mut video = Video::from_bytes(ctx, SILENT.to_vec()).unwrap();
        assert_eq!(video.sound(), None);
        video.play(ctx);
        assert!(ctx.audio.started().is_empty());
        video.advance(ctx, 0.75).unwrap();
        assert_eq!(frame_number(&video), 7);
    });
}

#[test]
fn freeing_a_video_frees_its_texture_and_sound() {
    with_ctx(|ctx| {
        let before = ctx.live_counts();
        let mut video = clip(ctx);
        video.play(ctx);
        let (texture, sound) = (video.texture(), video.sound().unwrap());
        assert_eq!(ctx.live_counts().textures, before.textures + 1);
        assert_eq!(ctx.live_counts().sounds, before.sounds + 1);
        video.free(ctx);
        assert!(!ctx.gfx.is_live(texture) && !ctx.audio.is_live(sound));
        assert_eq!(ctx.live_counts(), before);
    });
}

#[test]
fn other_codecs_are_refused_with_how_to_encode_instead() {
    with_ctx(|ctx| {
        let opus = Video::from_bytes(ctx, OPUS.to_vec()).unwrap_err();
        assert_eq!(opus.kind(), ErrorKind::Unsupported);
        assert!(
            opus.message().contains("A_OPUS") && opus.message().contains("libvorbis"),
            "{opus}"
        );
        let vp9 = Video::from_bytes(ctx, VP9.to_vec()).unwrap_err();
        assert_eq!(vp9.kind(), ErrorKind::Unsupported);
        assert!(
            vp9.message().contains("V_VP9") && vp9.message().contains("libsvtav1"),
            "{vp9}"
        );
        // Nothing is left behind by a refused file.
        assert_eq!(ctx.live_counts(), LiveCountsZero::get());
    });
}

/// What a fresh game holds.
struct LiveCountsZero;
impl LiveCountsZero {
    fn get() -> ponko::handle::LiveCounts {
        let mut counts = None;
        Game::new("empty", 1, 1).run_headless(0, |ctx| counts = Some(ctx.live_counts()));
        counts.unwrap()
    }
}

#[test]
fn damaged_files_are_errors_not_panics() {
    with_ctx(|ctx| {
        let garbage = Video::from_bytes(ctx, vec![0x1a, 0x45, 0xdf, 0xa3, 1, 2, 3]).unwrap_err();
        assert_eq!(garbage.kind(), ErrorKind::Decode);
        assert_eq!(
            Video::from_bytes(ctx, Vec::new()).unwrap_err().kind(),
            ErrorKind::Decode
        );
        // Every cut of the file either loads (and plays to its end) or is an error.
        for len in (0..CLIP.len()).step_by(997) {
            if let Ok(mut video) = Video::from_bytes(ctx, CLIP[..len].to_vec()) {
                video.play(ctx);
                for _ in 0..25 {
                    let _ = video.advance(ctx, 0.1);
                }
                video.free(ctx);
            }
        }
        // Flipped bytes in the middle of the picture data.
        let mut flipped = CLIP.to_vec();
        for b in &mut flipped[CLIP.len() / 2..CLIP.len() / 2 + 64] {
            *b ^= 0x5a;
        }
        if let Ok(mut video) = Video::from_bytes(ctx, flipped) {
            video.play(ctx);
            for _ in 0..25 {
                let _ = video.advance(ctx, 0.1);
            }
        }
    });
}
