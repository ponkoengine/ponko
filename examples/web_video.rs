//! Video in the browser: the fixture the runner `web/smoke/video.mjs` reads
//! (the guide's "Video", "In a browser"). Build it with
//! `web/build.sh --features video web_video`.
//!
//! It plays the engine's test clip (`tests/fixtures/video/clip.webm`: 64 x
//! 64, 10 frames a second, 2 seconds; frame `k` has red, green and blue
//! quarters and a gray quarter at level `20 + 10 k`), decoded by the
//! browser's own AV1 decoder behind `ponko::video::Video`, and checks every
//! frame it shows: that the pixels are the frame `frame_time` names, that
//! the quarters are in their places, and how many frames the picture is
//! behind the clock (`lag`; the browser hands frames back later). Then,
//! paused, it seeks to 1.3 s and back to 0.4 s and counts the updates until
//! each frame shows. It publishes, with the usual smoke state:
//!
//! `{"fixture":"web_video","video":{"phase":"done","shown":[0,1,...],
//! "max_lag":1,"wrong_frame":0,"wrong_quarters":0,"seek_13":3,
//! "seek_4":2,"finished":true,"error":null}}`
//!
//! On the desktop it opens a window like any other example and prints the
//! same state.

use ponko::prelude::*;
use ponko::video::Video;
use serde_json::json;

#[path = "common/web_smoke.rs"]
mod smoke;

const CLIP: &[u8] = include_bytes!("../tests/fixtures/video/clip.webm");

/// Updates a seek may take to show its frame before it counts as never.
const SEEK_UPDATES: u32 = 600;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Playing,
    /// Paused, waiting for frame 13 after seeking to 1.3 s.
    SeekForward,
    /// Then frame 4 after seeking back to 0.4 s.
    SeekBack,
    Done,
}

struct Fixture {
    smoke: smoke::Smoke,
    video: Option<Video>,
    phase: Phase,
    /// Every frame shown while playing, in order (no repeats).
    shown: Vec<i32>,
    max_lag: i32,
    wrong_frame: u32,
    wrong_quarters: u32,
    updates: u32,
    seek_13: Option<u32>,
    seek_4: Option<u32>,
    finished: bool,
    error: Option<String>,
}

/// The pixel at `(x, y)` of the frame shown.
fn pixel(video: &Video, x: usize, y: usize) -> [u8; 3] {
    let i = (y * 64 + x) * 4;
    let p = &video.frame_rgba()[i..i + 3];
    [p[0], p[1], p[2]]
}

/// Which frame is shown, read from the gray quarter's level.
fn frame_number(video: &Video) -> i32 {
    (i32::from(pixel(video, 48, 48)[0]) - 20 + 5).div_euclid(10)
}

/// Red, green and blue where the clip has them (AV1 is lossy: near).
fn quarters_ok(video: &Video) -> bool {
    let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(&x, y)| x.abs_diff(y) <= 24);
    near(pixel(video, 16, 16), [255, 0, 0])
        && near(pixel(video, 48, 16), [0, 255, 0])
        && near(pixel(video, 16, 48), [0, 0, 255])
}

impl Fixture {
    fn step(&mut self, ctx: &mut Ctx) {
        let Some(video) = self.video.as_mut() else {
            return;
        };
        if let Err(e) = video.update(ctx) {
            self.error = Some(e.to_string());
            self.phase = Phase::Done;
            return;
        }
        self.updates += 1;
        // Nothing decoded yet: still the black it starts with.
        let decoded = pixel(video, 16, 16)[0] > 128;
        match self.phase {
            Phase::Playing => {
                if decoded {
                    let number = frame_number(video);
                    if (number as f32 - video.frame_time() * 10.0).abs() > 0.01 {
                        self.wrong_frame += 1;
                    }
                    if !quarters_ok(video) {
                        self.wrong_quarters += 1;
                    }
                    if self.shown.last() != Some(&number) {
                        self.shown.push(number);
                    }
                    let due = ((video.time() * 10.0) + 1e-3).floor() as i32;
                    self.max_lag = self.max_lag.max(due.min(19) - number);
                }
                if video.finished() {
                    self.finished = true;
                    self.phase = Phase::SeekForward;
                    self.updates = 0;
                    if let Err(e) = video.seek(ctx, 1.3) {
                        self.error = Some(e.to_string());
                        self.phase = Phase::Done;
                    }
                }
            }
            Phase::SeekForward | Phase::SeekBack => {
                let (want, at) = if self.phase == Phase::SeekForward {
                    (13, &mut self.seek_13)
                } else {
                    (4, &mut self.seek_4)
                };
                if frame_number(video) == want && quarters_ok(video) {
                    *at = Some(self.updates);
                    self.updates = 0;
                    if self.phase == Phase::SeekForward {
                        self.phase = Phase::SeekBack;
                        if let Err(e) = video.seek(ctx, 0.4) {
                            self.error = Some(e.to_string());
                            self.phase = Phase::Done;
                        }
                    } else {
                        self.phase = Phase::Done;
                    }
                } else if self.updates > SEEK_UPDATES {
                    self.phase = Phase::Done;
                }
            }
            Phase::Done => {}
        }
        let center = video.size().as_vec2();
        ctx.gfx.draw_ex(
            video.texture(),
            center,
            DrawParams::default()
                .with_space(DrawSpace::Screen)
                .with_scale(Vec2::splat(2.0)),
        );
    }

    fn state(&self) -> serde_json::Value {
        let phase = match self.phase {
            Phase::Playing => "playing",
            Phase::SeekForward => "seek_forward",
            Phase::SeekBack => "seek_back",
            Phase::Done => "done",
        };
        json!({ "video": {
            "phase": phase,
            "shown": self.shown,
            "max_lag": self.max_lag,
            "wrong_frame": self.wrong_frame,
            "wrong_quarters": self.wrong_quarters,
            "seek_13": self.seek_13,
            "seek_4": self.seek_4,
            "finished": self.finished,
            "error": self.error,
        }})
    }
}

fn main() {
    Game::new("web_video", 256, 256)
        .setup(|ctx| {
            let smoke = smoke::Smoke::new("web_video");
            let (video, error) = match Video::from_bytes(ctx, CLIP.to_vec()) {
                Ok(mut video) => {
                    video.play(ctx);
                    (Some(video), None)
                }
                Err(e) => (None, Some(e.to_string())),
            };
            ctx.world.insert_resource(Fixture {
                smoke,
                phase: if video.is_some() { Phase::Playing } else { Phase::Done },
                video,
                shown: Vec::new(),
                max_lag: 0,
                wrong_frame: 0,
                wrong_quarters: 0,
                updates: 0,
                seek_13: None,
                seek_4: None,
                finished: false,
                error,
            });
        })
        .system(|ctx: &mut Ctx| {
            let Some(mut fixture) = ctx.world.remove_resource::<Fixture>() else {
                return;
            };
            ctx.gfx.clear(Color::BLACK);
            fixture.step(ctx);
            let state = fixture.state();
            let ticks = ctx.time.sim_tick();
            fixture.smoke.frame(ctx, ticks, state);
            ctx.world.insert_resource(fixture);
        })
        .run();
}
