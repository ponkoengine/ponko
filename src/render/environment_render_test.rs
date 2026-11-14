//! Sky IBL and reflection probes, drawn offscreen on a real adapter (the
//! scene collection, bake passes and shading together). Skips without a GPU.
use super::gpu::GpuTexture;
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::d3::{ProbeRefresh, ReflectionProbe as ReflectionProbe3D, ReflectionRefresh, Transform as Transform3D};
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{MaterialDesc, Mesh, MeshData, Model, Sky};
use crate::three::{GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};

const SIZE: u32 = 128;

/// A device, a renderer and a headless context to fill it from.
struct Rig {
    device: wgpu::Device,
    queue: wgpu::Queue,
    output: wgpu::Texture,
    textures: Vec<GpuTexture>,
    renderer: Renderer3D,
    ctx: Ctx,
    registered: usize,
}

impl Rig {
    fn new() -> Option<Rig> {
        let gpu = crate::render::render_test_support::gpu_with(|_| Ok(wgpu::DeviceDescriptor::default()))?;
        let (device, queue) = (gpu.device, gpu.queue);
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
        let renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, true);
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
        // Probe captures get size-only handles; `Rig::draw` backs them.
        ctx.set_headless(true);
        ctx.scene3d.lighting.sun_intensity = 0.0;
        ctx.scene3d.lighting.shadows = false;
        ctx.scene3d.sky.atmosphere = false;
        ctx.scene3d.post.bloom = 0.0;
        ctx.scene3d.post.ambient_occlusion.enabled = false;
        Some(Rig {
            device,
            queue,
            output,
            textures,
            renderer,
            ctx,
            registered: 0,
        })
    }

    /// A mesh with its material, uploaded to the renderer.
    fn mesh(&mut self, data: &MeshData, desc: MaterialDesc) -> Mesh {
        let material = self.ctx.gfx.create_material(desc);
        let mesh = self.ctx.gfx.create_mesh(data, material);
        self.renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&desc), &self.textures);
        self.renderer
            .create_mesh(&self.device, data, &[(0..data.indices.len() as u32, material)], true);
        mesh
    }

    /// Backs the context's probe capture handles with real targets.
    fn register_captures(&mut self) {
        let captures = self.ctx.gfx.assets3d.probes.captures.clone();
        for faces in &captures[self.registered.min(captures.len())..] {
            for face in faces {
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("test capture"),
                    size: wgpu::Extent3d {
                        width: 128,
                        height: 128,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: HDR_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_DST
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                self.renderer.add_render_target(face.id, &texture);
            }
        }
        self.registered = captures.len();
    }

    /// One frame, read back as RGBA8.
    fn draw(&mut self) -> Vec<u8> {
        self.register_captures();
        let skins = SkinFrame::default();
        self.ctx.collect_views(&skins);
        // A refreshing probe gets its slot while the views are collected.
        self.register_captures();
        let ctx = &mut self.ctx;
        ctx.gfx.frame3d.collect(
            &mut ctx.world,
            &ctx.gfx.assets3d,
            &skins,
            &ctx.scene3d.camera,
            &ctx.scene3d.lighting,
        );
        let rgb = |c| {
            let [r, g, b, _] = crate::three::linear(c);
            Vec3::new(r, g, b)
        };
        let scene = Scene3D {
            frame: &ctx.gfx.frame3d,
            lighting: &ctx.scene3d.lighting,
            sky: &ctx.scene3d.sky,
            light: if ctx.scene3d.sky.atmosphere {
                SkyLight::new(&ctx.scene3d.lighting, &ctx.scene3d.sky)
            } else {
                SkyLight {
                    sun: Vec3::ZERO,
                    sky: rgb(ctx.scene3d.sky.sky_color),
                    ground: rgb(ctx.scene3d.sky.ground_color),
                }
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
        let target = self.output.create_view(&Default::default());
        let buffers = self
            .renderer
            .draw(&self.device, &target, wgpu::Color::BLACK, &self.textures, None);
        self.queue.submit(buffers);
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
}

fn screen(view_proj: Mat4, p: Vec3) -> Vec2 {
    let ndc = view_proj.project_point3(p);
    Vec2::new((ndc.x * 0.5 + 0.5) * SIZE as f32, (0.5 - ndc.y * 0.5) * SIZE as f32)
}

fn rgb(pixels: &[u8], x: u32, y: u32) -> Vec3 {
    let i = ((y * SIZE + x) * 4) as usize;
    Vec3::new(f32::from(pixels[i]), f32::from(pixels[i + 1]), f32::from(pixels[i + 2]))
}

fn matte(color: Color) -> MaterialDesc {
    MaterialDesc {
        color,
        roughness: 1.0,
        ..Default::default()
    }
}

fn mirror(color: Color) -> MaterialDesc {
    MaterialDesc {
        color,
        metallic: 1.0,
        roughness: 0.04,
        ..Default::default()
    }
}

fn glowing(r: f32) -> MaterialDesc {
    glowing_colour(Color::rgb(r, 0.0, 0.0))
}

fn glowing_colour(emissive: Color) -> MaterialDesc {
    MaterialDesc {
        color: Color::BLACK,
        emissive,
        roughness: 1.0,
        ..Default::default()
    }
}

#[test]
fn a_rough_spheres_ambient_follows_the_sky_colour() {
    let Some(mut rig) = Rig::new() else { return };
    let sphere = MeshData::sphere(1.5, 32);
    let mesh = rig.mesh(&sphere, matte(Color::WHITE));
    rig.ctx.world.spawn((Transform3D::default(), Model::new(mesh)));
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, -6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.ctx.scene3d.sky.ground_color = Color::BLACK;
    let top = |rig: &mut Rig| {
        let view_proj = rig.ctx.scene3d.camera.view_proj();
        let p = screen(view_proj, vec3(0.0, 1.0, -1.0));
        let pixels = rig.draw();
        rgb(&pixels, p.x as u32, p.y as u32)
    };
    rig.ctx.scene3d.sky.sky_color = Color::rgb(1.0, 0.1, 0.1);
    let red = top(&mut rig);
    rig.ctx.scene3d.sky.sky_color = Color::rgb(0.1, 0.1, 1.0);
    let blue = top(&mut rig);
    assert!(red.x > 4.0 * red.z.max(1.0), "a red sky lights the sphere red: {red}");
    assert!(
        blue.z > 4.0 * blue.x.max(1.0),
        "a blue sky lights the sphere blue: {blue}"
    );
    rig.ctx.scene3d.lighting.ambient_intensity = 0.0;
    let dark = top(&mut rig);
    assert!(dark.max_element() < 3.0, "no ambient, no light: {dark}");
}

#[test]
fn a_three_color_ambient_lights_the_sides_with_the_equator_color() {
    let Some(mut rig) = Rig::new() else { return };
    let sphere = MeshData::sphere(1.5, 32);
    let mesh = rig.mesh(&sphere, matte(Color::WHITE));
    rig.ctx.world.spawn((Transform3D::default(), Model::new(mesh)));
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, -6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.ctx.scene3d.sky.sky_color = Color::rgb(1.0, 0.05, 0.05);
    rig.ctx.scene3d.sky.ground_color = Color::rgb(0.05, 0.05, 1.0);
    // The sphere's top, the side facing the camera, and its bottom.
    let points = |rig: &mut Rig| {
        let view_proj = rig.ctx.scene3d.camera.view_proj();
        let pixels = rig.draw();
        [vec3(0.0, 1.35, -0.6), vec3(0.0, 0.0, -1.5), vec3(0.0, -1.35, -0.6)].map(|p| {
            let p = screen(view_proj, p);
            rgb(&pixels, p.x as u32, p.y as u32)
        })
    };
    let [_, two_color_side, _] = points(&mut rig);
    assert!(
        two_color_side.y < 0.3 * two_color_side.x.max(two_color_side.z),
        "without an equator, the side is sky and ground mixed: {two_color_side}"
    );
    rig.ctx.scene3d.sky.equator_color = Some(Color::rgb(0.05, 1.0, 0.05));
    let [top, side, bottom] = points(&mut rig);
    assert!(
        top.x > top.y && top.x > 2.0 * top.z,
        "the top is lit most by the sky: {top}"
    );
    // (In sRGB after tone mapping, so the gaps look smaller than they are.)
    assert!(side.y > side.x.max(side.z) + 40.0, "the side by the horizon: {side}");
    assert!(
        bottom.z > bottom.y && bottom.z > 2.0 * bottom.x,
        "the bottom by the ground: {bottom}"
    );
}

/// How many red pixels the sphere's disc has on each side of its middle, as
/// (screen left, screen right), plus how many are blue.
fn sphere_redness(rig: &mut Rig) -> (f32, f32, f32) {
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let center = screen(view_proj, Vec3::ZERO);
    let radius = (screen(view_proj, vec3(1.5, 0.0, 0.0)) - center).length() * 0.8;
    let pixels = rig.draw();
    let (mut left, mut right, mut blue) = (0.0, 0.0, 0.0);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let at = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            if (at - center).length() > radius {
                continue;
            }
            let c = rgb(&pixels, x, y);
            if c.x > c.z + 60.0 {
                if at.x < center.x {
                    left += 1.0;
                } else {
                    right += 1.0;
                }
            }
            if c.z > c.x + 60.0 {
                blue += 1.0;
            }
        }
    }
    (left, right, blue)
}

#[test]
fn a_probe_reflects_a_bright_wall_inside_its_box_and_the_sky_outside() {
    let Some(mut rig) = Rig::new() else { return };
    // The camera looks along +Z, so +X is on the screen's left.
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, -6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.ctx.scene3d.sky.sky_color = Color::rgb(0.3, 0.4, 0.8);
    rig.ctx.scene3d.sky.ground_color = Color::rgb(0.3, 0.4, 0.8);
    let wall = rig.mesh(&MeshData::cube(1.0), glowing(6.0));
    let wall_transform = Transform3D {
        scale: vec3(0.2, 8.0, 8.0),
        ..Transform3D::at(vec3(4.0, 0.0, 0.0))
    };
    rig.ctx.world.spawn((wall_transform, Model::new(wall)));
    let probe = rig.ctx.world.spawn((
        Transform3D::default(),
        ReflectionProbe3D {
            size: Vec3::splat(10.0),
            blend_distance: 0.0,
            ..Default::default()
        },
    ));
    rig.ctx.gfx.bake_reflection_probe(&mut rig.ctx.world, probe).unwrap();
    rig.draw();
    // The ball goes in after the capture, so it doesn't hide the room.
    let ball = rig.mesh(&MeshData::sphere(1.5, 32), mirror(Color::WHITE));
    rig.ctx.world.spawn((Transform3D::default(), Model::new(ball)));
    let (left, right, _) = sphere_redness(&mut rig);
    assert!(
        left > 100.0 && right < 5.0,
        "the wall at +X shows on the ball's +X side: left {left}, right {right}"
    );
    // Shrink the box so the ball is outside it: the sky is all there is.
    rig.ctx.world.get_mut::<ReflectionProbe3D>(probe).unwrap().size = Vec3::splat(0.5);
    let (left, right, blue) = sphere_redness(&mut rig);
    assert!(
        left < 5.0 && right < 5.0,
        "outside the box the wall is gone: left {left}, right {right}"
    );
    assert!(blue > 1000.0, "outside the box the ball reflects the blue sky: {blue}");
}

/// The red weight centre of the floor's reflection, ignoring the patch seen directly.
fn reflection_centre(rig: &mut Rig, patch: &[Vec3]) -> Option<Vec2> {
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let corners: Vec<Vec2> = patch.iter().map(|&p| screen(view_proj, p)).collect();
    let low = corners.iter().fold(Vec2::splat(f32::MAX), |a, &c| a.min(c)) - 3.0;
    let high = corners.iter().fold(Vec2::splat(f32::MIN), |a, &c| a.max(c)) + 3.0;
    let pixels = rig.draw();
    let (mut sum, mut total) = (Vec2::ZERO, 0.0);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let at = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            if at.cmpge(low).all() && at.cmple(high).all() {
                continue;
            }
            let c = rgb(&pixels, x, y);
            let red = (c.x - c.y.max(c.z)).max(0.0);
            sum += at * red;
            total += red;
        }
    }
    (total > 200.0).then(|| sum / total)
}

#[test]
fn box_projection_moves_the_reflection_where_infinite_projection_puts_it_elsewhere() {
    let Some(mut rig) = Rig::new() else { return };
    // A mirror floor at y = 0 in a 16 x 10 x 16 room whose middle is at y = 3,
    // and a glowing patch high on the +X wall. The camera looks across the
    // floor towards that wall.
    rig.ctx.scene3d.camera.position = vec3(-4.0, 3.0, 0.0);
    rig.ctx.scene3d.camera.look_at(vec3(2.5, 0.0, 1.0));
    rig.ctx.scene3d.sky.sky_color = Color::rgb(0.05, 0.05, 0.05);
    rig.ctx.scene3d.sky.ground_color = Color::rgb(0.05, 0.05, 0.05);
    let at = vec3(7.8, 5.5, 1.5);
    let patch = rig.mesh(&MeshData::cube(1.0), glowing(8.0));
    let patch_transform = Transform3D {
        scale: vec3(0.2, 1.0, 1.0),
        ..Transform3D::at(at)
    };
    rig.ctx.world.spawn((patch_transform, Model::new(patch)));
    let probe = rig.ctx.world.spawn((
        Transform3D::at(vec3(0.0, 3.0, 0.0)),
        ReflectionProbe3D {
            size: vec3(16.0, 10.0, 16.0),
            blend_distance: 0.0,
            ..Default::default()
        },
    ));
    rig.ctx.gfx.bake_reflection_probe(&mut rig.ctx.world, probe).unwrap();
    rig.draw();
    // The floor goes in after the capture, so the capture is of the room alone.
    let floor = rig.mesh(&MeshData::plane(30.0), mirror(Color::WHITE));
    rig.ctx.world.spawn((Transform3D::default(), Model::new(floor)));
    // The patch's eight corners, for masking it where the camera sees it.
    let corners: Vec<Vec3> = (0..8u32)
        .map(|i| {
            let sign = |bit: u32| if i & bit == 0 { -0.5 } else { 0.5 };
            at + vec3(0.2 * sign(1), sign(2), sign(4))
        })
        .collect();
    let boxed = reflection_centre(&mut rig, &corners).expect("the floor reflects the patch");
    rig.ctx
        .world
        .get_mut::<ReflectionProbe3D>(probe)
        .unwrap()
        .box_projection = false;
    let infinite = reflection_centre(&mut rig, &corners).expect("the floor reflects the patch");
    // A mirror shows a point where its mirror image through the floor is.
    let image = screen(rig.ctx.scene3d.camera.view_proj(), vec3(at.x, -at.y, at.z));
    assert!(
        boxed.distance(infinite) > 10.0,
        "box projection shifts the reflection: {boxed} vs {infinite}"
    );
    assert!(
        boxed.distance(image) < 6.0 && boxed.distance(image) * 3.0 < infinite.distance(image),
        "box projection puts it at the true mirror image {image}: {boxed} vs {infinite}"
    );
}

#[test]
fn a_loaded_equirect_image_lights_the_ball_from_the_right_direction() {
    let Some(mut rig) = Rig::new() else { return };
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, -6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.ctx.scene3d.sky.sky_color = Color::rgb(0.05, 0.05, 0.05);
    rig.ctx.scene3d.sky.ground_color = Color::rgb(0.05, 0.05, 0.05);
    // A bright patch from +X (the image's middle) a tenth of the way back
    // towards -Z, around the horizon: it lies off to one side of the middle.
    let mut image = image::Rgb32FImage::from_pixel(128, 64, image::Rgb([0.0, 0.0, 0.5]));
    for y in 0..64u32 {
        for x in 0..128u32 {
            let (u, v) = (x as f32 / 128.0 - 0.5, y as f32 / 64.0 - 0.5);
            if (-0.1..0.0).contains(&u) && v.abs() < 0.2 {
                image.put_pixel(x, y, image::Rgb([8.0, 0.0, 0.0]));
            }
        }
    }
    let path = std::env::temp_dir().join(format!("ponko-probe-{}.exr", std::process::id()));
    image.save(&path).unwrap();
    let probe = rig.ctx.world.spawn((
        Transform3D::default(),
        ReflectionProbe3D {
            blend_distance: 0.0,
            box_projection: false,
            ..Default::default()
        },
    ));
    rig.ctx
        .gfx
        .try_load_reflection_probe(&mut rig.ctx.world, probe, path.to_str().unwrap())
        .unwrap();
    std::fs::remove_file(&path).ok();
    let ball = rig.mesh(&MeshData::sphere(1.5, 32), mirror(Color::WHITE));
    rig.ctx.world.spawn((Transform3D::default(), Model::new(ball)));
    let (left, right, blue) = sphere_redness(&mut rig);
    assert!(
        left > 100.0 && right < 5.0,
        "the patch shows on the ball's +X side: left {left}, right {right}"
    );
    assert!(blue > 500.0, "the rest of the image is blue: {blue}");
    // Its direction matters, not only its side: from the camera at -Z the
    // patch reflects between the ball's middle and 45 degrees round it.
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let (centre, edge) = (screen(view_proj, Vec3::ZERO), screen(view_proj, vec3(1.5, 0.0, 0.0)));
    let pixels = rig.draw();
    let radius = (edge - centre).length();
    let ring = |from: f32, to: f32| -> usize {
        (0..SIZE)
            .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let d = (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - centre).length() / radius;
                let c = rgb(&pixels, x, y);
                (from..to).contains(&d) && c.x > c.z + 60.0
            })
            .count()
    };
    assert!(
        ring(0.45, 0.7) > 3 * ring(0.75, 0.9),
        "the patch is where its direction puts it: {} inside vs {} outside",
        ring(0.45, 0.7),
        ring(0.75, 0.9)
    );
}

#[test]
fn a_probe_inside_a_bigger_one_wins_and_the_bigger_one_shows_around_it() {
    let Some(mut rig) = Rig::new() else { return };
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, -6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.ctx.scene3d.sky.sky_color = Color::rgb(0.02, 0.02, 0.02);
    rig.ctx.scene3d.sky.ground_color = Color::rgb(0.02, 0.02, 0.02);
    let wall = Transform3D {
        scale: vec3(0.2, 8.0, 8.0),
        ..Transform3D::at(vec3(4.0, 0.0, 0.0))
    };
    let probe = |size: f32| {
        (
            Transform3D::default(),
            ReflectionProbe3D {
                size: Vec3::splat(size),
                blend_distance: 0.0,
                ..Default::default()
            },
        )
    };
    // The big room's wall glows red; then the wall changes, and the small
    // room inside it is captured with a blue one.
    let red = rig.mesh(&MeshData::cube(1.0), glowing(6.0));
    let wall_entity = rig.ctx.world.spawn((wall, Model::new(red)));
    let big = rig.ctx.world.spawn(probe(12.0));
    rig.ctx.gfx.bake_reflection_probe(&mut rig.ctx.world, big).unwrap();
    rig.draw();
    rig.ctx.world.despawn(wall_entity);
    let blue = rig.mesh(&MeshData::cube(1.0), glowing_colour(Color::rgb(0.0, 0.0, 6.0)));
    rig.ctx.world.spawn((wall, Model::new(blue)));
    let small = rig.ctx.world.spawn(probe(9.0));
    rig.ctx.gfx.bake_reflection_probe(&mut rig.ctx.world, small).unwrap();
    rig.draw();
    let ball = rig.mesh(&MeshData::sphere(1.5, 32), mirror(Color::WHITE));
    rig.ctx.world.spawn((Transform3D::default(), Model::new(ball)));
    let (red_left, _, blue_pixels) = sphere_redness(&mut rig);
    assert!(
        blue_pixels > 100.0 && red_left < 5.0,
        "inside both, the smaller probe wins: red {red_left}, blue {blue_pixels}"
    );
    rig.ctx.world.get_mut::<ReflectionProbe3D>(small).unwrap().size = Vec3::splat(0.5);
    let (red_left, _, blue_pixels) = sphere_redness(&mut rig);
    assert!(
        red_left > 100.0 && blue_pixels < 5.0,
        "outside the smaller one, the bigger probe shows: red {red_left}, blue {blue_pixels}"
    );
}

#[test]
fn the_atmosphere_lights_a_rough_sphere_and_follows_the_sun_down() {
    let Some(mut rig) = Rig::new() else { return };
    let ball = rig.mesh(&MeshData::sphere(1.5, 32), matte(Color::WHITE));
    rig.ctx.world.spawn((Transform3D::default(), Model::new(ball)));
    // The camera is on the -X side, where the low sun (in +X) doesn't light
    // the ball directly: what's seen is the sky's light alone.
    rig.ctx.scene3d.camera.position = vec3(-6.0, 0.0, 0.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.ctx.scene3d.sky.atmosphere = true;
    rig.ctx.scene3d.sky.night_light = Color::BLACK;
    rig.ctx.scene3d.lighting.sun_intensity = 3.0;
    let centre = |rig: &mut Rig| {
        let pixels = rig.draw();
        rgb(&pixels, SIZE / 2, SIZE / 2)
    };
    rig.ctx.scene3d.lighting.sun_direction = vec3(-1.0, -0.3, 0.0);
    let day = centre(&mut rig);
    assert!(
        day.z > 30.0 && day.z >= day.x,
        "the daytime sky lights the ball blue-ish: {day}"
    );
    rig.ctx.scene3d.lighting.sun_direction = vec3(-1.0, 0.5, 0.0);
    let night = centre(&mut rig);
    assert!(
        night.max_element() * 3.0 < day.max_element(),
        "the baked sky follows the sun below the horizon: {day} then {night}"
    );
}

/// With the sun overhead the sky is invariant under azimuth. Compare the
/// seam at -X to +X at exactly the same elevation and pixel coordinates.
#[test]
fn the_atmosphere_has_no_dark_streak_at_negative_x() {
    let Some(mut rig) = Rig::new() else { return };
    // Keep the 3D frame active; the floor stays below the sampled sky.
    let floor = rig.mesh(&MeshData::plane(100.0), matte(Color::BLACK));
    rig.ctx
        .world
        .spawn((Transform3D::at(vec3(0.0, -2.0, 0.0)), Model::new(floor)));
    rig.ctx.scene3d.sky.atmosphere = true;
    rig.ctx.scene3d.sky.stars = 0.0;
    rig.ctx.scene3d.sky.night_light = Color::BLACK;
    rig.ctx.scene3d.lighting.sun_direction = Vec3::NEG_Y;
    rig.ctx.scene3d.lighting.sun_intensity = 3.0;
    rig.ctx.scene3d.camera.position = Vec3::ZERO;
    rig.ctx.scene3d.camera.look_at(vec3(1.0, 0.3, 0.0));
    let reference = rig.draw();
    rig.ctx.scene3d.camera.look_at(vec3(-1.0, 0.3, 0.0));
    let seam = rig.draw();
    for pixels in [&reference, &seam] {
        super::render_test_support::keep_image(&super::render_test_support::Image {
            width: SIZE,
            height: SIZE,
            rgba: pixels.clone(),
        });
    }
    let difference = |pixels: &[u8]| {
        let mut maximum = 0.0f32;
        for y in 8..SIZE / 2 {
            for x in SIZE / 2 - 2..SIZE / 2 + 2 {
                maximum = maximum.max((rgb(pixels, x, y) - rgb(&reference, x, y)).abs().max_element());
            }
        }
        maximum
    };
    let max_difference = difference(&seam);
    super::gpu3d::sky_render_test::use_unbounded_sky(&mut rig.renderer, &rig.device);
    let control = rig.draw();
    super::render_test_support::keep_image(&super::render_test_support::Image {
        width: SIZE,
        height: SIZE,
        rgba: control.clone(),
    });
    let control_difference = difference(&control);
    eprintln!(
        "atmosphere -X seam: maximum channel difference from +X = {max_difference}; original lookup = {control_difference}"
    );
    assert!(
        control_difference >= 15.0,
        "the original lookup must expose the dark streak: {control_difference}"
    );
    assert!(
        rgb(&reference, SIZE / 2, SIZE / 4).max_element() > 50.0,
        "control must be a lit sky"
    );
    assert!(
        max_difference <= 3.0,
        "azimuth-invariant atmosphere has a dark streak at -X: {max_difference}"
    );
}

/// Sets the frame time the next frames run with (`Ctx::new` starts at 0).
fn frame_time(rig: &mut Rig, dt: f32) {
    rig.ctx.time.begin_frame(crate::time::FrameTiming {
        dt,
        fixed_steps: 0,
        alpha: 0.0,
    });
}

/// The mean colour of the mirror ball of radius 1.5 at `centre`, over its
/// middle (in sRGB levels): what it reflects.
fn ball_colour(rig: &Rig, pixels: &[u8], centre: Vec3) -> Vec3 {
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let middle = screen(view_proj, centre);
    let right = rig.ctx.scene3d.camera.rotation * Vec3::X;
    let radius = (screen(view_proj, centre + right * 1.5) - middle).length() * 0.7;
    let (mut sum, mut count) = (Vec3::ZERO, 0.0f32);
    for y in 0..SIZE {
        for x in 0..SIZE {
            if (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - middle).length() <= radius {
                sum += rgb(pixels, x, y);
                count += 1.0;
            }
        }
    }
    sum / count.max(1.0)
}

/// A mirror ball inside a probe box, looked at; the probe refreshes as
/// `refresh` says. The sky is one colour all round.
fn refreshing_room(refresh: Option<ReflectionRefresh>) -> Option<(Rig, Entity)> {
    let mut rig = Rig::new()?;
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, -6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.ctx.scene3d.sky.sky_color = Color::rgb(0.1, 0.2, 0.9);
    rig.ctx.scene3d.sky.ground_color = Color::rgb(0.1, 0.2, 0.9);
    let ball = rig.mesh(&MeshData::sphere(1.5, 32), mirror(Color::WHITE));
    rig.ctx.world.spawn((Transform3D::default(), Model::new(ball)));
    let probe = ReflectionProbe3D {
        size: Vec3::splat(10.0),
        blend_distance: 0.0,
        ..Default::default()
    };
    let probe = match refresh {
        Some(refresh) => rig.ctx.world.spawn((Transform3D::default(), probe, refresh)),
        None => rig.ctx.world.spawn((Transform3D::default(), probe)),
    };
    frame_time(&mut rig, 1.0 / 60.0);
    Some((rig, probe))
}

/// The ball's colour over `frames` frames.
fn ball_over(rig: &mut Rig, frames: usize) -> Vec<Vec3> {
    (0..frames)
        .map(|_| {
            let pixels = rig.draw();
            ball_colour(rig, &pixels, Vec3::ZERO)
        })
        .collect()
}

/// The largest change of the ball's colour from one frame to the next.
fn largest_step(colours: &[Vec3]) -> f32 {
    colours
        .windows(2)
        .map(|w| (w[1] - w[0]).abs().max_element())
        .fold(0.0, f32::max)
}

#[test]
fn a_refreshing_probe_follows_a_sky_change_and_blends_in_without_a_pop() {
    let refresh = ReflectionRefresh {
        when: ProbeRefresh::LightingChanged(0.02),
        blend_seconds: 0.5,
    };
    let Some((mut rig, probe)) = refreshing_room(Some(refresh)) else {
        return;
    };
    let blue = *ball_over(&mut rig, 50).last().unwrap();
    assert!(
        blue.z > blue.x + 40.0,
        "the refreshed probe shows the blue sky by itself: {blue}"
    );
    // Sunset: the sky turns orange at once.
    rig.ctx.scene3d.sky.sky_color = Color::rgb(0.9, 0.35, 0.05);
    rig.ctx.scene3d.sky.ground_color = Color::rgb(0.9, 0.35, 0.05);
    let after = ball_over(&mut rig, 60);
    // A bake now, for what the ball shows with the orange sky captured.
    rig.ctx.gfx.bake_reflection_probe(&mut rig.ctx.world, probe).unwrap();
    let baked = *ball_over(&mut rig, 2).last().unwrap();
    let last = *after.last().unwrap();
    assert!(
        baked.x > baked.z + 40.0,
        "a bake with the orange sky reflects it: {baked}"
    );
    assert!(
        (last - baked).abs().max_element() < 6.0,
        "the refresh ends where a bake does: {last} vs {baked}"
    );
    // While the new capture is made, the old one is shown...
    let capturing = largest_step(&[&[blue][..], &after[..ReflectionRefresh::REFRESH_STEPS as usize - 1]].concat());
    assert!(capturing < 3.0, "the old capture holds while capturing: {capturing}");
    // ...then it moves over half a second (30 frames), never in a jump.
    let total = (last - blue).abs().max_element();
    let step = largest_step(&after);
    eprintln!("probe refresh: largest step {step} of {total}");
    assert!(
        step < total / 8.0,
        "the change is spread over the blend: largest step {step} of {total}"
    );

    // Negative control: without a blend, the same change is one pop.
    let Some((mut rig, _)) = refreshing_room(Some(ReflectionRefresh {
        blend_seconds: 0.0,
        ..refresh
    })) else {
        return;
    };
    ball_over(&mut rig, 50);
    rig.ctx.scene3d.sky.sky_color = Color::rgb(0.9, 0.35, 0.05);
    rig.ctx.scene3d.sky.ground_color = Color::rgb(0.9, 0.35, 0.05);
    let popped = largest_step(&ball_over(&mut rig, 30));
    assert!(
        popped > total * 0.7,
        "with no blend the change is one jump: {popped} of {total}"
    );
}

#[test]
fn a_probe_that_never_refreshes_keeps_the_old_sky() {
    let Some((mut rig, probe)) = refreshing_room(Some(ReflectionRefresh {
        when: ProbeRefresh::Never,
        blend_seconds: 0.5,
    })) else {
        return;
    };
    rig.ctx.gfx.bake_reflection_probe(&mut rig.ctx.world, probe).unwrap();
    ball_over(&mut rig, 2);
    rig.ctx.scene3d.sky.sky_color = Color::rgb(0.9, 0.35, 0.05);
    rig.ctx.scene3d.sky.ground_color = Color::rgb(0.9, 0.35, 0.05);
    let stale = *ball_over(&mut rig, 60).last().unwrap();
    assert!(
        stale.z > stale.x + 40.0,
        "a probe that never refreshes still shows the blue sky: {stale}"
    );
    // On demand: a request brings the new sky in.
    rig.ctx.world.get_mut::<ReflectionRefresh>(probe).unwrap().when = ProbeRefresh::OnDemand;
    rig.ctx.gfx.refresh_reflection_probe(&mut rig.ctx.world, probe).unwrap();
    let fresh = *ball_over(&mut rig, 60).last().unwrap();
    assert!(
        fresh.x > fresh.z + 40.0,
        "a requested refresh shows the orange sky: {fresh}"
    );
}

#[test]
fn a_day_night_cycle_keeps_a_refreshing_probe_matching_one_baked_every_frame() {
    let Some(mut rig) = Rig::new() else { return };
    // Two mirror balls, each in its own probe's box: one refreshes by
    // itself, the other is baked again every frame (the reference).
    rig.ctx.scene3d.camera.position = vec3(0.0, 1.0, -9.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    rig.ctx.scene3d.sky.atmosphere = true;
    rig.ctx.scene3d.sky.stars = 0.0;
    rig.ctx.scene3d.lighting.sun_intensity = 3.0;
    let ball = rig.mesh(&MeshData::sphere(1.5, 32), mirror(Color::WHITE));
    let (left, right) = (vec3(2.2, 0.0, 0.0), vec3(-2.2, 0.0, 0.0));
    let probe = ReflectionProbe3D {
        size: Vec3::splat(4.0),
        blend_distance: 0.0,
        ..Default::default()
    };
    for at in [left, right] {
        rig.ctx.world.spawn((Transform3D::at(at), Model::new(ball)));
    }
    rig.ctx.world.spawn((
        Transform3D::at(left),
        probe,
        ReflectionRefresh {
            when: ProbeRefresh::LightingChanged(0.02),
            blend_seconds: 0.25,
        },
    ));
    let baked = rig.ctx.world.spawn((Transform3D::at(right), probe));
    frame_time(&mut rig, 1.0 / 30.0);
    // Afternoon to dusk over 20 s at 30 frames a second: the sun from 40
    // degrees up to 5 below (a whole day in under three minutes).
    let sun = |t: f32| {
        let elevation = (40.0 - 45.0 * t).to_radians();
        -vec3(elevation.cos() * 0.8, elevation.sin(), elevation.cos() * 0.6)
    };
    rig.ctx.scene3d.lighting.sun_direction = sun(0.0);
    for _ in 0..30 {
        rig.ctx.gfx.bake_reflection_probe(&mut rig.ctx.world, baked).unwrap();
        rig.draw();
    }
    let frames = 600;
    let (mut worst, mut worst_step, mut previous) = (0.0f32, 0.0f32, None);
    let (mut first, mut last) = (Vec3::ZERO, Vec3::ZERO);
    for frame in 0..frames {
        rig.ctx.scene3d.lighting.sun_direction = sun(frame as f32 / frames as f32);
        rig.ctx.gfx.bake_reflection_probe(&mut rig.ctx.world, baked).unwrap();
        let pixels = rig.draw();
        let (refreshing, reference) = (ball_colour(&rig, &pixels, left), ball_colour(&rig, &pixels, right));
        worst = worst.max((refreshing - reference).abs().max_element());
        if let Some(previous) = previous {
            let step: Vec3 = refreshing - previous;
            worst_step = worst_step.max(step.abs().max_element());
        }
        previous = Some(refreshing);
        if frame == 0 {
            first = reference;
        }
        last = reference;
    }
    let change = (last - first).abs().max_element();
    eprintln!("day-night: largest gap to the reference {worst}, largest step {worst_step}, sky change {change}");
    assert!(
        change > 40.0,
        "the sky changes a lot from day to dusk: {first} to {last}"
    );
    // A probe baked once at the start would be off by the whole change; the
    // refreshing one trails the reference by about a second at most (the
    // capture's 14 frames, the blend, and a capture waiting on a blend).
    assert!(
        worst < change / 4.0,
        "the refreshing probe stays close to one baked every frame: {worst} of {change}"
    );
    assert!(worst_step < 4.0, "and changes smoothly: largest step {worst_step}");
}

#[test]
fn a_refreshing_probes_own_textures_count_as_probes() {
    let Some((mut rig, _)) = refreshing_room(Some(ReflectionRefresh::default())) else {
        return;
    };
    let probes = |rig: &Rig| {
        let mut t = crate::render::memory::MemoryTally::default();
        rig.renderer.tally_memory(&mut t);
        t.finish().probes
    };
    let before = probes(&rig);
    ball_over(&mut rig, 2);
    let after = probes(&rig);
    // The cube strip (768 x 128) and the maps' copy (896 x 256), 8 bytes a texel.
    let staging = (768 * 128 + 896 * 256) * 8;
    assert!(
        after >= before + staging,
        "the refresh's copy is counted: {before} then {after}"
    );
}

/// Cost of each step of a probe refresh in a scene of 400 cubes: the wall
/// time of a whole frame (recording, GPU work and the read back, waited
/// for) with that step, against frames without one. Run in release
/// (`cargo test --release --lib --features 3d -- --ignored --nocapture
/// probe_refresh_cost`). Wall time, not GPU timestamps: some adapters
/// report no time for many of these small passes.
#[test]
#[ignore = "benchmark: run in release"]
fn probe_refresh_cost() {
    let Some((mut rig, probe)) = refreshing_room(Some(ReflectionRefresh {
        when: ProbeRefresh::OnDemand,
        blend_seconds: 0.25,
    })) else {
        return;
    };
    let cube = rig.mesh(&MeshData::cube(1.0), matte(Color::rgb(0.8, 0.6, 0.4)));
    for i in 0..20 {
        for j in 0..20 {
            let at = vec3(
                i as f32 * 2.0 - 19.0,
                -3.0 + ((i * j) % 5) as f32 * 0.3,
                j as f32 * 2.0 - 19.0,
            );
            rig.ctx.world.spawn((Transform3D::at(at), Model::new(cube)));
        }
    }
    use crate::three::probe::RefreshStep;
    let kind = |steps: &[RefreshStep]| {
        let work = steps.iter().find(|s| !matches!(s, RefreshStep::Seed(_)));
        match work {
            None => "none",
            Some(RefreshStep::Capture { .. }) => "capture a face",
            Some(RefreshStep::Filter { face: 0, .. }) => "lay out and filter face 0",
            Some(RefreshStep::Filter { .. }) => "filter a face",
            Some(RefreshStep::Harmonics(_)) => "harmonics",
            Some(RefreshStep::Finish(_)) => "diffuse light",
            Some(RefreshStep::Blend { .. }) => "blend",
            Some(RefreshStep::Seed(_)) => unreachable!(),
        }
    };
    let mut times = std::collections::BTreeMap::<&str, Vec<f64>>::new();
    for round in 0..21 {
        if round > 0 {
            rig.ctx.gfx.refresh_reflection_probe(&mut rig.ctx.world, probe).unwrap();
        }
        for _ in 0..40 {
            let start = std::time::Instant::now();
            rig.draw();
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            if round > 0 {
                times
                    .entry(kind(&rig.ctx.gfx.frame3d.probe_refresh))
                    .or_default()
                    .push(ms);
            }
        }
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let none = median(times.get_mut("none").expect("frames without a step"));
    println!("probe refresh: frame without a step: median {none:.3} ms");
    for (kind, v) in &mut times {
        let m = median(v);
        let high = v[v.len() * 9 / 10];
        println!(
            "probe refresh: {kind}: median {m:.3} ms (+{:.3}), 90th percentile {high:.3} ms, {} frames",
            m - none,
            v.len()
        );
    }
}
