//! Screen-space UI text on the physical pixel grid: retained labels, rich
//! text runs and inline images, and the immediate UI, at fractional
//! positions and window scale factors 1 and 2 (1.5 too). Glyph bitmaps are
//! rasterized for whole physical pixels and the atlas is sampled linearly,
//! so a glyph quad between pixels smears its stems.
use glam::{Vec2, vec2};

use crate::color::Color;
use crate::ctx::Ctx;
use crate::text::Font;
use crate::texture::Texture;
use crate::ui::{UiLayout, UiTextAlign};

/// The texture id of the glyph atlas (`stand_in_textures`).
const ATLAS: u32 = 1;
/// The texture id of the inline image.
const IMAGE: u32 = 2;

fn ctx(scale_factor: f32) -> Ctx {
    let mut ctx = Ctx::new(1.0 / 60.0, vec2(400.0, 200.0));
    ctx.gfx.stand_in_textures();
    ctx.set_scale_factor(scale_factor);
    ctx.ui.set_inline_image("pad", Texture::new(IMAGE, 32, 32));
    ctx
}

/// The quads `ctx` drew with `texture` since the last call, as top-left
/// corner and size in physical pixels.
fn quads(ctx: &mut Ctx, texture: u32) -> Vec<(Vec2, Vec2)> {
    let scale = ctx.gfx.scale_factor;
    let (sprites, batches) = ctx.gfx.ui_list.finish();
    let out = batches
        .iter()
        .filter(|b| b.texture == texture)
        .flat_map(|b| &sprites[b.start as usize..b.end as usize])
        .map(|s| (Vec2::from(s.pos) * scale, Vec2::from(s.size) * scale))
        .collect();
    ctx.gfx.ui_list.clear();
    out
}

fn whole(v: Vec2) -> bool {
    (v - v.round()).abs().max_element() < 1e-3
}

/// A screen with one label at `offset` (UI units) from the window's
/// corner, `width` wide, holding `text`; drawn as the window draws it.
fn label(ctx: &mut Ctx, offset: Vec2, width: f32, text: &str, rich: bool) -> crate::ui::UiId {
    let root = ctx
        .ui
        .root()
        .layout(UiLayout::fill())
        .background(Color::TRANSPARENT)
        .id();
    let layout = UiLayout::fixed(width, 40.0).left(offset.x).top(offset.y);
    let label = ctx
        .ui
        .label(root, text)
        .size(17.0)
        .align(UiTextAlign::Center)
        .background(Color::TRANSPARENT)
        .layout(layout)
        .id();
    ctx.ui.set_rich(label, rich);
    ctx.ui.push_modeless(root);
    ctx.draw_retained_ui();
    label
}

/// UI scales that put labels at fractional window pixels.
const UI_SCALES: [f32; 3] = [1.0, 1.25, 1.3];
/// Fractions of a UI unit the labels are moved by.
const FRACTIONS: [f32; 4] = [0.0, 0.25, 0.5, 0.75];

#[test]
fn centered_retained_labels_land_on_whole_physical_pixels() {
    for scale_factor in [1.0, 1.5, 2.0] {
        for ui_scale in UI_SCALES {
            let mut first: Option<Vec<Vec2>> = None;
            for f in FRACTIONS {
                let mut ctx = ctx(scale_factor);
                ctx.ui.set_scale(ui_scale);
                // An odd width so centering adds half a pixel too.
                label(&mut ctx, vec2(10.0 + f, 5.0 + f), 151.0 + f, "Wave 12: ready", false);
                let glyphs = quads(&mut ctx, ATLAS);
                assert!(glyphs.len() >= 10, "{} glyphs", glyphs.len());
                let what = format!("scale factor {scale_factor}, UI scale {ui_scale}, offset {f}");
                for (at, _) in &glyphs {
                    assert!(whole(*at), "glyph at {at} physical pixels ({what})");
                }
                // The label looks the same wherever it sits: the glyphs keep
                // their places relative to the first.
                let relative: Vec<Vec2> = glyphs.iter().map(|(at, _)| *at - glyphs[0].0).collect();
                match &first {
                    None => first = Some(relative),
                    Some(first) => assert_eq!(&relative, first, "{what}"),
                }
            }
        }
    }
}

#[test]
fn rich_text_runs_share_one_baseline_on_whole_pixels() {
    // Runs in two sizes; capitals with flat bottoms, so every glyph's
    // bottom is the baseline.
    let text = "[size=13]HI[/size][size=29]EL[/size]H";
    let mut naive_split = false;
    for scale_factor in [1.0, 1.5, 2.0] {
        for ui_scale in UI_SCALES {
            for f in FRACTIONS {
                let mut ctx = ctx(scale_factor);
                ctx.ui.set_scale(ui_scale);
                let id = label(&mut ctx, vec2(10.0 + f, 5.0 + f), 150.0, text, true);
                let glyphs = quads(&mut ctx, ATLAS);
                assert_eq!(glyphs.len(), 5);
                let what = format!("scale factor {scale_factor}, UI scale {ui_scale}, offset {f}");
                let bottoms: Vec<f32> = glyphs.iter().map(|(at, size)| at.y + size.y).collect();
                for (at, _) in &glyphs {
                    assert!(whole(*at), "glyph at {at} physical pixels ({what})");
                }
                assert!(
                    bottoms.iter().all(|b| (b - bottoms[0]).abs() < 1e-3),
                    "baselines {bottoms:?} ({what})"
                );
                // The negative control: snapping each run's top on its own
                // (where the layout puts it) splits the baseline somewhere.
                let rect = ctx.ui.text_box(id).unwrap();
                let items = &ctx.ui_state.rich_layout.items;
                let snap = |y: f32| (y * scale_factor).round();
                let naive: Vec<f32> = [(0, 0), (1, 0), (2, 1), (3, 1), (4, 2)]
                    .iter()
                    .map(|&(glyph, item)| {
                        let item = &items[item];
                        let ascent = (item.baseline - item.at.y) * scale_factor;
                        let ours = snap(rect.min.y + item.baseline) - ascent.round();
                        bottoms[glyph] - ours + snap(rect.min.y + item.at.y)
                    })
                    .collect();
                naive_split |= naive.iter().any(|b| (b - naive[0]).abs() > 0.5);
            }
        }
    }
    assert!(naive_split, "snapping each run's top alone never split the baseline");
}

#[test]
fn inline_images_have_whole_pixel_edges() {
    for scale_factor in [1.0, 1.5, 2.0] {
        for ui_scale in UI_SCALES {
            for f in FRACTIONS {
                let mut ctx = ctx(scale_factor);
                ctx.ui.set_scale(ui_scale);
                label(&mut ctx, vec2(10.0 + f, 5.0 + f), 150.0, "Press [img=pad] now", true);
                let images = quads(&mut ctx, IMAGE);
                assert_eq!(images.len(), 1);
                let (at, size) = images[0];
                let what = format!("scale factor {scale_factor}, UI scale {ui_scale}, offset {f}");
                assert!(whole(at) && whole(at + size), "image {at}..{} ({what})", at + size);
            }
        }
    }
}

#[test]
fn immediate_labels_and_buttons_land_on_whole_physical_pixels() {
    for scale_factor in [1.0, 2.0] {
        for f in FRACTIONS {
            let mut ctx = ctx(scale_factor);
            ctx.ui.set_scale(1.3);
            let mut ui = ctx.immediate_ui();
            ui.label(vec2(10.0 + f, 20.0 + f), "Score 120");
            ui.button(crate::camera::Rect::new(40.0 + f, 60.0 + f, 101.0, 30.0), "Play");
            let glyphs = quads(&mut ctx, ATLAS);
            assert!(glyphs.len() >= 12, "{} glyphs", glyphs.len());
            for (at, _) in glyphs {
                assert!(whole(at), "glyph at {at} (scale factor {scale_factor}, offset {f})");
            }
        }
    }
}

#[test]
fn labels_are_laid_out_as_wide_as_they_are_drawn() {
    // Text is measured for layout at the size it is drawn, in physical
    // pixels: an auto-sized label is as wide as its drawn text, so
    // centering it in itself moves it by nothing.
    for scale_factor in [1.0, 1.5, 2.0] {
        for ui_scale in UI_SCALES {
            let mut ctx = ctx(scale_factor);
            ctx.ui.set_scale(ui_scale);
            let root = ctx.ui.root().layout(UiLayout::fill()).id();
            let mut layout = UiLayout::auto();
            layout.unstable_style_mut().position = taffy::Position::Absolute;
            let text = "Settings and more";
            let id = ctx.ui.label(root, text).size(17.0).layout(layout).id();
            ctx.ui.push_modeless(root);
            ctx.draw_retained_ui();
            let laid_out = ctx.ui.rect(id).unwrap().size().x;
            let drawn = ctx.gfx.measure_text_ex(text, Font::DEFAULT, 17.0 * ui_scale).x;
            // Taffy rounds edges to whole UI units.
            assert!(
                (laid_out - drawn).abs() <= 0.5 * ui_scale + 1e-3,
                "laid out {laid_out}, drawn {drawn} (scale factor {scale_factor}, UI scale {ui_scale})"
            );
        }
    }
}
