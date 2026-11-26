//! An offscreen handle on the 3D renderer's command recording, for
//! `ponko_bench`: the same `Renderer3D::prepare` and `draw` a game's frame
//! runs, into a texture, without a window. It times `draw` alone: turning
//! the frame's passes into command buffers (on several cores), which is
//! where a change to how passes are described or checked would show up.
//! Not part of the public API.

use std::time::{Duration, Instant};

use glam::{Vec2, Vec3};

use super::{Prepare3D, Renderer3D};
use crate::ctx::Ctx;
use crate::d3::Transform as Transform3D;
use crate::render::gpu::GpuTexture;
use crate::render::{AntiAliasing, MaterialDesc, Mesh, MeshData, Model};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};

/// What the frames of a [`RecordBench`] look like.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct RecordScene {
    /// The output's width and height, in pixels.
    pub size: (u32, u32),
    /// Models, on a grid in front of the camera.
    pub models: usize,
    /// Distinct meshes among them, each with its own material (so each is
    /// at least one draw call per pass).
    pub meshes: u32,
    /// Temporal antialiasing, ambient occlusion, screen-space reflections
    /// and an atmosphere on top of the default frame (sun shadows, depth
    /// prepass, opaque, bloom, post).
    pub effects: bool,
}

/// An offscreen 3D renderer with a scene, drawn a frame at a time.
#[doc(hidden)]
pub struct RecordBench {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: wgpu::TextureView,
    textures: Vec<GpuTexture>,
    renderer: Renderer3D,
    ctx: Ctx,
    frame: Frame3D,
    size: (u32, u32),
    frames: u32,
    adapter: String,
}

/// The texture bind group layout the 3D renderer is built with, a sampler
/// for it, and texture 0: one white pixel. Shared by the offscreen benchmarks.
pub(super) fn white_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::BindGroupLayout, wgpu::Sampler, GpuTexture) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let white = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
    });
    queue.write_texture(
        white.as_image_copy(),
        &[255; 4],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        white.size(),
    );
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    let view = white.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
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
    (
        layout,
        sampler,
        GpuTexture {
            texture: white,
            nearest: false,
            mips: None,
            bind_group,
        },
    )
}

impl RecordBench {
    /// `None` without a GPU.
    #[must_use]
    pub fn new(scene: RecordScene) -> Option<Self> {
        // A test takes one of the places GPU tests share (render_test_support).
        #[cfg(all(test, feature = "window", feature = "3d"))]
        crate::render::render_test_support::take_gpu_place();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
        let (layout, _sampler, white) = white_texture(&device, &queue);
        let textures = vec![white];
        let white = textures[0].texture.clone();
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: scene.size.0,
                height: scene.size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target = output.create_view(&Default::default());
        let mut renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true);
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(scene.size.0 as f32, scene.size.1 as f32));
        ctx.scene3d.sky.atmosphere = scene.effects;
        if scene.effects {
            ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
            ctx.scene3d.post.ambient_occlusion.enabled = true;
            ctx.scene3d.post.ssr = 1.0;
        }
        let meshes: Vec<Mesh> = (0..scene.meshes.max(1))
            .map(|i| {
                let desc = MaterialDesc {
                    color: crate::color::Color::rgb(0.3 + 0.7 * (i % 7) as f32 / 7.0, 0.6, 0.5),
                    ..Default::default()
                };
                let data = MeshData::cube(0.3 + 0.01 * (i % 16) as f32);
                let material = ctx.gfx.create_material(desc);
                let mesh = ctx.gfx.create_mesh(&data, material);
                renderer.create_material(&device, &GpuMaterialDesc::new(&desc), &textures);
                renderer.create_mesh_at(
                    &device,
                    mesh.slot(),
                    &data,
                    &[(0..data.indices.len() as u32, material)],
                    true,
                );
                mesh
            })
            .collect();
        let side = (scene.models as f32).sqrt().ceil().max(1.0) as usize;
        for i in 0..scene.models {
            let (x, z) = ((i % side) as f32, (i / side) as f32);
            let at = Vec3::new((x - side as f32 * 0.5) * 1.5, 0.0, -z * 1.5);
            let mesh = meshes[(i as u32).wrapping_mul(2_654_435_761) as usize % meshes.len()];
            ctx.world.spawn((Transform3D::at(at), Model::new(mesh)));
        }
        ctx.scene3d.camera.position = Vec3::new(0.0, side as f32 * 0.5, side as f32 * 0.4);
        ctx.scene3d.camera.look_at(Vec3::new(0.0, 0.0, -(side as f32) * 0.6));
        ctx.scene3d.camera.far = side as f32 * 4.0;
        Some(Self {
            device,
            queue,
            target,
            textures,
            renderer,
            ctx,
            frame: Frame3D::default(),
            size: scene.size,
            frames: 0,
            adapter: format!("{} ({:?})", info.name, info.backend),
        })
    }

    /// The GPU the frames are recorded for: its name and backend.
    #[must_use]
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Prepares and draws one frame, submits it and waits for the GPU.
    /// Returns how long recording took (`Renderer3D::draw`: every pass into
    /// its command buffer) and how many command buffers that made.
    pub fn frame(&mut self) -> (Duration, usize) {
        let ctx = &mut self.ctx;
        // The camera drifts, as a game's does: every frame's uniforms change.
        ctx.scene3d.camera.position.x = (self.frames as f32 * 0.01).sin();
        let mut frame = std::mem::take(&mut self.frame);
        frame.lod = ctx.scene3d.lod;
        frame.motion.enabled = ctx.scene3d.post.anti_aliasing == AntiAliasing::Taa
            || (ctx.scene3d.post.ao_temporal && ctx.scene3d.post.ambient_occlusion.enabled);
        frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &SkinFrame::default(),
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        let scene = Scene3D {
            frame: &frame,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            light: SkyLight {
                sun: Vec3::splat(ctx.scene3d.lighting.sun_intensity),
                sky: Vec3::splat(0.2),
                ground: Vec3::splat(0.1),
            },
            camera: ctx.scene3d.camera.position,
            post: &ctx.scene3d.post,
            time: (self.frames as f64 / 60.0, 1.0 / 60.0),
            occlusion: ctx.scene3d.camera.occlusion_culling,
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
        let start = Instant::now();
        let buffers = self
            .renderer
            .draw(&self.device, &self.target, wgpu::Color::BLACK, &self.textures, None);
        let took = start.elapsed();
        let count = buffers.len();
        self.queue.submit(buffers);
        self.renderer.submitted();
        // Not timed: keeps the GPU's queue from growing over the run.
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        self.frame = frame;
        self.frames += 1;
        (took, count)
    }
}
