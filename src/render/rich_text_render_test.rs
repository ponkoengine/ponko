//! Rich text in a retained UI label, drawn offscreen: a dialogue line with
//! a colored name, an inline button glyph and wrapping goes through the
//! engine's UI drawing, the glyph atlas and the sprite pipeline and pass the
//! window uses, into a texture read back here and compared with its
//! reference in `tests/fixtures/render`. The negative control is the same
//! line read as plain text (its markup shown, no color, no glyph). With
//! `text-shaping`, a right-to-left line checks that its colored name, the
//! first run, is drawn at the right. Skips without a GPU (a skip is not a
//! pass).
use glam::vec2;

use super::batch::SpriteInstance;
use super::gpu::{GpuTexture, Renderer, SpritePass, sprite_pipeline};
use super::render_test_support::{
    Image, TestGpu, Tolerance, assert_region, check_reference, gpu, keep_image, reference_rejects, target,
    white_textures,
};
use crate::color::Color;
use crate::ctx::Ctx;
use crate::text::Text;
use crate::texture::Texture;
use crate::ui::{UiLayout, UiTextAlign};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: (u32, u32) = (256, 96);
/// The button glyph's side in pixels.
const GLYPH: u32 = 32;
/// Text is rasterized on the CPU and sampled texel for texel: adapters
/// differ only in the last bits of blending.
const REFERENCE: Tolerance = Tolerance {
    channel: 24,
    outliers: 0.004,
    edge: 1,
};

/// The button glyph: a green disc with a dark square in the middle.
fn glyph_pixels() -> Vec<u8> {
    let r = GLYPH as f32 / 2.0;
    let mut rgba = Vec::with_capacity((GLYPH * GLYPH * 4) as usize);
    for y in 0..GLYPH {
        for x in 0..GLYPH {
            let (dx, dy) = (x as f32 + 0.5 - r, y as f32 + 0.5 - r);
            let alpha = (r - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            let inner = dx.abs() < r * 0.35 && dy.abs() < r * 0.35;
            let [red, green, blue] = if inner { [20, 60, 30] } else { [40, 200, 90] };
            rgba.extend_from_slice(&[red, green, blue, (alpha * 255.0) as u8]);
        }
    }
    rgba
}

/// The window's sprite drawing, offscreen, with the glyph atlas as texture
/// 1 and the button glyph as texture 2.
struct Rig {
    gpu: TestGpu,
    pipeline: wgpu::RenderPipeline,
    camera_layout: wgpu::BindGroupLayout,
    textures: Vec<GpuTexture>,
}

impl Rig {
    fn new() -> Option<Self> {
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
        add(GLYPH, Some(&glyph_pixels()));
        Some(Self {
            pipeline: sprite_pipeline(device, &layout, &shader, FORMAT),
            camera_layout,
            textures,
            gpu,
        })
    }

    /// A `Ctx` with no window drawing into this rig's textures, with the
    /// button glyph named `pad_a` for rich text.
    fn ctx(&self) -> Ctx {
        let mut ctx = Ctx::new(1.0 / 60.0, vec2(SIZE.0 as f32, SIZE.1 as f32));
        ctx.gfx.stand_in_textures();
        ctx.ui.set_inline_image("pad_a", Texture::new(2, GLYPH, GLYPH));
        ctx
    }

    /// A screen with one wrapping label 240 units wide at (8, 8), holding
    /// `text`, rich or not, drawn as the window draws the UI.
    fn label(&self, ctx: &mut Ctx, text: &str, rich: bool, align: UiTextAlign) -> Image {
        let root = ctx
            .ui
            .root()
            .layout(UiLayout::fill().padding(8.0))
            .background(Color::TRANSPARENT)
            .id();
        let label = ctx
            .ui
            .label(root, text)
            .size(20.0)
            .wrap()
            .align(align)
            .color(Color::WHITE)
            .background(Color::TRANSPARENT)
            .layout(UiLayout::auto().width(240.0))
            .id();
        ctx.ui.set_rich(label, rich);
        ctx.ui.push_screen(root, false);
        ctx.ui.layout(vec2(SIZE.0 as f32, SIZE.1 as f32));
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
            Some(Color::BLACK),
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

/// The name's color, #ffcc00, as drawn (text antialiasing blends edges).
fn yellow(p: [u8; 4]) -> bool {
    p[0] > 180 && p[1] > 130 && p[2] < 90
}
/// The glyph's green disc.
fn green(p: [u8; 4]) -> bool {
    p[1] > 150 && p[0] < 100 && p[2] < 150
}
/// White (or gray, antialiased) text.
fn white(p: [u8; 4]) -> bool {
    p[0] > 128 && p[1] > 128 && p[2] > 128
}
/// Any ink.
fn lit(p: [u8; 4]) -> bool {
    p[0] > 100 || p[1] > 100 || p[2] > 100
}

/// The bands of rows with ink in them: (first row, last row) each.
fn ink_bands(image: &Image) -> Vec<(u32, u32)> {
    let mut bands: Vec<(u32, u32)> = Vec::new();
    for y in 0..image.height {
        if (0..image.width).any(|x| lit(image.pixel(x, y))) {
            match bands.last_mut() {
                Some(band) if band.1 + 1 == y => band.1 = y,
                _ => bands.push((y, y)),
            }
        }
    }
    bands
}

/// The columns where `test` holds anywhere: (leftmost, rightmost).
fn columns(image: &Image, test: impl Fn([u8; 4]) -> bool) -> Option<(u32, u32)> {
    let xs: Vec<u32> = (0..image.width)
        .filter(|&x| (0..image.height).any(|y| test(image.pixel(x, y))))
        .collect();
    Some((*xs.first()?, *xs.last()?))
}

const DIALOGUE: &str =
    "[color=#ffcc00]Mira:[/color] Press [img=pad_a alt=A] to talk to the keeper before the bell rings at dusk.";

#[test]
fn a_dialogue_line_has_a_colored_name_a_button_glyph_and_wraps() {
    let Some(rig) = Rig::new() else { return };
    let mut ctx = rig.ctx();
    let image = rig.label(&mut ctx, DIALOGUE, true, UiTextAlign::Start);
    let bands = ink_bands(&image);
    assert!(bands.len() >= 3, "the line wraps onto three lines or more: {bands:?}");
    let first = bands[0];
    // The name starts the first line, in its color.
    let (name_left, name_right) = columns(&image, yellow).expect("the name is drawn in yellow");
    assert!(
        (8..14).contains(&name_left) && name_right < 70,
        "the name starts the line: {name_left}..{name_right}"
    );
    assert_region(
        &image,
        name_left..name_right + 1,
        first.0..first.1 + 1,
        0.05,
        "the name's ink is yellow",
        yellow,
    );
    // The glyph sits on the first line, after "Press", as tall as the text.
    let (glyph_left, glyph_right) = columns(&image, green).expect("the button glyph is drawn");
    assert!(
        glyph_left > name_right + 30,
        "the glyph follows \"Press\": {glyph_left}"
    );
    let glyph_rows: Vec<u32> = (0..SIZE.1)
        .filter(|&y| (glyph_left..glyph_right + 1).any(|x| green(image.pixel(x, y))))
        .collect();
    let (top, bottom) = (glyph_rows[0], *glyph_rows.last().unwrap());
    assert!(
        top >= first.0.saturating_sub(2) && bottom <= first.1 + 2,
        "the glyph is on the first line: rows {top}..{bottom}, line {first:?}"
    );
    assert!(
        (14..=22).contains(&(bottom - top)),
        "the glyph is as tall as the text: {top}..{bottom}"
    );
    // The rest of the line is white, and stays inside the label.
    assert!(columns(&image, white).is_some(), "the rest is white");
    let (_, right) = columns(&image, lit).unwrap();
    assert!(right < 8 + 240, "every line fits the label: {right}");
    check_reference(
        "rich_text_dialogue",
        &image,
        REFERENCE,
        "a wrapping retained UI label 240 units wide at (8, 8), 20 unit text in the built-in font: \"Mira:\" in \
         #ffcc00, then white text with a 32x32 button glyph (a green disc with a dark square) inline, as tall as \
         the text; on black, 256x96, drawn with the window's sprite pipeline into an sRGB target",
        &rig.gpu.info,
    );
    // The same text read as plain text: its markup shown, no color, no glyph.
    let mut ctx = rig.ctx();
    let plain = rig.label(&mut ctx, DIALOGUE, false, UiTextAlign::Start);
    assert!(columns(&plain, yellow).is_none() && columns(&plain, green).is_none());
    reference_rejects("rich_text_dialogue", &plain, REFERENCE, "the markup as plain text");
}

#[cfg(feature = "text-shaping")]
#[test]
fn a_right_to_left_line_starts_with_its_colored_name_at_the_right() {
    let Some(rig) = Rig::new() else { return };
    let mut ctx = rig.ctx();
    let bytes = std::fs::read(format!(
        "{}/assets/fonts/NotoSansArabic-Subset.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let arabic = ctx.gfx.text.add_font(&bytes).unwrap();
    ctx.gfx.set_font_fallbacks(crate::text::Font::DEFAULT, &[arabic]);
    // "Hello: world world", the first word colored as a speaker's name.
    let line = "[color=#ffcc00]مرحبا:[/color] بالعالم بالعالم";
    let image = rig.label(&mut ctx, line, true, UiTextAlign::Start);
    let (name_left, name_right) = columns(&image, yellow).expect("the name is drawn in yellow");
    let (text_left, _) = columns(&image, white).expect("the rest is drawn in white");
    assert!(
        name_right > 8 + 240 - 12,
        "a right-to-left paragraph starts at the right edge: the name ends at {name_right}"
    );
    assert!(
        text_left < name_left,
        "the words after the name are to its left: {text_left} vs {name_left}"
    );
    check_reference(
        "rich_text_arabic",
        &image,
        REFERENCE,
        "a wrapping retained UI label 240 units wide at (8, 8), 20 unit text in the built-in font with Noto Sans \
         Arabic as its fallback: a right-to-left line whose first word and colon are #ffcc00, at the right edge, \
         the rest white to its left; on black, 256x96, drawn with the window's sprite pipeline into an sRGB target",
        &rig.gpu.info,
    );
    let mut ctx = rig.ctx();
    ctx.gfx.set_font_fallbacks(crate::text::Font::DEFAULT, &[arabic]);
    let plain = rig.label(&mut ctx, line, false, UiTextAlign::Start);
    reference_rejects("rich_text_arabic", &plain, REFERENCE, "the markup as plain text");
}

/// How concentrated the ink is: the mean brightness of the ink, weighted
/// by itself (1 when every inked pixel is fully lit). Glyphs smeared across
/// two pixels score lower.
fn crispness(image: &Image) -> f32 {
    let (mut sum, mut squares) = (0.0f32, 0.0f32);
    for y in 0..image.height {
        for x in 0..image.width {
            let v = image.pixel(x, y)[0] as f32 / 255.0;
            sum += v;
            squares += v * v;
        }
    }
    squares / sum.max(1e-6)
}

/// The largest channel difference between `a` and `b` moved by `shift`
/// pixels, over the part of `a` both cover.
fn largest_difference(a: &Image, b: &Image, shift: (u32, u32)) -> u8 {
    let mut largest = 0;
    for y in 0..a.height - shift.1 {
        for x in 0..a.width - shift.0 {
            let (p, q) = (a.pixel(x, y), b.pixel(x + shift.0, y + shift.1));
            for c in 0..3 {
                largest = largest.max(p[c].abs_diff(q[c]));
            }
        }
    }
    largest
}

#[test]
fn text_at_a_fractional_position_is_drawn_as_crisply_as_at_a_whole_one() {
    use crate::text::TextParams;
    let Some(rig) = Rig::new() else { return };
    let text = "Hill 11: lit";
    let params = TextParams::default().with_size(20.0).with_color(Color::WHITE);
    let mut ctx = rig.ctx();
    let mut draw = |pos| {
        ctx.gfx.draw_text_ex(text, pos, params);
        rig.shot(&mut ctx)
    };
    let whole = draw(vec2(20.0, 30.0));
    // A quarter pixel off rounds back to the same pixels; three quarters
    // to the next ones, the same picture a pixel further.
    let quarter = draw(vec2(20.25, 30.25));
    let three_quarters = draw(vec2(20.75, 30.75));
    assert!(
        largest_difference(&whole, &quarter, (0, 0)) <= 2,
        "a quarter pixel off draws the same pixels"
    );
    assert!(
        largest_difference(&whole, &three_quarters, (1, 1)) <= 2,
        "three quarters off draws the same picture a pixel further"
    );
    // The negative control: the glyph quads where they were placed before
    // snapping, half a pixel off, are resampled between pixels.
    let mut ctx = rig.ctx();
    let side = Text::ATLAS_SIZE as f32;
    ctx.gfx.text.layout(text, params.font, params.size, true);
    let pos = vec2(20.5, 30.5);
    for p in ctx.gfx.text.placed() {
        let Some(at) = p.glyph.at else { continue };
        let size = vec2(p.glyph.size[0] as f32, p.glyph.size[1] as f32);
        let uv = super::batch::uv_region([at[0] as f32 / side, at[1] as f32 / side], (size / side).into());
        let quad = SpriteInstance::new((pos + p.pos).into(), size.into(), [0.0, 0.0], 0.0, [255; 4]).with_uv(uv);
        ctx.gfx.ui_list.push(1, 0.0, quad);
    }
    let smeared = rig.shot(&mut ctx);
    let (sharp, soft) = (crispness(&whole), crispness(&smeared));
    assert!(
        crispness(&quarter) >= sharp * 0.99 && crispness(&three_quarters) >= sharp * 0.99,
        "snapped text is as crisp as at a whole pixel"
    );
    assert!(
        soft < sharp * 0.9,
        "half a pixel off, unsnapped, smears the glyphs: crispness {soft} vs {sharp}"
    );
    assert!(largest_difference(&whole, &smeared, (0, 0)) > 40, "the control differs");
}
