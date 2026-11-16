//! 2D cameras in parts of the window, drawn offscreen with the window's
//! sprite pipeline and pass (`Renderer::sprites_in_views`): two players'
//! halves, each camera's own layers and clear, the UI over both, and 2D
//! lighting (lights, shadows, ambient) in every half.

use super::batch::SpriteInstance;
use super::gpu::{GpuTexture, Lighting, Renderer, SpritePass, sprite_pipeline};
use super::lights2d::Lights2D;
use super::regression_stage::REFERENCE;
use super::render_test_support::*;
use super::views::{SpriteView, ViewPipelines};
use crate::camera::views::Camera2DFrame;
use crate::camera::{Rect, RenderLayers, Viewport};
use crate::ecs::Transform2D;
use crate::light2d::{Occluder2D, PointLight2D};
use crate::prelude::*;
use wgpu::util::DeviceExt;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const WINDOW: (u32, u32) = (128, 64);

/// The window's sprite pipeline, a white texture and the rectangle fill,
/// on a device with no window.
struct Painter {
    gpu: TestGpu,
    pipeline: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    white: GpuTexture,
    fill: ViewPipelines,
    lights: Lights2D,
}

impl Painter {
    fn new() -> Option<Self> {
        let gpu = gpu()?;
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::include_wgsl!("sprite.wgsl"));
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let (texture_layout, _, mut textures) = white_textures(device, &gpu.queue);
        let lights = Lights2D::new(device, &gpu.queue, FORMAT, &camera_layout, &texture_layout);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&camera_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        Some(Self {
            pipeline: sprite_pipeline(device, &layout, &shader, FORMAT),
            camera_layout,
            white: textures.swap_remove(0),
            fill: ViewPipelines::new(device, FORMAT),
            lights,
            gpu,
        })
    }

    fn camera(&self, matrix: Mat4) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = self.gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&matrix.to_cols_array()),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        (buffer, group)
    }

    /// What `ctx` drew this frame, through its cameras as the renderer
    /// draws them (`Ctx::collect_cameras2d`), on black, lit by its 2D
    /// lights (gathered as the frame does, `Ctx::collect_lights2d`).
    fn shot(&mut self, ctx: &mut Ctx) -> Image {
        let device = &self.gpu.device;
        ctx.place_cameras();
        ctx.collect_lights2d();
        let mut cameras = vec![Camera2DFrame::of(&ctx.camera, None)];
        cameras.extend(ctx.views2d.iter().map(|v| Camera2DFrame::of(&v.camera, v.clear)));
        let (world, world_batches) = ctx.gfx.draw_list.finish();
        let world = world.to_vec();
        let world_batches = world_batches.to_vec();
        let (ui, ui_batches) = ctx.gfx.ui_list.finish();
        let sprites: Vec<SpriteInstance> = world.iter().chain(ui).copied().collect();
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&sprites),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let groups: Vec<_> = cameras.iter().map(|c| self.camera(c.view_proj)).collect();
        let window = [WINDOW.0, WINDOW.1];
        let views: Vec<_> = cameras
            .iter()
            .map(|c| SpriteView {
                rect: c.rect.physical(window),
                layers: c.layers,
                clear: c.clear,
            })
            .collect();
        let (_, ui_camera) = self.camera(crate::ctx::screen_projection(vec2(WINDOW.0 as f32, WINDOW.1 as f32)));
        let frame = &ctx.gfx.lights2d.frame;
        if frame.on {
            self.lights.prepare(
                device,
                &self.gpu.queue,
                frame,
                cameras[0].view_proj,
                cameras.iter().map(|c| c.view_proj),
                window,
                &[],
                std::array::from_ref(&self.white),
                &world_batches,
            );
        }
        let lighting = frame.on.then_some(Lighting {
            lights: &self.lights,
            frame,
            normal_maps: &[],
        });
        let output = target(device, WINDOW, FORMAT);
        let view = output.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        Renderer::sprites_in_views(
            &mut encoder,
            &view,
            Some(Color::BLACK),
            None,
            &SpritePass {
                viewport: window,
                scale: 1.0,
                pipeline: &self.pipeline,
                instances: &instances,
                textures: std::array::from_ref(&self.white),
                layers: &[
                    (&groups[0].1, &world_batches, 0),
                    (&ui_camera, ui_batches, world.len() as u32),
                ],
            },
            None,
            lighting.as_ref(),
            (&views, &groups, Some(&self.fill)),
        );
        self.gpu.queue.submit([encoder.finish()]);
        let image = Image::read(device, &self.gpu.queue, &output);
        keep_image(&image);
        image
    }
}

fn is(c: Color) -> impl Fn([u8; 4]) -> bool {
    let want = c.to_rgba8();
    move |p| (0..3).all(|k| p[k].abs_diff(want[k]) <= 2)
}

/// A frame of the scene: a red square near the world's origin with a
/// yellow marker on layer 1 over it, a green square 200 to the right, and
/// a white UI bar along the window's bottom.
fn scene(ctx: &mut Ctx) {
    ctx.gfx.stand_in_textures();
    ctx.gfx.draw_rect(Rect::new(10.0, 10.0, 20.0, 20.0), Color::RED);
    ctx.gfx.set_layers(RenderLayers::layer(1));
    ctx.gfx.draw_rect(Rect::new(15.0, 15.0, 10.0, 10.0), Color::YELLOW);
    ctx.gfx.set_layers(RenderLayers::DEFAULT);
    ctx.gfx.draw_rect(Rect::new(210.0, 10.0, 20.0, 20.0), Color::GREEN);
    let bar = SpriteInstance::new([0.0, 60.0], [128.0, 4.0], [0.0, 0.0], 0.0, [255; 4]);
    ctx.gfx.ui_list.push(0, 0.0, bar);
}

#[test]
fn two_2d_cameras_draw_the_world_into_their_halves_on_their_layers() {
    let Some(mut painter) = Painter::new() else { return };
    let mut ctx = Ctx::new(1.0 / 60.0, vec2(WINDOW.0 as f32, WINDOW.1 as f32));
    ctx.camera.rect = Viewport::split(2, 0);
    ctx.camera.layers = RenderLayers::ALL.without(1);
    let mut p2 = ctx.camera;
    p2.rect = Viewport::split(2, 1);
    p2.position = vec2(200.0, 0.0);
    p2.layers = RenderLayers::ALL;
    ctx.views2d.push(d2::View::new(p2).with_clear(Color::BLUE));
    scene(&mut ctx);
    let image = painter.shot(&mut ctx);
    ctx.gfx.draw_list.clear();
    ctx.gfx.ui_list.clear();

    // Player one, in the left half, sees the red square without its marker.
    let at = |camera: &d2::Camera, p: Vec2| {
        let s = camera.world_to_screen(p);
        (s.x as u32, s.y as u32)
    };
    let (rx, ry) = at(&ctx.camera, vec2(20.0, 20.0));
    assert_eq!((rx, ry), (20, 20));
    assert!(
        is(Color::RED)(image.pixel(rx, ry)),
        "red at the middle: {:?}",
        image.pixel(rx, ry)
    );
    assert_region(&image, 0..64, 0..60, 1.0, "no marker on the left", |p| {
        !is(Color::YELLOW)(p)
    });
    // Player two, in the right half on blue, sees the green square at its
    // own spot: 64 + (220 - 200).
    let (gx, gy) = at(&ctx.views2d[0].camera, vec2(220.0, 20.0));
    assert_eq!((gx, gy), (84, 20));
    assert!(
        is(Color::GREEN)(image.pixel(gx, gy)),
        "green: {:?}",
        image.pixel(gx, gy)
    );
    assert!(
        is(Color::BLUE)(image.pixel(120, 40)),
        "player two's clear: {:?}",
        image.pixel(120, 40)
    );
    assert!(
        is(Color::BLACK)(image.pixel(50, 40)),
        "player one's background: {:?}",
        image.pixel(50, 40)
    );
    // The red square drawn at x 10..30 by player two's camera would land
    // at 64 + (10 - 200): off its half, so not drawn into player one's.
    assert_region(&image, 0..64, 0..60, 1.0, "no green on the left", |p| {
        !is(Color::GREEN)(p)
    });
    // The UI bar goes over both halves.
    assert_region(&image, 0..128, 60..64, 1.0, "the UI over both", is(Color::WHITE));

    // Negative control: player one on every layer sees the marker.
    ctx.camera.layers = RenderLayers::ALL;
    scene(&mut ctx);
    let marked = painter.shot(&mut ctx);
    assert!(
        is(Color::YELLOW)(marked.pixel(rx, ry)),
        "the marker: {:?}",
        marked.pixel(rx, ry)
    );
}

/// A grey floor under both halves, one shadowed light at world (40, 32)
/// and a wall left of it, seen by player one from the world's corner and
/// by player two from 16 to the right (the light 24 in from its half's
/// left edge), on a dim blue ambient light.
fn lit_scene(ctx: &mut Ctx, shadows: bool) {
    ctx.gfx.stand_in_textures();
    ctx.gfx
        .draw_rect(Rect::new(-100.0, -100.0, 400.0, 300.0), Color::rgb(0.8, 0.8, 0.8));
    ctx.world.clear();
    ctx.world.spawn((
        Transform2D::at(vec2(40.0, 32.0)),
        PointLight2D {
            radius: 30.0,
            falloff: 0.5,
            shadows,
            ..Default::default()
        },
    ));
    ctx.world
        .spawn((Transform2D::at(vec2(30.0, 32.0)), Occluder2D::rect(vec2(2.0, 16.0))));
    ctx.gfx.set_ambient_light2d(Some(Color::rgb(0.1, 0.1, 0.3)));
}

#[test]
fn split_screen_2d_lights_and_shadows_light_both_halves() {
    let Some(mut painter) = Painter::new() else { return };
    let mut ctx = Ctx::new(1.0 / 60.0, vec2(WINDOW.0 as f32, WINDOW.1 as f32));
    ctx.camera.rect = Viewport::split(2, 0);
    let mut p2 = ctx.camera;
    p2.rect = Viewport::split(2, 1);
    p2.position = vec2(16.0, 0.0);
    ctx.views2d.push(d2::View::new(p2));
    lit_scene(&mut ctx, true);
    let image = painter.shot(&mut ctx);
    ctx.gfx.draw_list.clear();
    lit_scene(&mut ctx, false);
    let unshadowed = painter.shot(&mut ctx);
    ctx.gfx.draw_list.clear();

    let red = |image: &Image, x: u32, y: u32| image.pixel(x, y)[0];
    let blue = |image: &Image, x: u32, y: u32| image.pixel(x, y)[2];
    // Each half: the light's center bright (it sits at world 40: x 40 on
    // the left, 64 + 24 = 88 on the right), the floor beyond its reach on
    // the blue ambient light only, and the shadow behind the wall.
    for (half, light_x) in [("left", 40), ("right", 88)] {
        let (lit, ambient) = (red(&image, light_x, 32), red(&image, light_x + 20, 8));
        assert!(lit > 180, "{half}: the light's center {lit}");
        assert!(
            ambient < 40 && blue(&image, light_x + 20, 8) > ambient,
            "{half}: the dim blue ambient beyond the light"
        );
        // World x 24 (behind the wall at 30, seen from the light at 40).
        let behind = light_x - 16;
        assert!(
            red(&image, behind, 32) < 40,
            "{half}: behind the wall {}",
            red(&image, behind, 32)
        );
        assert!(
            red(&unshadowed, behind, 32) > 90,
            "{half}: the control without shadows lights it ({})",
            red(&unshadowed, behind, 32)
        );
    }
    // The halves are the same light seen 16 apart: the same picture.
    for (x, y) in [(40, 32), (24, 32), (50, 20), (60, 50)] {
        let (a, b) = (image.pixel(x, y), image.pixel(x + 48, y));
        assert!(
            (0..3).all(|k| a[k].abs_diff(b[k]) <= 3),
            "({x}, {y}) {a:?} vs ({}, {y}) {b:?}",
            x + 48
        );
    }
    check_reference(
        "views2d_split_screen_lights",
        &image,
        REFERENCE,
        "Two 2D cameras side by side (64x64 each, 128x64 window), the right one 16 world units to the right: a \
         light-grey floor lit by one shadowed white point light (radius 30, falloff 0.5) at world (40, 32), a wall \
         (2 x 16) at world (30, 32) casting its shadow to the left, dim blue ambient light; no UI.",
        &painter.gpu.info,
    );
    reference_rejects(
        "views2d_split_screen_lights",
        &unshadowed,
        REFERENCE,
        "the light without shadows",
    );
}
