//! A window that can't be drawn as sRGB (OpenGL ES without EGL's sRGB
//! color space, as on the Android emulator), offscreen: the frame is drawn
//! into the sRGB frame image and the encode pass writes it into the linear
//! window (`srgb_output.rs`).
//!
//! - a game's whole frame (3D with a lit and an emissive mesh, UI squares,
//!   `ctx.debug` lines in 3D and 2D, the inspector) holds the same bytes as
//!   in an sRGB window, within one step; written without encoding (the
//!   negative control, computed from the sRGB window's frame) it would be
//!   far darker;
//! - every drawing format moves to the frame image, the window keeps its
//!   linear format, and a screenshot matches the sRGB window's.
//!
//! Skips without a GPU adapter (a skip is not a pass).
use super::render_test_support::{Image, gpu, keep_image, offscreen_ctx};
use crate::prelude::*;

const SIZE: (u32, u32) = (64, 64);
const GREY_UI: (u32, u32) = (4, 18);
const ORANGE_UI: (u32, u32) = (4, 32);

/// What a game draws, on a dark blue background: a lit grey cube and an
/// emissive one, UI squares (white, grey, orange) and `ctx.debug` lines in
/// 3D and 2D. Drawn again each frame (debug drawing lasts one frame).
fn scene(ctx: &mut crate::ctx::Ctx) {
    ctx.scene3d.sky.atmosphere = false;
    ctx.scene3d.lighting.shadows = false;
    ctx.scene3d.post.bloom = 0.0;
    ctx.gfx.clear_color = Color::rgb(0.1, 0.12, 0.2);
    ctx.scene3d.camera.position = vec3(0.0, 0.0, 4.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    let debug = &mut ctx.debug;
    debug.set_enabled(true);
    debug
        .line(vec3(-2.0, -0.9, 0.0), vec3(2.0, -0.9, 0.0), Color::GREEN)
        .width(4.0);
    debug
        .line(vec2(-30.0, -28.0), vec2(30.0, -28.0), Color::rgb(1.0, 0.5, 0.0))
        .width(3.0);
    let (w, h) = (SIZE.0 as f32, SIZE.1 as f32);
    let ui = &mut ctx.ui;
    for (i, color) in [Color::WHITE, Color::rgb(0.5, 0.5, 0.5), Color::rgb(1.0, 0.5, 0.0)]
        .into_iter()
        .enumerate()
    {
        let root = ui
            .root()
            .layout(crate::ui::UiLayout::anchored(
                0.0,
                i as f32 * 14.0 / h,
                12.0 / w,
                12.0 / h,
            ))
            .background(color)
            .border_width(0.0)
            .id();
        ui.push_modeless(root);
    }
    let still = crate::ui::UiInput {
        pointer: vec2(-10.0, -10.0),
        ..Default::default()
    };
    ctx.ui.frame(vec2(w, h), &still);
    ctx.hand_over_debug_draw();
}

/// The meshes, made once.
fn meshes(ctx: &mut crate::ctx::Ctx) {
    let grey = ctx
        .gfx
        .create_material(crate::render::MaterialDesc::color(Color::rgb(0.6, 0.6, 0.6)));
    let glowing = ctx
        .gfx
        .create_material(crate::render::MaterialDesc::color(Color::BLACK).with_emissive(Color::rgb(0.9, 0.3, 0.1)));
    let lit = ctx.gfx.create_mesh(&crate::render::MeshData::cube(1.0), grey);
    let emissive = ctx.gfx.create_mesh(&crate::render::MeshData::cube(0.8), glowing);
    ctx.world.spawn((
        crate::d3::Transform::at(vec3(-0.9, 0.3, 0.0)),
        crate::render::Model::new(lit),
    ));
    ctx.world.spawn((
        crate::d3::Transform::at(vec3(0.9, 0.3, 0.0)),
        crate::render::Model::new(emissive),
    ));
}

/// The inspector's drawing for the next frame: a mid-grey panel and
/// half-transparent white, at the top right.
#[cfg(feature = "inspector")]
fn inspector_overlay(ctx: &mut crate::ctx::Ctx) {
    let egui = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(SIZE.0 as f32, SIZE.1 as f32),
        )),
        ..Default::default()
    };
    let out = egui.run_ui(input, |ui| {
        let painter = ui.painter();
        let rect = |y: f32| egui::Rect::from_min_size(egui::pos2(40.0, y), egui::vec2(20.0, 10.0));
        painter.rect_filled(rect(2.0), 0.0, egui::Color32::from_gray(128));
        painter.rect_filled(rect(14.0), 0.0, egui::Color32::from_white_alpha(128));
    });
    let primitives = egui.tessellate(out.shapes, out.pixels_per_point);
    ctx.gfx
        .renderer
        .as_mut()
        .unwrap()
        .set_overlay(crate::inspector::Overlay {
            primitives,
            textures: out.textures_delta,
            pixels_per_point: out.pixels_per_point,
        });
}

/// A frame of `ctx`, as the offscreen window holds it: format and bytes.
fn output(ctx: &mut crate::ctx::Ctx) -> (wgpu::TextureFormat, Vec<u8>) {
    scene(ctx);
    #[cfg(feature = "inspector")]
    inspector_overlay(ctx);
    assert_eq!(ctx.render_frame(), crate::render::FrameStatus::Offscreen);
    ctx.gfx.renderer.as_ref().unwrap().read_output().expect("a frame")
}

/// A screenshot of the next frame.
fn shot(ctx: &mut crate::ctx::Ctx) -> Image {
    let path = std::env::temp_dir().join(format!(
        "ponko-srgb-encode-{}-{:?}.png",
        std::process::id(),
        std::thread::current().id()
    ));
    scene(ctx);
    ctx.gfx.queue_screenshot(path.clone(), 0, None);
    ctx.render_frame();
    let shot = image::open(&path).expect("the frame was captured").to_rgba8();
    let _ = std::fs::remove_file(&path);
    let image = Image {
        width: shot.width(),
        height: shot.height(),
        rgba: shot.into_raw(),
    };
    keep_image(&image);
    image
}

/// An 8-bit sRGB value to linear light, as an unencoded window would hold it.
fn unencoded(byte: u8) -> u8 {
    let c = f32::from(byte) / 255.0;
    let linear = if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    };
    (linear * 255.0).round() as u8
}

fn px(bytes: &[u8], (x, y): (u32, u32)) -> [u8; 4] {
    let i = ((y * SIZE.0 + x) * 4) as usize;
    [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
}

#[test]
fn a_window_without_srgb_shows_the_same_colours_as_an_srgb_window() {
    let Some(mut srgb) = offscreen_ctx(SIZE) else {
        return;
    };
    let Some(gpu) = gpu() else {
        return;
    };
    let mut linear = crate::ctx::Ctx::new(1.0 / 60.0, vec2(SIZE.0 as f32, SIZE.1 as f32));
    linear.audio.headless = true;
    linear.gfx.renderer = Some(super::gpu::Renderer::offscreen_without_srgb(gpu, SIZE));
    use wgpu::TextureFormat::{Rgba8Unorm as Unorm, Rgba8UnormSrgb as Srgb};
    let mut frames = Vec::new();
    for ctx in [&mut srgb, &mut linear] {
        meshes(ctx);
        // The meshes reach the GPU with the first frame drawn.
        output(ctx);
        frames.push(output(ctx));
    }
    let formats = linear.gfx.renderer.as_ref().unwrap().formats_for_test();
    assert_eq!(
        formats,
        (Srgb, Some(Srgb), Unorm),
        "sprites and the 3D frame's end draw into the sRGB frame image; the window stays linear"
    );
    let ((srgb_format, want), (linear_format, got)) = (&frames[0], &frames[1]);
    assert_eq!((*srgb_format, *linear_format), (Srgb, Unorm));

    // Byte for byte what the sRGB window holds, within one step.
    let mut worst = (0u8, 0usize);
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        if i % 4 != 3 {
            worst = worst.max((g.abs_diff(*w), i / 4));
        }
    }
    let at = ((worst.1 as u32) % SIZE.0, (worst.1 as u32) / SIZE.0);
    assert!(
        worst.0 <= 1,
        "pixel {at:?}: {:?} without sRGB, {:?} in an sRGB window",
        px(got, at),
        px(want, at)
    );
    // The frame has its colours: the grey and orange squares, both cubes,
    // the background (not a black or empty frame).
    let grey = px(want, GREY_UI);
    assert!((120..=136).contains(&grey[0]), "the grey square: {grey:?}");
    assert!(px(want, ORANGE_UI)[0] > 240 && px(want, ORANGE_UI)[2] < 10);
    let distinct: std::collections::BTreeSet<[u8; 4]> = want.as_chunks::<4>().0.iter().copied().collect();
    assert!(distinct.len() > 20, "a varied frame: {} colours", distinct.len());

    // Negative control: written without encoding (as before the pass), the
    // linear window would hold the linear values, far darker.
    let mut darker = 0;
    let mut pixels = 0;
    for p in want.as_chunks::<4>().0 {
        if p[..3].iter().any(|&c| c > 20) {
            pixels += 1;
            if p[..3].iter().any(|&c| c - unencoded(c) > 20) {
                darker += 1;
            }
        }
    }
    assert!(
        pixels > 0 && darker * 10 > pixels * 9,
        "{darker} of {pixels} lit pixels"
    );
    assert!(
        unencoded(grey[0]) < grey[0] / 2,
        "the grey square, unencoded: {}",
        unencoded(grey[0])
    );

    // Screenshots read the frame as an sRGB window's.
    let (a, b) = (shot(&mut srgb), shot(&mut linear));
    let most = a.rgba.iter().zip(&b.rgba).map(|(p, q)| p.abs_diff(*q)).max().unwrap();
    assert!(most <= 1, "screenshots differ by {most}");
}
