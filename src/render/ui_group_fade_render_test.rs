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
use super::render_test_support::{Image, keep_image, offscreen_ctx};
use crate::prelude::*;
use crate::ui::{UiInput, UiLayout, UiTween};

const SIZE: (u32, u32) = (96, 64);

/// Where to look: inside the red square only, inside both, inside the blue
/// square only, and outside the group.
const RED_ONLY: (u32, u32) = (16, 28);
const OVERLAP: (u32, u32) = (44, 32);
const BLUE_ONLY: (u32, u32) = (68, 36);
const OUTSIDE: (u32, u32) = (90, 4);

/// A game drawing the two squares in one group, at `opacity`, fading as a
/// group or piece by piece.
fn scene(as_group: bool, opacity: f32) -> Option<crate::ctx::Ctx> {
    let mut ctx = offscreen_ctx(SIZE)?;
    ctx.scene3d.sky.atmosphere = false;
    ctx.scene3d.lighting.shadows = false;
    ctx.gfx.clear_color = Color::BLACK;
    let (w, h) = (SIZE.0 as f32, SIZE.1 as f32);
    let at = |x: f32, y: f32, sw: f32, sh: f32| UiLayout::anchored(x / w, y / h, sw / w, sh / h);
    let ui = &mut ctx.ui;
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
    let still = UiInput {
        pointer: vec2(-10.0, -10.0),
        ..Default::default()
    };
    ctx.ui.frame(vec2(w, h), &still);
    assert_eq!(ctx.ui.drawn(group).unwrap().opacity, opacity);
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
