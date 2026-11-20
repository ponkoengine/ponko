//! Plays a WebM video (AV1 pictures, Vorbis sound) into a texture, drawn to
//! fit the window, with its sound on a "cutscene" bus. Build only in CI;
//! running it opens a window and plays sound:
//!
//! ```sh
//! cargo run --example video_player --features video -- intro.webm
//! ```
//!
//! Space pauses and plays, Left and Right seek 5 seconds, L loops, Escape
//! quits. Without the `video` feature it only says how to run it. The
//! guide's "Video" chapter says how to encode a file.
use ponko::prelude::*;

#[cfg(feature = "video")]
fn player(ctx: &mut Ctx) -> Option<ponko::video::Video> {
    let path = std::env::args().nth(1)?;
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut video = ponko::video::Video::from_bytes(ctx, bytes).unwrap_or_else(|e| panic!("{path}: {e}"));
    let cutscene = ctx.audio.bus("cutscene");
    video.set_bus(ctx, cutscene);
    video.play(ctx);
    Some(video)
}

#[cfg(feature = "video")]
fn frame(ctx: &mut Ctx, video: &mut ponko::video::Video) {
    if ctx.input.key_pressed(Key::Space) {
        if video.playing() {
            video.pause(ctx);
        } else {
            video.play(ctx);
        }
    }
    for (key, by) in [(Key::Left, -5.0), (Key::Right, 5.0)] {
        if ctx.input.key_pressed(key) {
            let to = video.time() + by;
            if let Err(e) = video.seek(ctx, to) {
                eprintln!("{e}");
            }
        }
    }
    if ctx.input.key_pressed(Key::L) {
        video.set_looping(!video.looping());
    }
    if let Err(e) = video.update(ctx) {
        eprintln!("{e}");
    }
    // As large as fits the window, centered.
    let window = ctx.window_size();
    let size = video.size().as_vec2();
    let scale = (window.x / size.x).min(window.y / size.y);
    ctx.gfx.draw_ex(
        video.texture(),
        window / 2.0,
        DrawParams::default()
            .with_space(DrawSpace::Screen)
            .with_scale(Vec2::splat(scale)),
    );
    let state = if video.finished() {
        "finished"
    } else if video.playing() {
        "playing"
    } else {
        "paused"
    };
    let looping = if video.looping() { ", looping" } else { "" };
    let status = format!("{:.1} / {:.1} s, {state}{looping}", video.time(), video.seconds());
    ctx.gfx.draw_text(&status, vec2(10.0, 10.0), 18.0, Color::WHITE);
}

fn main() {
    Game::new("Video player", 960, 540)
        .app_id("dev.ponko.video-player")
        .run(|ctx| {
            #[cfg(feature = "video")]
            let mut video = player(ctx);
            #[cfg(not(feature = "video"))]
            let _ = ctx;
            move |ctx: &mut Ctx| {
                if ctx.input.key_pressed(Key::Escape) {
                    ctx.quit();
                }
                #[cfg(feature = "video")]
                if let Some(video) = &mut video {
                    frame(ctx, video);
                    return;
                }
                let help = if cfg!(feature = "video") {
                    "Pass a WebM file: cargo run --example video_player --features video -- intro.webm"
                } else {
                    "Build with --features video to play a WebM file"
                };
                ctx.gfx.draw_text(help, vec2(10.0, 10.0), 18.0, Color::WHITE);
            }
        });
}
