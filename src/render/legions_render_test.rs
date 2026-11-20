//! The showcase battle (`examples/legions`) drawn offscreen: the example's
//! own rules play a few seconds of a battle headless, then its own frame
//! systems (the sprite animation and the ground) and the engine's sprite
//! drawing fill the draw list, which the window's sprite shader, pipeline
//! and pass draw into a texture read back here. Only the window is missing.
//!
//! What must hold on any GPU: both armies show in their colours where the
//! fight is, the ground shows around them, and every unit, arrow and fallen
//! unit is an instance of a few batches (one per draw order), not a draw
//! call each. The negative control is the same battle without sprites. The
//! whole frame is also compared with a reference image.
//!
//! `legions_frame_cost` (ignored; run in release) measures what drawing
//! 10,000, 100,000 and 250,000 units costs offscreen.

use super::batch::SpriteInstance;
use super::gpu::{GpuTexture, Renderer, SpritePass, sprite_pipeline};
use super::render_test_support::*;
use crate::camera::views::Camera2DFrame;
use crate::ctx::Ctx;
use crate::prelude::*;
use crate::texture::Texture;
use wgpu::util::DeviceExt;

#[allow(dead_code, clippy::all)]
#[path = "../../examples/legions/game.rs"]
mod legions;

use legions::{Art, Battle, Ground, Legions, Look, Score};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: (u32, u32) = (256, 144);
/// Texture ids in the draw list: the engine's stand-ins are 0 (white) and
/// 1 (the glyph atlas); the battle's sheet and ground come next.
const SHEET: u32 = 2;
const GROUND: u32 = 3;

/// The window's sprite pipeline on a device with no window, and the
/// battle's two textures.
struct Rig {
    gpu: TestGpu,
    pipeline: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    textures: Vec<GpuTexture>,
}

impl Rig {
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&camera_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        // Texture 1, the glyph atlas, is never drawn here: white stands in.
        let white = GpuTexture {
            texture: textures[0].texture.clone(),
            nearest: false,
            mips: None,
            bind_group: textures[0].bind_group.clone(),
        };
        textures.push(white);
        let mut rig = Self {
            pipeline: sprite_pipeline(device, &layout, &shader, FORMAT),
            camera_layout,
            texture_layout,
            textures,
            gpu,
        };
        let (w, h, sheet) = legions::sheet_pixels();
        rig.add_texture(w, h, &sheet);
        Some(rig)
    }

    /// Adds a nearest-filtered texture as the next id.
    fn add_texture(&mut self, width: u32, height: u32, rgba: &[u8]) {
        let device = &self.gpu.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.gpu.queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: None,
            },
            texture.size(),
        );
        let view = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        self.textures.push(GpuTexture {
            bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.texture_layout,
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
            texture,
            nearest: true,
            mips: None,
        });
    }

    /// Draws what `ctx`'s draw list holds through its camera, on black.
    /// Returns the picture, the number of sprites and of batches.
    fn shot(&self, ctx: &mut Ctx, size: (u32, u32)) -> (Image, usize, usize) {
        let device = &self.gpu.device;
        ctx.place_cameras();
        let frame = Camera2DFrame::of(&ctx.camera, None);
        let (sprites, batches) = ctx.gfx.draw_list.finish();
        let counts = (sprites.len(), batches.len());
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice::<SpriteInstance, u8>(sprites),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let camera = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&frame.view_proj.to_cols_array()),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let camera = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.as_entire_binding(),
            }],
        });
        let output = target(device, size, FORMAT);
        let view = output.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        Renderer::sprite_pass(
            &mut encoder,
            &view,
            Some(Color::BLACK),
            None,
            &SpritePass {
                viewport: [size.0, size.1],
                scale: 1.0,
                pipeline: &self.pipeline,
                instances: &instances,
                textures: &self.textures,
                layers: &[(&camera, batches, 0)],
            },
        );
        self.gpu.queue.submit([encoder.finish()]);
        let image = Image::read(device, &self.gpu.queue, &output);
        keep_image(&image);
        ctx.gfx.draw_list.clear();
        (image, counts.0, counts.1)
    }
}

/// The battle's sheet as the draw list names it.
fn art() -> Art {
    let (w, h, _) = legions::sheet_pixels();
    Art {
        sheet: crate::sheet::SpriteSheet::new(Texture::new(SHEET, w, h), legions::FRAME, legions::FRAME),
    }
}

/// `units` units of seed 1 after `ticks` ticks, played by the example's
/// rules headless (with sprites when `drawn`).
fn battle(units: usize, ticks: u64, drawn: bool) -> World {
    Game::new("legions", 1, 1)
        .seed(legions::SEED)
        .plugin(Legions { units, art: false })
        .run_headless(ticks, move |ctx| {
            if drawn {
                // The same battle, with the sheet the test uploads.
                ctx.world.insert_resource(Look(Some(art())));
                legions::start_round(ctx);
            }
        })
}

/// A `Ctx` showing `world` the way the window does, with the camera on
/// the middle of the fight at `zoom` pixels per world unit.
fn frame_of(world: World, size: (u32, u32), zoom: f32) -> Ctx {
    let mut ctx = Ctx::new(1.0 / 60.0, vec2(size.0 as f32, size.1 as f32));
    ctx.gfx.stand_in_textures();
    ctx.world = world;
    let (nav_size, middle) = {
        let b = ctx.world.resource::<Battle>();
        let s = ctx.world.resource::<Score>();
        (b.nav.size(), (s.center[0] + s.center[1]) / 2.0)
    };
    ctx.world
        .insert_resource(Ground(Texture::new(GROUND, nav_size.x, nav_size.y)));
    ctx.camera.zoom = zoom;
    ctx.camera.look_at(middle);
    // The window's frame systems, then the engine's sprite drawing.
    legions::animate(&mut ctx);
    legions::draw_ground(&mut ctx);
    ctx.draw_world_sprites();
    ctx
}

fn red(p: [u8; 4]) -> bool {
    p[0] > 150 && p[1] < 140 && p[2] < 130
}
fn blue(p: [u8; 4]) -> bool {
    p[2] > 150 && p[0] < 140
}
fn grass_or_mud(p: [u8; 4]) -> bool {
    p[0] < 100 && p[1] < 80 && p[2] < 60 && p[0].max(p[1]) > 20
}

#[test]
fn the_battle_draws_both_armies_as_a_few_instanced_batches() {
    let Some(mut rig) = Rig::new() else { return };
    let units = 4000;
    let world = battle(units, 330, true);
    let ground = legions::ground_pixels(&world.resource::<Battle>().nav);
    let nav = world.resource::<Battle>().nav.size();
    rig.add_texture(nav.x, nav.y, &ground);
    let things = {
        let mut w = world;
        let n = w.query::<&legions::Unit>().count()
            + w.query::<&legions::Arrow>().count()
            + w.query::<&legions::Corpse>().count();
        let arrows = w.query::<&legions::Arrow>().count();
        let fallen = w.query::<&legions::Corpse>().count();
        eprintln!("legions frame: {n} sprites of units, arrows ({arrows}) and the fallen ({fallen})");
        assert!(
            arrows > 0 && fallen > 0,
            "the fight is on: {arrows} arrows, {fallen} fallen"
        );
        (n, w)
    };
    let (n, world) = things;
    let mut ctx = frame_of(world, SIZE, 2.0);
    let (image, sprites, batches) = rig.shot(&mut ctx, SIZE);
    // Every unit, arrow and corpse plus the ground is one sprite; they come
    // in one batch per draw order (ground, fallen, units, arrows).
    assert_eq!(sprites, n + 1);
    assert!(batches <= 4, "{batches} batches for {sprites} sprites");
    eprintln!("legions frame: {sprites} sprites in {batches} instanced draws");
    // Red on the left half, blue on the right, the ground between.
    let (w, h) = SIZE;
    assert_region(&image, 0..w / 2, 0..h, 0.02, "red soldiers on the left", red);
    assert_region(&image, w / 2..w, 0..h, 0.02, "blue soldiers on the right", blue);
    assert!(image.fraction(0..w / 2, 0..h, blue) < image.fraction(w / 2..w, 0..h, blue));
    assert_region(&image, 0..w, 0..h, 0.2, "the ground", grass_or_mud);
    check_reference(
        "legions_battle",
        &image,
        Tolerance {
            channel: 12,
            outliers: 0.01,
            edge: 1,
        },
        "The legions example, 4000 units of seed 1 after 330 ticks, drawn with its own animation and ground at \
         2 pixels per world unit on the middle of the fight: red soldiers and archers on the left, blue on the \
         right, arrows and the fallen between, on grass, mud and rock.",
        &rig.gpu.info,
    );

    // Negative control: the same battle without sprites shows no army.
    let mut plain = frame_of(battle(units, 330, false), SIZE, 2.0);
    let (image, sprites, _) = rig.shot(&mut plain, SIZE);
    assert_eq!(sprites, 1, "only the ground");
    assert!(image.fraction(0..w, 0..h, red) < 0.001);
    assert!(image.fraction(0..w, 0..h, blue) < 0.001);
}

/// What drawing the battle costs per frame, offscreen at 1280 x 720 with
/// the whole map in view: the CPU side (animation, filling and sorting the
/// draw list, the instance upload) and the GPU pass, for 10k, 100k and
/// 250k units. Run in release:
/// `cargo test --release --lib --features 3d legions_frame_cost -- --ignored --nocapture`
#[test]
#[ignore = "a measurement; run in release"]
fn legions_frame_cost() {
    use std::time::Instant;
    let Some(mut rig) = Rig::new() else { return };
    let size = (1280, 720);
    for units in [10_000, 100_000, 250_000] {
        // A few seconds in: the armies are close and fighting begins.
        let world = battle(units, 300, true);
        let nav = world.resource::<Battle>().nav.size();
        let world_size = nav.as_vec2() * legions::CELL;
        rig.textures.truncate(GROUND as usize);
        rig.add_texture(nav.x, nav.y, &legions::ground_pixels(&world.resource::<Battle>().nav));
        let zoom = (vec2(size.0 as f32, size.1 as f32) / world_size).min_element();
        let mut ctx = frame_of(world, size, zoom);
        ctx.gfx.draw_list.clear();
        let mut cpu = Vec::new();
        let mut total = Vec::new();
        for _ in 0..40 {
            let start = Instant::now();
            legions::animate(&mut ctx);
            legions::draw_ground(&mut ctx);
            ctx.draw_world_sprites();
            let filled = start.elapsed();
            let _ = rig.shot(&mut ctx, size);
            cpu.push(filled.as_secs_f64() * 1e3);
            total.push(start.elapsed().as_secs_f64() * 1e3);
        }
        let median = |v: &mut Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        eprintln!(
            "legions frame cost, {units} units: fill the draw list {:.2} ms, whole frame with upload, GPU pass and \
             read-back {:.2} ms (medians of 40)",
            median(&mut cpu),
            median(&mut total)
        );
    }
}
