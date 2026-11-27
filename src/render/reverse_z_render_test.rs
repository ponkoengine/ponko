//! Reverse-Z scene depth with a tiny near plane: a first-person camera's
//! `near = 0.01` next to a far plane kilometres away. With standard depth
//! the depth buffer gets too coarse at mid range for the passes that
//! rebuild positions from it (ambient occlusion, contact shadows, temporal
//! reprojection), and their noise crawls between frames of a still scene;
//! reverse-Z keeps them steady and makes the near plane all but invisible
//! beyond it. The standard-Z side of the comparison is the depth step size
//! (`three::tests::reverse_z_depth_steps_are_micrometres_where_standard_z_has_centimetres`):
//! the renderer has no standard-Z mode left to draw with. Skips without a GPU.
use super::regression_stage::{Setup, Stage};
use super::render_test_support::{Image, Tolerance, diff, needs};
use crate::prelude::*;
use crate::render::{AntiAliasing, GpuFeature};

/// The side of the frames.
const SIDE: u32 = 192;

/// A flat field 100 to 400 metres from the camera with big blocks on it
/// (their creases and feet take ambient occlusion and contact shadows),
/// seen through a narrow lens, with `near` and a far plane 16 km away.
fn field(stage: &mut Stage, near: f32) {
    let ground = stage.cube(Color::rgb(0.6, 0.6, 0.6));
    stage.block(ground, vec3(-200.0, -1.0, -460.0), vec3(200.0, 0.0, -60.0));
    let block = stage.cube(Color::rgb(0.8, 0.75, 0.7));
    for (i, z) in [-110.0, -170.0, -230.0, -290.0, -350.0, -400.0].into_iter().enumerate() {
        for x in [-36.0, -12.0, 12.0, 36.0] {
            let x = x + if i % 2 == 0 { 0.0 } else { 12.0 };
            stage.block(
                block,
                vec3(x - 4.0, 0.0, z - 4.0),
                vec3(x + 4.0, 6.0 + i as f32, z + 4.0),
            );
        }
    }
    let ctx = &mut stage.ctx;
    let camera = &mut ctx.scene3d.camera;
    camera.position = vec3(0.0, 60.0, 20.0);
    camera.look_at(vec3(0.0, 0.0, -260.0));
    camera.fov_y = 22f32.to_radians();
    camera.near = near;
    camera.far = 16_000.0;
    let lighting = &mut ctx.scene3d.lighting;
    lighting.ambient_intensity = 0.6;
    lighting.sun_intensity = 1.5;
    lighting.sun_direction = vec3(-0.6, -0.5, -0.3);
    let post = &mut ctx.scene3d.post;
    post.ambient_occlusion.enabled = true;
    post.ambient_occlusion.radius = 3.0;
    post.ambient_occlusion.intensity = 2.0;
    post.contact_shadows = 2.5;
}

/// A stage of [`SIDE`] pixels with [`field`] at `near`; `None` without a
/// GPU or without screen-space ambient occlusion.
fn stage(near: f32) -> Option<Stage> {
    let mut stage = Stage::with(Setup {
        size: (SIDE, SIDE),
        ..Setup::default()
    })?;
    if !needs(&stage.device, GpuFeature::AmbientOcclusion) {
        return None;
    }
    field(&mut stage, near);
    Some(stage)
}

/// The rows of the frame that show the field between 100 and 400 m.
fn field_rows(stage: &Stage) -> std::ops::Range<u32> {
    let near_edge = stage.pixel(vec3(0.0, 0.0, -100.0)).1;
    let far_edge = stage.pixel(vec3(0.0, 0.0, -400.0)).1;
    far_edge.min(near_edge)..far_edge.max(near_edge) + 1
}

/// The part of `image` in `rows`, as an image of its own.
fn crop(image: &Image, rows: std::ops::Range<u32>) -> Image {
    let row = (image.width * 4) as usize;
    Image {
        width: image.width,
        height: rows.end - rows.start,
        rgba: image.rgba[rows.start as usize * row..rows.end as usize * row].to_vec(),
    }
}

#[test]
fn ao_and_contact_shadows_hold_still_at_100_to_400_m_with_a_near_plane_of_a_centimetre() {
    let Some(mut stage) = stage(0.01) else { return };
    if !needs(&stage.device, GpuFeature::TemporalAntialiasing) {
        return;
    }
    stage.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    stage.ctx.scene3d.post.ao_temporal = true;
    let rows = field_rows(&stage);
    assert!(
        rows.end - rows.start > SIDE / 3,
        "the field fills the frame: rows {rows:?}"
    );
    // Settled: TAA and the AO history have converged.
    let settled = crop(&stage.settled(48), rows.clone());
    let frames: Vec<Image> = (0..16).map(|_| crop(&stage.image(), rows.clone())).collect();
    // Not a blank field: the occlusion is there to hold still. Without
    // ambient occlusion and contact shadows the same frames are visibly
    // lighter at the blocks' feet and creases.
    stage.ctx.scene3d.post.ambient_occlusion.enabled = false;
    stage.ctx.scene3d.post.contact_shadows = 0.0;
    let unshaded = crop(&stage.settled(48), rows);
    let shading = diff(&settled, &unshaded, 6);
    eprintln!("ao and contact shadows against none: {shading}");
    assert!(
        shading.max_channel >= 24 && shading.outliers as f32 >= 0.02 * shading.pixels as f32,
        "ambient occlusion and contact shadows show on the field: {shading}"
    );
    // Frame to frame of the still scene, over a whole cycle of the
    // antialiasing jitter (16 frames) and the occlusion's sample pattern
    // (8): no crawling or pulsing shade. Before the occlusion history
    // allowed for the jitter on distant, grazing ground, every 8th frame
    // brightened 60% of the field by about 3 levels.
    let steady = Tolerance {
        channel: 4,
        outliers: 0.01,
        edge: 0,
    };
    for (k, frame) in frames.iter().enumerate() {
        let d = diff(&settled, frame, steady.channel);
        assert!(d.within(steady), "frame {} after settling differs: {d}", k + 1);
        assert!(d.mean < 1.0, "frame {} after settling: the shading crawls ({d})", k + 1);
    }
    stage.finish();
}

#[test]
fn temporal_ao_turns_its_slices_without_pulsing_the_far_field() {
    // The same field with only the occlusion's history (no temporal
    // antialiasing, whose jitter moves pixels on its own): what changes
    // from frame to frame is the turn of the slices alone. Every turn has
    // to read the same: with all pixels turning in lockstep and the
    // estimate divided by the slice count, the open ground 300 to 400 m
    // away read darker on some of the 8 turns and the field pulsed every 8
    // frames (mean up to 0.77 per frame on an Apple M4 Max; without
    // rotation it is exactly still).
    let Some(mut stage) = stage(0.01) else { return };
    stage.ctx.scene3d.post.ao_temporal = true;
    let rows = field_rows(&stage);
    let settled = crop(&stage.settled(48), rows.clone());
    let still = Tolerance {
        channel: 4,
        outliers: 0.002,
        edge: 0,
    };
    for k in 0..16 {
        let frame = crop(&stage.image(), rows.clone());
        let d = diff(&settled, &frame, still.channel);
        assert!(d.within(still), "frame {} after settling differs: {d}", k + 1);
        assert!(
            d.mean < 0.2,
            "frame {} after settling: the occlusion pulses with its turn ({d})",
            k + 1
        );
    }
    stage.finish();
}

#[test]
fn a_near_plane_of_a_centimetre_draws_the_world_beyond_it_as_one_of_ten() {
    let images = [0.1, 0.01].map(|near| {
        let mut stage = stage(near)?;
        let image = stage.settled(4);
        stage.finish();
        Some(image)
    });
    let [Some(tenth), Some(hundredth)] = images else { return };
    // The same picture, but for rounding.
    let same = Tolerance {
        channel: 3,
        outliers: 0.002,
        edge: 0,
    };
    let d = diff(&tenth, &hundredth, same.channel);
    eprintln!("near 0.1 against near 0.01: {d}");
    assert!(d.within(same), "the near plane changes how the world shades: {d}");
    // Negative control: the comparison sees a small change: the camera
    // half a metre to the side.
    let Some(mut stage) = stage(0.01) else { return };
    let moved = stage.shifted(vec3(0.5, 0.0, 0.0), 4);
    stage.finish();
    let control = diff(&hundredth, &moved, same.channel);
    eprintln!("the camera half a metre aside: {control}");
    assert!(!control.within(same), "the comparison sees a small change: {control}");
}
