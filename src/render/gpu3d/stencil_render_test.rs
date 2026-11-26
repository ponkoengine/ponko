//! Stencil state on materials (`stencil.rs`): overlapping transparent strips
//! with `Stencil::once` blend once where they cross, an outline with
//! `Stencil::outside` shows only around the object that marked the stencil,
//! and a scene without stencil materials keeps its plain depth target. Each
//! check has a negative control without the stencil. Skips without a GPU.
use super::*;
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::gpu::GpuTexture;
use crate::render::{Blend, Material, MaterialDesc, Mesh, MeshData, Model, Stencil};
use crate::three::{Frame3D, Scene3D, SkinFrame, SkyLight};

const SIZE: u32 = 128;

/// An offscreen renderer with a 1x1 white texture, and a `Ctx` to fill.
struct Rig {
    device: wgpu::Device,
    queue: wgpu::Queue,
    output: wgpu::Texture,
    target: wgpu::TextureView,
    textures: Vec<GpuTexture>,
    renderer: Renderer3D,
    ctx: Ctx,
    frame: Frame3D,
    errors: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl Rig {
    /// `None` without a GPU.
    fn new(samples: u32) -> Option<Self> {
        Self::build(wgpu::Limits::default(), samples, false)
    }

    /// A rig on WebGL2's path: its limits, one sample, and the scene depth
    /// read as a float texture with a copy for the transparent pass, as on
    /// the GL backend (`shaders::reads_depth_as_float`).
    fn webgl2() -> Option<Self> {
        Self::build(wgpu::Limits::downlevel_webgl2_defaults(), 1, true)
    }

    fn build(limits: wgpu::Limits, samples: u32, gl_depth: bool) -> Option<Self> {
        let gpu = crate::render::render_test_support::gpu_with(move |_| {
            Ok(wgpu::DeviceDescriptor {
                required_limits: limits,
                ..Default::default()
            })
        })?;
        let (device, queue) = (gpu.device, gpu.queue);
        let errors = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = errors.clone();
        device.on_uncaptured_error(std::sync::Arc::new(move |e: wgpu::Error| {
            sink.lock().unwrap().push(format!("GPU error: {e}"));
        }));
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
        let sampler = device.create_sampler(&Default::default());
        let view = white.create_view(&Default::default());
        let textures = vec![GpuTexture {
            texture: white.clone(),
            nearest: false,
            mips: None,
            bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
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
            }),
        }];
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
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
        super::shaders::FORCE_FLOAT_DEPTH.set(gl_depth);
        let renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, samples, true);
        super::shaders::FORCE_FLOAT_DEPTH.set(false);
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
        ctx.scene3d.sky.atmosphere = false;
        ctx.scene3d.lighting.shadows = false;
        ctx.scene3d.camera.position = vec3(0.0, 0.0, 8.0);
        ctx.scene3d.camera.look_at(Vec3::ZERO);
        Some(Self {
            device,
            queue,
            output,
            target,
            textures,
            renderer,
            ctx,
            frame: Frame3D::default(),
            errors,
        })
    }

    /// A material on the CPU and the GPU.
    fn material(&mut self, desc: MaterialDesc) -> Material {
        let material = self.ctx.gfx.create_material(desc);
        self.renderer
            .create_material(&self.device, &crate::three::GpuMaterialDesc::new(&desc), &self.textures);
        material
    }

    /// A cube mesh drawn with `desc`, on the CPU and the GPU.
    fn cube(&mut self, desc: MaterialDesc) -> Mesh {
        let material = self.material(desc);
        let data = MeshData::cube(1.0);
        let mesh = self.ctx.gfx.create_mesh(&data, material);
        self.renderer
            .create_mesh(&self.device, &data, &[(0..data.indices.len() as u32, material)], true);
        mesh
    }

    fn spawn(&mut self, mesh: Mesh, at: Vec3, scale: Vec3) {
        let transform = Transform3D {
            scale,
            ..Transform3D::at(at)
        };
        self.ctx.world.spawn((transform, Model::new(mesh)));
    }

    /// Draws the world (a few frames, so it settles) and reads the pixels back (RGBA).
    fn draw(&mut self) -> Vec<u8> {
        for _ in 0..3 {
            self.render();
        }
        let pixels = self.read();
        let errors = self.errors.lock().unwrap();
        assert!(errors.is_empty(), "the GPU failed: {errors:?}");
        pixels
    }

    fn render(&mut self) {
        let ctx = &mut self.ctx;
        let frame = &mut self.frame;
        frame.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &SkinFrame::default(),
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        let scene = Scene3D {
            frame,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            light: SkyLight {
                sun: Vec3::splat(ctx.scene3d.lighting.sun_intensity),
                sky: Vec3::splat(0.3),
                ground: Vec3::splat(0.1),
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
                size: (SIZE, SIZE),
            },
        );
        let buffers = self
            .renderer
            .draw(&self.device, &self.target, wgpu::Color::BLACK, &self.textures, None);
        self.queue.submit(buffers);
    }

    fn read(&self) -> Vec<u8> {
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIZE * SIZE * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIZE * 4),
                    rows_per_image: None,
                },
            },
            self.output.size(),
        );
        self.queue.submit([encoder.finish()]);
        readback.map_async(wgpu::MapMode::Read, .., |r| r.unwrap());
        self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        readback.get_mapped_range(..).unwrap().to_vec()
    }

    /// The pixel (RGB) where world point `at` shows.
    fn at(&self, pixels: &[u8], at: Vec3) -> [u8; 3] {
        let ndc = self.ctx.scene3d.camera.view_proj().project_point3(at);
        let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as u32;
        let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as u32;
        let i = ((y * SIZE + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    }
}

/// A material that shows `color` whatever the light.
fn glowing(color: Color) -> MaterialDesc {
    MaterialDesc {
        color: Color::rgb(0.0, 0.0, 0.0),
        emissive: color,
        roughness: 1.0,
        ..Default::default()
    }
}

/// Two see-through strips crossing in a plus, and the pixels where only the
/// horizontal one shows and where both do.
fn crossing_strips(samples: u32, stencil: Option<Stencil>) -> Option<([u8; 3], [u8; 3])> {
    strips_in(&mut Rig::new(samples)?, stencil)
}

fn strips_in(rig: &mut Rig, stencil: Option<Stencil>) -> Option<([u8; 3], [u8; 3])> {
    let glass = MaterialDesc {
        color: Color::rgba(0.0, 0.0, 0.0, 0.5),
        emissive: Color::rgb(0.6, 0.0, 0.0),
        blend: Blend::Transparent,
        stencil,
        ..Default::default()
    };
    let strip = rig.cube(glass);
    rig.spawn(strip, Vec3::ZERO, vec3(5.0, 0.8, 0.05));
    rig.spawn(strip, vec3(0.0, 0.0, 0.5), vec3(0.8, 5.0, 0.05));
    let pixels = rig.draw();
    Some((rig.at(&pixels, vec3(1.8, 0.0, 0.0)), rig.at(&pixels, Vec3::ZERO)))
}

/// Whether the crossing looks like one strip: every channel within 3 levels.
/// The two pixels are at different places on screen, and some adapters
/// shade them a level apart in green and blue (11 against 10, measured on
/// a non-Apple GPU); a second blend adds more than 20 to red (the negative
/// controls check that).
fn blend_once(single: [u8; 3], crossed: [u8; 3]) -> bool {
    single.iter().zip(crossed).all(|(&a, b)| a.abs_diff(b) <= 3)
}

#[test]
fn overlapping_transparent_strips_with_a_stencil_blend_once() {
    for samples in [1, 4] {
        let Some((single, crossed)) = crossing_strips(samples, None) else {
            return;
        };
        eprintln!("{samples} samples, no stencil: one strip {single:?}, both {crossed:?}");
        // Negative control: without a stencil the crossing blends twice.
        assert!(
            crossed[0] > single[0] + 20,
            "without a stencil, the crossing is brighter: {single:?} vs {crossed:?}"
        );
        let (single, crossed) = crossing_strips(samples, Some(Stencil::once(1))).unwrap();
        eprintln!("{samples} samples, Stencil::once: one strip {single:?}, both {crossed:?}");
        assert!(single[0] > 40, "the strip shows: {single:?}");
        assert!(
            blend_once(single, crossed),
            "with Stencil::once, the crossing blends once: {single:?} vs {crossed:?}"
        );
    }
}

/// A blue cube marking the stencil, and a larger yellow copy around it
/// with `outline`'s stencil, drawn after it. The pixels at the middle (the
/// cube) and beside it (the outline's rim).
fn outlined_cube(samples: u32, outline: Option<Stencil>) -> Option<([u8; 3], [u8; 3], [u8; 3])> {
    let mut rig = Rig::new(samples)?;
    // The rim's mesh first, so without stencil orders it would draw first.
    let rim = rig.cube(MaterialDesc {
        stencil: outline,
        ..glowing(Color::rgb(1.0, 1.0, 0.0))
    });
    let body = rig.cube(glowing(Color::rgb(0.0, 0.0, 1.0)).with_stencil(Stencil::mark(1)));
    rig.spawn(body, Vec3::ZERO, Vec3::splat(2.0));
    rig.spawn(rim, Vec3::ZERO, Vec3::splat(2.6));
    let pixels = rig.draw();
    Some((
        rig.at(&pixels, Vec3::ZERO),
        rig.at(&pixels, vec3(1.15, 0.0, 1.3)),
        rig.at(&pixels, vec3(2.5, 0.0, 0.0)),
    ))
}

#[test]
fn a_stencil_outline_shows_only_around_the_marked_object() {
    for samples in [1, 4] {
        // Negative control: an outline without a stencil hides the cube.
        let Some((middle, _, _)) = outlined_cube(samples, None) else {
            return;
        };
        assert!(
            middle[0] > 150 && middle[2] < 80,
            "without a stencil the yellow copy covers the cube: {middle:?}"
        );
        let (middle, rim, outside) = outlined_cube(samples, Some(Stencil::outside(1).with_order(1))).unwrap();
        eprintln!("{samples} samples: middle {middle:?}, rim {rim:?}, outside {outside:?}");
        assert!(
            middle[2] > 150 && middle[0] < 80,
            "the marked cube shows through: {middle:?}"
        );
        assert!(
            rim[0] > 150 && rim[1] > 150 && rim[2] < 80,
            "the outline's rim shows: {rim:?}"
        );
        assert_eq!(outside, [0, 0, 0], "nothing beyond the outline");
    }
}

#[test]
fn solid_stencil_materials_draw_in_their_order() {
    let Some(mut rig) = Rig::new(4) else {
        return;
    };
    // The outline asks to be drawn before the cube marks the stencil: it then
    // covers the cube (there's no mark yet where it tests).
    let rim = rig.cube(glowing(Color::rgb(1.0, 1.0, 0.0)).with_stencil(Stencil::outside(1).with_order(-1)));
    let body = rig.cube(glowing(Color::rgb(0.0, 0.0, 1.0)).with_stencil(Stencil::mark(1)));
    rig.spawn(body, Vec3::ZERO, Vec3::splat(2.0));
    rig.spawn(rim, Vec3::ZERO, Vec3::splat(2.6));
    let pixels = rig.draw();
    let middle = rig.at(&pixels, Vec3::ZERO);
    assert!(
        middle[0] > 150 && middle[2] < 80,
        "the earlier outline covers the cube: {middle:?}"
    );
}

#[test]
fn scenes_without_stencil_materials_keep_a_plain_depth_target() {
    let Some(mut rig) = Rig::new(4) else {
        return;
    };
    let plain = rig.cube(glowing(Color::rgb(0.0, 1.0, 0.0)));
    rig.spawn(plain, Vec3::ZERO, Vec3::ONE);
    let before = rig.draw();
    let plain_depth = wgpu::TextureFormat::Depth32Float;
    assert_eq!(rig.renderer.shaders.depth_format(), plain_depth);
    let targets = rig.renderer.targets.as_ref().unwrap();
    assert_eq!(targets.depth.texture().format(), plain_depth);
    assert!(!rig.renderer.stencil.any());

    // The first stencil material gives the scene depth its stencil bits;
    // the plain cube draws as before.
    let marked = rig.cube(glowing(Color::rgb(0.0, 0.0, 1.0)).with_stencil(Stencil::mark(1)));
    rig.spawn(marked, vec3(3.0, 0.0, 0.0), Vec3::ONE);
    let after = rig.draw();
    assert_eq!(rig.renderer.shaders.depth_format(), stencil::STENCIL_FORMAT);
    let targets = rig.renderer.targets.as_ref().unwrap();
    assert_eq!(targets.depth.texture().format(), stencil::STENCIL_FORMAT);
    assert_eq!(rig.at(&before, Vec3::ZERO), rig.at(&after, Vec3::ZERO));
    assert!(rig.at(&after, vec3(3.0, 0.0, 0.0))[2] > 150, "the stencil cube draws");
}

/// A transparent material that reads the scene behind it.
const SCENE_READER: &str = "
fn surface(s: Surface) -> Surface {
    var out = s;
    out.emissive = scene_color(s.screen) * 0.5 + vec3<f32>(0.0, 0.3, 0.0);
    return out;
}";

#[test]
fn transparent_stencil_materials_draw_beside_a_scene_reading_one() {
    for samples in [1, 4] {
        let Some(mut rig) = Rig::new(samples) else {
            return;
        };
        // A see-through surface that reads the scene splits the transparent
        // pass off; there the stencil is tested but not written.
        let shader = rig.ctx.gfx.create_shader("reader", SCENE_READER);
        rig.renderer.create_shader(&rig.device, "reader", SCENE_READER).unwrap();
        let lake = rig.cube(MaterialDesc {
            color: Color::rgba(0.0, 0.0, 0.0, 0.5),
            blend: Blend::Transparent,
            shader: Some(shader),
            ..Default::default()
        });
        rig.spawn(lake, vec3(-2.5, 2.5, 0.0), Vec3::ONE);
        let glass = rig.cube(MaterialDesc {
            color: Color::rgba(0.0, 0.0, 0.0, 0.5),
            emissive: Color::rgb(0.6, 0.0, 0.0),
            blend: Blend::Transparent,
            stencil: Some(Stencil::once(1)),
            ..Default::default()
        });
        rig.spawn(glass, Vec3::ZERO, vec3(5.0, 0.8, 0.05));
        let pixels = rig.draw();
        assert!(rig.renderer.stencil_read_only, "the transparent pass was split off");
        let strip = rig.at(&pixels, vec3(1.8, 0.0, 0.0));
        assert!(strip[0] > 40, "the stencil strip shows: {strip:?}");
        assert!(rig.at(&pixels, vec3(-2.5, 2.5, 0.5))[1] > 20, "the scene reader shows");
    }
}

#[test]
fn stencil_materials_draw_on_the_webgl2_path() {
    // Negative control: without a stencil the crossing blends twice there too.
    let Some(mut rig) = Rig::webgl2() else {
        return;
    };
    assert!(rig.renderer.float_depth, "the rig takes the GL path");
    let (single, crossed) = strips_in(&mut rig, None).unwrap();
    assert!(
        crossed[0] > single[0] + 20,
        "without a stencil, the crossing is brighter: {single:?} vs {crossed:?}"
    );
    let (single, crossed) = strips_in(&mut Rig::webgl2().unwrap(), Some(Stencil::once(1))).unwrap();
    assert!(single[0] > 40, "the strip shows: {single:?}");
    assert!(
        blend_once(single, crossed),
        "with Stencil::once, the crossing blends once: {single:?} vs {crossed:?}"
    );

    // A scene reader splits the transparent pass off: the depth with its
    // stencil bits is copied and read as a float texture, and the pass
    // tests the stencil without writing it.
    let mut rig = Rig::webgl2().unwrap();
    let shader = rig.ctx.gfx.create_shader("reader", SCENE_READER);
    rig.renderer.create_shader(&rig.device, "reader", SCENE_READER).unwrap();
    let lake = rig.cube(MaterialDesc {
        color: Color::rgba(0.0, 0.0, 0.0, 0.5),
        blend: Blend::Transparent,
        shader: Some(shader),
        ..Default::default()
    });
    rig.spawn(lake, vec3(-2.5, 2.5, 0.0), Vec3::ONE);
    let (single, _) = strips_in(&mut rig, Some(Stencil::once(1))).unwrap();
    assert_eq!(rig.renderer.shaders.depth_format(), stencil::STENCIL_FORMAT);
    assert!(rig.renderer.stencil_read_only, "the transparent pass was split off");
    assert!(
        rig.renderer.targets.as_ref().unwrap().depth_copy.is_some(),
        "the depth was copied"
    );
    assert!(single[0] > 40, "the stencil strip shows: {single:?}");
}
