//! On-screen touch controls, drawn offscreen: a stick, a round button and
//! a corner button go through the engine's UI drawing, the glyph atlas and
//! the sprite pipeline and pass the window uses, into a texture read back
//! here and compared with their references in `tests/fixtures/render`, idle
//! and held. Held, the stick's knob follows the finger and the button
//! brightens. The negative controls: each state is rejected by the other's
//! reference, and nothing is drawn off a touch screen. Skips without a GPU
//! (a skip is not a pass).
use glam::{Vec2, vec2};

use super::batch::SpriteInstance;
use super::gpu::{GpuTexture, Renderer, SpritePass, sprite_pipeline};
use super::render_test_support::{
    Image, TestGpu, Tolerance, check_reference, gpu, keep_image, reference_rejects, target, white_textures,
};
use crate::actions::InputEvent;
use crate::color::Color;
use crate::ctx::Ctx;
use crate::text::Text;
use crate::texture::Texture;
use crate::ui::touch::{DISC, TouchControls, TouchLayout, TouchPlace, TouchStyle, disc_pixels};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: (u32, u32) = (256, 144);
/// Discs and text are rasterized on the CPU and sampled with linear
/// filtering: adapters differ only in the last bits of blending.
const REFERENCE: Tolerance = Tolerance {
    channel: 24,
    outliers: 0.004,
    edge: 1,
};

/// The window's sprite drawing, offscreen, with the glyph atlas as texture
/// 1 and the controls' disc as texture 2.
struct Rig {
    gpu: TestGpu,
    pipeline: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    textures: Vec<GpuTexture>,
    /// The view's size in pixels.
    size: (u32, u32),
}

impl Rig {
    fn new() -> Option<Self> {
        Self::sized(SIZE)
    }

    /// A rig drawing a view `size` pixels large.
    fn sized(size: (u32, u32)) -> Option<Self> {
        let gpu = gpu()?;
        let (device, queue) = (&gpu.device, &gpu.queue);
        let (texture_layout, _white, mut textures) = white_textures(device, queue);
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&camera_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("sprite.wgsl"));
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mut add = |side: u32, pixels: Option<&[u8]>| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            if let Some(pixels) = pixels {
                queue.write_texture(
                    texture.as_image_copy(),
                    pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * side),
                        rows_per_image: Some(side),
                    },
                    wgpu::Extent3d {
                        width: side,
                        height: side,
                        depth_or_array_layers: 1,
                    },
                );
            }
            let view = texture.create_view(&Default::default());
            textures.push(GpuTexture {
                bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &texture_layout,
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
                nearest: false,
                mips: None,
            });
        };
        add(Text::ATLAS_SIZE, None);
        add(DISC, Some(&disc_pixels()));
        Some(Self {
            pipeline: sprite_pipeline(device, &layout, &shader, FORMAT),
            camera_layout,
            textures,
            gpu,
            size,
        })
    }

    /// A `Ctx` with no window, on a touch screen, drawing into this rig's
    /// textures, with `controls` on it.
    fn ctx(&self, controls: TouchControls) -> Ctx {
        let mut ctx = Ctx::new(1.0 / 60.0, vec2(self.size.0 as f32, self.size.1 as f32));
        ctx.gfx.stand_in_textures();
        ctx.ui_state.touch.texture = Some(Texture::new(2, DISC, DISC));
        ctx.input.set_touch_device(true);
        ctx.input.set_touch_controls(controls);
        ctx
    }

    /// The controls as the window draws them this frame.
    fn draw(&self, ctx: &mut Ctx) -> Image {
        ctx.update_touch_controls();
        ctx.ui.layout(vec2(self.size.0 as f32, self.size.1 as f32));
        ctx.draw_retained_ui();
        self.shot(ctx)
    }

    /// The picture of what `ctx` drew into its UI list since the last one.
    fn shot(&self, ctx: &mut Ctx) -> Image {
        use wgpu::util::DeviceExt;
        let (device, queue) = (&self.gpu.device, &self.gpu.queue);
        for (at, size, rgba) in ctx.gfx.text.uploads.drain(..) {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.textures[1].texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: at[0],
                        y: at[1],
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * size[0]),
                    rows_per_image: Some(size[1]),
                },
                wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
            );
        }
        let (sprites, batches) = ctx.gfx.ui_list.finish();
        assert!(!sprites.is_empty(), "nothing was drawn");
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice::<SpriteInstance, u8>(sprites),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let size = vec2(self.size.0 as f32, self.size.1 as f32);
        let camera = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&crate::ctx::screen_projection(size).to_cols_array()),
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
        let output = target(device, self.size, FORMAT);
        let view = output.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        Renderer::sprite_pass(
            &mut encoder,
            &view,
            Some(Color::BLACK),
            None,
            &SpritePass {
                viewport: [self.size.0, self.size.1],
                scale: 1.0,
                pipeline: &self.pipeline,
                instances: &instances,
                textures: &self.textures,
                layers: &[(&camera, batches, 0)],
            },
        );
        queue.submit([encoder.finish()]);
        let image = Image::read(device, queue, &output);
        ctx.gfx.ui_list.clear();
        keep_image(&image);
        image
    }
}

fn controls() -> TouchControls {
    TouchControls::new()
        .stick("move")
        .button("jump", "A")
        .corner_button("pause", "II")
}

/// Brightness (the largest channel) at a pixel.
fn light(p: [u8; 4]) -> u8 {
    p[0].max(p[1]).max(p[2])
}

/// The mean brightness inside the circle of `radius` around `centre`.
fn mean_light(image: &Image, centre: Vec2, radius: f32) -> f32 {
    let (mut sum, mut n) = (0.0f32, 0.0f32);
    for y in 0..image.height {
        for x in 0..image.width {
            if vec2(x as f32 + 0.5, y as f32 + 0.5).distance(centre) <= radius {
                sum += light(image.pixel(x, y)) as f32;
                n += 1.0;
            }
        }
    }
    sum / n.max(1.0)
}

/// Where the brightest part (the knob) of the disc around `centre` is: the
/// centroid of its pixels within `bright` of the brightest.
fn knob(image: &Image, centre: Vec2, radius: f32) -> Vec2 {
    let inside = |x: u32, y: u32| vec2(x as f32 + 0.5, y as f32 + 0.5).distance(centre) <= radius;
    let mut top = 0;
    for y in 0..image.height {
        for x in 0..image.width {
            if inside(x, y) {
                top = top.max(light(image.pixel(x, y)));
            }
        }
    }
    let (mut sum, mut n) = (Vec2::ZERO, 0.0f32);
    for y in 0..image.height {
        for x in 0..image.width {
            if inside(x, y) && light(image.pixel(x, y)) + 30 >= top {
                sum += vec2(x as f32 + 0.5, y as f32 + 0.5);
                n += 1.0;
            }
        }
    }
    sum / n.max(1.0)
}

#[test]
fn onscreen_controls_show_a_knob_that_follows_the_finger_and_a_button_that_lights_when_held() {
    let Some(rig) = Rig::new() else { return };
    let mut ctx = rig.ctx(controls());
    let stick = ctx.input.touch_control_center("move").expect("a stick");
    let jump = ctx.input.touch_control_center("jump").expect("a button");
    let pause = ctx.input.touch_control_center("pause").expect("a corner button");
    let (stick_r, jump_r) = (ctx.input.touch.zones[0].radius, ctx.input.touch.zones[1].radius);
    let idle = rig.draw(&mut ctx);
    // Idle: the knob in the middle of its stick, every disc see-through.
    let at = knob(&idle, stick, stick_r);
    assert!(
        at.distance(stick) < stick_r * 0.15,
        "the knob rests in the middle: {at} vs {stick}"
    );
    let (idle_jump, idle_pause) = (mean_light(&idle, jump, jump_r * 0.8), mean_light(&idle, pause, 10.0));
    assert!(
        idle_jump > 20.0 && idle_pause > 20.0,
        "the buttons are drawn: {idle_jump} {idle_pause}"
    );
    assert!(
        light(idle.pixel(SIZE.0 / 2, SIZE.1 / 2)) == 0,
        "the middle of the view stays clear"
    );
    check_reference(
        "touch_controls_idle",
        &idle,
        REFERENCE,
        "on-screen touch controls in a 256x144 view on black: a stick (a see-through white disc with a solid rim \
         and a brighter knob in its middle) at the bottom left, a round button labelled A at the bottom right and \
         a small corner button labelled II at the top right, none held; drawn with the window's sprite pipeline \
         into an sRGB target",
        &rig.gpu.info,
    );
    // Held: a finger pushes the stick to the right, another holds the button.
    ctx.input.feed(InputEvent::touch_start(1, stick));
    ctx.input.feed(InputEvent::touch_move(1, stick + vec2(200.0, 0.0)));
    ctx.input.feed(InputEvent::touch_start(2, jump));
    let held = rig.draw(&mut ctx);
    let moved = knob(&held, stick, stick_r * 1.2);
    assert!(
        moved.x > stick.x + stick_r * 0.35 && (moved.y - stick.y).abs() < stick_r * 0.15,
        "the knob followed the finger to the right: {moved} vs {stick}"
    );
    let held_jump = mean_light(&held, jump, jump_r * 0.8);
    assert!(
        held_jump > idle_jump * 1.25,
        "the held button lights up: {held_jump} vs {idle_jump}"
    );
    assert!(
        (mean_light(&held, pause, 10.0) - idle_pause).abs() < 4.0,
        "the button not held stays as it was"
    );
    check_reference(
        "touch_controls_held",
        &held,
        REFERENCE,
        "the same on-screen touch controls with the stick pushed fully to the right (its knob at the right of its \
         disc) and the A button held (brighter); the corner button idle",
        &rig.gpu.info,
    );
    // The negative controls: each state is not the other.
    reference_rejects("touch_controls_idle", &held, REFERENCE, "held controls");
    reference_rejects("touch_controls_held", &idle, REFERENCE, "idle controls");
    // Off a touch screen the controls are not drawn at all.
    let mut ctx = rig.ctx(controls());
    ctx.input.set_touch_device(false);
    ctx.update_touch_controls();
    ctx.ui.layout(vec2(SIZE.0 as f32, SIZE.1 as f32));
    ctx.draw_retained_ui();
    assert!(
        ctx.gfx.ui_list.finish().0.is_empty(),
        "nothing is drawn without a touch screen"
    );
}

/// The mean color of the points `0.75 * radius` left, right and below
/// `centre`: a button's disc beside its label.
fn disc_color(image: &Image, centre: Vec2, radius: f32) -> [f32; 3] {
    let mut sum = [0.0f32; 3];
    let spots = [vec2(-0.75, 0.0), vec2(0.75, 0.0), vec2(0.0, 0.75)];
    for d in spots {
        let at = centre + d * radius;
        let p = image.pixel(at.x as u32, at.y as u32);
        for c in 0..3 {
            sum[c] += p[c] as f32 / spots.len() as f32;
        }
    }
    sum
}

#[test]
fn restyled_moved_controls_are_drawn_in_their_color_where_they_were_put_and_fade_with_the_players_opacity() {
    let Some(rig) = Rig::new() else { return };
    let red = TouchStyle {
        button: Color::rgba(1.0, 0.0, 0.0, 0.8),
        ..TouchStyle::default()
    };
    let middle = vec2(SIZE.0 as f32, SIZE.1 as f32) / 2.0;
    let default_jump = rig
        .ctx(controls())
        .input
        .touch_control_center("jump")
        .expect("a button");
    let mut ctx = rig.ctx(
        controls()
            .control_style("jump", red)
            .place("jump", TouchPlace::new(vec2(0.5, 0.5), Vec2::ZERO)),
    );
    assert_eq!(ctx.input.touch_control_center("jump"), Some(middle));
    let radius = ctx.input.touch.zones[1].radius;
    let image = rig.draw(&mut ctx);
    let [r, g, b] = disc_color(&image, middle, radius);
    assert!(
        r > 80.0 && g < r * 0.3 && b < r * 0.3,
        "red where it was put: {r} {g} {b}"
    );
    assert!(
        mean_light(&image, default_jump, radius * 0.6) < 3.0,
        "nothing where the engine would have put it"
    );
    let stick = ctx.input.touch_control_center("move").expect("a stick");
    let [sr, sg, sb] = disc_color(&image, stick, ctx.input.touch.zones[0].radius * 0.8);
    assert!(
        (sr - sg).abs() < 4.0 && (sr - sb).abs() < 4.0 && sr > 10.0,
        "the stick keeps the engine's white: {sr} {sg} {sb}"
    );
    // The player's layout at half the opacity: the same red, fainter.
    let layout = TouchLayout {
        opacity: 0.5,
        ..TouchLayout::default()
    };
    ctx.input.set_touch_layout(layout);
    let faint = rig.draw(&mut ctx);
    let [fr, fg, fb] = disc_color(&faint, middle, radius);
    assert!(fr < r * 0.8 && fr > 20.0, "fainter: {fr} vs {r}");
    assert!(fg < fr * 0.3 && fb < fr * 0.3, "still red: {fr} {fg} {fb}");
}

/// The arrange screen's view: 512x384 UI units at UI scale 0.5, so the
/// reference stays small.
const ARRANGE: (u32, u32) = (256, 192);

#[test]
fn the_arrange_screen_shows_its_panel_beside_the_controls_and_lights_the_picked_one() {
    let Some(rig) = Rig::sized(ARRANGE) else { return };
    let controls = TouchControls::new()
        .stick("move")
        .button("jump", "A")
        .button("fire", "B")
        .corner_button("pause", "II");
    let mut ctx = rig.ctx(controls);
    ctx.ui.set_scale(0.5);
    // A desktop: the controls show only because the screen arranges them.
    ctx.input.set_touch_device(false);
    ctx.arrange_touch_controls();
    // Placed for the scale.
    ctx.update_touch_controls();
    // A finger taps B: picked, to resize.
    let fire = ctx.input.touch_control_center("fire").expect("a button");
    let jump = ctx.input.touch_control_center("jump").expect("a button");
    let radius = ctx.input.touch.zones[2].radius;
    ctx.input.feed(InputEvent::touch_start(1, fire));
    ctx.input.feed(InputEvent::touch_end(1, fire));
    let image = rig.draw(&mut ctx);
    let card = ctx
        .ui
        .rect(ctx.ui.find("touch.arrange.card").expect("a panel"))
        .expect("laid out");
    // The panel: an opaque dark card in the middle, with text on it.
    let (mut lit, mut dark) = (0u32, 0u32);
    for y in card.min.y as u32 + 2..card.max.y as u32 - 2 {
        for x in card.min.x as u32 + 2..card.max.x as u32 - 2 {
            let l = light(image.pixel(x, y));
            if l > 180 {
                lit += 1;
            } else if (8..60).contains(&l) {
                dark += 1;
            }
        }
    }
    let area = card.size().x * card.size().y;
    assert!(dark as f32 > area * 0.5, "a dark panel: {dark} of {area}");
    assert!(lit > 100, "with light text and widgets on it: {lit}");
    for zone in &ctx.input.touch.zones {
        let near = zone.center.clamp(card.min, card.max);
        assert!(
            near.distance(zone.center) >= zone.radius - 0.5,
            "the panel {card:?} keeps clear of {} at {}",
            zone.action,
            zone.center
        );
    }
    assert!(
        card.min.y < 16.0,
        "the stick is under the middle: the panel goes to the top"
    );
    let (picked, other) = (
        mean_light(&image, fire, radius * 0.8),
        mean_light(&image, jump, radius * 0.8),
    );
    assert!(other > 20.0, "the controls are drawn beside it: {other}");
    assert!(picked > other * 1.25, "the picked control is lit: {picked} vs {other}");
    check_reference(
        "touch_arrange_screen",
        &image,
        REFERENCE,
        "the ready-made screen for arranging on-screen touch controls in a 256x192 view at UI scale 0.5 on black: \
         a dark panel at the top titled \"Arrange the controls\" with a note, the picked control's row (B, - and + buttons, 100 %), \
         Size and Opacity sliders and Reset, Cancel and Save buttons; around it a stick at the bottom left, buttons \
         A and B at the bottom right (B brighter: picked) and a corner button II at the top right",
        &rig.gpu.info,
    );
    // The negative control: while a control is dragged the panel steps
    // aside, and that frame is not the reference.
    ctx.input.feed(InputEvent::touch_start(2, jump));
    ctx.input.feed(InputEvent::touch_move(2, jump - vec2(200.0, 0.0)));
    let dragging = rig.draw(&mut ctx);
    assert!(
        light(dragging.pixel(card.center().x as u32, card.min.y as u32 + 20)) < 8,
        "no panel while a control is dragged"
    );
    reference_rejects("touch_arrange_screen", &dragging, REFERENCE, "a control being dragged");
}
