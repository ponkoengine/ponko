//! A UI group fading as one image (`Ui::set_fade_as_group`), drawn through
//! a game's own frame (`Ctx::render_frame` with the offscreen renderer), so
//! the layer goes through the production renderer's offscreen pass and the
//! window's sprite pass that shows it.
//!
//! The scene: a red square and a blue square overlapping in one group at
//! half opacity, over a black window. Faded piece by piece (the negative
//! control), the red square shows through the blue one where they overlap;
//! faded as a group, the overlap is the blue square alone at half opacity,
//! and the parts that don't overlap are what fading each piece gives. At
//! rest the group draws what it draws without the setting. Checks are
//! semantic, with no reference image. Skips without a GPU (a skip is not a
//! pass).
//!
//! The same scene in a world-space panel (`UiPanel3D`), read back from the
//! panel's render target after the real panel pass. A layer sized to what
//! its group draws is checked against the window-sized layer it replaced,
//! pixel by pixel, and a moving fading group against making textures.
//! `group_fade_gpu_timings` (ignored) measures the layers' GPU time.
use super::render_test_support::{Image, keep_image, offscreen_ctx, read_texture};
use crate::prelude::*;
use crate::ui::{Ui, UiId, UiInput, UiLayout, UiTween};

const SIZE: (u32, u32) = (96, 64);

/// Where to look: inside the red square only, inside both, inside the blue
/// square only, and outside the group.
const RED_ONLY: (u32, u32) = (16, 28);
const OVERLAP: (u32, u32) = (44, 32);
const BLUE_ONLY: (u32, u32) = (68, 36);
const OUTSIDE: (u32, u32) = (90, 4);

/// The two squares in one group in `ui` (`SIZE` pixels), at `opacity`,
/// fading as a group or piece by piece, laid out.
fn squares(ui: &mut Ui, as_group: bool, opacity: f32) -> UiId {
    let (w, h) = (SIZE.0 as f32, SIZE.1 as f32);
    let at = |x: f32, y: f32, sw: f32, sh: f32| UiLayout::anchored(x / w, y / h, sw / w, sh / h);
    let group = ui
        .root()
        .layout(UiLayout::fill())
        .background(Color::TRANSPARENT)
        .border_width(0.0)
        .id();
    for (rect, color) in [
        (at(8.0, 8.0, 48.0, 40.0), Color::rgb(1.0, 0.0, 0.0)),
        (at(32.0, 16.0, 48.0, 40.0), Color::rgb(0.0, 0.0, 1.0)),
    ] {
        ui.panel(group).layout(rect).background(color).border_width(0.0);
    }
    ui.set_fade_as_group(group, as_group);
    ui.push_modeless(group);
    if opacity < 1.0 {
        ui.animate(group, UiTween::opacity(opacity).seconds(0.0));
    }
    ui.frame(vec2(w, h), &still());
    assert_eq!(ui.drawn(group).unwrap().opacity, opacity);
    group
}

/// No pointer over the UI, no time passing.
fn still() -> UiInput {
    UiInput {
        pointer: vec2(-10.0, -10.0),
        ..Default::default()
    }
}

/// An offscreen game of `size` with a black window and no 3D sky.
fn game(size: (u32, u32)) -> Option<crate::ctx::Ctx> {
    let mut ctx = offscreen_ctx(size)?;
    ctx.scene3d.sky.atmosphere = false;
    ctx.scene3d.lighting.shadows = false;
    ctx.gfx.clear_color = Color::BLACK;
    Some(ctx)
}

/// A game drawing the two squares in one group, at `opacity`, fading as a
/// group or piece by piece.
fn scene(as_group: bool, opacity: f32) -> Option<crate::ctx::Ctx> {
    let mut ctx = game(SIZE)?;
    squares(&mut ctx.ui, as_group, opacity);
    Some(ctx)
}

/// Draws a frame and reads the window back.
fn shot(ctx: &mut crate::ctx::Ctx) -> Image {
    let path = std::env::temp_dir().join(format!(
        "ponko-group-fade-{}-{:?}.png",
        std::process::id(),
        std::thread::current().id()
    ));
    ctx.gfx.queue_screenshot(path.clone(), 0, None);
    assert_eq!(ctx.render_frame(), crate::render::FrameStatus::Offscreen);
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

fn pixel(image: &Image, (x, y): (u32, u32)) -> [u8; 4] {
    image.pixel(x, y)
}

/// Whether `a` and `b` are the same color within `levels` in each channel.
fn near(a: [u8; 4], b: [u8; 4], levels: u8) -> bool {
    a.iter().zip(&b).take(3).all(|(x, y)| x.abs_diff(*y) <= levels)
}

#[test]
fn a_group_faded_as_one_image_does_not_show_its_children_through_each_other() {
    let Some(mut pieces) = scene(false, 0.5) else { return };
    let Some(mut group) = scene(true, 0.5) else { return };
    let (pieces, group) = (shot(&mut pieces), shot(&mut group));

    // Negative control: faded piece by piece, the red square shows through
    // the blue one where they overlap.
    let overlap = pixel(&pieces, OVERLAP);
    assert!(
        overlap[0] > 90 && overlap[2] > 120,
        "piece by piece, the overlap mixes red into blue: {overlap:?}"
    );

    // As one image, the overlap is the blue square alone, half faded.
    let overlap = pixel(&group, OVERLAP);
    assert!(
        overlap[0] < 12 && overlap[1] < 12 && overlap[2] > 120,
        "as a group, the overlap is the top square only: {overlap:?}"
    );
    let blue = pixel(&group, BLUE_ONLY);
    assert!(
        near(overlap, blue, 3),
        "the overlap is as blue as the blue square alone: {overlap:?} and {blue:?}"
    );

    // Where nothing overlaps, both ways give the same picture: the layer is
    // shown at the group's opacity exactly (premultiplied, no double fade).
    for (what, at) in [("red only", RED_ONLY), ("blue only", BLUE_ONLY), ("outside", OUTSIDE)] {
        let (a, b) = (pixel(&pieces, at), pixel(&group, at));
        assert!(near(a, b, 3), "{what}: piece by piece {a:?}, as a group {b:?}");
    }
    let red = pixel(&group, RED_ONLY);
    assert!(
        red[0] > 150 && red[0] < 230 && red[2] < 12,
        "half faded red, not full and not gone: {red:?}"
    );
    assert!(
        near(pixel(&group, OUTSIDE), [0, 0, 0, 255], 2),
        "the window around it stays black"
    );
}

#[test]
fn a_group_at_rest_draws_what_it_draws_without_the_setting() {
    let Some(mut plain) = scene(false, 1.0) else { return };
    let Some(mut group) = scene(true, 1.0) else { return };
    let (plain, group) = (shot(&mut plain), shot(&mut group));
    for at in [RED_ONLY, OVERLAP, BLUE_ONLY, OUTSIDE] {
        assert_eq!(pixel(&plain, at), pixel(&group, at), "at {at:?}");
    }
    assert!(pixel(&group, OVERLAP)[2] > 240, "opaque blue on top");
}

/// A game with the two squares in a world-space panel of `SIZE` pixels
/// (and nothing on the screen), and the panel's render target.
fn panel_scene(as_group: bool, opacity: f32) -> Option<(crate::ctx::Ctx, u32)> {
    let mut ctx = game(SIZE)?;
    let target = ctx.gfx.create_render_target(SIZE.0, SIZE.1);
    let mut panel = crate::d3::UiPanel::new(target, vec2(1.5, 1.0));
    squares(&mut panel.ui, as_group, opacity);
    ctx.world.spawn((crate::d3::Transform::default(), panel));
    Some((ctx, target.texture().id))
}

/// Draws a frame and reads the panel's render target back: linear RGBA,
/// row by row.
fn panel_shot(ctx: &mut crate::ctx::Ctx, target: u32) -> Vec<[f32; 4]> {
    assert!(ctx.render_frame().drew());
    let renderer = ctx.gfx.renderer.as_ref().expect("an offscreen renderer");
    let (device, queue, texture) = renderer.texture_for_test(target);
    let texture = texture.expect("the panel's render target");
    assert_eq!(texture.format(), wgpu::TextureFormat::Rgba16Float);
    let bytes = read_texture(device, queue, texture);
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|p| std::array::from_fn(|c| super::batch::f16_value(u16::from_le_bytes([p[2 * c], p[2 * c + 1]]))))
        .collect()
}

fn at(image: &[[f32; 4]], (x, y): (u32, u32)) -> [f32; 4] {
    image[(y * SIZE.0 + x) as usize]
}

#[test]
fn a_group_in_a_world_space_panel_fades_as_one_image_too() {
    let Some((mut pieces, pieces_target)) = panel_scene(false, 0.5) else {
        return;
    };
    let Some((mut group, group_target)) = panel_scene(true, 0.5) else {
        return;
    };
    let (pieces, group) = (
        panel_shot(&mut pieces, pieces_target),
        panel_shot(&mut group, group_target),
    );

    // Negative control: piece by piece, the red square shows through the
    // blue one in the panel (red 1 at half, then half covered: 0.25).
    let overlap = at(&pieces, OVERLAP);
    assert!(
        overlap[0] > 0.2 && overlap[2] > 0.45,
        "piece by piece, the overlap mixes red into blue: {overlap:?}"
    );

    // As one image, the overlap is the blue square alone at half.
    let overlap = at(&group, OVERLAP);
    assert!(
        overlap[0] < 0.01 && overlap[1] < 0.01 && (overlap[2] - 0.5).abs() < 0.01,
        "as a group, the overlap is the top square only: {overlap:?}"
    );
    let blue = at(&group, BLUE_ONLY);
    assert!(
        (0..4).all(|c| (overlap[c] - blue[c]).abs() < 0.01),
        "the overlap is as blue as the blue square alone: {overlap:?} and {blue:?}"
    );
    // Where nothing overlaps, both ways give the same panel.
    for (what, p) in [("red only", RED_ONLY), ("blue only", BLUE_ONLY), ("outside", OUTSIDE)] {
        let (a, b) = (at(&pieces, p), at(&group, p));
        assert!(
            (0..3).all(|c| (a[c] - b[c]).abs() < 0.01),
            "{what}: piece by piece {a:?}, as a group {b:?}"
        );
    }
    let red = at(&group, RED_ONLY);
    assert!((red[0] - 0.5).abs() < 0.01 && red[2] < 0.01, "half red: {red:?}");
    assert!(
        at(&group, OUTSIDE).iter().all(|&c| c < 0.001),
        "the panel stays clear outside"
    );
}

const WIDE: (u32, u32) = (200, 120);

/// A fading group on a `WIDE` window: an offset panel with a label (text
/// at fractional positions), a child reaching out of it and a fading
/// group inside it, over a gray square. Returns the game and the group.
fn layered_scene() -> Option<(crate::ctx::Ctx, UiId)> {
    let mut ctx = game(WIDE)?;
    let ui = &mut ctx.ui;
    let root = ui
        .root()
        .layout(UiLayout::fill())
        .background(Color::TRANSPARENT)
        .border_width(0.0)
        .id();
    ui.panel(root)
        .layout(UiLayout::anchored(0.0, 0.0, 0.5, 0.5))
        .background(Color::rgb(0.5, 0.5, 0.5))
        .border_width(0.0);
    let group = ui
        .panel(root)
        .layout(UiLayout::anchored(0.2, 0.25, 0.4, 0.4))
        .background(Color::rgb(0.1, 0.3, 0.8))
        .border_width(1.5)
        .id();
    ui.label(group, "Fade me");
    ui.panel(group)
        .layout(UiLayout::anchored(0.7, 0.6, 0.6, 0.55))
        .background(Color::rgb(0.9, 0.6, 0.1))
        .border_width(0.0);
    let inner = ui
        .panel(group)
        .layout(UiLayout::anchored(0.1, 0.55, 0.3, 0.3))
        .background(Color::rgb(0.2, 0.9, 0.3))
        .border_width(0.0)
        .id();
    ui.set_fade_as_group(group, true);
    ui.set_fade_as_group(inner, true);
    ui.push_modeless(root);
    ui.animate(group, UiTween::opacity(0.6).seconds(0.0));
    ui.animate(inner, UiTween::opacity(0.5).seconds(0.0));
    ctx.ui.frame(vec2(WIDE.0 as f32, WIDE.1 as f32), &still());
    Some((ctx, group))
}

#[test]
fn a_layer_the_size_of_its_group_draws_what_the_window_sized_layer_drew() {
    let Some((mut whole, _)) = layered_scene() else { return };
    let Some((mut fit, _)) = layered_scene() else { return };
    // The reference: each layer the whole window, as before.
    whole.ui_state.fade.whole_target = true;
    let window_sized = shot(&mut whole);
    let window_textures = whole.ui_state.fade.texture_sizes().to_vec();
    let fitted = shot(&mut fit);
    let fitted_textures = fit.ui_state.fade.texture_sizes().to_vec();
    assert_eq!(window_textures, [[WIDE.0, WIDE.1]; 2]);
    let area = |t: &[[u32; 2]]| t.iter().map(|[w, h]| u64::from(w * h)).sum::<u64>();
    assert!(
        area(&fitted_textures) * 2 < area(&window_textures),
        "{fitted_textures:?} against {window_textures:?}"
    );
    // Something faded is there: the group's fill, half over black.
    assert!(window_sized.pixel(70, 40)[2] > 60, "{:?}", window_sized.pixel(70, 40));
    // Every pixel the same, or within one level: the layer's camera is the
    // window's moved by whole pixels, computed in f32, so a glyph at a
    // fractional position may sample its atlas a few millionths of a texel
    // apart.
    let (mut differ, mut most) = (0, 0);
    for (a, b) in window_sized.rgba.iter().zip(&fitted.rgba) {
        let d = a.abs_diff(*b);
        differ += usize::from(d > 0);
        most = most.max(d);
    }
    eprintln!(
        "fitted layers {fitted_textures:?} against {window_textures:?}: {differ} channels differ, by at most {most}"
    );
    assert!(most <= 1, "{differ} channels differ, by up to {most} levels");
}

#[test]
fn a_moving_fading_group_makes_no_layer_texture_once_warm() {
    let Some((mut ctx, group)) = layered_scene() else {
        return;
    };
    let linear = crate::skeleton2d::Bezier::LINEAR;
    ctx.ui
        .animate(group, UiTween::offset(vec2(60.0, 20.0)).seconds(1.0).with_ease(linear));
    let size = vec2(WIDE.0 as f32, WIDE.1 as f32);
    let step = UiInput { dt: 0.05, ..still() };
    let made = |ctx: &crate::ctx::Ctx| ctx.gfx.renderer.as_ref().unwrap().ui_layer_textures_made();
    ctx.ui.frame(size, &step);
    assert!(ctx.render_frame().drew());
    let warm = made(&ctx);
    assert_eq!(warm, 2, "one texture for each of the two layers");
    let before = ctx.ui.rect(group).unwrap();
    for _ in 0..12 {
        ctx.ui.frame(size, &step);
        assert!(ctx.render_frame().drew());
    }
    let moved = ctx.ui.drawn(group).unwrap();
    assert!(
        moved.rect.min.x > before.min.x + 30.0,
        "it moved: {before:?} to {:?}",
        moved.rect
    );
    assert_eq!(made(&ctx), warm, "no texture made while it moved");
}

/// GPU time of a fading group: the layers' passes ("ui layers") and the
/// sprite pass that shows them, offscreen at 1080p and 4K, faded piece by
/// piece, as a group in a window-sized layer and as a group in a layer of
/// its size. `cargo test --release -p ponko --lib --features 3d
/// group_fade_gpu_timings -- --ignored --nocapture`.
#[test]
#[ignore = "offscreen GPU timestamps at 1080p and 4K; run in release with --ignored --nocapture"]
fn group_fade_gpu_timings() {
    for (name, size) in [("1080p", (1920, 1080)), ("4K", (3840, 2160))] {
        for way in ["piece by piece", "window-sized layer", "fitted layer"] {
            let Some(gpu) = super::render_test_support::gpu_with(|adapter| {
                Ok(wgpu::DeviceDescriptor {
                    required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
                    ..Default::default()
                })
            }) else {
                return;
            };
            let mut ctx = crate::ctx::Ctx::new(1.0 / 60.0, vec2(size.0 as f32, size.1 as f32));
            ctx.audio.headless = true;
            ctx.gfx.renderer = Some(super::gpu::Renderer::offscreen(gpu, size));
            ctx.scene3d.sky.atmosphere = false;
            ctx.scene3d.lighting.shadows = false;
            ctx.gfx.clear_color = Color::BLACK;
            // A menu of 480 x 400 points: a panel, twelve rows of a label
            // on a fill, fading at half.
            let ui = &mut ctx.ui;
            let menu = ui
                .root()
                .layout(UiLayout::centered(480.0, 400.0).padding(8.0).gap(4.0))
                .id();
            for i in 0..12 {
                let row = ui.panel(menu).layout(UiLayout::fixed(460.0, 26.0)).id();
                ui.label(row, format!("Option {i}"));
            }
            ui.set_fade_as_group(menu, way != "piece by piece");
            ui.push_modeless(menu);
            ui.animate(menu, UiTween::opacity(0.5).seconds(0.0));
            ctx.ui_state.fade.whole_target = way == "window-sized layer";
            let renderer = ctx.gfx.renderer.as_mut().unwrap();
            renderer.set_gpu_timing_every(1);
            let started = std::time::Instant::now();
            for _ in 0..240 {
                ctx.ui.frame(vec2(size.0 as f32, size.1 as f32), &still());
                ctx.render_frame();
            }
            let renderer = ctx.gfx.renderer.as_ref().unwrap();
            let pass = |n: &str| {
                renderer
                    .gpu_passes()
                    .iter()
                    .filter(|p| p.name == n)
                    .map(|p| p.ms)
                    .sum::<f32>()
            };
            let total: f32 = renderer.gpu_passes().iter().map(|p| p.ms).sum();
            let layers = ctx.ui_state.fade.texture_sizes().to_vec();
            eprintln!(
                "{name} {way}: ui layers {:.4} ms, sprites {:.4} ms, all passes {:.4} ms; layer textures {layers:?} ({:.1} MB); {:.1} s",
                pass("ui layers"),
                pass("sprites"),
                total,
                layers.iter().map(|[w, h]| f64::from(w * h) * 4.0).sum::<f64>() / 1e6,
                started.elapsed().as_secs_f32()
            );
        }
    }
}
