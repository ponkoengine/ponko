//! The stereo prototype of the VR and XR evaluation: the 3D reference
//! game's level (`examples/shard_grove`, its `grove.glb` under the game's
//! sun) drawn for two eyes, each into its own half of an offscreen frame,
//! through the engine's window views (`ctx.scene3d.views`). The left eye is
//! the main camera on the left half, the right eye one view on the right
//! half: the side-by-side layout a headset runtime accepts as one image
//! with a rectangle per eye. No window, headset or runtime is needed.
//!
//! What must hold: each eye sees the scene from its own position, so a
//! marker in front of the head lands where the eyes' separation puts it
//! (the negative control is the same frame with no separation: no
//! disparity), and both eyes read one set of sun shadow cascades, fitted
//! around both, while each culls its own models.
//!
//! The benchmark (ignored; run in release) times a stereo frame against
//! one view at the same eye size and against one view as wide as both
//! eyes. Its results are in `book/src/xr.md`:
//! `cargo test --release --lib --features 3d stereo_frame_cost -- --ignored --nocapture`
use super::render_test_support::{Image, gpu_with, keep_image};
use crate::camera::Viewport;
use crate::prelude::*;
use crate::render::{MaterialDesc, MeshData, Model};

/// The reference game's level, as it loads it.
const LEVEL: &str = "shard_grove/grove.glb";
/// The head: standing (1.7 m) beside the game's first shrine.
const HEAD: Vec3 = Vec3::new(-16.0, 1.7, 7.0);
/// What the head looks at: the middle of the grove.
const LOOK_AT: Vec3 = Vec3::new(0.0, 1.0, 0.0);
/// A typical distance between the eyes, in meters.
const IPD: f32 = 0.063;
/// A headset's vertical field of view, about.
const FOV_Y_DEGREES: f32 = 90.0;

/// An offscreen game drawing the grove for one eye or two.
struct Rig {
    ctx: crate::ctx::Ctx,
    device: wgpu::Device,
    stereo: bool,
}

impl Rig {
    /// A frame `eye` pixels per eye: twice as wide with `stereo`, else one
    /// view (`wide`: one view as wide as two eyes). `timestamps` asks for
    /// GPU pass timing where the adapter has it. `None` without a GPU.
    fn new(eye: (u32, u32), stereo: bool, wide: bool, timestamps: bool) -> Option<Self> {
        let gpu = gpu_with(|adapter| {
            let mut descriptor = wgpu::DeviceDescriptor::default();
            if timestamps {
                descriptor.required_features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
            }
            Ok(descriptor)
        })?;
        let device = gpu.device.clone();
        let size = if stereo || wide { (eye.0 * 2, eye.1) } else { eye };
        let mut ctx = crate::ctx::Ctx::new(1.0 / 60.0, vec2(size.0 as f32, size.1 as f32));
        ctx.audio.headless = true;
        ctx.gfx.renderer = Some(super::gpu::Renderer::offscreen(gpu, size));
        // The game's sun (`examples/shard_grove/game.rs`).
        let lighting = &mut ctx.scene3d.lighting;
        lighting.sun_direction = vec3(-0.45, -0.7, -0.35);
        lighting.sun_color = Color::rgb(1.0, 0.88, 0.72);
        lighting.ambient_intensity = 0.35;
        lighting.shadows = true;
        let level = ctx.gfx.load_mesh(LEVEL);
        ctx.world.spawn((crate::d3::Transform::default(), Model::new(level)));
        let mut rig = Self { ctx, device, stereo };
        rig.place_head(HEAD, IPD);
        Some(rig)
    }

    /// Puts the head at `head`, looking at the grove, with the eyes `ipd`
    /// apart along its right axis (stereo), or the one camera at the head.
    fn place_head(&mut self, head: Vec3, ipd: f32) {
        let mut camera = self.ctx.scene3d.camera;
        camera.position = head;
        camera.look_at(LOOK_AT);
        camera.fov_y = FOV_Y_DEGREES.to_radians();
        camera.rect = Viewport::FULL;
        self.ctx.scene3d.views.clear();
        if !self.stereo {
            self.ctx.scene3d.camera = camera;
            return;
        }
        let right = camera.rotation * Vec3::X;
        let mut left_eye = camera;
        left_eye.position = head - right * (ipd / 2.0);
        left_eye.rect = Viewport::split(2, 0);
        let mut right_eye = camera;
        right_eye.position = head + right * (ipd / 2.0);
        right_eye.rect = Viewport::split(2, 1);
        self.ctx.scene3d.camera = left_eye;
        self.ctx.scene3d.views.push(crate::d3::View::new(right_eye));
    }

    /// Draws a frame and waits for the GPU to finish it.
    fn frame(&mut self) {
        assert_eq!(self.ctx.render_frame(), crate::render::FrameStatus::Offscreen);
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// Draws a frame and reads it back, as RGBA8.
    fn capture(&mut self) -> Image {
        let path = std::env::temp_dir().join(format!(
            "ponko-stereo-{}-{:?}.png",
            std::process::id(),
            std::thread::current().id()
        ));
        self.ctx.gfx.queue_screenshot(path.clone(), 0, None);
        assert_eq!(self.ctx.render_frame(), crate::render::FrameStatus::Offscreen);
        let shot = image::open(&path).expect("the frame was captured").to_rgba8();
        let _ = std::fs::remove_file(&path);
        let image = Image {
            width: shot.width(),
            height: shot.height(),
            rgba: shot.into_raw(),
        };
        keep_image(&image);
        image
    }
}

/// The mean column of the clearly red pixels of columns `x0..x0 + width`
/// of `image`, relative to `x0`; `None` when there are none.
fn red_column(image: &Image, x0: u32, width: u32) -> Option<f32> {
    let (mut sum, mut count) = (0.0f64, 0u32);
    for y in 0..image.height {
        for x in x0..x0 + width {
            let i = ((y * image.width + x) * 4) as usize;
            let p = &image.rgba[i..i + 3];
            if p[0] > 150 && p[1] < 70 && p[2] < 70 {
                sum += f64::from(x - x0);
                count += 1;
            }
        }
    }
    (count > 0).then(|| (sum / f64::from(count)) as f32)
}

/// Eye size of the tests.
const EYE: (u32, u32) = (192, 192);
/// The marker's distance in front of the head.
const MARKER_DISTANCE: f32 = 1.5;

/// The grove in stereo with a red marker `MARKER_DISTANCE` in front of the
/// head, eyes `ipd` apart: the marker's column in the left and the right
/// eye's half.
fn marker_columns(ipd: f32) -> Option<(f32, f32)> {
    let mut rig = Rig::new(EYE, true, false, false)?;
    rig.place_head(HEAD, ipd);
    let red = MaterialDesc::color(Color::BLACK).with_emissive(Color::rgb(4.0, 0.0, 0.0));
    let red = rig.ctx.gfx.create_material(red);
    let marker = rig.ctx.gfx.create_mesh(&MeshData::cube(0.12), red);
    let forward = (LOOK_AT - HEAD).normalize();
    rig.ctx.world.spawn((
        crate::d3::Transform::at(HEAD + forward * MARKER_DISTANCE),
        Model::new(marker),
    ));
    // The meshes reach the GPU with the first frame.
    rig.frame();
    let image = rig.capture();
    let left = red_column(&image, 0, EYE.0).expect("the left eye sees the marker");
    let right = red_column(&image, EYE.0, EYE.0).expect("the right eye sees the marker");
    Some((left, right))
}

#[test]
fn each_eye_sees_a_near_marker_shifted_by_the_eyes_separation() {
    let Some((left, right)) = marker_columns(IPD) else {
        return;
    };
    // A point straight ahead at distance d lands f * (ipd / 2) / d pixels
    // inside each eye's centre: right of it for the left eye.
    let focal = EYE.1 as f32 / 2.0 / (FOV_Y_DEGREES.to_radians() / 2.0).tan();
    let expected = focal * IPD / MARKER_DISTANCE;
    let disparity = left - right;
    assert!(
        (disparity - expected).abs() < expected * 0.35,
        "the marker is {disparity:.2} px further right in the left eye than in the right, expected {expected:.2}"
    );
    // Negative control: the eyes at one point see it at one place.
    let Some((left, right)) = marker_columns(0.0) else {
        return;
    };
    assert!(
        (left - right).abs() < 0.3,
        "with no separation both eyes see the marker at one column ({left:.2} and {right:.2})"
    );
}

#[test]
fn both_eyes_read_one_set_of_shadow_cascades_fitted_around_both() {
    let Some(mut rig) = Rig::new(EYE, true, false, false) else {
        return;
    };
    rig.frame();
    rig.frame();
    let main = &rig.ctx.gfx.frame3d;
    assert!(main.cascades > 0, "the sun casts shadows");
    let right_eye = rig.ctx.scene3d.views[0].camera;
    assert_eq!(
        main.shadow_views.len(),
        1,
        "the right eye is a shadow view of the main frame"
    );
    assert_eq!(main.shadow_views[0].position, right_eye.position);
    let eye = &main.window_views[0];
    assert_eq!(
        eye.sun_view_proj, main.sun_view_proj,
        "the right eye reads the left's cascades"
    );
    assert_eq!(eye.cascades, main.cascades);
    // Each eye culls for itself: both draw the level.
    assert!(main.visible > 0 && eye.visible > 0, "both eyes draw the level");
    // Negative control: one view lists no other shadow view.
    let Some(mut mono) = Rig::new(EYE, false, false, false) else {
        return;
    };
    mono.frame();
    assert!(mono.ctx.gfx.frame3d.shadow_views.is_empty());
    assert!(mono.ctx.gfx.frame3d.window_views.is_empty());
}

/// Frames drawn before timing, and timed per round.
const WARM_UP: usize = 20;
const FRAMES: usize = 60;
/// Each row is measured this many times; the best medians are kept, and
/// the spread between the rounds' medians is the noise.
const ROUNDS: usize = 3;

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// One row's medians: the CPU's frame (collect, prepare, record, submit),
/// the frame until the GPU is done, and the GPU passes' sum.
#[derive(Clone, Copy, Debug)]
struct Cost {
    cpu: f64,
    to_idle: f64,
    gpu: Option<f64>,
    worst_to_idle: f64,
    /// Passes timed in the last frame (the timer holds at most 32).
    timed: usize,
}

fn measure(eye: (u32, u32), stereo: bool, wide: bool) -> Option<Cost> {
    let mut rig = Rig::new(eye, stereo, wide, true)?;
    rig.ctx.perf.set_gpu_timing(crate::perf::GpuTiming::Every(1));
    let mut best: Option<Cost> = None;
    for _ in 0..ROUNDS {
        for _ in 0..WARM_UP {
            rig.frame();
        }
        let (mut cpu, mut to_idle, mut gpu) = (Vec::new(), Vec::new(), Vec::new());
        let mut timed = 0;
        for i in 0..FRAMES {
            // The head turns a little, as a real one does.
            let yaw = (i as f32 * 0.05).sin() * 0.02;
            let head = HEAD + vec3(yaw, 0.0, 0.0);
            rig.place_head(head, IPD);
            let start = std::time::Instant::now();
            assert_eq!(rig.ctx.render_frame(), crate::render::FrameStatus::Offscreen);
            cpu.push(start.elapsed().as_secs_f64() * 1e3);
            let _ = rig.device.poll(wgpu::PollType::wait_indefinitely());
            to_idle.push(start.elapsed().as_secs_f64() * 1e3);
            let passes = rig.ctx.perf.gpu_passes();
            timed = passes.len();
            if !passes.is_empty() {
                gpu.push(passes.iter().map(|p| f64::from(p.ms)).sum());
            }
        }
        let round = Cost {
            cpu: median(&mut cpu),
            to_idle: median(&mut to_idle),
            gpu: (!gpu.is_empty()).then(|| median(&mut gpu)),
            worst_to_idle: to_idle.iter().copied().fold(0.0, f64::max),
            timed,
        };
        best = Some(match best {
            None => round,
            Some(b) => Cost {
                cpu: b.cpu.min(round.cpu),
                to_idle: b.to_idle.min(round.to_idle),
                gpu: match (b.gpu, round.gpu) {
                    (Some(x), Some(y)) => Some(x.min(y)),
                    (x, y) => x.or(y),
                },
                worst_to_idle: b.worst_to_idle.max(round.worst_to_idle),
                timed: round.timed,
            },
        });
    }
    best
}

#[test]
#[ignore = "offscreen stereo timing; run in release with --ignored --nocapture"]
fn stereo_frame_cost_against_one_view() {
    for eye in [(960, 1080), (1440, 1600)] {
        let Some(mono) = measure(eye, false, false) else { return };
        let Some(wide) = measure(eye, false, true) else { return };
        let Some(stereo) = measure(eye, true, false) else {
            return;
        };
        println!(
            "\neye {}x{} (medians, ms; best of {ROUNDS} rounds of {FRAMES} frames)",
            eye.0, eye.1
        );
        println!("| Frame | CPU | until GPU idle | GPU passes (timed) | worst until idle |");
        println!("|---|---|---|---|---|");
        for (name, c) in [
            ("one view, one eye's size", mono),
            ("one view, both eyes' size", wide),
            ("stereo: two views, one per eye", stereo),
        ] {
            let gpu = c
                .gpu
                .map_or("not timed".to_owned(), |g| format!("{g:.2} ({})", c.timed));
            println!(
                "| {name} | {:.2} | {:.2} | {gpu} | {:.2} |",
                c.cpu, c.to_idle, c.worst_to_idle
            );
        }
        println!(
            "stereo / one eye: CPU x{:.2}, until idle x{:.2}",
            stereo.cpu / mono.cpu,
            stereo.to_idle / mono.to_idle
        );
    }
}
