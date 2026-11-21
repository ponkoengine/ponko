//! The image regression suite: one controlled offscreen scene per part of
//! the frame (opaque, transparency, alpha cutout, shadows, skinned meshes,
//! reflections, ambient occlusion, global illumination, post-processing
//! and the output formats). Each test first asserts what must hold on any
//! GPU ("the floor behind the box is in shadow"), with a negative control,
//! then compares the frame with its reference image within a tolerance
//! (see `render_test_support` and the book's "GPU regression tests").
//!
//! The sequences (resizes, camera cuts, capability fallbacks, shader hot
//! reload) are in `regression_sequence_render_test.rs`; both draw on
//! `regression_stage.rs`. They skip, with the reason logged, without a GPU
//! adapter or where a forced tier lacks the effect under test.
use super::regression_stage::*;
use super::render_test_support::*;
use crate::prelude::*;
use crate::render::{AntiAliasing, GpuFeature, Tonemap};

#[test]
fn opaque_boxes_are_lit_by_the_sun_and_hide_what_is_behind_them() {
    let Some(mut stage) = Stage::new() else { return };
    let (red_front, blue_top, overlap) = opaque(&mut stage);
    let image = stage.image();
    let (red, blue, hidden) = (
        stage.color_at(&image, red_front),
        stage.color_at(&image, blue_top),
        stage.color_at(&image, overlap),
    );
    let floor = stage.color_at(&image, vec3(-2.5, 0.0, 2.0));
    eprintln!("red face {red:?}, blue top {blue:?}, red over blue {hidden:?}, floor {floor:?}");
    assert!(red[0] > 60.0 && red[0] > red[2] * 3.0, "the near box is red: {red:?}");
    assert!(
        blue[2] > 60.0 && blue[2] > blue[0] * 3.0,
        "the far box is blue: {blue:?}"
    );
    // The depth test: where the red box stands in front, no blue shows.
    assert!(
        hidden[0] > hidden[2] * 3.0,
        "the near box hides the far one: {hidden:?}"
    );
    // Lit from above: the sunlit top is brighter than the same grey would
    // be in the ambient light alone.
    assert!(
        luma(floor) > 60.0 && (floor[0] - floor[2]).abs() < 6.0,
        "the floor is sunlit grey: {floor:?}"
    );
    assert_region(&image, 0..SIZE, 0..6, 1.0, "the sky above is black", |p| {
        p[..3] == [0; 3]
    });
    // Negative control: without the sun the same floor is dim.
    stage.ctx.scene3d.lighting.sun_intensity = 0.0;
    let dark = stage.image();
    let unlit = stage.color_at(&dark, vec3(-2.5, 0.0, 2.0));
    assert!(
        luma(unlit) < luma(floor) * 0.6,
        "the sun lights it: {unlit:?} vs {floor:?}"
    );
    stage.ctx.scene3d.lighting.sun_intensity = 1.0;
    check_reference(
        "regression_opaque",
        &stage.image(),
        REFERENCE,
        "A red box (1.2 wide) in front of a taller blue box on a grey floor, white sun from the upper left front \
         (direction -0.5, -1, -0.35) at intensity 1 plus white ambient light at 0.05, no shadows; camera at \
         (0, 2.4, 5) looking at (0, 0.6, 0); 128x128, 4x multisampling, no tonemapping, no bloom.",
        &stage.info,
    );
    reference_rejects("regression_opaque", &dark, REFERENCE, "the sun off");
    let aside = stage.shifted(vec3(0.15, 0.0, 0.0), 1);
    reference_rejects("regression_opaque", &aside, REFERENCE, "the camera 0.15 to the side");
    stage.finish();
}

/// An sRGB-encoded colour (0..=255) as linear light (0..=1).
fn decoded(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| {
        let v = v / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    })
}

/// What the panes `set` (indices into `PANES`) show where only they
/// overlap, given what each shows alone over black (`alone`, linear):
/// blended in the order `order` gives them, first drawn first.
fn blended(alone: &[[f32; 3]; 3], order: &[usize]) -> [f32; 3] {
    let mut c = [0.0f32; 3];
    for &i in order {
        for (k, channel) in c.iter_mut().enumerate() {
            *channel = *channel * (1.0 - PANE_ALPHA) + alone[i][k];
        }
    }
    c
}

fn close(a: [f32; 3], b: [f32; 3], slack: f32) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= slack)
}

/// Checks every overlap of the panes in `image` against far-to-near
/// blending of what each pane shows alone; `far_to_near` lists the panes
/// that way for the camera that drew it.
fn assert_blended_far_to_near(stage: &Stage, image: &Image, far_to_near: [usize; 3], what: &str) {
    let seen = |set: &[usize]| {
        let (x, y) = PANE_SPOTS.iter().find(|(s, _)| *s == set).unwrap().1;
        decoded(stage.color_at(image, vec3(x, y, 0.0)))
    };
    let alone = [seen(&[0]), seen(&[1]), seen(&[2])];
    eprintln!("{what}: each pane alone (linear) {alone:?}");
    for (i, (_, tint)) in PANES.iter().enumerate() {
        let hue = (0..3).max_by(|&a, &b| tint[a].total_cmp(&tint[b])).unwrap();
        assert!(
            alone[i][hue] > 0.2 && (0..3).all(|k| k == hue || alone[i][k] < alone[i][hue] * 0.5),
            "{what}: pane {i} shows its own colour: {:?}",
            alone[i]
        );
    }
    for (set, _) in &PANE_SPOTS[3..] {
        let order: Vec<usize> = far_to_near.iter().copied().filter(|i| set.contains(i)).collect();
        let reversed: Vec<usize> = order.iter().rev().copied().collect();
        let (got, want, wrong) = (seen(set), blended(&alone, &order), blended(&alone, &reversed));
        eprintln!("{what}: panes {set:?} drew {got:?}; far to near gives {want:?}, near to far {wrong:?}");
        assert!(
            close(got, want, 0.03),
            "{what}: panes {set:?}: {got:?}, wanted {want:?}"
        );
        // Negative control: the other order would show.
        assert!(
            !close(want, wrong, 0.08),
            "{what}: the order shows in {set:?}: {want:?} vs {wrong:?}"
        );
    }
}

#[test]
fn see_through_panes_blend_far_to_near_whatever_order_they_were_spawned_in() {
    let mut drawn = Vec::new();
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0]] {
        let Some(mut stage) = Stage::new() else { return };
        panes(&mut stage, order);
        let image = stage.image();
        assert_blended_far_to_near(&stage, &image, [2, 1, 0], &format!("spawned {order:?}"));
        drawn.push((order, image, stage));
    }
    for (order, image, _) in &drawn[1..] {
        assert_similar(
            image,
            &drawn[0].1,
            SAME_PICTURE,
            &format!("panes spawned {order:?} against those spawned near to far"),
        );
    }
    let (_, image, stage) = drawn.swap_remove(0);
    check_reference(
        "regression_transparency",
        &image,
        REFERENCE,
        "Three see-through 2x2 panes (alpha 0.5) of one mesh tinted red, green and blue, the red nearest and \
         each 0.1 behind the one before, overlapping like a Venn diagram, lit by white ambient light at 1 on \
         black; camera at (0, -0.2, 5.5) looking down -z; 128x128, 4x multisampling, no tonemapping.",
        &stage.info,
    );
    let mut stage = stage;
    let aside = stage.shifted(vec3(0.15, 0.0, 0.0), 1);
    reference_rejects(
        "regression_transparency",
        &aside,
        REFERENCE,
        "the camera 0.15 to the side",
    );
    // One pane fewer: what a pane dropped from the transparent pass looks like.
    let last = stage
        .ctx
        .world
        .query::<(Entity, &Model)>()
        .iter()
        .map(|(e, _)| e)
        .last()
        .unwrap();
    stage.ctx.world.despawn(last);
    reference_rejects("regression_transparency", &stage.image(), REFERENCE, "one pane missing");
    stage.finish();
}

#[test]
fn see_through_panes_stay_sorted_when_the_camera_goes_round_to_the_other_side() {
    let Some(mut stage) = Stage::new() else { return };
    panes(&mut stage, [0, 1, 2]);
    stage.image();
    // From behind, the blue pane is the nearest and the picture mirrors.
    stage.ctx.scene3d.camera.position = vec3(0.0, -0.2, -5.5);
    stage.ctx.scene3d.camera.look_at(vec3(0.0, -0.2, 0.0));
    let image = stage.image();
    assert_blended_far_to_near(&stage, &image, [0, 1, 2], "from behind");
    stage.finish();
}

#[test]
fn a_cutout_shows_what_is_behind_its_holes_and_its_shadow_has_them_too() {
    let Some(mut stage) = Stage::new() else { return };
    let (shader, [hole, solid, lit_spot, shadow]) = cutout(&mut stage, HOLES_SHADER);
    let image = stage.settled(3);
    let (through, sheet, spot, shade) = (
        stage.color_at(&image, hole),
        stage.color_at(&image, solid),
        stage.color_at(&image, lit_spot),
        stage.color_at(&image, shadow),
    );
    eprintln!(
        "through a hole {through:?}, the sheet {sheet:?}; floor under the hole {spot:?}, under the sheet {shade:?}"
    );
    assert!(
        sheet[0] > 120.0 && sheet[0] > sheet[2] * 2.5,
        "the sheet is yellow: {sheet:?}"
    );
    assert!(
        (through[0] - through[2]).abs() < 12.0,
        "the grey floor shows through the hole: {through:?}"
    );
    // The shadow has the same holes: sunlit under a hole, dark beside it.
    assert!(
        luma(spot) > luma(shade) * 1.5,
        "the hole lets the sun through: {spot:?} vs {shade:?}"
    );
    // Negative control: with shadows off both floor points are lit alike.
    stage.ctx.scene3d.lighting.shadows = false;
    let plain = stage.settled(2);
    let (a, b) = (stage.color_at(&plain, lit_spot), stage.color_at(&plain, shadow));
    assert!(
        (luma(a) - luma(b)).abs() < 8.0,
        "without shadows the floor is even: {a:?} vs {b:?}"
    );
    stage.ctx.scene3d.lighting.shadows = true;
    check_reference(
        "regression_cutout",
        &stage.settled(3),
        REFERENCE,
        "A yellow 3x3 sheet with a 3x3 grid of round holes (alpha cutout through a custom surface shader), \
         1 above a grey floor, white sun at 45 degrees from the left casting its shadow with the same holes; \
         camera at (-0.5, 5.5, 3.5) looking at (-0.9, 0.3, 0); 128x128, 4x multisampling, no tonemapping.",
        &stage.info,
    );
    reference_rejects("regression_cutout", &plain, REFERENCE, "shadows off");
    let aside = stage.shifted(vec3(0.15, 0.0, 0.0), 3);
    reference_rejects("regression_cutout", &aside, REFERENCE, "the camera 0.15 to the side");
    stage.reload(shader, "holes.wgsl", STRIPES_SHADER).unwrap();
    let striped = stage.settled(3);
    reference_rejects(
        "regression_cutout",
        &striped,
        REFERENCE,
        "stripes cut out instead of holes",
    );
    stage.finish();
}

#[test]
fn a_box_shadows_the_floor_on_the_far_side_from_the_sun() {
    let Some(mut stage) = Stage::new() else { return };
    let (entity, in_shadow, in_sun) = shadowed(&mut stage);
    let image = stage.settled(3);
    let (shade, sun) = (stage.color_at(&image, in_shadow), stage.color_at(&image, in_sun));
    eprintln!("floor in the box's shadow {shade:?}, in the sun {sun:?}");
    assert!(luma(sun) > 60.0, "the open floor is sunlit: {sun:?}");
    assert!(
        luma(shade) < luma(sun) * 0.5,
        "the shadow is dark: {shade:?} vs {sun:?}"
    );
    // The shadow follows its caster: with the box moved away along z, the
    // two points swap.
    stage
        .ctx
        .world
        .get_mut::<crate::d3::Transform>(entity)
        .unwrap()
        .position
        .z += 2.0;
    let moved = stage.settled(3);
    let (was_shade, now_shade) = (stage.color_at(&moved, in_shadow), stage.color_at(&moved, in_sun));
    eprintln!("after moving the box: old spot {was_shade:?}, new spot {now_shade:?}");
    assert!(
        luma(was_shade) > luma(now_shade) * 2.0,
        "the shadow moved with the box: {was_shade:?} vs {now_shade:?}"
    );
    assert!((luma(was_shade) - luma(sun)).abs() < 10.0, "and left no trace behind");
    stage
        .ctx
        .world
        .get_mut::<crate::d3::Transform>(entity)
        .unwrap()
        .position
        .z -= 2.0;
    check_reference(
        "regression_shadows",
        &stage.settled(3),
        REFERENCE,
        "A white box (1 x 1.5 x 1) on a grey floor, white sun at 45 degrees from the left (direction 1, -1, 0) \
         casting its shadow to the right, white ambient light at 0.05; camera at (0, 2.4, 5) looking at \
         (0, 0.6, 0); 128x128, 4x multisampling, default shadow settings, no tonemapping.",
        &stage.info,
    );
    reference_rejects("regression_shadows", &moved, REFERENCE, "the box moved 2 along z");
    let aside = stage.shifted(vec3(0.15, 0.0, 0.0), 3);
    reference_rejects("regression_shadows", &aside, REFERENCE, "the camera 0.15 to the side");
    stage.ctx.scene3d.lighting.shadows = false;
    let unshadowed = stage.settled(2);
    reference_rejects("regression_shadows", &unshadowed, REFERENCE, "shadows off");
    stage.finish();
}

#[test]
fn a_posed_skeleton_bends_the_lit_bar_and_its_shadow() {
    let Some(mut stage) = Stage::new() else { return };
    let bar = skinned(&mut stage);
    let rest = stage.settled(3);
    // The bar stands at x -0.5: its upper half reaches up to 2.
    let (upright, leaning) = (vec3(-0.5, 1.6, 0.2), vec3(-1.2, 1.0, 0.2));
    let orange = |c: [f32; 3]| c[0] > 40.0 && c[0] > c[2] * 2.0;
    let (stands, beside) = (stage.color_at(&rest, upright), stage.color_at(&rest, leaning));
    assert!(
        orange(stands) && !orange(beside),
        "at rest the bar stands up: {stands:?}, {beside:?}"
    );
    // The upright bar's shadow lies along x on the floor: from 1 to 2 high
    // it falls 1 to 2 to the right of the bar's foot.
    let top_shadow = vec3(1.0, 0.0, 0.0);
    let lit_floor = luma(stage.color_at(&rest, vec3(1.0, 0.0, 2.0)));
    let shaded = luma(stage.color_at(&rest, top_shadow));
    assert!(
        shaded < lit_floor * 0.6,
        "the upper half casts a shadow: {shaded} vs {lit_floor}"
    );

    bend(&mut stage, bar, std::f32::consts::FRAC_PI_2);
    let bent = stage.settled(3);
    let (gone, leans) = (stage.color_at(&bent, upright), stage.color_at(&bent, leaning));
    eprintln!(
        "where the top stood: {stands:?} at rest, {gone:?} bent; to its left: {beside:?} at rest, {leans:?} bent"
    );
    assert!(!orange(gone), "bent, the upper half no longer stands there: {gone:?}");
    assert!(orange(leans), "bent, the upper half points left: {leans:?}");
    let was_bar = stage.color_at(&bent, upright);
    let was_shadow = luma(stage.color_at(&bent, top_shadow));
    eprintln!("bent: where it stood {was_bar:?}; where its top's shadow fell {was_shadow} (lit floor {lit_floor})");
    assert!(
        (was_shadow - lit_floor).abs() < 12.0,
        "the shadow bent with it: {was_shadow} vs {lit_floor}"
    );
    assert_differs(&bent, &rest, SAME_PICTURE, "the bent bar against the bar at rest");
    check_reference(
        "regression_skinned",
        &bent,
        REFERENCE,
        "An orange skinned bar (0.4 x 2 x 0.4, two boxes on two joints) standing on a grey floor at x -0.5 with \
         its upper half turned 90 degrees to the left, white sun at 45 degrees from the left casting its shadow; camera at \
         (0, 2.4, 5) looking at (0, 0.6, 0); 128x128, 4x multisampling, no tonemapping.",
        &stage.info,
    );
    reference_rejects("regression_skinned", &rest, REFERENCE, "the bar at rest");
    bend(&mut stage, bar, std::f32::consts::FRAC_PI_2 - 0.5);
    let less = stage.settled(3);
    reference_rejects("regression_skinned", &less, REFERENCE, "the bar bent 0.5 radians less");
    // The bar's shadow is some 70 pixels, under the reference's budget:
    // the image doesn't protect it, the assertion above ("the shadow bent
    // with it") does.
    // Back at rest it draws as at first.
    bend(&mut stage, bar, 0.0);
    assert_similar(&stage.settled(3), &rest, SAME_PICTURE, "the bar back at rest");
    stage.finish();
}

/// The motion vector at pixel (x, y), or `None` where nothing moved.
fn motion_at(stage: &Stage, (x, y): (u32, u32)) -> Option<(f32, f32)> {
    let texture = stage.renderer.velocity_texture()?;
    let bytes = read_texture(&stage.device, &stage.queue, texture);
    let at = ((y * texture.width() + x) * 4) as usize;
    let half = |i: usize| half::f16::from_le_bytes([bytes[at + i * 2], bytes[at + i * 2 + 1]]).to_f32();
    (half(0) != 0.0 || half(1) != 0.0).then(|| (half(0), half(1)))
}

#[test]
fn a_moved_skinned_model_writes_its_motion_and_a_still_one_none() {
    let Some(mut stage) = Stage::new() else { return };
    if !needs(&stage.device, GpuFeature::TemporalAntialiasing) {
        return;
    }
    let bar = skinned(&mut stage);
    bend(&mut stage, bar, 0.4);
    stage.ctx.scene3d.lighting.shadows = false;
    stage.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    stage.settled(3);
    let foot = vec3(-0.5, 0.5, 0.2);
    assert_eq!(
        motion_at(&stage, stage.pixel(foot)),
        None,
        "a still model writes no motion"
    );
    // The motion vectors come from the model's matrix. (A pose changing
    // under a still matrix writes none: temporal effects see the bones
    // move only through the rejection of mismatched history.)
    let step = vec3(0.3, 0.0, 0.0);
    stage.ctx.world.get_mut::<crate::d3::Transform>(bar).unwrap().position += step;
    stage.render();
    let view_proj = stage.ctx.scene3d.camera.view_proj();
    let (before, now) = (view_proj.project_point3(foot), view_proj.project_point3(foot + step));
    let expected = ((before.x - now.x) * 0.5, (now.y - before.y) * 0.5);
    let (vx, vy) = motion_at(&stage, stage.pixel(foot + step)).expect("the moved bar covers its lower half");
    eprintln!("motion {vx:.5}, {vy:.5}; expected {:.5}, {:.5}", expected.0, expected.1);
    assert!((vx - expected.0).abs() < 0.001 && (vy - expected.1).abs() < 0.001);
    assert!(expected.0 < 0.0, "it moved right: it was to the left");
    // Held again, the motion is gone.
    stage.render();
    assert_eq!(motion_at(&stage, stage.pixel(foot + step)), None, "at rest again");
    stage.finish();
}

#[test]
fn a_mirror_floor_reflects_the_glowing_box() {
    let Some(mut stage) = Stage::new() else { return };
    if !needs(&stage.device, GpuFeature::ScreenSpaceReflections) {
        return;
    }
    let (reflection, aside) = mirror(&mut stage);
    let off = stage.settled(2);
    let before = stage.color_at(&off, reflection);
    assert!(before[0] < 40.0, "without reflections the floor is dark: {before:?}");
    stage.ctx.scene3d.post.ssr = 1.0;
    // The first frame has no image of the last one to reflect.
    let image = stage.settled(4);
    let (mirrored, beside) = (stage.color_at(&image, reflection), stage.color_at(&image, aside));
    eprintln!("floor in front of the box {mirrored:?} (was {before:?}), to the side {beside:?}");
    assert!(
        mirrored[0] > 150.0 && mirrored[0] > mirrored[1] * 3.0,
        "the floor shows the red box: {mirrored:?}"
    );
    assert!(beside[0] < 40.0, "and only the dark beside it: {beside:?}");
    check_reference(
        "regression_ssr",
        &image,
        EFFECT_REFERENCE,
        "A red glowing box (emissive 4, 0, 0) 0.5 above a white mirror floor (metallic 1, roughness 0), nothing \
         lit, screen-space reflections at 1.0, fourth frame; camera at (0, 2, 6) looking at (0, 0.5, 0); 128x128, \
         4x multisampling, no tonemapping.",
        &stage.info,
    );
    reference_rejects("regression_ssr", &off, EFFECT_REFERENCE, "reflections off");
    stage.finish();
}

#[test]
fn ambient_occlusion_darkens_the_crease_and_leaves_open_ground_alone() {
    let Some(mut stage) = Stage::new() else { return };
    if !needs(&stage.device, GpuFeature::AmbientOcclusion) {
        return;
    }
    let (corner, open) = crease(&mut stage);
    let off = stage.image();
    let (flat_corner, flat_open) = (stage.color_at(&off, corner), stage.color_at(&off, open));
    assert!(
        (luma(flat_corner) - luma(flat_open)).abs() < 6.0,
        "without it the floor is one shade: {flat_corner:?} vs {flat_open:?}"
    );
    stage.ctx.scene3d.post.ambient_occlusion.enabled = true;
    stage.ctx.scene3d.post.ao_temporal = false;
    let image = stage.settled(2);
    let (dark, same) = (stage.color_at(&image, corner), stage.color_at(&image, open));
    eprintln!("crease {dark:?} (was {flat_corner:?}), open floor {same:?} (was {flat_open:?})");
    assert!(
        luma(dark) < luma(flat_corner) * 0.93,
        "the crease is darker: {dark:?} vs {flat_corner:?}"
    );
    assert!(
        luma(same) > luma(flat_open) * 0.97,
        "the open floor stays: {same:?} vs {flat_open:?}"
    );
    check_reference(
        "regression_ao",
        &image,
        EFFECT_REFERENCE,
        "A white floor meeting a white wall in a crease, lit only by white ambient light at 0.6, ambient \
         occlusion at its defaults without temporal filtering, second frame; camera at (0, 2.5, 6) looking at \
         (0, 0.5, -1); 128x128, 4x multisampling, no tonemapping.",
        &stage.info,
    );
    reference_rejects("regression_ao", &off, EFFECT_REFERENCE, "ambient occlusion off");
    stage.finish();
}

#[test]
fn light_bounced_off_the_red_wall_tints_the_floor_beside_it() {
    let Some(mut off) = Stage::new() else { return };
    if !needs(&off.device, GpuFeature::GlobalIllumination) {
        return;
    }
    let mut on = Stage::new().unwrap();
    let (near_red, far_side, wall) = red_room(&mut off, false);
    red_room(&mut on, true);
    // Enough frames for every probe to look around twice (two bounces).
    let (dark, lit) = (off.settled(3), on.settled(80));
    let (a, b, far) = (
        off.color_at(&dark, near_red),
        on.color_at(&lit, near_red),
        on.color_at(&lit, far_side),
    );
    eprintln!(
        "floor by the red wall: {a:?} off, {b:?} on; further away {far:?}; the sunlit wall {:?}",
        on.color_at(&lit, wall)
    );
    assert!(on.color_at(&lit, wall)[0] > 150.0, "the wall is in the sun");
    assert!((a[0] - a[1]).abs() <= 5.0, "no red without bounced light: {a:?}");
    assert!(b[0] > b[1] + 20.0, "red bounced light on the floor: {b:?}");
    assert!(
        b[0] - b[1] > far[0] - far[1],
        "redder by the red wall: {b:?} vs {far:?}"
    );
    check_reference(
        "regression_gi",
        &lit,
        EFFECT_REFERENCE,
        "A closed white room with a red west wall, the sun (intensity 4) shining through a roof opening onto \
         that wall, grey ambient light at 0.3, global illumination with probes 1 apart updated 16 a frame, \
         frame 80; camera inside at (2, 1.3, 0.3) looking at (-3.5, -0.2, 0); 128x128, 4x multisampling, no \
         tonemapping.",
        &on.info,
    );
    reference_rejects("regression_gi", &dark, EFFECT_REFERENCE, "global illumination off");
    // Half-way there: a probe update that stalls early is not the settled room.
    let mut early = Stage::new().unwrap();
    red_room(&mut early, true);
    reference_rejects(
        "regression_gi",
        &early.settled(8),
        EFFECT_REFERENCE,
        "frame 8 instead of 80",
    );
    early.finish();
    off.finish();
    on.finish();
}

#[test]
fn every_tonemap_keeps_the_order_of_brightness_and_only_none_clips() {
    let Some(mut stage) = Stage::new() else { return };
    let (at, _) = bars(&mut stage);
    let mut images = Vec::new();
    for tonemap in [Tonemap::None, Tonemap::Aces, Tonemap::AgX, Tonemap::Neutral] {
        stage.ctx.scene3d.post.tonemap = tonemap;
        let image = stage.image();
        let [dim, white, bright] = at.map(|p| stage.color_at(&image, p));
        eprintln!("{tonemap:?}: dim {dim:?}, white {white:?}, six times white {bright:?}");
        assert!(
            luma(dim) > 20.0 && luma(dim) < luma(white) - 20.0,
            "{tonemap:?}: the dim bar is darker than the white one: {dim:?} vs {white:?}"
        );
        assert!(
            bright[0] >= white[0] - 2.0,
            "{tonemap:?}: the brightest bar is no darker: {bright:?} vs {white:?}"
        );
        if tonemap == Tonemap::None {
            // As lit: white is white, and brighter clips to it.
            assert!(white.iter().all(|&c| c > 250.0), "untouched white: {white:?}");
        } else {
            // A curve leaves headroom: scene white is below the display's,
            // so the brighter bar can still be told from it.
            assert!(luma(white) < 245.0, "{tonemap:?} compresses white: {white:?}");
            assert!(
                luma(bright) > luma(white) + 4.0 || bright[0] > white[0] + 4.0,
                "{tonemap:?} keeps the highlight apart: {bright:?} vs {white:?}"
            );
        }
        images.push((tonemap, image));
    }
    for (tonemap, image) in &images[1..] {
        assert_differs(
            image,
            &images[0].1,
            SAME_PICTURE,
            &format!("{tonemap:?} against no tonemapping"),
        );
    }
    stage.finish();
}

#[test]
fn exposure_saturation_and_contrast_change_the_picture_the_way_they_say() {
    let Some(mut stage) = Stage::new() else { return };
    let (at, _) = bars(&mut stage);
    let plain = stage.image();
    let [dim, _, orange] = at.map(|p| stage.color_at(&plain, p));
    let neutral = stage.ctx.scene3d.post;

    // One stop up doubles the light: the dim bar gets brighter, one stop
    // down darker.
    stage.ctx.scene3d.post.exposure = neutral.exposure + 1.0;
    let up = stage.image();
    stage.ctx.scene3d.post.exposure = neutral.exposure - 1.0;
    let down = stage.image();
    let (brighter, darker) = (stage.color_at(&up, at[0]), stage.color_at(&down, at[0]));
    eprintln!("the dim bar: {darker:?} one stop down, {dim:?}, {brighter:?} one stop up");
    assert!(luma(brighter) > luma(dim) + 12.0 && luma(darker) < luma(dim) - 12.0);
    stage.ctx.scene3d.post = neutral;

    // Without saturation the orange bar is grey.
    assert!(orange[0] > orange[2] + 40.0, "the bright bar is orange: {orange:?}");
    stage.ctx.scene3d.post.saturation = 0.0;
    let grey = stage.image();
    let g = stage.color_at(&grey, at[2]);
    eprintln!("the orange bar without saturation: {g:?} (was {orange:?})");
    assert!(
        (g[0] - g[1]).abs() < 4.0 && (g[1] - g[2]).abs() < 4.0,
        "no saturation, no hue: {g:?}"
    );
    stage.ctx.scene3d.post = neutral;

    // More contrast pushes the dim bar further from the white one.
    stage.ctx.scene3d.post.contrast = neutral.contrast * 1.5;
    let steep = stage.image();
    let d = stage.color_at(&steep, at[0]);
    eprintln!("the dim bar with 1.5 times the contrast: {d:?} (was {dim:?})");
    assert!(
        luma(d) < luma(dim) - 4.0,
        "more contrast darkens the dim bar: {d:?} vs {dim:?}"
    );
    stage.ctx.scene3d.post = neutral;
    assert_similar(&stage.image(), &plain, SAME_PICTURE, "the settings put back");
    stage.finish();
}

#[test]
fn bloom_spreads_the_brightest_bars_glow_and_antialiasing_softens_edges() {
    let Some(mut stage) = Stage::new() else { return };
    let (at, between) = bars(&mut stage);
    let plain = stage.image();
    let gap = stage.color_at(&plain, between);
    assert!(luma(gap) < 2.0, "between the bars it is black: {gap:?}");
    stage.ctx.scene3d.post.bloom = 0.3;
    let glowing = stage.image();
    let (halo, far_side) = (
        stage.color_at(&glowing, between),
        stage.color_at(&glowing, vec3(-3.2, 0.0, -1.0)),
    );
    eprintln!("beside the bright bar {halo:?} with bloom (was {gap:?}); beside the dim bar {far_side:?}");
    assert!(halo[0] > gap[0] + 8.0, "the glow spills over: {halo:?}");
    assert!(halo[0] > halo[2] + 4.0, "in the bar's orange: {halo:?}");
    assert!(
        luma(far_side) < luma(halo) * 0.5,
        "the dim bar, under the threshold, glows less: {far_side:?} vs {halo:?}"
    );
    let dim = (stage.color_at(&plain, at[0]), stage.color_at(&glowing, at[0]));
    assert!(
        (luma(dim.0) - luma(dim.1)).abs() < 25.0,
        "the bars themselves stay: {dim:?}"
    );
    stage.ctx.scene3d.post.tonemap = Tonemap::Aces;
    check_reference(
        "regression_post",
        &stage.image(),
        EFFECT_REFERENCE,
        "Three glowing bars on black (emissive 0.25 grey, 1.0 white, and orange 6, 3, 0.6), nothing lit, bloom \
         0.3 at the default threshold, ACES tonemapping, no antialiasing beyond 4x multisampling; camera at \
         (0, 0, 8) looking at the origin; 128x128.",
        &stage.info,
    );
    stage.ctx.scene3d.post.bloom = 0.0;
    reference_rejects("regression_post", &stage.image(), EFFECT_REFERENCE, "bloom off");
    stage.ctx.scene3d.post.bloom = 0.3;
    stage.ctx.scene3d.post.tonemap = Tonemap::AgX;
    reference_rejects(
        "regression_post",
        &stage.image(),
        EFFECT_REFERENCE,
        "AgX instead of ACES",
    );
    stage.ctx.scene3d.post.tonemap = Tonemap::None;
    stage.ctx.scene3d.post.bloom = 0.0;

    // Post antialiasing on a single-sample frame: tilted edges get
    // in-between shades they do not have without it.
    let Some(mut single) = Stage::with(Setup {
        samples: 1,
        ..Default::default()
    }) else {
        return;
    };
    bars(&mut single);
    single
        .ctx
        .world
        .query::<&mut crate::d3::Transform>()
        .for_each(|t| t.rotation = Quat::from_rotation_z(0.3));
    let hard = single.image();
    for mode in [AntiAliasing::Fxaa, AntiAliasing::Smaa] {
        single.ctx.scene3d.post.anti_aliasing = mode;
        let soft = single.settled(2);
        let d = diff(&soft, &hard, 8);
        eprintln!("{mode:?} against no antialiasing: {d}");
        // It changes the edges' pixels, and only those: the bars' edges
        // are a few hundred of the frame's pixels.
        assert!(d.outliers > 30, "{mode:?} softens the edges: {d}");
        assert!(d.outliers < d.pixels / 8, "{mode:?} leaves flat areas alone: {d}");
    }
    single.finish();
    stage.finish();
}

/// sRGB-encoded bytes decoded to linear and stored as 8 bits again: what a
/// linear target holds of the picture an sRGB target shows.
fn as_linear(image: &Image) -> Image {
    let mut out = image.clone();
    for p in out.rgba.as_chunks_mut::<4>().0 {
        for c in &mut p[..3] {
            let v = f32::from(*c) / 255.0;
            let linear = if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            };
            *c = (linear * 255.0).round() as u8;
        }
    }
    out
}

#[test]
fn every_output_format_shows_the_same_picture() {
    use wgpu::TextureFormat::{Bgra8Unorm, Bgra8UnormSrgb, Rgba8Unorm, Rgba8UnormSrgb};
    let draw = |format, view_format| {
        let mut stage = Stage::with(Setup {
            format,
            view_format,
            ..Default::default()
        })?;
        opaque(&mut stage);
        let image = stage.image();
        stage.finish();
        Some(image)
    };
    let Some(reference) = draw(Rgba8UnormSrgb, None) else {
        return;
    };
    // The red box is red in every format: a swapped channel order would
    // show it blue.
    let red = mean_color(&reference, SIZE * 3 / 8, SIZE * 5 / 8, 2);
    assert!(red[0] > red[2] * 3.0, "the near box is red: {red:?}");
    // BGRA (most desktop surfaces), read back as RGBA: the same picture.
    let bgra = draw(Bgra8UnormSrgb, None).unwrap();
    assert_similar(&bgra, &reference, SAME_PICTURE, "BGRA sRGB against RGBA sRGB");
    // A linear texture drawn through an sRGB view (a browser canvas): the
    // bytes are the sRGB target's.
    for format in [Rgba8Unorm, Bgra8Unorm] {
        let through_view = draw(format, Some(format.add_srgb_suffix())).unwrap();
        assert_similar(
            &through_view,
            &reference,
            SAME_PICTURE,
            &format!("{format:?} through its sRGB view against RGBA sRGB"),
        );
    }
    // A linear target with no sRGB view holds the light itself: the same
    // picture decoded. Dark shades have few 8-bit linear levels, so the
    // comparison is in linear bytes, where a level is a level.
    let expected = as_linear(&reference);
    for format in [Rgba8Unorm, Bgra8Unorm] {
        let linear = draw(format, None).unwrap();
        assert_similar(
            &linear,
            &expected,
            SAME_PICTURE,
            &format!("{format:?} against the sRGB picture decoded"),
        );
        // Negative control: it is not the encoded picture.
        assert_differs(
            &linear,
            &reference,
            SAME_PICTURE,
            &format!("{format:?} against the encoded picture"),
        );
    }
}

/// `Stage::render_timed` times each frame once, as a game's frame does: it
/// begins the timer's frame itself and submits the timestamps with the
/// frame, so after waiting for the GPU the next frame reads the one before
/// it back (one frame old), and each frame brings one sample. A caller that
/// also began the frame would make every sample two frames old.
#[test]
fn render_timed_times_each_frame_once_and_reads_it_back_the_next() {
    let Some(mut stage) = Stage::with(Setup {
        timestamps: true,
        ..Setup::default()
    }) else {
        return;
    };
    let Some(mut timer) = super::timer::GpuTimer::new(&stage.device, &stage.queue) else {
        skip("timestamp queries unavailable");
        return;
    };
    let _ = opaque(&mut stage);
    let mut sampled = 0;
    for frame in 0..12 {
        let read = timer.frames_read;
        stage.render_timed(Some(&mut timer));
        stage.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        timer.collect(&stage.device);
        if frame < 2 {
            continue;
        }
        assert!(
            timer.frames_read <= read + 1,
            "frame {frame}: more than one frame came back"
        );
        if timer.frames_read == read + 1 {
            // The frame before this one came back, and only it.
            let (ms, age) = timer.latest_frame().expect("a frame came back");
            assert_eq!(age, 1, "frame {frame}: the sample is {age} frames old, not one");
            assert!(ms.is_finite() && ms >= 0.0, "frame {frame}: {ms} ms");
            sampled += 1;
        }
    }
    // A pass whose timestamps a busy GPU didn't write usably is not a
    // sample; nearly every frame is.
    assert!(sampled >= 8, "{sampled} of 10 frames brought a sample");
    // Negative control: beginning the frame again before `render_timed`
    // (what the debug-lines timing test did after the merge) makes the
    // samples two frames old.
    timer.begin_frame();
    stage.render_timed(Some(&mut timer));
    stage.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    timer.begin_frame();
    stage.render_timed(Some(&mut timer));
    stage.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let read = timer.frames_read;
    timer.collect(&stage.device);
    if timer.frames_read > read {
        let (_, age) = timer.latest_frame().expect("a frame came back");
        assert_eq!(age, 2, "a frame begun twice counts twice");
    }
}
