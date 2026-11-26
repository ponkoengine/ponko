//! Level-of-detail groups drawn offscreen: the right mesh for the camera's
//! distance, a crossfade drawing both levels through complementary dither
//! patterns without holes (depth prepass and main pass agreeing), and the
//! shadow maps following the level and its fade, also for groups the
//! camera doesn't see. Moving the camera never leaves a frame without the
//! group, and hysteresis keeps a level while the camera wobbles at its
//! threshold. Skips without a GPU.
use super::gpu::GpuTexture;
use super::gpu3d::*;
#[allow(unused_imports)]
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{LodGroup, LodHysteresis, LodLevel, LodRenderer, LodShadow, MaterialDesc, Mesh, MeshData, Model};
use crate::three::{Frame3D, GpuMaterialDesc, Scene3D, SkinFrame, SkyLight};

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
    /// Kept between draws, as the engine keeps its frame (hysteresis
    /// remembers the last pick in it).
    frame: Frame3D,
}

impl Rig {
    /// `None` without a GPU.
    fn new(solid_prepass: bool) -> Option<Self> {
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
        let target = output.create_view(&Default::default());
        let renderer = Renderer3D::new(&device, &layout, output.format(), 0, &white, 4, solid_prepass);
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(SIZE as f32, SIZE as f32));
        ctx.scene3d.sky.atmosphere = false;
        Some(Self {
            device,
            queue,
            output,
            target,
            textures,
            renderer,
            ctx,
            frame: Frame3D::default(),
        })
    }

    /// A mesh of `data` with `desc`, on the CPU and the GPU.
    fn mesh(&mut self, data: &MeshData, desc: MaterialDesc) -> Mesh {
        let material = self.ctx.gfx.create_material(desc);
        let mesh = self.ctx.gfx.create_mesh(data, material);
        self.renderer
            .create_material(&self.device, &GpuMaterialDesc::new(&desc), &self.textures);
        self.renderer
            .create_mesh(&self.device, data, &[(0..data.indices.len() as u32, material)], true);
        mesh
    }

    /// Draws the world and reads the pixels back (RGBA).
    fn draw(&mut self) -> Vec<u8> {
        let ctx = &mut self.ctx;
        let frame = &mut self.frame;
        frame.lod = ctx.scene3d.lod;
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
            // Sunlight only in the shadow scene: elsewhere the cubes glow
            // on black.
            light: SkyLight {
                sun: if ctx.scene3d.lighting.shadows {
                    Vec3::splat(ctx.scene3d.lighting.sun_intensity)
                } else {
                    Vec3::ZERO
                },
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
                size: (SIZE, SIZE),
            },
        );
        let buffers = self
            .renderer
            .draw(&self.device, &self.target, wgpu::Color::BLACK, &self.textures, None);
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

/// A material that shows as a flat `color`, whatever the light.
fn glowing(color: Color) -> MaterialDesc {
    MaterialDesc {
        color: Color::rgb(0.0, 0.0, 0.0),
        emissive: color,
        roughness: 1.0,
        ..Default::default()
    }
}

#[derive(Debug, Default, PartialEq, Clone, Copy)]
struct Count {
    red: usize,
    blue: usize,
    dark: usize,
}

fn count(pixels: &[u8]) -> Count {
    let mut c = Count::default();
    for p in pixels.as_chunks::<4>().0 {
        let (r, g, b) = (u32::from(p[0]), u32::from(p[1]), u32::from(p[2]));
        if r > 120 && g < 60 && b < 60 {
            c.red += 1;
        } else if b > 120 && g < 60 && r < 60 {
            c.blue += 1;
        } else if r + g + b < 40 {
            c.dark += 1;
        }
    }
    c
}

/// The camera `d` in front of the origin.
fn camera_at(ctx: &mut Ctx, d: f32) {
    ctx.scene3d.camera.position = vec3(0.0, 0.0, d);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
}

/// Level 0 is a small red cube, level 1 a bigger, nearer blue one (so a
/// depth pass that ignored the fade would hide the red behind the blue).
fn two_level_scene(rig: &mut Rig) {
    let red = rig.mesh(&MeshData::cube(3.0), glowing(Color::rgb(1.0, 0.0, 0.0)));
    let blue = rig.mesh(&MeshData::cube(3.6), glowing(Color::rgb(0.0, 0.0, 1.0)));
    // Size 8: 8 / (2 d tan 30°) of the screen's height at distance d.
    // Level 0 from 30% up, fading into level 1 over 30% to 65%.
    let group = LodGroup::new(
        8.0,
        vec![
            LodLevel::new(0.30, vec![LodRenderer::new(red)]).with_fade(0.5),
            LodLevel::new(0.05, vec![LodRenderer::new(blue)]),
        ],
    );
    rig.ctx.world.spawn((Transform3D::default(), group));
}

/// A distance where the group covers `height` of the screen.
fn distance_for(height: f32) -> f32 {
    LodGroup::distance_at(8.0, height, 60f32.to_radians())
}

#[test]
fn the_level_for_the_distance_is_the_one_drawn() {
    for solid_prepass in [true, false] {
        let Some(mut rig) = Rig::new(solid_prepass) else {
            return;
        };
        two_level_scene(&mut rig);
        camera_at(&mut rig.ctx, distance_for(0.9));
        let near = count(&rig.draw());
        assert!(near.red > 150 && near.blue == 0, "near: only the red cube: {near:?}");
        camera_at(&mut rig.ctx, distance_for(0.15));
        let far = count(&rig.draw());
        assert!(far.blue > 40 && far.red == 0, "far: only the blue cube: {far:?}");
        camera_at(&mut rig.ctx, distance_for(0.02));
        let gone = count(&rig.draw());
        assert!(
            gone.red == 0 && gone.blue == 0,
            "past the last level nothing is drawn: {gone:?}"
        );
        // The bias moves the switch: at the far distance, a bigger bias
        // brings the red cube back (5 x 0.15 is above the fade band).
        camera_at(&mut rig.ctx, distance_for(0.15));
        rig.ctx.scene3d.lod.bias = 5.0;
        let biased = count(&rig.draw());
        assert!(biased.red > 20 && biased.blue == 0, "bias 4: {biased:?}");
    }
}

#[test]
fn a_crossfade_draws_both_levels_through_complementary_dither() {
    for solid_prepass in [true, false] {
        let Some(mut rig) = Rig::new(solid_prepass) else {
            return;
        };
        two_level_scene(&mut rig);
        // Half way through the band: 0.475 of the screen.
        camera_at(&mut rig.ctx, distance_for(0.475));
        let mid = rig.draw();
        let faded = count(&mid);
        let drawn = faded.red + faded.blue;
        assert!(drawn > 150, "{faded:?}");
        assert!(
            faded.red * 4 > drawn && faded.blue * 4 > drawn,
            "half and half, give or take: {faded:?}"
        );
        // No holes: wherever the red cube is solid in an undithered frame
        // (crossfades off, level 0 only), the dithered one shows red or
        // blue, never the background.
        rig.ctx.scene3d.lod.crossfade = false;
        camera_at(&mut rig.ctx, distance_for(0.475));
        let solid = rig.draw();
        let (red_only, blue_only) = (count(&solid).red, count(&solid).blue);
        assert!(red_only == 0 || blue_only == 0, "crossfade off draws one level");
        // Level 1 alone covers more than level 0 alone, so use the pixels
        // of the level-0 cube: shrink the check to its middle.
        let at = |p: &[u8], x: u32, y: u32| {
            let i = ((y * SIZE + x) * 4) as usize;
            u32::from(p[i]) + u32::from(p[i + 1]) + u32::from(p[i + 2])
        };
        let (mut holes, mut inside) = (0, 0);
        for y in 0..SIZE {
            for x in 0..SIZE {
                // The undithered red cube's interior, two pixels in.
                let covered = |dx: i32, dy: i32| {
                    let (xx, yy) = (x as i32 + dx, y as i32 + dy);
                    (0..SIZE as i32).contains(&xx)
                        && (0..SIZE as i32).contains(&yy)
                        && at(&solid, xx as u32, yy as u32) > 100
                };
                if (-2..=2).all(|dy| (-2..=2).all(|dx| covered(dx, dy))) {
                    inside += 1;
                    if at(&mid, x, y) < 100 {
                        holes += 1;
                    }
                }
            }
        }
        assert!(inside > 50, "solid cube has an interior ({inside})");
        assert_eq!(
            holes, 0,
            "{holes} of {inside} interior pixels are holes (prepass {solid_prepass})"
        );
    }
}

#[test]
fn warmed_pipelines_build_nothing_while_drawing_a_crossfade() {
    // Cold: the first crossfade builds its variants inside the frame.
    let Some(mut rig) = Rig::new(true) else {
        return;
    };
    two_level_scene(&mut rig);
    camera_at(&mut rig.ctx, distance_for(0.475));
    let before = rig.renderer.pipeline_stats();
    rig.draw();
    assert!(
        rig.renderer.take_pipelines_built() > 0,
        "a cold crossfade builds variants"
    );
    let cold = rig.renderer.pipeline_stats();
    assert!(cold.misses > before.misses, "the cache saw the misses");
    assert_eq!(cold.builds - before.builds, cold.misses - before.misses);
    assert!(cold.slowest_build_ms > 0.0 && cold.build_ms >= cold.slowest_build_ms);
    rig.draw();
    assert_eq!(rig.renderer.take_pipelines_built(), 0, "once built, they're kept");
    let warm = rig.renderer.pipeline_stats();
    assert_eq!(warm.misses, cold.misses, "the second frame only hits");
    assert!(warm.hits > cold.hits);

    // Warmed while loading: the same crossfade draws the same pixels and
    // builds nothing, and shaders made afterwards come warm too.
    let Some(mut rig) = Rig::new(true) else {
        return;
    };
    two_level_scene(&mut rig);
    rig.renderer.warm_pipelines();
    let late = rig
        .renderer
        .create_shader(&rig.device, "late", "fn surface(s: Surface) -> Surface { return s; }")
        .unwrap();
    assert!(rig.renderer.shader_warm(late), "a shader made after warming comes warm");
    camera_at(&mut rig.ctx, distance_for(0.475));
    let faded = count(&rig.draw());
    assert_eq!(rig.renderer.take_pipelines_built(), 0);
    assert!(faded.red > 0 && faded.blue > 0, "still a crossfade: {faded:?}");
}

#[test]
fn the_dither_follows_the_blend() {
    let Some(mut rig) = Rig::new(true) else {
        return;
    };
    two_level_scene(&mut rig);
    let share = |rig: &mut Rig, height: f32| {
        camera_at(&mut rig.ctx, distance_for(height));
        let c = count(&rig.draw());
        c.red as f32 / (c.red + c.blue).max(1) as f32
    };
    // Band 0.30 to 0.65: the red cube's share of the pixels rises with the
    // screen height from 0 to 1.
    let low = share(&mut rig, 0.36);
    let mid = share(&mut rig, 0.475);
    let high = share(&mut rig, 0.59);
    assert!(low < mid && mid < high, "{low} {mid} {high}");
    assert!(low < 0.5 && high > 0.5, "{low} {high}");
}

/// A glowing cube over a white ground, lit by a sun straight above: the
/// ground under the cube is in its shadow, but not if the level shown casts
/// none.
fn shadow_scene(rig: &mut Rig) -> impl Fn(&[u8]) -> u32 + use<> {
    rig.ctx.scene3d.lighting.sun_direction = vec3(0.0, -1.0, 0.0001);
    rig.ctx.scene3d.lighting.shadows = true;
    let matte = MaterialDesc {
        roughness: 1.0,
        ..Default::default()
    };
    let ground = rig.mesh(&MeshData::plane(30.0), matte);
    rig.ctx.world.spawn((Transform3D::default(), Model::new(ground)));
    let red = rig.mesh(&MeshData::cube(1.0), glowing(Color::rgb(1.0, 0.0, 0.0)));
    let blue = rig.mesh(&MeshData::cube(1.0), glowing(Color::rgb(0.0, 0.0, 1.0)));
    rig.ctx.scene3d.camera.position = vec3(0.0, 6.0, 6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    // 6.7 from the camera; size chosen so the group covers 60% of the
    // screen: half way through the fade band (50% to 70%).
    let distance = rig.ctx.scene3d.camera.position.distance(vec3(0.0, 3.0, 0.0));
    let size = LodGroup::screen_height_at(1.0, distance, rig.ctx.scene3d.camera.fov_y).recip() * 0.6;
    let group = LodGroup::new(
        size,
        vec![
            LodLevel::new(0.5, vec![LodRenderer::new(red)]).with_fade(0.4),
            // The far level shows a cube too but casts no shadow.
            LodLevel::new(0.1, vec![LodRenderer::new(blue)]).with_shadow(LodShadow::Casters(vec![])),
        ],
    )
    .with_center(Vec3::ZERO);
    rig.ctx.world.spawn((Transform3D::at(vec3(0.0, 3.0, 0.0)), group));
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    move |pixels: &[u8]| {
        let ndc = view_proj.project_point3(vec3(0.0, 0.0, 0.3));
        let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as u32;
        let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as u32;
        // A few pixels around, so the dither averages out.
        let mut sum = 0;
        for dy in 0..3 {
            for dx in 0..3 {
                let i = (((y + dy - 1) * SIZE + x + dx - 1) * 4) as usize;
                sum += u32::from(pixels[i]) + u32::from(pixels[i + 1]) + u32::from(pixels[i + 2]);
            }
        }
        sum
    }
}

#[test]
fn shadows_follow_the_level_and_its_fade() {
    let Some(mut rig) = Rig::new(true) else {
        return;
    };
    let ground_at = shadow_scene(&mut rig);
    rig.ctx.scene3d.lod.bias = 2.0; // 120% of the screen: level 0 alone
    let near = ground_at(&rig.draw());
    rig.ctx.scene3d.lod.bias = 0.5; // 30%: level 1 alone, which casts no shadow
    let far = ground_at(&rig.draw());
    rig.ctx.scene3d.lod.bias = 1.0; // 60%: a crossfade of the two
    let mid = ground_at(&rig.draw());
    assert!(
        near * 3 < far,
        "level 0 shadows the ground, level 1 doesn't: {near} vs {far}"
    );
    assert!(
        near < mid && mid < far,
        "half way through the fade the shadow is half there: {near} < {mid} < {far}"
    );
}

/// Level 0 a red cube from 30% of the screen up, level 1 a blue one down
/// to 5%, switching at once (no crossfade); with `hysteresis`, a
/// [`LodHysteresis`] of 10%.
fn instant_scene(rig: &mut Rig, hysteresis: bool) {
    let red = rig.mesh(&MeshData::cube(3.0), glowing(Color::rgb(1.0, 0.0, 0.0)));
    let blue = rig.mesh(&MeshData::cube(3.0), glowing(Color::rgb(0.0, 0.0, 1.0)));
    let group = LodGroup::new(
        8.0,
        vec![
            LodLevel::new(0.30, vec![LodRenderer::new(red)]),
            LodLevel::new(0.05, vec![LodRenderer::new(blue)]),
        ],
    );
    let e = rig.ctx.world.spawn((Transform3D::default(), group));
    if hysteresis {
        rig.ctx.world.insert(e, LodHysteresis(0.1));
    }
}

/// Which cube a frame shows: 'r', 'b', '-' for neither, '?' for both.
fn shown(pixels: &[u8]) -> char {
    let c = count(pixels);
    match (c.red > 20, c.blue > 20) {
        (true, false) => 'r',
        (false, true) => 'b',
        (false, false) => '-',
        (true, true) => '?',
    }
}

#[test]
fn hysteresis_keeps_the_level_while_the_camera_wobbles_at_a_threshold() {
    for hysteresis in [false, true] {
        let Some(mut rig) = Rig::new(true) else {
            return;
        };
        instant_scene(&mut rig, hysteresis);
        let mut frames = String::new();
        // Close up, then 2% either side of level 0's threshold, frame by
        // frame; then far away, and the same wobble from below.
        camera_at(&mut rig.ctx, distance_for(0.9));
        frames.push(shown(&rig.draw()));
        for i in 0..6 {
            camera_at(&mut rig.ctx, distance_for(if i % 2 == 0 { 0.294 } else { 0.306 }));
            frames.push(shown(&rig.draw()));
        }
        camera_at(&mut rig.ctx, distance_for(0.15));
        frames.push(shown(&rig.draw()));
        for i in 0..6 {
            camera_at(&mut rig.ctx, distance_for(if i % 2 == 0 { 0.306 } else { 0.294 }));
            frames.push(shown(&rig.draw()));
        }
        // Then well past the band (10% above 0.30 is 0.33): it switches.
        camera_at(&mut rig.ctx, distance_for(0.4));
        frames.push(shown(&rig.draw()));
        if hysteresis {
            assert_eq!(frames, "rrrrrrrbbbbbbbr", "held through each wobble");
        } else {
            assert_eq!(frames, "rbrbrbrbrbrbrbr", "switching every frame");
        }
    }
}

#[test]
fn a_fade_time_crossfades_over_frames_wherever_the_camera_stops() {
    let Some(mut rig) = Rig::new(true) else {
        return;
    };
    instant_scene(&mut rig, false);
    // As a scene file (an imported animated cross-fade) gives it.
    let group = rig.ctx.world.query::<(Entity, &LodGroup)>().iter().next().unwrap().0;
    rig.ctx
        .world
        .set_json(group, "LodFadeTime", serde_json::json!(0.5))
        .unwrap();
    rig.frame.lod_dt = Some(0.125);
    // (red, blue) pixels of each frame.
    let frame = |rig: &mut Rig, height: f32| {
        camera_at(&mut rig.ctx, distance_for(height));
        let c = count(&rig.draw());
        (c.red, c.blue)
    };
    assert_eq!(frame(&mut rig, 0.9).1, 0, "close: red alone");
    // Just past the threshold, and still: 0.5 s of 0.125 s frames fade,
    // the red share shrinking and the blue growing, then blue alone.
    let fade: Vec<(usize, usize)> = (0..5).map(|_| frame(&mut rig, 0.25)).collect();
    for k in 0..3 {
        assert!(fade[k].0 > 0 && fade[k].1 > 0, "both, dithered: {fade:?}");
    }
    assert!(fade[0].0 > fade[1].0 && fade[1].0 > fade[2].0, "{fade:?}");
    assert!(fade[0].1 < fade[1].1 && fade[1].1 < fade[2].1, "{fade:?}");
    assert!(fade[3].0 == 0 && fade[4].0 == 0 && fade[4].1 > fade[2].1, "{fade:?}");
    // Back close for half a fade, then out again: it runs back from
    // there (two frames back to blue alone), not from the start.
    let back: Vec<(usize, usize)> = [0.9, 0.9, 0.25, 0.25, 0.25]
        .iter()
        .map(|&h| frame(&mut rig, h))
        .collect();
    assert!(back[1].0 > 0 && back[2].0 > 0 && back[2].0 < back[1].0, "{back:?}");
    assert!(back[3].0 == 0 && back[4].0 == 0, "{back:?}");
}

#[test]
fn dollying_out_never_leaves_a_frame_without_the_group() {
    for solid_prepass in [true, false] {
        let Some(mut rig) = Rig::new(solid_prepass) else {
            return;
        };
        two_level_scene(&mut rig);
        // From 90% of the screen down to 6% (just above the last level's
        // threshold) in 40 frames, through the crossfade: every frame shows
        // the group, and the red share only falls (with a little slack for
        // the dither and the shrinking cube).
        let mut last_share = 1.0f32;
        let mut seen = String::new();
        for step in 0..=40 {
            let height = 0.9 * (0.06f32 / 0.9).powf(step as f32 / 40.0);
            camera_at(&mut rig.ctx, distance_for(height));
            let c = count(&rig.draw());
            let drawn = c.red + c.blue;
            assert!(drawn > 8, "frame {step} (height {height}) is missing the group: {c:?}");
            let share = c.red as f32 / drawn as f32;
            assert!(
                share <= last_share + 0.1,
                "frame {step} (height {height}) went back to the finer level: {share} after {last_share}"
            );
            last_share = share;
            seen.push(match (c.red > 0, c.blue > 0) {
                (true, false) => 'r',
                (false, true) => 'b',
                (true, true) => '?',
                (false, false) => '-',
            });
        }
        assert!(seen.starts_with('r') && seen.ends_with('b'), "{seen}");
        assert!(seen.contains('?'), "a crossfade on the way: {seen}");
        // A jump back to close up shows the detailed level in that frame.
        camera_at(&mut rig.ctx, distance_for(0.9));
        assert_eq!(shown(&rig.draw()), 'r');
    }
}

#[test]
fn a_group_the_camera_does_not_see_casts_its_shadow_with_the_cameras_level() {
    let Some(mut rig) = Rig::new(true) else {
        return;
    };
    rig.ctx.scene3d.lighting.sun_direction = vec3(0.0, -1.0, 0.0001);
    rig.ctx.scene3d.lighting.shadows = true;
    let matte = MaterialDesc {
        roughness: 1.0,
        ..Default::default()
    };
    let ground = rig.mesh(&MeshData::plane(30.0), matte);
    rig.ctx.world.spawn((Transform3D::default(), Model::new(ground)));
    let red = rig.mesh(&MeshData::cube(1.0), glowing(Color::rgb(1.0, 0.0, 0.0)));
    let blue = rig.mesh(&MeshData::cube(1.0), glowing(Color::rgb(0.0, 0.0, 1.0)));
    // Looking down at the ground from low; the group hangs 5 m above the
    // middle of the view, out of the top of the picture, its shadow in the
    // middle.
    rig.ctx.scene3d.camera.position = vec3(0.0, 1.0, 6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    let at = vec3(0.0, 5.0, 0.0);
    let distance = rig.ctx.scene3d.camera.position.distance(at);
    // 60% of the screen at bias 1.
    let size = LodGroup::screen_height_at(1.0, distance, rig.ctx.scene3d.camera.fov_y).recip() * 0.6;
    let group = LodGroup::new(
        size,
        vec![
            LodLevel::new(0.5, vec![LodRenderer::new(red)]),
            LodLevel::new(0.1, vec![LodRenderer::new(blue)]).with_shadow(LodShadow::Casters(vec![])),
        ],
    );
    rig.ctx.world.spawn((Transform3D::at(at), group));
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let ground_at = move |pixels: &[u8]| {
        let ndc = view_proj.project_point3(Vec3::ZERO);
        let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as u32;
        let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as u32;
        let mut sum = 0;
        for dy in 0..3 {
            for dx in 0..3 {
                let i = (((y + dy - 1) * SIZE + x + dx - 1) * 4) as usize;
                sum += u32::from(pixels[i]) + u32::from(pixels[i + 1]) + u32::from(pixels[i + 2]);
            }
        }
        sum
    };
    rig.ctx.scene3d.lod.bias = 2.0; // level 0, which casts
    let near = rig.draw();
    rig.ctx.scene3d.lod.bias = 0.5; // level 1, which casts nothing
    let far = rig.draw();
    for (name, pixels) in [("near", &near), ("far", &far)] {
        let c = count(pixels);
        assert!(
            c.red == 0 && c.blue == 0,
            "{name}: the group itself is out of view: {c:?}"
        );
    }
    let (near, far) = (ground_at(&near), ground_at(&far));
    assert!(far > 200, "the ground is lit without the shadow: {far}");
    assert!(
        near * 3 < far,
        "the unseen group shadows the ground at the camera's level 0, not at level 1: {near} vs {far}"
    );
}
