//! Frame times with pipelined rendering on (the tuned builds).
//!
//! With Bevy's `PipelinedRenderingPlugin` (on in `DefaultPlugins` with
//! `multi_threaded`), `App::update` runs the main world's schedules and hands
//! the frame to the render thread, which draws it while the main world runs
//! the next update. A frame's end is therefore on the render thread: a
//! system at the end of the `Render` schedule waits until the GPU has
//! finished the frame (`PollType::Wait`, as the other rows do) and notes the
//! time. A frame is the time from one finished frame to the next, in steady
//! state, so it is the longer of the two overlapped halves, never less than
//! the GPU's own time.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::{
    prelude::*,
    render::{Render, RenderApp, RenderSystems, render_resource::PollType, renderer::RenderDevice},
};

/// When each frame was finished on the GPU, written by the render thread.
#[derive(Resource, Clone, Default)]
pub struct Finished(Arc<Mutex<Vec<Instant>>>);

fn finish_frame(device: Res<RenderDevice>, finished: Res<Finished>) {
    device
        .wgpu_device()
        .poll(PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("the GPU finishes the frame");
    finished.0.lock().expect("the frame stamps").push(Instant::now());
}

impl Finished {
    /// Adds the stamping system to the render world. Call it after the
    /// plugins are added and before `App::finish` and `App::cleanup` (the
    /// pipelined plugin moves the render world to its thread in `cleanup`).
    pub fn install(app: &mut App) -> Finished {
        let finished = Finished::default();
        app.sub_app_mut(RenderApp)
            .insert_resource(finished.clone())
            .add_systems(Render, finish_frame.in_set(RenderSystems::Cleanup));
        finished
    }

    pub fn count(&self) -> usize {
        self.0.lock().expect("the frame stamps").len()
    }

    /// Waits (at most a minute) until `n` frames are finished.
    pub fn wait_for(&self, n: usize) {
        let start = Instant::now();
        while self.count() < n {
            assert!(start.elapsed() < Duration::from_secs(60), "the render thread stopped");
            std::thread::sleep(Duration::from_micros(50));
        }
    }

    /// The times between consecutive finished frames after `warmup`, in ms.
    pub fn times(&self, warmup: usize) -> Vec<f64> {
        let stamps = self.0.lock().expect("the frame stamps");
        stamps
            .windows(2)
            .skip(warmup)
            .map(|w| (w[1] - w[0]).as_secs_f64() * 1e3)
            .collect()
    }
}
