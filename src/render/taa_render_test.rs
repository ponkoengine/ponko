//! Headless GPU tests of temporal antialiasing: converging edges, motion
//! vectors, ghosting, camera cuts and the fallback on devices without
//! compute. They skip without a GPU adapter.
use super::gpu::GpuTexture;
use super::gpu3d::*;
use super::render_test_support::*;
#[allow(unused_imports)]
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{AmbientOcclusion, AntiAliasing, GpuFeature, MaterialDesc, Mesh, MeshData, Model, Tonemap};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};

/// A renderer drawing a light box (or two) on black, read back as pixels.
struct Rig {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Box<Renderer3D>,
    textures: Vec<GpuTexture>,
    output: wgpu::Texture,
    ctx: Box<Ctx>,
    frame: Frame3D,
    cube: Mesh,
    size: u32,
    timer: Option<super::timer::GpuTimer>,
    disable_motion: bool,
    /// The adapter it draws on.
    info: wgpu::AdapterInfo,
}

impl Rig {
    fn new(size: u32, downlevel: bool) -> Option<Self> {
        let TestGpu {
            device, queue, info, ..
        } = gpu_with(|adapter| {
            let mut descriptor = wgpu::DeviceDescriptor {
                required_features: adapter.features()
                    & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES),
                ..Default::default()
            };
            if downlevel {
                descriptor.required_limits.max_compute_workgroups_per_dimension = 0;
                descriptor.required_limits.max_storage_textures_per_shader_stage = 0;
            }
            Ok(descriptor)
        })?;
        let (layout, white, textures) = white_textures(&device, &queue);
        let output = target(&device, (size, size), wgpu::TextureFormat::Rgba8UnormSrgb);
        // A box that shines white and lights nothing, on black.
        // Boxed: a test that holds two rigs by value comes close to a test
        // thread's 2 MiB stack.
        let mut ctx = Box::new(Ctx::new(1.0 / 60.0, Vec2::new(size as f32, size as f32)));
        let glow = MaterialDesc {
            color: Color::BLACK,
            roughness: 1.0,
            emissive: Color::WHITE,
            ..Default::default()
        };
        let material = ctx.gfx.create_material(glow);
        let cube_data = MeshData::cube(1.0);
        let cube = ctx.gfx.create_mesh(&cube_data, material);
        let mut renderer = Box::new(Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true));
        renderer.create_material(&device, &GpuMaterialDesc::new(&glow), &textures);
        renderer.create_mesh(
            &device,
            &cube_data,
            &[(0..cube_data.indices.len() as u32, material)],
            true,
        );
        ctx.scene3d.camera.position = vec3(0.0, 0.0, 5.0);
        ctx.scene3d.camera.look_at(Vec3::ZERO);
        ctx.scene3d.lighting.shadows = false;
        ctx.scene3d.lighting.sun_intensity = 0.0;
        ctx.scene3d.lighting.ambient_intensity = 0.0;
        ctx.scene3d.sky.atmosphere = false;
        ctx.scene3d.post.bloom = 0.0;
        ctx.scene3d.post.tonemap = Tonemap::None;
        Some(Self {
            device,
            queue,
            renderer,
            textures,
            output,
            ctx,
            frame: Frame3D::default(),
            cube,
            size,
            timer: None,
            disable_motion: false,
            info,
        })
    }

    /// Draws later frames at `size` x `size` (a window resize). Square
    /// either way: the camera's aspect ratio stays.
    fn resize(&mut self, size: u32) {
        self.output = target(&self.device, (size, size), self.output.format());
        self.size = size;
        self.ctx.scene3d.camera.set_viewport(Vec2::splat(size as f32));
    }

    /// A slab turned `angle` about the view axis: a long diagonal edge.
    fn slab(&mut self, angle: f32) {
        self.ctx.world.clear();
        self.ctx.world.spawn((
            Transform3D {
                rotation: Quat::from_rotation_z(angle),
                scale: vec3(4.0, 1.5, 0.05),
                ..Default::default()
            },
            Model::new(self.cube),
        ));
    }

    /// Draws a frame into `output`.
    fn render(&mut self) {
        let size = self.size;
        let ctx = &mut self.ctx;
        self.frame.motion.enabled = ctx.scene3d.post.anti_aliasing == AntiAliasing::Taa
            || (ctx.scene3d.post.ao_temporal && ctx.scene3d.post.ambient_occlusion.enabled);
        self.frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &SkinFrame::default(),
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        ctx.gfx.frame3d.views = std::mem::take(&mut self.frame.views);
        ctx.collect_views(&SkinFrame::default());
        self.frame.views = std::mem::take(&mut ctx.gfx.frame3d.views);
        if self.disable_motion {
            self.frame.motion.runs.clear();
        }
        let scene = Scene3D {
            frame: &self.frame,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            light: SkyLight {
                sun: Vec3::ZERO,
                sky: Vec3::ZERO,
                ground: Vec3::ZERO,
            },
            camera: ctx.scene3d.camera.position,
            post: &ctx.scene3d.post,
            time: (0.0, 1.0 / 60.0),
            occlusion: false,
        };
        self.renderer.prepare(
            &self.device,
            &self.queue,
            &Prepare3D {
                scene: &scene,
                lut: None,
                size: (size, size),
            },
        );
        self.frame.motion.reset = false;
        let target = self.output.create_view(&Default::default());
        if let Some(timer) = &mut self.timer {
            timer.collect(&self.device);
            timer.begin_frame();
        }
        let buffers = self.renderer.draw(
            &self.device,
            &target,
            wgpu::Color::BLACK,
            &self.textures,
            self.timer.as_ref(),
        );
        self.queue.submit(buffers);
        if let Some(timer) = &mut self.timer {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            timer.resolve(&mut encoder);
            self.queue.submit([encoder.finish()]);
            timer.submitted();
        }
    }

    /// Draws a frame and returns it as RGBA8 sRGB pixels.
    fn image(&mut self) -> Image {
        self.render();
        Image::read(&self.device, &self.queue, &self.output)
    }

    /// Draws a frame and returns its red channel, linear, row by row.
    fn draw(&mut self) -> Vec<f32> {
        self.render();
        let bytes = self.read(&self.output);
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| {
                // sRGB to linear.
                let c = f32::from(p[0]) / 255.0;
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            })
            .collect()
    }

    fn read(&self, texture: &wgpu::Texture) -> Vec<u8> {
        read_texture(&self.device, &self.queue, texture)
    }
}

/// Mean absolute difference from `reference` over the pixels either image
/// has between black and white (the edge).
fn edge_error(image: &[f32], reference: &[f32]) -> f32 {
    let edge = |v: f32| v > 0.004 && v < 0.996;
    let (mut sum, mut n) = (0.0, 0);
    for (&a, &b) in image.iter().zip(reference) {
        if edge(a) || edge(b) {
            sum += (a - b).abs();
            n += 1;
        }
    }
    assert!(n > 20, "the image has an edge: {n}");
    sum / n as f32
}

/// `image` (n x n) averaged down by `k` in each direction, in linear light.
fn downsample(image: &[f32], n: usize, k: usize) -> Vec<f32> {
    let m = n / k;
    (0..m * m)
        .map(|i| {
            let (x, y) = (i % m, i / m);
            let mut sum = 0.0;
            for dy in 0..k {
                for dx in 0..k {
                    sum += image[(y * k + dy) * n + x * k + dx];
                }
            }
            sum / (k * k) as f32
        })
        .collect()
}

const SIZE: u32 = 64;

#[test]
fn a_still_diagonal_edge_converges_smoother_than_without_it() {
    let Some(mut rig) = Rig::new(SIZE, false) else {
        return;
    };
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::TemporalAntialiasing) {
        return;
    }
    rig.slab(0.37);
    let none = rig.draw();
    // The truth: the same picture at 8 times the size, averaged down.
    let mut big = Rig::new(SIZE * 8, false).unwrap();
    big.slab(0.37);
    let truth = downsample(&big.draw(), (SIZE * 8) as usize, 8);
    drop(big);

    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    let mut frames = Vec::new();
    for _ in 0..40 {
        frames.push(rig.draw());
    }
    assert!(rig.renderer.taa_active());
    let (without, with) = (edge_error(&none, &truth), edge_error(&frames[39], &truth));
    let first = edge_error(&frames[0], &truth);
    eprintln!(
        "edge error against 8x8 supersampling: none {without:.4}, TAA first frame {first:.4}, after 40 frames {with:.4}"
    );
    assert!(with < without * 0.75, "TAA converges: {with} vs {without}");
    let levels = |image: &[f32]| {
        let mut v: Vec<u32> = image
            .iter()
            .filter(|&&v| v > 0.004 && v < 0.996)
            .map(|&v| (v * 255.0) as u32)
            .collect();
        v.sort_unstable();
        v.dedup();
        v.len()
    };
    assert!(levels(&frames[39]) > levels(&none), "more edge shades");
    // Still means still: the converged picture stays put frame to frame.
    let wobble = edge_error(&frames[39], &frames[38]);
    assert!(wobble < 0.05, "a settled edge does not shimmer: {wobble}");
}

/// Reads the motion vector at pixel (x, y): xy over alpha.
fn motion_at(rig: &Rig, x: u32, y: u32) -> Option<(f32, f32)> {
    let texture = rig.renderer.velocity_texture()?;
    let bytes = rig.read(texture);
    let at = ((y * rig.size + x) * 4) as usize;
    let half = |i: usize| half::f16::from_le_bytes([bytes[at + i * 2], bytes[at + i * 2 + 1]]).to_f32();
    (half(0) != 0.0 || half(1) != 0.0).then(|| (half(0), half(1)))
}

#[test]
fn a_moved_model_writes_its_motion_and_a_still_one_does_not() {
    let Some(mut rig) = Rig::new(SIZE * 2, false) else {
        return;
    };
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::TemporalAntialiasing) {
        return;
    }
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    // The world holds one entity; moving it keeps it the same entity.
    rig.ctx.world.spawn((Transform3D::at(Vec3::ZERO), Model::new(rig.cube)));
    rig.draw();
    rig.draw();
    let center = rig.size / 2;
    assert!(
        motion_at(&rig, center, center).is_none(),
        "a model that stays put writes no motion"
    );
    let step = vec3(0.3, 0.1, 0.0);
    rig.ctx
        .world
        .query::<&mut Transform3D>()
        .for_each(|t| t.position += step);
    rig.draw();
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    // The pixel at the middle of the box's front face (half a unit nearer).
    let face = vec3(0.0, 0.0, 0.5);
    let (before, now) = (view_proj.project_point3(face), view_proj.project_point3(face + step));
    let expected = ((before.x - now.x) * 0.5, (now.y - before.y) * 0.5);
    let to_pixel = |p: Vec3| ((p.x * 0.5 + 0.5) * rig.size as f32, (0.5 - p.y * 0.5) * rig.size as f32);
    let (px, py) = to_pixel(now);
    let (vx, vy) = motion_at(&rig, px as u32, py as u32).expect("the moved model covers its center");
    eprintln!("motion {vx:.5}, {vy:.5}; expected {:.5}, {:.5}", expected.0, expected.1);
    assert!((vx - expected.0).abs() < 0.0005 && (vy - expected.1).abs() < 0.0005);
    assert!(
        expected.0 < 0.0 && expected.1 > 0.0,
        "it moved right and up: it was left and down"
    );
}

/// Shifts every buffer element's cube by the element's `xyz`.
const OFFSETS: &str =
    "fn vertex(v: Vertex) -> Vertex { var o = v; o.position += items[v.instance_index].xyz; return o; }";

impl Rig {
    /// A cube drawn once per element of a GPU buffer holding `offsets`.
    fn buffered_cube(&mut self, offsets: &[Vec4]) -> Option<Mesh> {
        if self.device.limits().max_storage_buffers_per_shader_stage == 0 {
            return None;
        }
        let glow = MaterialDesc {
            color: Color::BLACK,
            roughness: 1.0,
            emissive: Color::WHITE,
            shader: Some(self.ctx.gfx.create_shader("offsets", OFFSETS)),
            ..Default::default()
        };
        self.renderer.create_shader(&self.device, "offsets", OFFSETS).unwrap();
        let material = self.ctx.gfx.create_material(glow);
        let data = MeshData::cube(1.0);
        let mesh = self.ctx.gfx.create_mesh(&data, material);
        self.renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&glow), &self.textures);
        self.renderer
            .create_mesh(&self.device, &data, &[(0..data.indices.len() as u32, material)], true);
        let len = offsets.len() as u32;
        let b = self.ctx.gfx.create_gpu_buffer(len, offsets);
        self.renderer.create_gpu_buffer(&self.device, b.id, len, offsets);
        self.ctx.gfx.set_mesh_gpu_buffer(mesh, b, len);
        self.renderer.set_mesh_gpu_buffer(mesh.id, b.id, len);
        // The offsets reach beyond the entity's own bounds.
        self.ctx.gfx.set_mesh_gpu_bounds(mesh, vec3(0.0, 1.0, 0.0), 2.0);
        Some(mesh)
    }
}

/// How bright the background behind a cube drifting right is in the last
/// frame, at rows `ys` (world height of each cube's middle): the trail.
/// `buffered` draws the cubes from a GPU buffer; `None` without a GPU.
fn trail(disabled: bool, buffered: bool) -> Option<f32> {
    let mut rig = Rig::new(SIZE, false)?;
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::TemporalAntialiasing) {
        return None;
    }
    rig.disable_motion = disabled;
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    // Two cubes a unit apart when buffered, else one: both rows are checked.
    let mut ys = vec![0.0];
    let mesh = if buffered {
        ys.push(1.5);
        rig.buffered_cube(&[vec4(0.0, 0.0, 0.0, 0.0), vec4(0.0, 1.5, 0.0, 0.0)])?
    } else {
        rig.cube
    };
    rig.ctx
        .world
        .spawn((Transform3D::at(vec3(-2.0, 0.0, 0.0)), Model::new(mesh)));
    // A still start, so the history holds the box, then a fast drift to the right.
    for _ in 0..8 {
        rig.draw();
    }
    let mut last = Vec::new();
    for _ in 0..12 {
        rig.ctx
            .world
            .query::<&mut Transform3D>()
            .for_each(|t| t.position.x += 0.3);
        last = rig.draw();
    }
    let n = SIZE as usize;
    // Where the box is now, in pixels; its trail is to the left of it.
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let x_of = |x: f32| (view_proj.project_point3(vec3(x, 0.0, 0.0)).x * 0.5 + 0.5) * SIZE as f32;
    let y_of = |y: f32| ((0.5 - view_proj.project_point3(vec3(0.0, y, 0.0)).y * 0.5) * SIZE as f32) as usize;
    let box_now = -2.0 + 12.0 * 0.3;
    let tail = x_of(box_now - 0.5);
    let mut ghost = 0.0f32;
    for &y in &ys {
        let mut inside = 0.0f32;
        for row in y_of(y) - 3..y_of(y) + 3 {
            for x in (tail as usize).saturating_sub(14)..(tail as usize).saturating_sub(2) {
                ghost = ghost.max(last[row * n + x]);
            }
            inside = inside.max(last[row * n + x_of(box_now) as usize]);
        }
        eprintln!("vectors disabled={disabled}, buffered={buffered}: ghost {ghost:.4}, box {inside:.4}");
        assert!(inside > 0.9, "the box at height {y} is drawn: {inside}");
    }
    Some(ghost)
}

/// Draws a box that holds still, moves for two frames and stops again,
/// with TAA; returns, per frame, whether the motion pass was skipped and
/// whether the box's middle had motion, and the last image.
fn hold_move_hold(always: bool) -> Option<(Vec<(bool, bool)>, Vec<f32>)> {
    let mut rig = Rig::new(SIZE, false)?;
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::TemporalAntialiasing) {
        return None;
    }
    rig.renderer.set_motion_pass_always(always);
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    rig.ctx.world.spawn((Transform3D::at(Vec3::ZERO), Model::new(rig.cube)));
    let center = rig.size / 2;
    let mut frames = Vec::new();
    let mut last = Vec::new();
    for n in 0..9 {
        if (4..6).contains(&n) {
            rig.ctx
                .world
                .query::<&mut Transform3D>()
                .for_each(|t| t.position.x += 0.05);
        }
        last = rig.draw();
        frames.push((
            rig.renderer.motion_pass_skipped(),
            motion_at(&rig, center, center).is_some(),
        ));
    }
    Some((frames, last))
}

#[test]
fn the_motion_pass_is_skipped_while_nothing_moves_and_its_image_stays_clear() {
    let (Some((skipping, image)), Some((always, reference))) = (hold_move_hold(false), hold_move_hold(true)) else {
        return;
    };
    let skipped: Vec<bool> = skipping.iter().map(|f| f.0).collect();
    // The first frame clears the new image; frames 4 and 5 move the box;
    // frame 6 clears its motion; the still frames around are skipped.
    assert_eq!(
        skipped,
        [false, true, true, true, false, false, false, true, true],
        "which frames skip the motion pass"
    );
    let moving: Vec<bool> = skipping.iter().map(|f| f.1).collect();
    assert_eq!(
        moving,
        [false, false, false, false, true, true, false, false, false],
        "the box has motion only while it moves, skipped frames included"
    );
    // The control runs the pass every frame and sees the same motion and
    // the same picture.
    assert!(always.iter().all(|f| !f.0), "the control never skips");
    assert_eq!(moving, always.iter().map(|f| f.1).collect::<Vec<_>>());
    assert_eq!(image, reference, "skipping the pass changes nothing on screen");
}

#[test]
fn a_moving_box_leaves_no_ghost_behind() {
    let (Some(camera_only), Some(with_vectors)) = (trail(true, false), trail(false, false)) else {
        return;
    };
    assert!(with_vectors < 0.03, "background behind moving box: {with_vectors}");
    assert!(
        camera_only > 0.1,
        "negative control must expose old ghosting: {camera_only}"
    );
}

#[test]
fn a_moving_buffer_driven_mesh_leaves_no_ghost_behind() {
    let (Some(camera_only), Some(with_vectors)) = (trail(true, true), trail(false, true)) else {
        return;
    };
    assert!(
        with_vectors < 0.03,
        "background behind moving buffer meshes: {with_vectors}"
    );
    assert!(camera_only > 0.1, "without vectors they ghost: {camera_only}");
}

#[test]
fn resetting_the_history_starts_over_from_the_current_frame() {
    let Some(mut rig) = Rig::new(SIZE, false) else {
        return;
    };
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::TemporalAntialiasing) {
        return;
    }
    rig.slab(0.37);
    let mut big = Rig::new(SIZE * 8, false).unwrap();
    big.slab(0.37);
    let truth = downsample(&big.draw(), (SIZE * 8) as usize, 8);
    drop(big);
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    for _ in 0..40 {
        rig.draw();
    }
    let settled = edge_error(&rig.draw(), &truth);
    // A camera cut: the history is dropped, so the next picture is a single
    // jittered sample again.
    rig.frame.motion.reset = true;
    let cut = edge_error(&rig.draw(), &truth);
    // And it settles again from there.
    let mut again = 0.0;
    for _ in 0..40 {
        again = edge_error(&rig.draw(), &truth);
    }
    eprintln!("edge error: settled {settled:.4}, right after the cut {cut:.4}, 40 frames later {again:.4}");
    assert!(cut > settled * 1.3, "the cut frame has no history: {cut} vs {settled}");
    assert!(again < cut * 0.8, "and it converges again: {again} vs {cut}");
}

#[test]
fn downlevel_devices_fall_back_to_fxaa() {
    let Some(mut rig) = Rig::new(SIZE, true) else {
        return;
    };
    rig.slab(0.37);
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Fxaa;
    let fxaa = rig.draw();
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    let taa = rig.draw();
    assert!(!rig.renderer.taa_active(), "no TAA without compute");
    assert_eq!(fxaa, taa, "TAA draws as FXAA there");
}

#[test]
fn temporal_ao_tracks_objects_without_taa_and_reset_discards_history() {
    let Some(mut rig) = Rig::new(SIZE * 2, false) else {
        return;
    };
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::AmbientOcclusion) {
        return;
    }
    rig.ctx.scene3d.post.ambient_occlusion.enabled = true;
    rig.ctx.scene3d.post.ao_temporal = true;
    rig.ctx.world.spawn((Transform3D::default(), Model::new(rig.cube)));
    rig.draw();
    rig.ctx
        .world
        .query::<&mut Transform3D>()
        .for_each(|t| t.position.x += 0.15);
    rig.draw();
    assert!(!rig.renderer.taa_active());
    assert!(motion_at(&rig, rig.size / 2, rig.size / 2).unwrap().0 < -0.01);
    // Negative control: camera-only reprojection has no object correction.
    rig.disable_motion = true;
    rig.ctx
        .world
        .query::<&mut Transform3D>()
        .for_each(|t| t.position.x += 0.15);
    rig.draw();
    assert!(motion_at(&rig, rig.size / 2, rig.size / 2).is_none());
    rig.frame.motion.reset = true;
    let reset = rig.draw();
    rig.ctx.scene3d.post.ao_temporal = false;
    assert_eq!(reset, rig.draw());
}

#[test]
#[ignore = "offscreen GPU timestamp benchmark; run in release with --ignored --nocapture"]
fn motion_vectors_gpu_cost() {
    let Some(mut rig) = Rig::new(1024, false) else {
        return;
    };
    rig.timer = super::timer::GpuTimer::new(&rig.device, &rig.queue);
    assert!(rig.timer.is_some(), "GPU timestamps required for measurement");
    for y in -10..10 {
        for x in -10..10 {
            rig.ctx.world.spawn((
                Transform3D {
                    position: vec3(x as f32 * 0.2, y as f32 * 0.2, 0.0),
                    scale: Vec3::splat(0.15),
                    ..Default::default()
                },
                Model::new(rig.cube),
            ));
        }
    }
    for (label, taa, ao, moving) in [
        ("TAA static", true, false, false),
        ("TAA moving", true, false, true),
        ("AO moving", false, true, true),
    ] {
        rig.ctx.scene3d.post.anti_aliasing = if taa { AntiAliasing::Taa } else { AntiAliasing::None };
        rig.ctx.scene3d.post.ambient_occlusion.enabled = ao;
        rig.ctx.scene3d.post.ao_temporal = ao;
        let mut sums = [0.0f32; 3];
        for frame in 0..48 {
            if moving {
                rig.ctx
                    .world
                    .query::<&mut Transform3D>()
                    .for_each(|t| t.position.x += if frame % 2 == 0 { 0.01 } else { -0.01 });
            }
            rig.draw();
            let timer = rig.timer.as_mut().unwrap();
            timer.collect(&rig.device);
            if frame >= 16 {
                for pass in &timer.last_frame {
                    sums[0] += pass.ms;
                    if pass.name == "motion vectors" {
                        sums[1] += pass.ms;
                    }
                    if pass.name == "depth prepass" {
                        sums[2] += pass.ms;
                    }
                }
            }
        }
        eprintln!(
            "{label}, 1024x1024, 400 boxes: total {:.4} ms, vectors {:.4} ms, depth {:.4} ms",
            sums[0] / 32.0,
            sums[1] / 32.0,
            sums[2] / 32.0
        );
    }
}

#[test]
fn rotation_animation_writes_motion_away_from_the_pivot() {
    let Some(mut rig) = Rig::new(256, false) else {
        return;
    };
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::TemporalAntialiasing) {
        return;
    }
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    rig.ctx.world.spawn((Transform3D::default(), Model::new(rig.cube)));
    rig.draw();
    let rotation = Quat::from_rotation_z(0.5);
    rig.ctx
        .world
        .query::<&mut Transform3D>()
        .for_each(|t| t.rotation = rotation);
    rig.draw();
    let local = vec3(0.3, 0.0, 0.5);
    let vp = rig.ctx.scene3d.camera.view_proj();
    let a = vp.project_point3(local);
    let b = vp.project_point3(rotation * local);
    let x = ((b.x * 0.5 + 0.5) * rig.size as f32) as u32;
    let y = ((0.5 - b.y * 0.5) * rig.size as f32) as u32;
    let motion = motion_at(&rig, x, y).expect("rotating face has motion");
    assert!((motion.0 - (a.x - b.x) * 0.5).abs() < 0.002);
    assert!((motion.1 - (b.y - a.y) * 0.5).abs() < 0.002);
}

/// A change to make before a frame, and the history reset it should cause.
type Step = (&'static str, fn(&mut Rig), HistoryReset);

/// The history reset each kind of change makes, frame by frame: the target
/// changes (resize, render scale, effect switches) and camera cuts reset
/// once, and the frame after each keeps its history again.
#[test]
fn each_history_trigger_resets_once_and_steady_frames_keep_the_history() {
    let Some(mut rig) = Rig::new(SIZE, false) else {
        return;
    };
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::TemporalAntialiasing) {
        return;
    }
    rig.slab(0.37);
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    rig.ctx.scene3d.post.ssr = 1.0;
    rig.ctx.scene3d.post.ambient_occlusion.enabled = true;
    rig.ctx.scene3d.post.ao_temporal = true;
    let none = HistoryReset::default();
    let targets = HistoryReset { targets: true, ..none };
    let cut = HistoryReset {
        camera_cut: true,
        ..none
    };
    let requested = HistoryReset {
        requested: true,
        ..none
    };
    let steps: [Step; 14] = [
        ("the first frame", |_| {}, targets),
        ("a resize", |r| r.resize(SIZE * 2), targets),
        ("a resize back", |r| r.resize(SIZE), targets),
        (
            "a lower render scale",
            |r| r.ctx.scene3d.post.render_scale = 0.5,
            targets,
        ),
        (
            "the full render scale",
            |r| r.ctx.scene3d.post.render_scale = 1.0,
            targets,
        ),
        (
            "FXAA",
            |r| r.ctx.scene3d.post.anti_aliasing = AntiAliasing::Fxaa,
            targets,
        ),
        (
            "TAA again",
            |r| r.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa,
            targets,
        ),
        ("reflections off", |r| r.ctx.scene3d.post.ssr = 0.0, targets),
        ("reflections on", |r| r.ctx.scene3d.post.ssr = 1.0, targets),
        ("temporal AO off", |r| r.ctx.scene3d.post.ao_temporal = false, targets),
        ("a small camera step", |r| r.ctx.scene3d.camera.position.x += 0.05, none),
        ("a camera teleport", |r| r.ctx.scene3d.camera.position.z += 100.0, cut),
        ("a requested reset", |r| r.frame.motion.reset = true, requested),
        (
            "a requested reset with a resize",
            |r| {
                r.frame.motion.reset = true;
                r.resize(SIZE * 2);
            },
            HistoryReset {
                requested: true,
                targets: true,
                camera_cut: false,
                content: false,
            },
        ),
    ];
    for (name, change, expected) in steps {
        change(&mut rig);
        rig.draw();
        assert!(rig.renderer.has_draws(), "{name}: the slab is drawn");
        assert_eq!(rig.renderer.history_reset(), expected, "{name}");
        rig.draw();
        assert_eq!(rig.renderer.history_reset(), none, "the frame after {name}");
    }
}

#[test]
fn a_reset_frame_reports_no_motion_for_a_teleported_model() {
    let Some(mut rig) = Rig::new(SIZE, false) else {
        return;
    };
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::TemporalAntialiasing) {
        return;
    }
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    rig.ctx
        .world
        .spawn((Transform3D::at(vec3(-1.5, 0.0, 0.0)), Model::new(rig.cube)));
    let teleport = |rig: &mut Rig, x: f32| {
        rig.ctx.world.query::<&mut Transform3D>().for_each(|t| t.position.x = x);
        rig.draw();
        motion_at(rig, rig.size / 2, rig.size / 2)
    };
    rig.draw();
    rig.draw();
    // Negative control: without a reset, a jump reads as fast motion.
    let jump = teleport(&mut rig, 0.0);
    assert!(jump.is_some_and(|m| m.0 < -0.05), "a jump is motion: {jump:?}");
    teleport(&mut rig, -1.5);
    teleport(&mut rig, -1.5);
    // With one, the old position is not the model's past.
    rig.frame.motion.reset = true;
    let cut = teleport(&mut rig, 0.0);
    assert_eq!(cut, None, "a teleport with a reset has no motion");
    // And the next real move has motion again.
    let step = teleport(&mut rig, 0.1);
    assert!(step.is_some_and(|m| m.0 < -0.001), "motion resumes: {step:?}");
}

#[test]
fn a_reset_requested_on_a_frame_that_draws_nothing_still_applies() {
    let Some(mut rig) = Rig::new(SIZE, false) else {
        return;
    };
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::TemporalAntialiasing) {
        return;
    }
    rig.slab(0.37);
    let mut big = Rig::new(SIZE * 8, false).unwrap();
    big.slab(0.37);
    let truth = downsample(&big.draw(), (SIZE * 8) as usize, 8);
    drop(big);
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    for _ in 0..40 {
        rig.draw();
    }
    let settled = edge_error(&rig.draw(), &truth);
    // A loading frame: nothing to draw, and the game asks for a reset.
    rig.ctx.world.clear();
    rig.frame.motion.reset = true;
    rig.draw();
    assert!(!rig.renderer.has_draws(), "the loading frame draws nothing");
    rig.slab(0.37);
    let after = edge_error(&rig.draw(), &truth);
    eprintln!("edge error: settled {settled:.4}, first frame after the reset {after:.4}");
    assert!(
        after > settled * 1.3,
        "the first drawn frame has no history: {after} vs {settled}"
    );
}

/// The box's brightest pixel in columns `xs` of the middle rows.
fn brightest(image: &[f32], size: u32, xs: std::ops::Range<u32>) -> f32 {
    let n = size as usize;
    let mut most = 0.0f32;
    for y in n / 2 - 2..n / 2 + 2 {
        for x in xs.clone() {
            most = most.max(image[y * n + x as usize]);
        }
    }
    most
}

#[test]
fn resizes_and_render_scale_changes_leave_no_ghost_of_the_old_frame() {
    let Some(mut rig) = Rig::new(SIZE, false) else {
        return;
    };
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    rig.ctx.scene3d.post.ssr = 1.0;
    rig.ctx.scene3d.post.ambient_occlusion.enabled = true;
    rig.ctx.scene3d.post.ao_temporal = true;
    rig.ctx
        .world
        .spawn((Transform3D::at(vec3(-1.2, 0.0, 0.0)), Model::new(rig.cube)));
    for _ in 0..16 {
        rig.draw();
    }
    // Every few frames the box jumps to the other side while the window
    // or the render scale changes: the old side must be dark at once.
    let mut x = -1.2f32;
    for (i, (size, scale)) in [(128, 1.0), (128, 0.5), (64, 0.5), (64, 1.0), (128, 0.75), (64, 1.0)]
        .into_iter()
        .enumerate()
    {
        x = -x;
        rig.resize(size);
        rig.ctx.scene3d.post.render_scale = scale;
        rig.ctx.world.query::<&mut Transform3D>().for_each(|t| t.position.x = x);
        for frame in 0..3 {
            let image = rig.draw();
            let (left, right) = (0..size / 4, size * 3 / 4..size);
            let (here, there) = if x > 0.0 { (right, left) } else { (left, right) };
            let (box_now, ghost) = (brightest(&image, size, here), brightest(&image, size, there));
            eprintln!("step {i} ({size}px, scale {scale}) frame {frame}: box {box_now:.3}, old side {ghost:.3}");
            assert!(box_now > 0.9, "step {i} frame {frame}: the box is drawn: {box_now}");
            assert!(ghost < 0.02, "step {i} frame {frame}: no ghost: {ghost}");
        }
    }
}

/// The slab at 8x8 supersampling, averaged down to `size`: the edge's truth.
fn slab_truth(size: u32) -> Vec<f32> {
    let mut big = Rig::new(size * 8, false).unwrap();
    big.slab(0.37);
    downsample(&big.draw(), (size * 8) as usize, 8)
}

/// A slab drawn with TAA for 40 frames, then `step` applied to the rig,
/// then the next frame (red channel, linear). `None` without a GPU.
fn frame_after(step: impl FnOnce(&mut Rig)) -> Option<Vec<f32>> {
    let mut rig = Rig::new(SIZE, false)?;
    if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::TemporalAntialiasing) {
        return None;
    }
    rig.slab(0.37);
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    for _ in 0..40 {
        rig.draw();
    }
    step(&mut rig);
    Some(rig.draw())
}

#[test]
fn a_camera_teleport_drops_the_history_like_an_explicit_reset() {
    // The camera and the slab both jump 100 units: the same picture on
    // screen, and nobody calls reset. The renderer must see the jump as a
    // cut and draw what an explicit reset draws (the same jitter phase).
    let teleport = |rig: &mut Rig| {
        let jump = vec3(100.0, 0.0, 0.0);
        rig.ctx.scene3d.camera.position += jump;
        rig.ctx
            .world
            .query::<&mut Transform3D>()
            .for_each(|t| t.position += jump);
    };
    let (Some(teleported), Some(reset), Some(kept)) = (
        frame_after(teleport),
        frame_after(|rig| rig.frame.motion.reset = true),
        frame_after(|_| {}),
    ) else {
        return;
    };
    let truth = slab_truth(SIZE);
    let (to_reset, to_kept) = (edge_error(&teleported, &reset), edge_error(&teleported, &kept));
    eprintln!(
        "teleport frame vs reset frame {to_reset:.4}, vs kept history {to_kept:.4}; edge error: teleport {:.4}, reset {:.4}, kept {:.4}",
        edge_error(&teleported, &truth),
        edge_error(&reset, &truth),
        edge_error(&kept, &truth)
    );
    assert!(to_reset < 0.01, "a teleport draws like a reset: {to_reset}");
    // Negative control: keeping the history draws something else.
    assert!(to_kept > 0.05, "the history visibly changes the frame: {to_kept}");
    assert!(edge_error(&kept, &truth) < edge_error(&teleported, &truth) * 0.8);
}

#[test]
fn a_resize_drops_the_history_and_converges_at_the_new_size() {
    // Settled at one size, then the window doubles: the first frame must be
    // what an explicit reset draws at the new size, not a blend with the
    // old, differently sized history.
    let big = SIZE * 2;
    let (Some(resized), Some(reset)) = (
        frame_after(|rig| rig.resize(big)),
        frame_after(|rig| {
            rig.resize(big);
            rig.frame.motion.reset = true;
        }),
    ) else {
        return;
    };
    let to_reset = edge_error(&resized, &reset);
    eprintln!("first frame after a resize vs a reset there: {to_reset:.4}");
    assert!(to_reset < 0.01, "a resize draws like a reset: {to_reset}");
    // And it converges at the new size.
    let mut rig = Rig::new(SIZE, false).unwrap();
    rig.slab(0.37);
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    for _ in 0..40 {
        rig.draw();
    }
    let truth = slab_truth(big);
    rig.resize(big);
    let after = edge_error(&rig.draw(), &truth);
    let mut again = 0.0;
    for _ in 0..40 {
        again = edge_error(&rig.draw(), &truth);
    }
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::None;
    let none = edge_error(&rig.draw(), &truth);
    eprintln!("after resizing to {big}: edge error first {after:.4}, 40 frames later {again:.4}, no AA {none:.4}");
    assert!(
        after > again * 1.3,
        "the first frame is a single sample: {after} vs {again}"
    );
    assert!(again < none * 0.75, "TAA converges at the new size: {again} vs {none}");
}

#[test]
fn a_still_slab_matches_its_reference_image() {
    let Some(mut rig) = Rig::new(SIZE, false) else {
        return;
    };
    rig.slab(0.37);
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::None;
    let image = rig.image();
    // What must hold anywhere: the middle is lit, the corners are black.
    let half = SIZE / 2;
    assert_region(
        &image,
        half - 4..half + 4,
        half - 2..half + 2,
        1.0,
        "the slab is lit",
        |p| p[0] > 250 && p[1] > 250 && p[2] > 250,
    );
    assert_region(&image, 0..6, 0..6, 1.0, "the top-left corner is black", |p| {
        p[..3] == [0; 3]
    });
    assert_region(
        &image,
        SIZE - 6..SIZE,
        SIZE - 6..SIZE,
        1.0,
        "the bottom-right corner is black",
        |p| p[..3] == [0; 3],
    );
    check_reference(
        "taa_still_slab_no_aa",
        &image,
        Tolerance {
            channel: 8,
            outliers: 0.02,
            edge: 0,
        },
        "A white emissive slab (4 x 1.5 x 0.05) turned 0.37 rad about the view axis, 5 units in front of \
         the camera, on black; 64x64, no antialiasing, no lighting, no tonemapping.",
        &rig.info,
    );
}

/// Validation scopes cover preparation, old-target retirement, shader reload,
/// command recording, submission and readback. Unscoped errors are captured too.
fn checked(rig: &mut Rig, draw: impl FnOnce(&mut Rig)) {
    use std::sync::{Arc, Mutex};
    let errors = Arc::new(Mutex::new(Vec::new()));
    let sink = errors.clone();
    rig.device
        .on_uncaptured_error(Arc::new(move |e| sink.lock().unwrap().push(e.to_string())));
    let scope = rig.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| draw(rig)));
    rig.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let error = pollster::block_on(scope.pop());
    assert!(error.is_none(), "GPU validation: {error:?}");
    let errors = errors.lock().unwrap().clone();
    assert!(errors.is_empty(), "GPU errors: {errors:?}");
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
}

fn assert_box_sides(rig: &mut Rig, x: f32) {
    let image = rig.image();
    keep_image(&image);
    let (here, there) = if x > 0.0 {
        (rig.size * 3 / 4..rig.size, 0..rig.size / 4)
    } else {
        (0..rig.size / 4, rig.size * 3 / 4..rig.size)
    };
    let lit = image.fraction(here, 0..rig.size, |p| p[0] > 240);
    eprintln!("box x={x}, {}px: bright fraction {lit}", rig.size);
    assert!(lit > 0.01, "the current box is drawn: {lit}");
    assert_region(&image, there, 0..rig.size, 1.0, "no old box or stale history", |p| {
        p[0] < 40
    });
}

#[test]
fn repeated_toggles_and_shader_hot_reload_leave_no_stale_history_or_invalid_bindings() {
    let Some(mut rig) = Rig::new(SIZE, false) else {
        return;
    };
    const LIGHT: &str = "fn surface(s: Surface) -> Surface { var o = s; o.emissive = vec3<f32>(1.0); return o; }";
    const DIM: &str = "fn surface(s: Surface) -> Surface { var o = s; o.emissive = vec3<f32>(0.0); return o; }";
    checked(&mut rig, |rig| {
        let shader = rig.renderer.create_shader(&rig.device, "history.wgsl", LIGHT).unwrap();
        let mut desc = GpuMaterialDesc::new(&MaterialDesc {
            color: Color::BLACK,
            emissive: Color::WHITE,
            ..Default::default()
        });
        desc.shader = shader;
        rig.renderer
            .update_material(&rig.device, &rig.queue, 0, &desc, &rig.textures);
        rig.ctx
            .world
            .spawn((Transform3D::at(vec3(-1.2, 0.0, 0.0)), Model::new(rig.cube)));
        for cycle in 0..12 {
            let x = if cycle % 2 == 0 { -1.2 } else { 1.2 };
            rig.resize(if cycle % 2 == 0 { SIZE } else { SIZE * 2 });
            rig.ctx.scene3d.post.render_scale = if cycle % 3 == 0 { 0.5 } else { 1.0 };
            rig.ctx.scene3d.post.anti_aliasing = if cycle % 2 == 0 {
                AntiAliasing::Taa
            } else {
                AntiAliasing::Fxaa
            };
            rig.ctx.scene3d.post.ssr = (cycle % 2) as f32;
            rig.ctx.scene3d.post.ambient_occlusion.enabled = cycle % 2 == 0;
            rig.ctx.scene3d.post.ao_temporal = cycle % 3 == 0;
            rig.ctx.world.query::<&mut Transform3D>().for_each(|t| t.position.x = x);
            rig.renderer
                .replace_shader(&rig.device, shader, "history.wgsl", DIM)
                .unwrap();
            let dark = rig.image();
            assert_region(
                &dark,
                0..rig.size,
                0..rig.size,
                1.0,
                "reloaded shader draws dark",
                |p| p[0] < 40,
            );
            rig.renderer
                .replace_shader(&rig.device, shader, "history.wgsl", LIGHT)
                .unwrap();
            assert_box_sides(rig, x);
            assert!(
                rig.renderer.history_reset().content,
                "the reload resets shading history"
            );
            let expected = (rig.size as f32 * rig.ctx.scene3d.post.render_scale) as u32;
            rig.renderer.assert_target_configuration((expected, expected), 4);
            rig.draw();
            assert!(!rig.renderer.history_reset().any(), "steady frame keeps history");
        }
    });
}

#[test]
fn surface_interruptions_reset_once_and_hidden_captures_draw_at_the_last_size() {
    use crate::render::surface_lifecycle::{SurfaceEvent as E, SurfaceLifecycle};
    let Some(mut rig) = Rig::new(SIZE, false) else {
        return;
    };
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    rig.ctx.scene3d.post.ssr = 1.0;
    rig.ctx.scene3d.post.ambient_occlusion.enabled = true;
    rig.ctx.scene3d.post.ao_temporal = true;
    rig.ctx
        .world
        .spawn((Transform3D::at(vec3(-1.2, 0.0, 0.0)), Model::new(rig.cube)));
    checked(&mut rig, |rig| {
        let mut lifecycle = SurfaceLifecycle::default();
        for _ in 0..8 {
            rig.draw();
        }
        for size in [(0, 0), (0, SIZE), (SIZE, 0)] {
            // This is the same size filter used for the window's capture
            // targets AND its cameras: neither may become zero/NaN.
            if let Some((width, _)) = SurfaceLifecycle::nonzero_size(size.0, size.1) {
                rig.resize(width);
            }
            assert_box_sides(rig, -1.2);
            assert_eq!(rig.size, SIZE);
        }
        for (i, event) in [
            E::Occluded(true),
            E::Occluded(false),
            E::Suspended(true),
            E::Suspended(false),
            E::Recreated,
            E::Recreated,
        ]
        .into_iter()
        .enumerate()
        {
            let x = if i % 2 == 0 { 1.2 } else { -1.2 };
            if lifecycle.update(event) {
                rig.renderer.invalidate_targets();
            }
            // A zero-sized/minimized window keeps its last nonzero capture size.
            assert!(SurfaceLifecycle::draw(false, true, false));
            rig.ctx.world.query::<&mut Transform3D>().for_each(|t| t.position.x = x);
            assert_box_sides(rig, x);
            assert!(rig.renderer.history_reset().targets, "{event:?}");
            assert_box_sides(rig, x);
            assert!(
                !rig.renderer.history_reset().any(),
                "offscreen history can accumulate while hidden"
            );
        }
    });
}

#[test]
fn runtime_sample_changes_rebuild_main_and_target_camera_attachments_without_history() {
    let mut supports_two = false;
    // Probe through the shared rig so forced-tier limits and skip reporting apply.
    let Some(probe) = gpu_with(|adapter| {
        supports_two = multisamples(adapter, 2);
        Ok(Default::default())
    }) else {
        return;
    };
    drop(probe);
    if !supports_two {
        skip("adapter does not support 2x scene color and depth");
        return;
    }
    let Some(mut rig) = Rig::new(SIZE, false) else {
        return;
    };
    let texture = target(&rig.device, (SIZE, SIZE), HDR_FORMAT);
    // target() includes copy-src, so both the main view and camera are read back.
    rig.ctx.set_headless(true);
    let handle = rig.ctx.gfx.create_render_target(SIZE, SIZE);
    rig.renderer.add_render_target(handle.texture.id, &texture);
    rig.ctx
        .scene3d
        .passes
        .target_cameras
        .push(crate::render::TargetCamera::new(rig.ctx.scene3d.camera, handle));
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    rig.ctx
        .world
        .spawn((Transform3D::at(vec3(-1.2, 0.0, 0.0)), Model::new(rig.cube)));
    checked(&mut rig, |rig| {
        // A custom material must retain its shader id and shading when the
        // sample-dependent pipelines are replaced (a built-in fallback is white).
        let shader = rig
            .renderer
            .create_shader(
                &rig.device,
                "sample-tint.wgsl",
                "fn surface(s: Surface) -> Surface { var o = s; o.emissive = vec3<f32>(1.0, 0.0, 0.0); return o; }",
            )
            .unwrap();
        let mut desc = GpuMaterialDesc::new(&MaterialDesc {
            color: Color::BLACK,
            emissive: Color::WHITE,
            ..Default::default()
        });
        desc.shader = shader;
        rig.renderer
            .update_material(&rig.device, &rig.queue, 0, &desc, &rig.textures);
        rig.draw();
        assert!(!rig.renderer.set_samples(&rig.device, 0));
        for (i, count) in [2, 4, 2, 4, 2, 4].into_iter().enumerate() {
            assert!(rig.renderer.set_samples(&rig.device, count));
            let x = if i % 2 == 0 { 1.2 } else { -1.2 };
            rig.ctx.world.query::<&mut Transform3D>().for_each(|t| t.position.x = x);
            assert_box_sides(rig, x);
            assert!(rig.renderer.history_reset().targets);
            rig.renderer.assert_target_configuration((SIZE, SIZE), count);
            assert_eq!(rig.frame.views.len(), 1);
            assert!(
                !rig.frame.views[0].frame.motion.enabled,
                "target cameras keep no motion/history"
            );
            // Target cameras render the current scene without TAA even when
            // the main camera accumulates it: a moved box leaves no old pixels.
            assert_target_box(rig, &texture, x);
            rig.draw();
            assert!(!rig.renderer.history_reset().any());
            // A no-op count must preserve targets and history too.
            assert!(rig.renderer.set_samples(&rig.device, count));
            rig.draw();
            assert!(!rig.renderer.history_reset().any());
            // Move without recreating attachments: the target camera still
            // has no old frame to blend, irrespective of the main view's TAA.
            rig.ctx
                .world
                .query::<&mut Transform3D>()
                .for_each(|t| t.position.x = -x);
            rig.draw();
            assert!(!rig.renderer.history_reset().any());
            assert_target_box(rig, &texture, -x);
        }
    });
}

fn assert_target_box(rig: &Rig, texture: &wgpu::Texture, x: f32) {
    let bytes = read_texture(&rig.device, &rig.queue, texture);
    let (pixels, _) = bytes.as_chunks::<8>();
    let red: Vec<f32> = pixels
        .iter()
        .map(|p| half::f16::from_le_bytes([p[0], p[1]]).to_f32())
        .collect();
    assert!(
        pixels
            .iter()
            .all(|p| half::f16::from_le_bytes([p[4], p[5]]).to_f32() < 0.02),
        "custom shader remains red after resampling"
    );
    let (here, there) = if x > 0.0 {
        (SIZE * 3 / 4..SIZE, 0..SIZE / 4)
    } else {
        (0..SIZE / 4, SIZE * 3 / 4..SIZE)
    };
    assert!(brightest(&red, SIZE, here) > 0.9);
    assert!(brightest(&red, SIZE, there) < 0.02);
}

#[test]
fn validation_scope_captures_invalid_resources_as_a_negative_control() {
    let Some(gpu) = gpu() else {
        return;
    };
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let _invalid = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("validation capture negative control"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 0,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    assert!(
        pollster::block_on(scope.pop()).is_some(),
        "the GPU error scope must observe an invalid resource"
    );
}
