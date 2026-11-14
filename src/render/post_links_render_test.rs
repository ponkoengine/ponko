//! The post chain's links against the renderer's two views of them: each
//! link switched on or off replaces the scene's images exactly when it has
//! images of its own that come or go, and adds exactly its own passes to
//! the frame's draw calls (the public `stats.draw_calls`), and the frame
//! checks see exactly the links that are on. The renderer
//! asks the post chain for both (`PostTargets::stale`,
//! `PostChain::draw_calls`); these numbers are the ones it gave when it
//! checked the chain's fields itself.
//!
//! Skips without a GPU adapter.

use super::*;
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::static_batch_render_test::{BatchRig, RigSetup};
use crate::render::{AntiAliasing, MaterialDesc, Model};

/// The post links the frame checks saw in the last frame drawn (debug
/// builds check every drawn frame against them): bloom, FXAA, SMAA, upscale.
fn links() -> [bool; 4] {
    let features = last_features().expect("the frame was checked");
    assert_eq!(
        features.contains(FrameFeatures::DOF),
        DOF.with(|on| on.get()),
        "depth of field as set"
    );
    [
        FrameFeatures::BLOOM,
        FrameFeatures::FXAA,
        FrameFeatures::SMAA,
        FrameFeatures::UPSCALE,
    ]
    .map(|f| features.contains(f))
}

thread_local! {
    /// Whether the test switched depth of field on (`links` checks it).
    static DOF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Draws a frame; whether it replaced the scene's images, and its draw calls.
fn frame(rig: &mut BatchRig) -> (bool, usize) {
    let before = rig.renderer.targets.as_ref().map(|t| t.hdr.clone());
    rig.frame();
    let after = rig.renderer.targets.as_ref().map(|t| t.hdr.clone());
    (before != after, rig.renderer.draw_calls())
}

#[test]
fn each_post_link_replaces_images_and_counts_draws_as_its_own() {
    // 128x128: bloom has four mips (64, 32, 16 and 8 pixels).
    let Some(mut rig) = BatchRig::new(RigSetup::default()) else {
        return;
    };
    let cube = rig.mesh(&MeshData::cube(1.0), MaterialDesc::default());
    rig.ctx
        .world
        .spawn((Transform3D::at(vec3(0.0, 0.0, -4.0)), Model::new(cube)));
    rig.ctx.scene3d.post.bloom = 0.0;
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::None;
    rig.ctx.scene3d.post.render_scale = 1.0;
    let (made, plain) = frame(&mut rig);
    assert!(made, "the first frame makes the images");
    assert_eq!(frame(&mut rig), (false, plain), "nothing changed: nothing is replaced");
    assert_eq!(links(), [false; 4]);

    // Bloom: its mips are always there; 4 downsamples and 3 upsamples.
    rig.ctx.scene3d.post.bloom = 0.3;
    assert_eq!(frame(&mut rig), (false, plain + 7), "bloom on");
    assert_eq!(links(), [true, false, false, false]);
    assert_eq!(frame(&mut rig), (false, plain + 7), "bloom stays on");
    rig.ctx.scene3d.post.bloom = 0.0;
    assert_eq!(frame(&mut rig), (false, plain), "bloom off");

    // FXAA: one pass, reading an image of its own.
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Fxaa;
    assert_eq!(frame(&mut rig), (true, plain + 1), "FXAA on");
    assert_eq!(links(), [false, true, false, false]);
    assert_eq!(frame(&mut rig), (false, plain + 1), "FXAA stays on");
    // SMAA: three passes; FXAA's image goes.
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Smaa;
    assert_eq!(frame(&mut rig), (true, plain + 3), "FXAA to SMAA");
    assert_eq!(links(), [false, false, true, false]);
    assert_eq!(frame(&mut rig), (false, plain + 3), "SMAA stays on");
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::None;
    assert_eq!(
        frame(&mut rig),
        (false, plain),
        "SMAA off: no image of the chain's goes"
    );

    // A render scale: the scene's images shrink (a new size), and the
    // scaling pass has an image of its own. It was never counted as a draw.
    rig.ctx.scene3d.post.render_scale = 0.5;
    assert_eq!(frame(&mut rig), (true, plain), "render scale on");
    assert_eq!(links(), [false, false, false, true]);
    assert!(rig.renderer.scales_output());
    assert_eq!(frame(&mut rig), (false, plain), "render scale stays");
    // At 64x64 bloom has three mips: 3 downsamples and 2 upsamples.
    rig.ctx.scene3d.post.bloom = 0.3;
    assert_eq!(frame(&mut rig), (false, plain + 5), "bloom at half size");
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Fxaa;
    assert_eq!(frame(&mut rig), (true, plain + 6), "FXAA with bloom at half size");
    assert_eq!(links(), [true, true, false, true]);
    rig.ctx.scene3d.post.render_scale = 1.0;
    assert_eq!(frame(&mut rig), (true, plain + 8), "back to full size");
    assert_eq!(links(), [true, true, false, false]);
    assert!(!rig.renderer.scales_output());

    // Depth of field: one pass, and no image of its own (it swaps between
    // the lit frame and the effects' scratch).
    rig.ctx.scene3d.post.bloom = 0.0;
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::None;
    assert_eq!(frame(&mut rig), (true, plain), "FXAA off again");
    rig.ctx.scene3d.post.depth_of_field.enabled = true;
    DOF.with(|on| on.set(true));
    assert_eq!(frame(&mut rig), (false, plain + 1), "depth of field on");
    assert_eq!(links(), [false; 4]);
    assert_eq!(frame(&mut rig), (false, plain + 1), "depth of field stays on");
    rig.ctx.scene3d.post.depth_of_field.enabled = false;
    DOF.with(|on| on.set(false));
    assert_eq!(frame(&mut rig), (false, plain), "depth of field off");
    assert_eq!(links(), [false; 4]);
}
