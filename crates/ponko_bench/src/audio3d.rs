//! Positional sound at scale, without a device (`audio3d/...`): 1,000
//! looping sources spread over a 200 m square, a listener walking through
//! them, the mix rendered offline at 48 kHz.
//!
//! - `audio3d/1000-frame`: the game side of one frame: every source moved
//!   to the listener, then the voice ranking (which are heard).
//! - `audio3d/1000-mix`: mixing that frame's 1/60 s of sound, the audio
//!   thread's work; it has to stay well under 16.7 ms.
//!
//! Each runs with the default limit of 64 voices heard (`-64`) and the
//! highest, 256 (`-256`).
use std::time::{Duration, Instant};

use ponko::d3::{AudioListener, AudioSettings, AudioSource, Transform};
use ponko::prelude::*;

use crate::{Bench, report};

const SOURCES: usize = 1_000;
const WARMUP: usize = 30;
const FRAMES: usize = 300;
const DT: f32 = 1.0 / 60.0;
const RATE: u32 = 48_000;

/// Every timed frame's game side and mix, and the voices heard.
struct Times(Vec<Duration>, Vec<Duration>, u32, u32);

pub fn run(b: &mut Bench) {
    for limit in [64u32, 256] {
        let (frame_name, mix_name) = (
            format!("audio3d/1000-frame-{limit}"),
            format!("audio3d/1000-mix-{limit}"),
        );
        if b.skips(&frame_name) && b.skips(&mix_name) {
            continue;
        }
        let mut world = Game::new("audio bench", 1, 1).run_headless(0, move |ctx| {
            ctx.audio.open_offline(RATE);
            ctx.audio.set_voice_limit(limit);
            // Eight different hums, a second each.
            let hums: Vec<Sound> = (0..8)
                .map(|k| {
                    let hz = 110.0 * (k + 1) as f32;
                    let samples: Vec<f32> = (0..RATE)
                        .map(|i| (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin() * 0.05)
                        .collect();
                    ctx.audio.create(RATE, &samples)
                })
                .collect();
            let mut seed = 7u64;
            let mut next = || {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                (seed >> 40) as f32 / (1u64 << 24) as f32
            };
            for i in 0..SOURCES {
                let at = vec3(next() * 200.0 - 100.0, 0.0, next() * 200.0 - 100.0);
                let settings = AudioSettings {
                    sound: SoundParams {
                        looping: true,
                        ..Default::default()
                    },
                    max_distance: 30.0,
                    ..Default::default()
                };
                ctx.world
                    .spawn((Transform::at(at), AudioSource::new(hums[i % hums.len()], settings)));
            }
            let (mut frames, mut mixes) = (Vec::with_capacity(FRAMES), Vec::with_capacity(FRAMES));
            for frame in 0..WARMUP + FRAMES {
                // Walking across the field at 5 m/s.
                let t = frame as f32 * DT;
                ctx.audio.set_listener(Some(AudioListener {
                    position: vec3(-60.0 + t * 5.0, 1.7, 10.0),
                    rotation: Quat::IDENTITY,
                }));
                let start = Instant::now();
                ctx.bench_audio_frame(DT);
                let game = start.elapsed();
                let start = Instant::now();
                std::hint::black_box(ctx.audio.render_offline(DT));
                let mix = start.elapsed();
                if frame >= WARMUP {
                    frames.push(game);
                    mixes.push(mix);
                }
            }
            let count = ctx.audio.voice_count();
            ctx.world
                .insert_resource(Times(frames, mixes, count.real, count.virtualized));
        });
        let Times(mut frames, mut mixes, real, virtualized) = world.remove_resource::<Times>().expect("the times");
        assert_eq!(real + virtualized, SOURCES as u32, "every source plays");
        assert!(real <= limit, "{real} heard over the limit of {limit}");
        eprintln!("audio3d (limit {limit}): {real} heard, {virtualized} virtual");
        if !b.skips(&frame_name) {
            b.report(report::Record::new(
                &frame_name,
                report::Kind::Steady,
                WARMUP,
                &mut frames,
            ));
        }
        if !b.skips(&mix_name) {
            b.report(report::Record::new(&mix_name, report::Kind::Steady, WARMUP, &mut mixes));
        }
    }
}
