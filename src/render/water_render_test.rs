//! Offscreen GPU tests of the water kit (`crate::water`): the drawn waves
//! are where the CPU height query says, the reference images of a lake
//! (shoreline foam, a planar reflection), a river (its flow), a sea (its
//! waves) and the underwater view, contact ripples spreading in their
//! compute pass, a game without water drawing exactly what it drew
//! before, and (ignored by default) each quality preset's GPU time.
//! Skips without a GPU.
use std::sync::{Arc, Mutex};

use super::gpu::GpuTexture;
use super::gpu3d::*;
use super::regression_stage::{REFERENCE, luma, mean_color};
use super::render_test_support::{self as support, Image, check_reference, reference_rejects};
use super::timer::GpuTimer;
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::{Material, MaterialDesc, Mesh, MeshData, Model, RenderTarget, Shader, TargetCamera, Tonemap};
use crate::three::{GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};
use crate::water::{
    FlowMap, Water, WaterDesc, WaterQuality, WaterReflection, Waves, ripple_material, shaders, surface_material,
    underwater_material, water_mesh,
};

/// The reference images' size: small, as the reference check asks.
const SIDE: u32 = 160;

/// An offscreen 3D renderer with a `Ctx` beside it. Everything is made on
/// the CPU first (for water, the kit's own `Water::try_spawn`), then the
/// same on the GPU in the same order, so the ids line up as a game's
/// `Graphics` keeps them.
struct Rig {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
    output: wgpu::Texture,
    view: wgpu::TextureView,
    layout: wgpu::BindGroupLayout,
    textures: Vec<GpuTexture>,
    renderer: Box<Renderer3D>,
    ctx: Box<Ctx>,
    size: (u32, u32),
    /// The shaders' clock (`frame.time.x`), in seconds.
    time: f32,
    /// Whether each frame runs the water kit's update.
    kit: bool,
    /// GPU materials made so far (their ids follow this count).
    materials: u32,
    timer: Option<GpuTimer>,
    errors: Arc<Mutex<Vec<String>>>,
}

impl Rig {
    /// `None` without a GPU (logged as a skip). `timestamps`: time the
    /// passes, where the adapter can.
    fn new(size: (u32, u32), timestamps: bool) -> Option<Self> {
        let support::TestGpu { device, queue, info } = support::gpu_with(move |adapter| {
            let limits = wgpu::Limits {
                max_sampled_textures_per_shader_stage: adapter
                    .limits()
                    .max_sampled_textures_per_shader_stage
                    .clamp(16, 24),
                ..wgpu::Limits::default()
            };
            let timed = timestamps && adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
            Ok(wgpu::DeviceDescriptor {
                required_limits: limits,
                required_features: if timed {
                    wgpu::Features::TIMESTAMP_QUERY
                } else {
                    wgpu::Features::empty()
                },
                ..Default::default()
            })
        })?;
        let errors = Arc::new(Mutex::new(Vec::new()));
        let sink = errors.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| sink.lock().unwrap().push(e.to_string())));
        let (layout, white, textures) = support::white_textures(&device, &queue);
        let output = support::target(&device, size, wgpu::TextureFormat::Rgba8UnormSrgb);
        let view = output.create_view(&Default::default());
        let renderer = Box::new(Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true));
        let mut ctx = Box::new(Ctx::new(1.0 / 60.0, Vec2::new(size.0 as f32, size.1 as f32)));
        // Its `Graphics` keeps the books (ids, render targets) as a
        // headless game's does; this rig makes the GPU side.
        ctx.gfx.headless = true;
        ctx.scene3d.post.bloom = 0.0;
        ctx.scene3d.post.tonemap = Tonemap::None;
        ctx.scene3d.sky.atmosphere = false;
        ctx.scene3d.sky.sky_color = Color::rgb(0.55, 0.7, 0.95);
        ctx.scene3d.sky.ground_color = Color::rgb(0.35, 0.3, 0.25);
        let lighting = &mut ctx.scene3d.lighting;
        lighting.shadows = false;
        lighting.sun_direction = vec3(-0.45, -0.6, -0.65);
        lighting.sun_color = Color::WHITE;
        lighting.sun_intensity = 2.5;
        lighting.ambient_intensity = 1.0;
        let timer = if timestamps {
            GpuTimer::new(&device, &queue)
        } else {
            None
        };
        Some(Self {
            device,
            queue,
            info,
            output,
            view,
            layout,
            textures,
            renderer,
            ctx,
            size,
            time: 0.0,
            kit: true,
            materials: 0,
            timer,
            errors,
        })
    }

    fn gpu_material(&mut self, id: u32, desc: &MaterialDesc) {
        let made = self
            .renderer
            .create_material(&self.device, &GpuMaterialDesc::new(desc), &self.textures);
        assert_eq!((made, made), (id, self.materials), "material ids line up");
        self.materials += 1;
    }

    fn gpu_shader(&mut self, id: u32, name: &str, source: &str) {
        let made = self
            .renderer
            .create_shader(&self.device, name, source)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        // The renderer's shader 0 is the built-in one.
        assert_eq!(made, id + 1, "shader ids line up");
    }

    fn gpu_mesh(&mut self, data: &MeshData, material: Material) {
        self.renderer
            .create_mesh(&self.device, data, &[(0..data.indices.len() as u32, material)], true);
    }

    /// A material shader, on both sides.
    fn shader(&mut self, name: &str, source: &str) -> Shader {
        let shader = self.ctx.gfx.create_shader(name, source);
        self.gpu_shader(shader.id, name, source);
        shader
    }

    /// A mesh of `data` with `desc`, on both sides.
    fn mesh(&mut self, data: &MeshData, desc: MaterialDesc) -> Mesh {
        let material = self.ctx.gfx.create_material(desc);
        self.gpu_material(material.id, &desc);
        let mesh = self.ctx.gfx.create_mesh(data, material);
        self.gpu_mesh(data, material);
        mesh
    }

    /// The GPU side of render target `target`, made as the engine makes
    /// them: an HDR texture the renderer knows as a target, at the index
    /// of its texture id.
    fn gpu_target(&mut self, target: RenderTarget) {
        let handle = target.texture();
        assert_eq!(handle.id as usize, self.textures.len(), "texture ids line up");
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("water test target"),
            size: wgpu::Extent3d {
                width: handle.width(),
                height: handle.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        self.renderer.add_render_target(handle.id, &texture);
        self.textures.push(GpuTexture {
            texture,
            nearest: false,
            mips: Some(false),
            bind_group,
        });
    }

    /// A render target, on both sides.
    fn render_target(&mut self, width: u32, height: u32) -> RenderTarget {
        let target = self.ctx.gfx.create_render_target(width, height);
        self.gpu_target(target);
        target
    }

    /// The GPU side of a flow map's data texture array.
    fn gpu_flow(&mut self, id: u32, map: &FlowMap) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("water test flow map"),
            size: wgpu::Extent3d {
                width: map.width,
                height: map.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            texture.as_image_copy(),
            &map.rgba(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(map.width * 4),
                rows_per_image: Some(map.height),
            },
            texture.size(),
        );
        assert_eq!(self.renderer.add_texture_array(&texture), id, "array ids line up");
    }

    /// A water body: the kit's own `Water::try_spawn` on the CPU, then
    /// each part it made, in the same order, on the GPU.
    fn water(&mut self, desc: WaterDesc) -> Entity {
        let entity = Water::try_spawn(&mut self.ctx, desc).expect("the water spawns");
        let water = self.ctx.world.get::<Water>(entity).unwrap();
        let (desc, parts) = (water.desc().clone(), water.parts);
        if let (Some(array), Some(map)) = (parts.flow, &desc.flow_map) {
            self.gpu_flow(array.id, map);
        }
        if let Some(target) = parts.reflection {
            self.gpu_target(target);
        }
        if let Some(r) = parts.ripples {
            let made = self
                .renderer
                .create_compute(&self.device, "water ripples", shaders::RIPPLES)
                .unwrap_or_else(|e| panic!("water ripples: {e}"));
            assert_eq!(
                Some(made),
                r.compute.map(|c| c.id),
                "the headless books made the compute pass"
            );
            self.gpu_target(r.target);
            self.gpu_material(r.material.id, &ripple_material(&desc, &[]));
        }
        let source = shaders::surface(&desc, parts.reflection.is_some(), parts.ripples.is_some());
        self.gpu_shader(parts.shader.id, "water surface", &source);
        let surface = surface_material(
            &desc,
            parts.shader,
            parts.reflection,
            parts.ripples.map(|r| r.target),
            parts.flow,
        );
        self.gpu_material(parts.material.id, &surface);
        self.gpu_mesh(&water_mesh(&desc), parts.material);
        if let Some((effect, material)) = parts.underwater {
            let made = self
                .renderer
                .create_effect(&self.device, "water underwater", &shaders::underwater(&desc))
                .unwrap_or_else(|e| panic!("water underwater: {e}"));
            assert_eq!(made, effect.id);
            self.gpu_material(material.id, &underwater_material(&desc));
        }
        entity
    }

    /// A box from `lo` to `hi`, turned by `rotation` about its centre.
    fn block(&mut self, lo: Vec3, hi: Vec3, rotation: Quat, desc: MaterialDesc) -> Entity {
        let mesh = self.mesh(&MeshData::cube(1.0), desc);
        self.ctx.world.spawn((
            Transform3D {
                position: (lo + hi) * 0.5,
                scale: hi - lo,
                rotation,
            },
            Model::new(mesh),
        ))
    }

    /// Runs the water kit's frame update and hands the ripple materials
    /// it changed to the GPU.
    fn update_water(&mut self) {
        crate::water::update_water(&mut self.ctx);
        let mut bodies = Vec::new();
        self.ctx
            .world
            .query::<(Entity, &Water)>()
            .for_each(|(e, _)| bodies.push(e));
        for e in bodies {
            let water = self.ctx.world.get_mut::<Water>(e).unwrap();
            if let (Some(desc), Some(r)) = (water.ripple_sent.take(), water.parts.ripples) {
                self.renderer.update_material(
                    &self.device,
                    &self.queue,
                    r.material.id,
                    &GpuMaterialDesc::new(&desc),
                    &self.textures,
                );
            }
        }
    }

    /// One frame as a game draws it (see `Ctx::prepare_3d`); then the
    /// clock moves on.
    fn frame(&mut self) {
        if self.kit {
            self.update_water();
        }
        let ctx = &mut self.ctx;
        ctx.collect_passes(None);
        let skins = SkinFrame::default();
        ctx.collect_views(&skins);
        ctx.gfx.frame3d.lod = ctx.scene3d.lod;
        ctx.gfx.frame3d.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &skins,
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        let scene = Scene3D {
            frame: &ctx.gfx.frame3d,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            light: SkyLight::authored(&ctx.scene3d.lighting, &ctx.scene3d.sky),
            camera: ctx.scene3d.camera.position,
            post: &ctx.scene3d.post,
            time: (f64::from(self.time), 1.0 / 60.0),
            occlusion: false,
        };
        self.renderer.prepare(
            &self.device,
            &self.queue,
            &Prepare3D {
                scene: &scene,
                lut: None,
                size: self.size,
            },
        );
        if let Some(timer) = &mut self.timer {
            timer.collect(&self.device);
            timer.begin_frame();
        }
        // Behind everything, the flat sky's colour (as a game's clear colour).
        let [r, g, b, _] = crate::three::linear(self.ctx.scene3d.sky.sky_color);
        let clear = wgpu::Color {
            r: f64::from(r),
            g: f64::from(g),
            b: f64::from(b),
            a: 1.0,
        };
        let buffers = self
            .renderer
            .draw(&self.device, &self.view, clear, &self.textures, self.timer.as_ref());
        self.queue.submit(buffers);
        if let Some(timer) = &mut self.timer {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            timer.resolve(&mut encoder);
            self.queue.submit([encoder.finish()]);
            timer.submitted();
        }
        self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        if let Some(timer) = &mut self.timer {
            timer.collect(&self.device);
        }
        self.time += 1.0 / 60.0;
    }

    /// Draws `frames` frames and reads the last one back.
    fn image(&mut self, frames: usize) -> Image {
        for _ in 0..frames {
            self.frame();
        }
        let image = Image::read(&self.device, &self.queue, &self.output);
        support::keep_image(&image);
        image
    }

    /// The texels (RGBA, row by row from the top) of render target `target`.
    fn read_target(&self, target: RenderTarget) -> Vec<[f32; 4]> {
        let gpu = &self.textures[target.texture().id as usize].texture;
        let bytes = support::read_texture(&self.device, &self.queue, gpu);
        bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|p| std::array::from_fn(|i| half::f16::from_le_bytes([p[2 * i], p[2 * i + 1]]).to_f32()))
            .collect()
    }

    /// The pixel the camera sees world point `p` at.
    fn pixel(&self, p: Vec3) -> (u32, u32) {
        let ndc = self.ctx.scene3d.camera.view_proj().project_point3(p);
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        (
            ((ndc.x * 0.5 + 0.5) * w).clamp(0.0, w - 1.0) as u32,
            ((0.5 - ndc.y * 0.5) * h).clamp(0.0, h - 1.0) as u32,
        )
    }

    /// The mean colour around the pixel world point `p` is seen at.
    fn color_at(&self, image: &Image, p: Vec3, radius: u32) -> [f32; 3] {
        let (x, y) = self.pixel(p);
        mean_color(image, x, y, radius)
    }

    /// The GPU time of the latest timed frame, in ms.
    fn gpu_ms(&self) -> Option<f32> {
        self.timer.as_ref()?.latest_frame().map(|(ms, _)| ms)
    }

    /// Ends the test: no GPU error was reported while it ran.
    fn finish(self) {
        self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let errors = std::mem::take(&mut *self.errors.lock().unwrap());
        assert!(
            errors.is_empty(),
            "the GPU reported errors on {}: {errors:#?}",
            support::describe_adapter(&self.info)
        );
    }
}

fn sand() -> MaterialDesc {
    MaterialDesc::color(Color::rgb(0.76, 0.66, 0.45)).with_roughness(0.95)
}

fn grass() -> MaterialDesc {
    MaterialDesc::color(Color::rgb(0.3, 0.5, 0.2)).with_roughness(0.95)
}

fn glow(color: Color) -> MaterialDesc {
    MaterialDesc {
        color: Color::BLACK,
        emissive: color,
        ..Default::default()
    }
}

/// The probe's surface: the water's own vertex hook moves the mesh, and
/// each drawn point writes where it is, unlit and opaque, scaled into the
/// range where half floats are finest (x and z over 16 m, height over
/// 4 m, plus one so the water is never zero).
const PROBE_SURFACE: &str = "
fn surface(s: Surface) -> Surface {
    var out = s;
    out.unlit = true;
    out.albedo = vec3<f32>(0.0);
    out.emissive = vec3<f32>(s.world.x / 16.0 + 0.5, s.world.y / 4.0 + 1.0, s.world.z / 16.0 + 0.5);
    return out;
}";

#[test]
fn the_drawn_waves_are_where_the_cpu_height_query_says() {
    let Some(mut rig) = Rig::new((64, 64), false) else {
        return;
    };
    rig.ctx.scene3d.sky.sky_color = Color::BLACK;
    rig.ctx.scene3d.sky.ground_color = Color::BLACK;
    // A patch of sea 14 m across, drawn by the kit's vertex hook over its
    // own wave table, seen from straight above by a target camera that
    // keeps HDR values.
    let desc = WaterDesc {
        follow_camera: false,
        center: Vec2::ZERO,
        size: vec2(14.0, 14.0),
        ..WaterDesc::sea(0.0, vec2(1.0, 0.35), 1.0).with_quality(WaterQuality::High)
    };
    let source = format!(
        "{}{}{PROBE_SURFACE}",
        shaders::wave_library(&desc.waves),
        shaders::VERTEX
    );
    let shader = rig.shader("water probe", &source);
    let probe = MaterialDesc {
        shader: Some(shader),
        double_sided: true,
        ..Default::default()
    };
    let mesh = rig.mesh(&water_mesh(&desc), probe);
    rig.ctx.world.spawn((Transform3D::default(), Model::new(mesh)));
    let target = rig.render_target(96, 96);
    let mut camera = rig.ctx.scene3d.camera;
    camera.position = vec3(0.0, 30.0, 0.0);
    camera.look_at(vec3(0.0, 0.0, -0.001));
    camera.fov_y = 0.3;
    rig.ctx
        .scene3d
        .passes
        .target_cameras
        .push(TargetCamera::new(camera, target));
    let t = 7.3;
    rig.time = t;
    rig.frame();
    let drawn: Vec<(Vec2, f32)> = rig
        .read_target(target)
        .iter()
        .filter(|p| p[1] > 0.5)
        .map(|p| (vec2((p[0] - 0.5) * 16.0, (p[2] - 0.5) * 16.0), (p[1] - 1.0) * 4.0))
        .collect();
    assert!(drawn.len() > 5000, "the probe saw the water: {} texels", drawn.len());
    let off = |at: f32| {
        drawn
            .iter()
            .map(|&(p, h)| (h - desc.waves.height_at(p, at)).abs())
            .fold(0.0f32, f32::max)
    };
    let (lo, hi) = drawn
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &(_, h)| (lo.min(h), hi.max(h)));
    // The tolerance: the mesh's flat triangles between vertices 9 cm
    // apart (about a millimetre on these waves), and half floats' rounding
    // of the values read back (under 2 mm of height, and under 4 mm along
    // the ground, times the slope).
    let worst = off(t);
    let later = off(t + 0.5);
    eprintln!(
        "drawn vs CPU height: {} texels from {lo} to {hi} m, worst {worst} m (half a second off: {later} m)",
        drawn.len()
    );
    assert!(
        worst < 0.01,
        "the drawn surface is within 1 cm of the CPU height: off by up to {worst} m"
    );
    assert!(hi - lo > 0.4, "and it has waves: {lo} to {hi}");
    // The negative control: the CPU's waves half a second later don't match.
    assert!(later > 0.1, "a different time's waves are off: {later} m");
    rig.finish();
}

/// The lake scene: a sandy bed, a beach sloping up out of the water on the
/// left to a grassy shore, a red lamp on a dark post standing in the water
/// and a blue one on the far side.
fn lake(rig: &mut Rig, reflection: WaterReflection) {
    rig.block(vec3(-20.0, -4.0, -20.0), vec3(20.0, -1.6, 20.0), Quat::IDENTITY, sand());
    // A slab tilted so its top rises out of the water at x = -4.5 and
    // reaches 0.4 m at x = -6.4.
    rig.block(
        vec3(-6.6, -2.0, -20.0),
        vec3(3.4, -0.6, 20.0),
        Quat::from_rotation_z(-0.2),
        sand(),
    );
    rig.block(vec3(-20.0, -2.0, -20.0), vec3(-6.3, 1.0, 20.0), Quat::IDENTITY, grass());
    rig.block(
        vec3(1.0, -2.0, -3.0),
        vec3(1.6, 0.3, -2.4),
        Quat::IDENTITY,
        MaterialDesc::color(Color::rgb(0.05, 0.05, 0.05)),
    );
    rig.block(
        vec3(1.0, 0.3, -3.0),
        vec3(1.6, 2.5, -2.4),
        Quat::IDENTITY,
        glow(Color::rgb(3.0, 0.2, 0.1)),
    );
    rig.block(
        vec3(-1.0, -2.0, -14.0),
        vec3(1.0, 4.0, -12.0),
        Quat::IDENTITY,
        glow(Color::rgb(0.2, 0.4, 3.0)),
    );
    rig.ctx.scene3d.camera.position = vec3(1.0, 3.2, 6.5);
    rig.ctx.scene3d.camera.look_at(vec3(-1.0, 0.0, -2.0));
    rig.water(WaterDesc {
        ripples: false,
        underwater: false,
        ..WaterDesc::lake(Vec2::ZERO, vec2(40.0, 40.0), 0.0)
            .with_reflection(reflection)
            .with_quality(WaterQuality::High)
    });
}

/// Where the camera sees the red lamp's mirror image: the lamp's front
/// face 1.4 m up, mirrored under the water.
const MIRROR_SPOT: Vec3 = vec3(1.3, -1.4, -2.4);

#[test]
fn a_lake_reflects_and_foams_at_its_shore() {
    let Some(mut rig) = Rig::new((SIDE, SIDE), false) else {
        return;
    };
    lake(&mut rig, WaterReflection::Planar);
    let image = rig.image(3);
    let mirrored = rig.color_at(&image, MIRROR_SPOT, 1);
    // Foam: where the bank meets the water the surface is whiter than out
    // in the open water.
    let shore = rig.color_at(&image, vec3(-4.3, 0.0, 1.0), 2);
    let open = rig.color_at(&image, vec3(2.0, 0.0, 2.5), 2);
    // The negative control: the same lake reflecting only the sky.
    let Some(mut sky) = Rig::new((SIDE, SIDE), false) else {
        return;
    };
    lake(&mut sky, WaterReflection::Sky);
    let plain = sky.image(3);
    let no_post = sky.color_at(&plain, MIRROR_SPOT, 1);
    eprintln!("lake: mirrored {mirrored:?} (sky only {no_post:?}), shore {shore:?}, open {open:?}");
    assert!(
        mirrored[0] > mirrored[2] * 1.5 && mirrored[0] > 60.0,
        "the red post shows in the water below it: {mirrored:?}"
    );
    assert!(
        no_post[0] < mirrored[0] * 0.6,
        "without the mirror the post's reflection is gone: {no_post:?} vs {mirrored:?}"
    );
    assert!(
        luma(shore) > luma(open) + 20.0,
        "foam brightens the shoreline: {shore:?} vs open water {open:?}"
    );
    check_reference(
        "water_lake",
        &image,
        REFERENCE,
        "Water kit lake (High, planar reflection, shoreline foam, wind ripples) over a sand bed and a tilted sand \
         beach rising to a grass shore on the left; a red glowing lamp on a dark post standing in the water, a \
         blue post far behind; sky-coloured background; flat sky light (0.55, 0.7, 0.95), sun (-0.45, -0.6, -0.65) at 2.5, ambient 1, no tonemapping; \
         camera at (1, 3.2, 6.5) looking at (-1, 0, -2); 160x160, 4x multisampling; frame 3 at 60 fps.",
        &rig.info,
    );
    reference_rejects("water_lake", &plain, REFERENCE, "the planar reflection off");
    rig.finish();
    sky.finish();
}

#[test]
fn screen_space_reflections_find_what_is_on_screen() {
    let Some(mut rig) = Rig::new((SIDE, SIDE), false) else {
        return;
    };
    lake(&mut rig, WaterReflection::ScreenSpace);
    let image = rig.image(3);
    let mirrored = rig.color_at(&image, MIRROR_SPOT, 1);
    let Some(mut sky) = Rig::new((SIDE, SIDE), false) else {
        return;
    };
    lake(&mut sky, WaterReflection::Sky);
    let plain = sky.image(3);
    let no_post = sky.color_at(&plain, MIRROR_SPOT, 1);
    eprintln!("screen-space: mirrored {mirrored:?}, sky only {no_post:?}");
    assert!(
        mirrored[0] > no_post[0] + 25.0,
        "the march finds the red post: {mirrored:?} vs {no_post:?} with the sky only"
    );
    rig.finish();
    sky.finish();
}

/// The river scene: water running along -z between two grass banks, its
/// current fastest mid-stream.
/// `speed`: the current mid-stream, in m/s.
fn river(rig: &mut Rig, shore_foam: Option<f32>, speed: f32) {
    rig.block(vec3(-20.0, -4.0, -40.0), vec3(20.0, -1.2, 10.0), Quat::IDENTITY, sand());
    rig.block(vec3(-20.0, -2.0, -40.0), vec3(-2.5, 0.5, 10.0), Quat::IDENTITY, grass());
    rig.block(vec3(2.5, -2.0, -40.0), vec3(20.0, 0.5, 10.0), Quat::IDENTITY, grass());
    let flow = FlowMap::from_fn(8, 8, 2.0, |uv| {
        let across = uv.x * 2.0 - 1.0;
        vec2(0.0, -speed * (1.0 - across * across).max(0.2))
    });
    rig.ctx.scene3d.camera.position = vec3(0.0, 2.6, 4.0);
    rig.ctx.scene3d.camera.look_at(vec3(0.0, 0.0, -5.0));
    let desc = WaterDesc::river(vec2(0.0, -15.0), vec2(5.0, 50.0), 0.0, Vec2::ZERO)
        .with_flow_map(flow)
        .with_quality(WaterQuality::High)
        .with_reflection(WaterReflection::Sky);
    rig.water(WaterDesc {
        ripples: false,
        underwater: false,
        shore_foam: shore_foam.unwrap_or(desc.shore_foam),
        ..desc
    });
}

/// The shift along the image's rows that best lines `b` up with `a` in the
/// given rectangle (least squared brightness difference), from `-reach`
/// to `reach`.
fn best_shift(a: &Image, b: &Image, x: std::ops::Range<u32>, y: std::ops::Range<u32>, reach: i32) -> i32 {
    let bright = |image: &Image, px: u32, py: u32| {
        let q = image.pixel(px, py);
        f32::from(q[0]) + f32::from(q[1]) + f32::from(q[2])
    };
    let mut best = (f32::MAX, 0);
    for shift in -reach..=reach {
        let mut sum = 0.0;
        for py in y.clone() {
            for px in x.clone() {
                sum += (bright(a, px, py) - bright(b, px, (py as i32 + shift) as u32)).powi(2);
            }
        }
        if sum < best.0 {
            best = (sum, shift);
        }
    }
    best.1
}

#[test]
fn a_river_carries_its_ripples_and_foam_downstream() {
    let Some(mut rig) = Rig::new((SIDE, SIDE), false) else {
        return;
    };
    river(&mut rig, None, 2.0);
    let before = rig.image(2);
    let after = rig.image(12);
    // The current's own pattern: the same river foaming bank to bank (its
    // foam slides with the current, where the ripples also restart). Half
    // a second later the foam mid-stream has moved a metre away from the
    // camera: up the picture, to rows above where it was.
    let Some(mut foamy) = Rig::new((SIDE, SIDE), false) else {
        return;
    };
    river(&mut foamy, Some(6.0), 2.0);
    let first = foamy.image(2);
    let later = foamy.image(30);
    let (top, bottom) = (foamy.pixel(vec3(0.0, 0.0, -7.0)).1, foamy.pixel(vec3(0.0, 0.0, -1.0)).1);
    let (left, right) = (
        foamy.pixel(vec3(-1.0, 0.0, -3.0)).0,
        foamy.pixel(vec3(1.0, 0.0, -3.0)).0,
    );
    let moved = best_shift(&first, &later, left..right, top + 12..bottom - 12, 12);
    eprintln!("river: the foam moved {moved} rows (rows {top}..{bottom}, columns {left}..{right})");
    assert!(moved <= -3, "the foam moved downstream (up the picture): shift {moved}");
    // The negative control: in still water it stays put.
    let Some(mut still) = Rig::new((SIDE, SIDE), false) else {
        return;
    };
    river(&mut still, Some(6.0), 0.0);
    let (first, later) = (still.image(2), still.image(30));
    let stayed = best_shift(&first, &later, left..right, top + 12..bottom - 12, 12);
    assert_eq!(stayed, 0, "still water's foam stays");
    still.finish();
    check_reference(
        "water_river",
        &before,
        REFERENCE,
        "Water kit river (High, sky reflection, flow map up to 2 m/s along -z, fastest mid-stream) between two \
         grass banks over a sand bed; sky-coloured background; flat sky light (0.55, 0.7, 0.95), sun (-0.45, -0.6, -0.65) at 2.5, ambient \
         1, no tonemapping; camera at (0, 2.6, 4) looking at (0, 0, -5); 160x160, 4x multisampling; frame 2 at \
         60 fps.",
        &rig.info,
    );
    reference_rejects("water_river", &after, REFERENCE, "the same river 0.2 s later");
    rig.finish();
    foamy.finish();
}

/// The open sea from a boat's height.
fn sea(rig: &mut Rig, waves: Waves) {
    rig.ctx.scene3d.camera.position = vec3(0.0, 4.0, 8.0);
    rig.ctx.scene3d.camera.look_at(vec3(0.0, 0.0, -12.0));
    rig.water(WaterDesc {
        ripples: false,
        underwater: false,
        ..WaterDesc::sea(0.0, vec2(0.6, -1.0), 1.4)
            .with_waves(waves)
            .with_quality(WaterQuality::High)
            .with_reflection(WaterReflection::Sky)
    });
}

/// How much the water's brightness changes at the scale of waves: over
/// the lower half of the picture, blurred 9 x 9 pixels (fine ripples
/// average out), the mean distance of each spot from its row's mean (the
/// change with distance doesn't count).
fn swell(image: &Image, r: u32) -> f32 {
    let (mut sum, mut n) = (0.0, 0.0);
    for y in (image.height / 2..image.height - r).step_by(2) {
        let row: Vec<f32> = (r..image.width - r)
            .step_by(2)
            .map(|x| luma(mean_color(image, x, y, r)))
            .collect();
        let mean = row.iter().sum::<f32>() / row.len() as f32;
        sum += row.iter().map(|v| (v - mean).abs()).sum::<f32>();
        n += row.len() as f32;
    }
    sum / n
}

#[test]
fn the_sea_has_waves() {
    let Some(mut rig) = Rig::new((SIDE, SIDE), false) else {
        return;
    };
    sea(&mut rig, Waves::sea(vec2(0.6, -1.0), 1.4));
    let image = rig.image(3);
    let Some(mut flat) = Rig::new((SIDE, SIDE), false) else {
        return;
    };
    sea(&mut flat, Waves::none());
    let calm = flat.image(3);
    let (rough, still) = (swell(&image, 8), swell(&calm, 8));
    eprintln!("sea: swell {rough} with waves, {still} flat");
    assert!(rough > 3.5, "waves light and shade the sea: {rough}");
    assert!(
        still < rough * 0.6,
        "flat water has only fine ripples: {still} vs {rough}"
    );
    check_reference(
        "water_sea",
        &image,
        REFERENCE,
        "Water kit sea (High, sky reflection, Waves::sea for wind (0.6, -1) and 1.4 m, crest foam), mesh \
         following the camera; sky-coloured background; flat sky light (0.55, 0.7, 0.95), sun (-0.45, -0.6, -0.65) at 2.5, ambient 1, \
         no tonemapping; camera at (0, 4, 8) looking at (0, 0, -12); 160x160, 4x multisampling; frame 3 at 60 \
         fps.",
        &rig.info,
    );
    reference_rejects("water_sea", &calm, REFERENCE, "the waves off");
    rig.finish();
    flat.finish();
}

/// Under the lake: a sand bed, a red post rising out of the water, a
/// grass block on the bed, seen from 1.2 m down.
fn dive(rig: &mut Rig, underwater: bool) {
    rig.block(vec3(-20.0, -4.0, -40.0), vec3(20.0, -2.6, 20.0), Quat::IDENTITY, sand());
    rig.block(
        vec3(0.5, -2.6, -6.0),
        vec3(1.5, 2.5, -5.0),
        Quat::IDENTITY,
        glow(Color::rgb(3.0, 0.2, 0.1)),
    );
    rig.block(vec3(-3.0, -2.6, -9.0), vec3(-1.0, -1.2, -7.0), Quat::IDENTITY, grass());
    rig.ctx.scene3d.camera.position = vec3(0.0, -1.2, 2.0);
    rig.ctx.scene3d.camera.look_at(vec3(0.0, -0.6, -6.0));
    rig.water(WaterDesc {
        ripples: false,
        underwater,
        ..WaterDesc::lake(Vec2::ZERO, vec2(60.0, 60.0), 0.0).with_quality(WaterQuality::High)
    });
}

#[test]
fn under_water_the_view_is_tinted_and_fades_with_distance() {
    let Some(mut rig) = Rig::new((SIDE, SIDE), false) else {
        return;
    };
    dive(&mut rig, true);
    let image = rig.image(3);
    assert_eq!(rig.ctx.gfx.frame3d.effects.len(), 1, "the underwater effect runs");
    let Some(mut dry) = Rig::new((SIDE, SIDE), false) else {
        return;
    };
    dive(&mut dry, false);
    let clear = dry.image(3);
    // The far sand is lost in the water's colour: darker and less red
    // than without the effect.
    let far_spot = vec3(-6.0, -2.6, -22.0);
    let far = rig.color_at(&image, far_spot, 2);
    let far_clear = dry.color_at(&clear, far_spot, 2);
    // The near post still shows red.
    let post = rig.color_at(&image, vec3(1.0, -1.8, -5.0), 1);
    eprintln!(
        "underwater: far {far:?} at {:?} (clear {far_clear:?}), post {post:?}",
        rig.pixel(far_spot)
    );
    assert!(
        far[0] < far_clear[0] * 0.6 && far[1] >= far[0],
        "far away the water swallows the red first: {far:?} vs {far_clear:?}"
    );
    assert!(
        post[0] > post[1] && post[0] > 40.0,
        "the near post is still red: {post:?}"
    );
    check_reference(
        "water_underwater",
        &image,
        REFERENCE,
        "Water kit lake (High, screen-space reflection) seen from 1.2 m under its surface with the underwater \
         effect: a sand bed, a red glowing post rising out of the water, a grass block on the bed; sky-coloured background, flat sky light \
         (0.55, 0.7, 0.95), sun (-0.45, -0.6, -0.65) at 2.5, ambient 1, no tonemapping; camera at (0, -1.2, 2) \
         looking at (0, -0.6, -6); 160x160, 4x multisampling; frame 3 at 60 fps.",
        &rig.info,
    );
    reference_rejects("water_underwater", &clear, REFERENCE, "the underwater effect off");
    rig.finish();
    dry.finish();
}

/// The largest ripple height on the circle of radius `r` texels around
/// the field's centre.
fn ring(texels: &[[f32; 4]], side: usize, r: f32) -> f32 {
    (0..96)
        .map(|i| {
            let a = i as f32 / 96.0 * std::f32::consts::TAU;
            let (x, y) = (side as f32 / 2.0 + a.cos() * r, side as f32 / 2.0 + a.sin() * r);
            texels[y as usize * side + x as usize][0].abs()
        })
        .fold(0.0, f32::max)
}

#[test]
fn a_touch_spreads_rings_in_the_ripple_field() {
    let Some(mut rig) = Rig::new((64, 64), false) else {
        return;
    };
    let e = rig.water(WaterDesc {
        underwater: false,
        ripple_area: 32.0,
        ..WaterDesc::lake(Vec2::ZERO, vec2(32.0, 32.0), 0.0)
            .with_reflection(WaterReflection::Sky)
            .with_quality(WaterQuality::Medium)
    });
    let target = rig.ctx.world.get::<Water>(e).unwrap().ripple_target().unwrap();
    let side = target.texture().width() as usize;
    rig.frame();
    assert!(
        rig.read_target(target).iter().all(|t| t[0] == 0.0),
        "no touch, no ripples"
    );
    // A touch 2 m wide in the middle.
    rig.ctx.world.get_mut::<Water>(e).unwrap().touch(Vec2::ZERO, 1.0, 0.2);
    for _ in 0..20 {
        rig.frame();
    }
    let texels = rig.read_target(target);
    let rings: Vec<f32> = (2..side / 2 - 2).map(|r| ring(&texels, side, r as f32)).collect();
    let peak = rings
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(i, _)| i + 2);
    let far = rings[rings.len() - 8..].iter().copied().fold(0.0, f32::max);
    eprintln!(
        "ripples after 20 frames: strongest ring {peak} texels out ({}), {far} at the edge",
        rings[peak - 2]
    );
    // The ring travels about 0.7 texels a frame: some 14 texels out by
    // now, past the touch's own 4 texels and nowhere near the edge.
    assert!(
        rings[peak - 2] > 0.005,
        "the ring carries the touch: {}",
        rings[peak - 2]
    );
    assert!((8..=24).contains(&peak), "the ring is on its way out: {peak} texels");
    assert!(far < rings[peak - 2] * 0.05, "it hasn't reached the edge: {far}");
    // The negative control: a touch outside the field does nothing.
    let Some(mut quiet) = Rig::new((64, 64), false) else {
        return;
    };
    let e = quiet.water(WaterDesc {
        underwater: false,
        ripple_area: 32.0,
        ..WaterDesc::lake(Vec2::ZERO, vec2(32.0, 32.0), 0.0).with_reflection(WaterReflection::Sky)
    });
    quiet
        .ctx
        .world
        .get_mut::<Water>(e)
        .unwrap()
        .touch(vec2(100.0, 0.0), 1.0, 0.2);
    for _ in 0..5 {
        quiet.frame();
    }
    let target = quiet.ctx.world.get::<Water>(e).unwrap().ripple_target().unwrap();
    assert!(
        quiet.read_target(target).iter().all(|t| t[0] == 0.0),
        "a touch outside makes no ripples"
    );
    rig.finish();
    quiet.finish();
}

/// What the renderer holds on the GPU, and what a frame runs: its screen
/// effects, target effects, compute passes and target views (the only
/// ways the kit adds GPU passes).
fn footprint(rig: &Rig) -> (crate::perf_report::GpuMemory, [usize; 4]) {
    let mut tally = super::memory::MemoryTally::default();
    rig.renderer.tally_memory(&mut tally);
    let frame = &rig.ctx.gfx.frame3d;
    (
        tally.finish(),
        [
            frame.effects.len(),
            frame.target_effects.len(),
            frame.computes.len() + frame.scene_computes.len(),
            frame.views.len(),
        ],
    )
}

#[test]
fn a_game_without_water_draws_and_holds_exactly_what_it_did() {
    // The same scene drawn by a game that never runs the kit, and by one
    // that runs its update every frame (as `WaterPlugin` does) with no
    // water.
    let scene = |rig: &mut Rig| {
        rig.block(vec3(-5.0, -1.0, -5.0), vec3(5.0, 0.0, 5.0), Quat::IDENTITY, sand());
        rig.ctx.scene3d.camera.position = vec3(0.0, 3.0, 6.0);
        rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    };
    let Some(mut plain) = Rig::new((64, 64), false) else {
        return;
    };
    plain.kit = false;
    scene(&mut plain);
    let Some(mut kit) = Rig::new((64, 64), false) else {
        return;
    };
    scene(&mut kit);
    for frame in 0..4 {
        let a = plain.image(1);
        let b = kit.image(1);
        assert!(a.rgba == b.rgba, "frame {frame}: the same picture");
        assert_eq!(
            plain.renderer.draw_calls(),
            kit.renderer.draw_calls(),
            "frame {frame}: the same draws"
        );
    }
    let without = footprint(&plain);
    assert_eq!(footprint(&kit), without, "the same GPU memory and passes");
    let live = kit.ctx.gfx.live_counts();
    assert_eq!(
        (live.render_targets, live.computes, live.effects, live.shaders),
        (0, 0, 0, 0),
        "the kit made nothing"
    );
    // The positive control: a lake with a mirror and ripples adds its
    // compute pass, its view and their memory.
    kit.water(WaterDesc::lake(Vec2::ZERO, vec2(8.0, 8.0), 0.0).with_reflection(WaterReflection::Planar));
    kit.image(1);
    let (with, runs) = footprint(&kit);
    assert_eq!(runs, [0, 0, 1, 1], "the ripple pass and the mirror's view");
    assert!(with != without.0, "and their memory");
    plain.finish();
    kit.finish();
}

/// GPU time per quality preset. Run with
/// `cargo test --release --features 3d --lib water_quality_gpu_cost -- --ignored --nocapture`.
/// A 1280x720 frame of open sea (screen-space reflections, or the planar
/// mirror; no ripples or underwater view), against the same frame without
/// water drawn by a second renderer on the same GPU, in alternating
/// blocks of frames so both see the same clocks. Prints the median added
/// GPU time per preset for each of three runs.
#[test]
#[ignore = "a measurement: run in release on a quiet machine"]
fn water_quality_gpu_cost() {
    let presets = [
        ("Low", WaterQuality::Low, WaterReflection::ScreenSpace),
        ("Medium", WaterQuality::Medium, WaterReflection::ScreenSpace),
        ("High", WaterQuality::High, WaterReflection::ScreenSpace),
        ("High, planar", WaterQuality::High, WaterReflection::Planar),
    ];
    let scene = |rig: &mut Rig| {
        rig.block(
            vec3(-30.0, -1.0, -60.0),
            vec3(-10.0, 3.0, -20.0),
            Quat::IDENTITY,
            grass(),
        );
        rig.block(vec3(-40.0, -5.0, -80.0), vec3(40.0, -3.0, 10.0), Quat::IDENTITY, sand());
        rig.ctx.scene3d.camera.position = vec3(0.0, 4.0, 8.0);
        rig.ctx.scene3d.camera.look_at(vec3(0.0, 0.0, -12.0));
    };
    for (name, quality, reflection) in presets {
        let mut runs = Vec::new();
        for _ in 0..3 {
            let Some(mut with) = Rig::new((1280, 720), true) else {
                return;
            };
            let Some(mut without) = Rig::new((1280, 720), true) else {
                return;
            };
            if with.timer.is_none() || without.timer.is_none() {
                support::skip("no timestamp queries");
                return;
            }
            scene(&mut with);
            scene(&mut without);
            with.water(WaterDesc {
                ripples: false,
                underwater: false,
                ..WaterDesc::sea(0.0, vec2(0.6, -1.0), 1.4)
                    .with_quality(quality)
                    .with_reflection(reflection)
            });
            let mut ms = [Vec::new(), Vec::new()];
            for block in 0..12 {
                for (i, rig) in [&mut with, &mut without].into_iter().enumerate() {
                    for frame in 0..20 {
                        rig.frame();
                        if block >= 2
                            && frame >= 4
                            && let Some(t) = rig.gpu_ms()
                        {
                            ms[i].push(t);
                        }
                    }
                }
            }
            let median = |v: &mut Vec<f32>| {
                v.sort_by(f32::total_cmp);
                v[v.len() / 2]
            };
            let [mut a, mut b] = ms;
            runs.push(median(&mut a) - median(&mut b));
            with.finish();
            without.finish();
        }
        runs.sort_by(f32::total_cmp);
        eprintln!(
            "water {name}: +{:.3} ms GPU at 1280x720 (median of 3 runs; runs {:.3}, {:.3}, {:.3})",
            runs[1], runs[0], runs[1], runs[2]
        );
    }
}
