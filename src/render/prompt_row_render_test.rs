//! Button prompts drawn offscreen: one prompt row per device kind
//! (keyboard and mouse, then pads labelled Xbox, PlayStation, Nintendo and
//! generic), each "Jump", "Fire" and "Move" with the glyphs
//! `Input::action_glyphs_for` picks for that device, drawn by
//! `GlyphAtlas::draw_prompt` through the window's sprite pipeline into a
//! texture read back here and compared with its reference in
//! `tests/fixtures/render`. The negative control is the same rows with the
//! labels only. Skips without a GPU (a skip is not a pass).
use glam::vec2;

use super::batch::SpriteInstance;
use super::gpu::{GpuTexture, Renderer, SpritePass, sprite_pipeline};
use super::render_test_support::{
    Image, TestGpu, Tolerance, check_reference, gpu, keep_image, reference_rejects, target, white_textures,
};
use crate::actions::{Action, ActionMap, Axis2Binding, ButtonBinding, InputEvent};
use crate::color::Color;
use crate::ctx::Ctx;
use crate::input::{
    ControllerFamily, Gamepad, GamepadButton, GamepadStick, GlyphAtlas, Glyphs, InputDevice, Key, MouseButton,
};
use crate::text::Text;
use crate::texture::Texture;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: (u32, u32) = (256, 152);
/// Glyph cells are drawn at 32 pixels and shown at 20.
const CELL: u32 = 32;
const HEIGHT: f32 = 20.0;
/// From one row to the next.
const ROW: u32 = 28;
/// The atlas and the text are rasterized on the CPU and sampled with linear
/// filtering at 5/8 scale: adapters differ only in the last bits of blending.
const REFERENCE: Tolerance = Tolerance {
    channel: 24,
    outliers: 0.004,
    edge: 1,
};

/// The window's sprite drawing, offscreen, with the font atlas as texture 1
/// and the glyph atlas as texture 2.
struct Rig {
    gpu: TestGpu,
    pipeline: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    textures: Vec<GpuTexture>,
    atlas: GlyphAtlas,
}

impl Rig {
    fn new() -> Option<Self> {
        let gpu = gpu()?;
        let atlas = GlyphAtlas::new(CELL);
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
        let mut add = |width: u32, height: u32, pixels: Option<&[u8]>| {
            let size = wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            };
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
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
                        bytes_per_row: Some(4 * width),
                        rows_per_image: Some(height),
                    },
                    size,
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
        add(Text::ATLAS_SIZE, Text::ATLAS_SIZE, None);
        let (w, h) = atlas.size();
        add(w, h, Some(atlas.rgba()));
        Some(Self {
            pipeline: sprite_pipeline(device, &layout, &shader, FORMAT),
            camera_layout,
            textures,
            atlas,
            gpu,
        })
    }

    fn atlas_texture(&self) -> Texture {
        let (w, h) = self.atlas.size();
        Texture::new(2, w, h)
    }

    /// A `Ctx` with no window, the prompt actions installed (gamepad
    /// bindings on slot 0) and pads in slots 0 to 3 labelled Xbox,
    /// PlayStation, Nintendo and generic.
    fn ctx(&self) -> Ctx {
        let mut ctx = Ctx::new(1.0 / 60.0, vec2(SIZE.0 as f32, SIZE.1 as f32));
        ctx.gfx.stand_in_textures();
        let pad = Gamepad::from_slot(0).unwrap();
        let mut map = ActionMap::default();
        map.set(
            "jump",
            Action::Button(vec![
                ButtonBinding::Key(Key::Space),
                ButtonBinding::Gamepad {
                    pad,
                    button: GamepadButton::South,
                },
            ]),
        );
        map.set(
            "fire",
            Action::Button(vec![
                ButtonBinding::Mouse(MouseButton::Left),
                ButtonBinding::Gamepad {
                    pad,
                    button: GamepadButton::RightTrigger,
                },
            ]),
        );
        map.set(
            "move",
            Action::Axis2(vec![
                Axis2Binding::Keys {
                    left: Key::A,
                    right: Key::D,
                    up: Key::W,
                    down: Key::S,
                },
                Axis2Binding::Gamepad {
                    pad,
                    stick: GamepadStick::Left,
                    deadzone: 0.2,
                },
            ]),
        );
        ctx.input.set_actions(map);
        for (slot, family) in FAMILIES.into_iter().enumerate() {
            let pad = Gamepad::from_slot(slot).unwrap();
            ctx.input.feed(InputEvent::GamepadConnected(pad));
            ctx.input.set_gamepad_family(pad, family);
        }
        ctx
    }

    /// Draws the five rows; `glyphs` false draws the labels only.
    fn rows(&self, ctx: &mut Ctx, glyphs: bool) -> Image {
        let texture = self.atlas_texture();
        for (row, device) in devices().into_iter().enumerate() {
            let y = 8.0 + (row as u32 * ROW) as f32;
            let mut x = 8.0;
            for (action, label) in [("jump", "Jump"), ("fire", "Fire"), ("move", "Move")] {
                let shown = if glyphs {
                    ctx.input.action_glyphs_for(action, device)
                } else {
                    Glyphs::default()
                };
                x += self
                    .atlas
                    .draw_prompt_on(&mut ctx.gfx, texture, vec2(x, y), HEIGHT, &shown, label, Color::WHITE);
                x += 10.0;
            }
        }
        self.shot(ctx)
    }

    /// The picture of what `ctx` drew into its UI list.
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
        let size = vec2(SIZE.0 as f32, SIZE.1 as f32);
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
        let output = target(device, SIZE, FORMAT);
        let view = output.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        Renderer::sprite_pass(
            &mut encoder,
            &view,
            Some(Color::rgb(0.12, 0.13, 0.16)),
            None,
            &SpritePass {
                viewport: [SIZE.0, SIZE.1],
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

const FAMILIES: [ControllerFamily; 4] = [
    ControllerFamily::Xbox,
    ControllerFamily::PlayStation,
    ControllerFamily::Nintendo,
    ControllerFamily::Generic,
];

/// The rows' devices: keyboard and mouse, then the pads in slots 0 to 3.
fn devices() -> [InputDevice; 5] {
    let pad = |slot| InputDevice::Gamepad(Gamepad::from_slot(slot).unwrap());
    [InputDevice::KeyboardMouse, pad(0), pad(1), pad(2), pad(3)]
}

/// The first glyph's square on `row`.
fn first_glyph(row: u32) -> (std::ops::Range<u32>, std::ops::Range<u32>) {
    let y = 8 + row * ROW;
    (8..28, y..y + 20)
}

/// A keycap's face.
fn keycap(p: [u8; 4]) -> bool {
    p[0] > 200 && p[1] > 200 && p[2] > 200
}
/// The Xbox-style south letter.
fn green(p: [u8; 4]) -> bool {
    p[1] > 150 && p[0] < 150 && p[2] < 150
}
/// The PlayStation-style cross.
fn blue(p: [u8; 4]) -> bool {
    p[2] > 170 && p[0] < 140
}
/// The generic glyph's lit dot.
fn amber(p: [u8; 4]) -> bool {
    p[0] > 200 && p[1] > 130 && p[2] < 110
}
/// A face button's dark disc.
fn disc(p: [u8; 4]) -> bool {
    p[0] < 70 && p[1] < 75 && p[2] < 85 && p[2] > 40
}

#[test]
fn a_prompt_row_shows_the_glyphs_of_each_device() {
    let Some(rig) = Rig::new() else { return };
    let mut ctx = rig.ctx();
    let image = rig.rows(&mut ctx, true);
    let at = |row: u32, test: fn([u8; 4]) -> bool| {
        let (x, y) = first_glyph(row);
        image.fraction(x, y, test)
    };
    assert!(
        at(0, keycap) > 0.3,
        "row 0 starts with the Space keycap: {}",
        at(0, keycap)
    );
    assert!(
        at(1, green) > 0.01,
        "row 1 starts with the Xbox-style green A: {}",
        at(1, green)
    );
    assert!(
        at(2, blue) > 0.01,
        "row 2 starts with the PlayStation-style cross: {}",
        at(2, blue)
    );
    assert!(
        at(3, disc) > 0.3,
        "row 3 starts with a Nintendo-style dark face button: {}",
        at(3, disc)
    );
    assert!(
        at(4, amber) > 0.01,
        "row 4 starts with the generic lit dot: {}",
        at(4, amber)
    );
    for row in 1..5 {
        assert!(at(row, keycap) < 0.05, "row {row} shows no keycap");
    }
    check_reference(
        "prompt_row",
        &image,
        REFERENCE,
        "five prompt rows on #1f2129, 256x152, glyphs 20 px (atlas cells 32 px) from x 8, rows 28 px apart from y 8, \
         each \"Jump\", \"Fire\", \"Move\" in white 12 px text with 10 px between prompts: keyboard and mouse \
         (Space, left mouse button, W A S D keycaps), then pads labelled Xbox, PlayStation, Nintendo and generic \
         (south face button, right trigger, left stick); drawn with the window's sprite pipeline into an sRGB target",
        &rig.gpu.info,
    );
    let mut ctx = rig.ctx();
    let labels_only = rig.rows(&mut ctx, false);
    let (x, y) = first_glyph(1);
    assert_eq!(labels_only.fraction(x, y, green), 0.0, "no Xbox-style glyph drawn");
    let (x, y) = first_glyph(2);
    assert_eq!(
        labels_only.fraction(x, y, blue),
        0.0,
        "no PlayStation-style glyph drawn"
    );
    reference_rejects("prompt_row", &labels_only, REFERENCE, "the labels without glyphs");
}
