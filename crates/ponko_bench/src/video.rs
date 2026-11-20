//! Video playback (`video/frame`, feature `video`): the clip named by
//! `PONKO_BENCH_VIDEO` (a WebM file with AV1 pictures; the benchmark is
//! skipped without it) played headless in steps of 1/240 s. Each step that
//! shows a new frame is timed: demuxing, decoding the frame with rav1d and
//! converting it to RGBA, as `Video::update` does in a game.
use std::time::{Duration, Instant};

use ponko::prelude::*;
use ponko::video::Video;

use crate::{Bench, report};

const WARMUP: usize = 10;
const STEP: f32 = 1.0 / 240.0;

struct Times(Vec<Duration>, UVec2);

pub fn run(b: &mut Bench) {
    let name = "video/frame";
    if b.skips(name) {
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
        let mut shown = video.frame_time();
        let mut frame = 0;
        while !video.finished() {
            let start = Instant::now();
            video.advance(ctx, STEP).expect("every frame decodes");
            let took = start.elapsed();
            if video.frame_time() != shown {
                shown = video.frame_time();
                frame += 1;
                if frame > WARMUP {
                    times.push(took);
                }
            }
        }
        ctx.world.insert_resource(Times(times, video.size()));
    });
    let Times(mut times, size) = world.remove_resource::<Times>().expect("the times");
    eprintln!("video/frame: {} x {}, {} frames timed", size.x, size.y, times.len());
    b.report(report::Record::new(name, report::Kind::Steady, WARMUP, &mut times));
}
