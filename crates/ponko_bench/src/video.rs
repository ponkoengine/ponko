//! Video playback (`video/frame`, `video/convert`, feature `video`): the
//! clip named by `PONKO_BENCH_VIDEO` (a WebM file with AV1 pictures; the
//! benchmark is skipped without it) played headless in steps of 1/240 s.
//! Each step that shows a new frame is timed: demuxing and decoding the
//! frame with rav1d, as `Video::update` does in a game (`video/frame`; with
//! a GPU, the renderer then converts it to RGBA on the GPU). Then
//! `video/convert`: converting that frame to RGBA on the CPU, what
//! `Video::frame_rgba` does when asked (and what every frame cost before
//! the conversion moved to the GPU).
use std::time::{Duration, Instant};

use ponko::prelude::*;
use ponko::video::Video;

use crate::{Bench, report};

const WARMUP: usize = 10;
const STEP: f32 = 1.0 / 240.0;

struct Times(Vec<Duration>, Vec<Duration>, UVec2);

pub fn run(b: &mut Bench) {
    let name = "video/frame";
    let convert = "video/convert";
    if b.skips(name) && b.skips(convert) {
        return;
    }
    let Ok(path) = std::env::var("PONKO_BENCH_VIDEO") else {
        eprintln!("video/frame: skipped (set PONKO_BENCH_VIDEO to a WebM file with AV1 pictures)");
        return;
    };
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("PONKO_BENCH_VIDEO={path}: {e}"));
    let mut bytes = Some(bytes);
    let mut world = Game::new("video bench", 1, 1).run_headless(0, move |ctx| {
        let Some(bytes) = bytes.take() else { return };
        let mut video = Video::from_bytes(ctx, bytes).expect("a video ponko plays");
        video.play(ctx);
        let mut times = Vec::new();
        let mut conversions = Vec::new();
        let mut shown = video.frame_time();
        let mut frame = 0;
        while !video.finished() {
            let start = Instant::now();
            video.advance(ctx, STEP).expect("every frame decodes");
            let took = start.elapsed();
            if video.frame_time() != shown {
                shown = video.frame_time();
                frame += 1;
                let start = Instant::now();
                std::hint::black_box(video.frame_rgba());
                let converted = start.elapsed();
                if frame > WARMUP {
                    times.push(took);
                    conversions.push(converted);
                }
            }
        }
        ctx.world.insert_resource(Times(times, conversions, video.size()));
    });
    let Times(mut times, mut conversions, size) = world.remove_resource::<Times>().expect("the times");
    eprintln!("video/frame: {} x {}, {} frames timed", size.x, size.y, times.len());
    if !b.skips(name) {
        b.report(report::Record::new(name, report::Kind::Steady, WARMUP, &mut times));
    }
    if !b.skips(convert) {
        b.report(report::Record::new(
            convert,
            report::Kind::Steady,
            WARMUP,
            &mut conversions,
        ));
    }
}
