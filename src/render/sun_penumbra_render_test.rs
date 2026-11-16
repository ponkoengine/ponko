//! Contact-hardening sun shadows (`ShadowFilter::ContactHardening`): sharp
//! where a shadow meets its caster, softer the further it falls, with an
//! edge as wide in world units in every cascade. The floors here are drawn
//! with a shader that shows `sunlight` itself (1 lit, 0 in shadow, unlit),
//! so edges are measured in world units straight from the shadow lookup.
//! Each check has a negative control: the fixed `Softest` filter, whose
//! edge is as wide at a caster's foot as far from it, and changes with the
//! cascade's texel size. Skips without a GPU.
use super::gpu3d::*;
use super::regression_stage::*;
use super::render_test_support::*;
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::{AntiAliasing, Blend, GpuFeature, MaterialDesc, MeshData, Model, ShadowFilter};

/// A floor material showing how much sun reaches it, unlit.
const MASK: &str = "fn surface(s: Surface) -> Surface {
    var o = s;
    o.unlit = true;
    o.albedo = vec3<f32>(0.0);
    o.emissive = vec3<f32>(sunlight(s.world, s.normal));
    return o;
}";

/// The frame size measurements are made at: a pixel of the pole scene
/// covers 2 cm of floor.
const MEASURE: (u32, u32) = (256, 256);

fn new_stage(size: (u32, u32)) -> Option<Stage> {
    Stage::with(Setup {
        size,
        ..Setup::default()
    })
}

/// A floor showing the sun mask, its top at height 0, from `lo` to `hi`
/// across (x, z).
fn mask_floor(stage: &mut Stage, lo: Vec2, hi: Vec2) {
    let (shader, _) = stage.shader("sun_mask.wgsl", MASK);
    let floor = stage.mesh(
        &MeshData::cube(1.0),
        MaterialDesc {
            shader: Some(shader),
            ..MaterialDesc::color(Color::WHITE)
        },
    );
    stage.block(floor, vec3(lo.x, -1.0, lo.y), vec3(hi.x, 0.0, hi.y));
}

/// A pole 0.5 thick and 3 tall standing at the origin, the sun coming down
/// at 45 degrees towards +x, the camera looking straight down on the
/// shadow: it lies from x 0.25 to 3.25, z -0.25 to 0.25. Each point of its
/// edge is as far from the pole as √2 times its x past 0.25.
fn pole(stage: &mut Stage, filter: ShadowFilter) {
    mask_floor(stage, vec2(-10.0, -10.0), vec2(10.0, 10.0));
    let white = stage.cube(Color::WHITE);
    stage.block(white, vec3(-0.25, 0.0, -0.25), vec3(0.25, 3.0, 0.25));
    let lighting = &mut stage.ctx.scene3d.lighting;
    lighting.sun_direction = vec3(1.0, -1.0, 0.0);
    lighting.shadows = true;
    lighting.shadow_filter = filter;
    lighting.shadow_sun_size = 3.0;
    stage.ctx.scene3d.camera.position = vec3(1.7, 4.5, 0.01);
    stage.ctx.scene3d.camera.look_at(vec3(1.7, 0.0, 0.0));
}

/// The sun mask at floor point `p` (0 to 1, linear): the red channel,
/// decoded from sRGB.
fn mask_at(stage: &Stage, image: &Image, p: Vec3) -> f32 {
    let (x, y) = stage.pixel(p);
    let v = f32::from(image.pixel(x, y)[0]) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// How wide a shadow edge is in world units, from 10% to 90% of the way
/// from lit to shadow: walking from `lit` to `dark` (floor points; the
/// shadow's middle at `dark`), each step averaged over `along` either side
/// (along the edge, where the shadow doesn't change: that averages the
/// lookup pattern's noise away without widening the edge).
fn edge_width(stage: &Stage, image: &Image, lit: Vec3, dark: Vec3, along: Vec3) -> f32 {
    let at = |p: Vec3| {
        (-2..=2)
            .map(|k| mask_at(stage, image, p + along * (k as f32 / 2.0)))
            .sum::<f32>()
            / 5.0
    };
    let (top, bottom) = (at(lit), at(dark));
    assert!(
        top - bottom > 0.5,
        "an edge from {lit} (mask {top}) to {dark} (mask {bottom})"
    );
    let steps = 400;
    let mut start = None;
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        let v = at(lit.lerp(dark, t));
        if start.is_none() && v < top - 0.1 * (top - bottom) {
            start = Some(t);
        }
        if v < top - 0.9 * (top - bottom) {
            return (t - start.unwrap_or(t)) * lit.distance(dark);
        }
    }
    panic!("the edge from {lit} to {dark} never got dark");
}

/// The pole's shadow edge (its side at z -0.25) near the pole's foot and
/// near the tip of its shadow, in world units.
fn pole_edges(stage: &mut Stage) -> (f32, f32) {
    let image = stage.settled(3);
    let along = vec3(0.1, 0.0, 0.0);
    let edge = |x: f32| edge_width(stage, &image, vec3(x, 0.0, -0.75), vec3(x, 0.0, 0.0), along);
    (edge(0.6), edge(2.75))
}

#[test]
fn contact_hardening_shadows_are_sharp_at_the_casters_foot_and_soft_far_from_it() {
    let Some(mut stage) = new_stage(MEASURE) else { return };
    pole(&mut stage, ShadowFilter::ContactHardening);
    let (foot, tip) = pole_edges(&mut stage);
    // At 3 degrees the sun's disc seen from the pole is 0.052 wide per unit
    // of distance: about 0.03 at x 0.6 (0.5 away) and 0.19 at x 2.75 (3.5
    // away); 10% to 90% of a straight ramp is 80% of it.
    eprintln!("contact-hardening edge: {foot:.3} at the foot, {tip:.3} near the tip");
    assert!(tip > 0.09, "soft far from the caster: {tip}");
    assert!(
        tip > foot * 2.5,
        "softer near the tip ({tip}) than at the foot ({foot})"
    );
    let errors = stage.take_errors();
    assert!(errors.is_empty(), "{errors:#?}");

    // Negative control: the fixed filter's edge is as wide everywhere.
    let Some(mut fixed) = new_stage(MEASURE) else { return };
    pole(&mut fixed, ShadowFilter::Softest);
    let (fixed_foot, fixed_tip) = pole_edges(&mut fixed);
    eprintln!("Softest edge: {fixed_foot:.3} at the foot, {fixed_tip:.3} near the tip");
    assert!(
        fixed_tip < fixed_foot * 1.6 + 0.02,
        "the fixed filter doesn't harden: {fixed_foot} vs {fixed_tip}"
    );
    assert!(
        foot < fixed_foot + 0.03,
        "at the foot, as sharp as the fixed filter: {foot} vs {fixed_foot}"
    );
    fixed.finish();
}

#[test]
fn contact_hardening_edges_keep_their_world_width_whatever_the_cascade() {
    // One cascade over the whole shadow distance has texels about ten times
    // the size of the nearest of four. A far edge is as wide in both; the
    // fixed filter's isn't.
    let tip = |filter, cascades| {
        let mut stage = new_stage(MEASURE)?;
        pole(&mut stage, filter);
        stage.ctx.scene3d.lighting.shadow_cascades = cascades;
        let (_, tip) = pole_edges(&mut stage);
        stage.finish();
        Some(tip)
    };
    let Some(four) = tip(ShadowFilter::ContactHardening, 4) else {
        return;
    };
    let one = tip(ShadowFilter::ContactHardening, 1).unwrap();
    let fixed_four = tip(ShadowFilter::Softest, 4).unwrap();
    let fixed_one = tip(ShadowFilter::Softest, 1).unwrap();
    eprintln!(
        "edge near the tip: contact-hardening {four:.3} (4 cascades) vs {one:.3} (1); Softest {fixed_four:.3} vs {fixed_one:.3}"
    );
    assert!(
        (one / four - 1.0).abs() < 0.3,
        "the same width in world units: {four} with four cascades, {one} with one"
    );
    assert!(
        fixed_one > fixed_four * 2.0,
        "negative control: the fixed filter widens with the texels: {fixed_four} vs {fixed_one}"
    );
}

/// A long beam 2 above the floor along z, the sun at 45 degrees towards +x,
/// two 1024² cascades over 20 units, seen from low down along the shadow:
/// the shadow lies from x 1.5 to 2.9 the whole way, its edge at x 1.5 cast
/// by the beam's lower left edge 2.83 away, and crosses from the near
/// cascade into the far one.
fn beam(stage: &mut Stage, filter: ShadowFilter) {
    mask_floor(stage, vec2(-10.0, -30.0), vec2(10.0, 10.0));
    let white = stage.cube(Color::WHITE);
    stage.block(white, vec3(-0.5, 2.0, -25.0), vec3(0.5, 2.4, 5.0));
    let lighting = &mut stage.ctx.scene3d.lighting;
    lighting.sun_direction = vec3(1.0, -1.0, 0.0);
    lighting.shadows = true;
    lighting.shadow_filter = filter;
    lighting.shadow_sun_size = 6.0;
    lighting.shadow_cascades = 2;
    lighting.shadow_resolution = 1024;
    lighting.shadow_distance = 20.0;
    lighting.shadow_refresh = 1;
    stage.ctx.scene3d.camera.position = vec3(2.0, 1.2, 4.0);
    stage.ctx.scene3d.camera.look_at(vec3(2.0, 0.0, -4.0));
}

/// The beam's shadow edge (x 1.5) just inside the near cascade and just
/// past it, in world units: the last point along it whose cascade 0 box
/// holds it well inside, and the first it no longer holds.
fn border_edges(stage: &mut Stage) -> (f32, f32) {
    let image = stage.settled(3);
    let frame = &stage.ctx.gfx.frame3d;
    assert_eq!(frame.cascades, 2);
    let box_edge = |z: f32| {
        let ndc = frame.sun_view_proj[0].project_point3(vec3(1.5, 0.0, z));
        ndc.x.abs().max(ndc.y.abs())
    };
    let zs: Vec<f32> = (0..300).map(|k| 3.0 - k as f32 * 0.05).collect();
    let near = *zs
        .iter()
        .take_while(|&&z| box_edge(z) < 0.75)
        .last()
        .expect("the near cascade holds the edge's start");
    let far = *zs
        .iter()
        .find(|&&z| box_edge(z) > 1.05)
        .expect("the edge leaves the near cascade");
    eprintln!("near cascade holds the edge to z {near}, not from z {far}");
    let along = vec3(0.0, 0.0, 0.1);
    let edge = |z: f32| edge_width(stage, &image, vec3(0.7, 0.0, z), vec3(2.2, 0.0, z), along);
    (edge(near), edge(far))
}

#[test]
fn contact_hardening_edges_dont_jump_where_cascades_meet() {
    let Some(mut stage) = new_stage(MEASURE) else { return };
    beam(&mut stage, ShadowFilter::ContactHardening);
    let (near, far) = border_edges(&mut stage);
    eprintln!("contact-hardening edge: {near:.3} in the near cascade, {far:.3} in the far one");
    // 6 degrees: the sun's disc is 0.105 wide per unit, 0.30 at 2.83 away.
    assert!(near > 0.15, "the edge has the width the distance gives: {near}");
    assert!(
        (far / near - 1.0).abs() < 0.3,
        "no jump at the border: {near} then {far}"
    );
    stage.finish();

    let Some(mut fixed) = new_stage(MEASURE) else { return };
    beam(&mut fixed, ShadowFilter::Softest);
    let (fixed_near, fixed_far) = border_edges(&mut fixed);
    eprintln!("Softest edge: {fixed_near:.3} in the near cascade, {fixed_far:.3} in the far one");
    assert!(
        fixed_far > fixed_near * 2.0,
        "negative control: the fixed filter's edge jumps at the border: {fixed_near} then {fixed_far}"
    );
    fixed.finish();
}

#[test]
fn contact_hardening_references_the_pole_and_the_cascade_border() {
    let Some(mut stage) = new_stage((SIZE, SIZE)) else {
        return;
    };
    pole(&mut stage, ShadowFilter::ContactHardening);
    let image = stage.settled(3);
    check_reference(
        "sun_penumbra_pole",
        &image,
        REFERENCE,
        "A white pole (0.5 thick, 3 tall) at the origin on a floor drawn as the sun mask (unlit, white lit, black \
         in shadow), sun direction (1, -1, 0), ShadowFilter::ContactHardening, shadow_sun_size 3 degrees, 4 \
         cascades of 2048² over 30 units; camera at (1.7, 4.5, 0.01) looking straight down at (1.7, 0, 0); \
         128x128, 4x multisampling, no tonemapping, no antialiasing. The shadow is sharp at the pole's foot (left) \
         and soft towards its tip (right).",
        &stage.info,
    );
    stage.ctx.scene3d.lighting.shadow_filter = ShadowFilter::Softest;
    let fixed = stage.settled(3);
    assert_differs(&image, &fixed, SAME_PICTURE, "the fixed filter's pole shadow");
    stage.finish();

    let Some(mut stage) = new_stage((SIZE, SIZE)) else {
        return;
    };
    beam(&mut stage, ShadowFilter::ContactHardening);
    let image = stage.settled(3);
    check_reference(
        "sun_penumbra_cascade_border",
        &image,
        REFERENCE,
        "A white beam (1 wide, 0.4 high, 2 above the floor, from z -25 to 5) over a floor drawn as the sun mask \
         (unlit), sun direction (1, -1, 0), ShadowFilter::ContactHardening, shadow_sun_size 6 degrees, 2 cascades \
         of 1024² over 20 units, every cascade redrawn every frame; camera at (2, 1.2, 4) looking at (2, 0, -4); \
         128x128, 4x multisampling, no tonemapping, no antialiasing. The beam's shadow runs away from the camera \
         across the border between the two cascades with edges of one width.",
        &stage.info,
    );
    stage.ctx.scene3d.lighting.shadow_filter = ShadowFilter::Softest;
    let fixed = stage.settled(3);
    assert_differs(&image, &fixed, SAME_PICTURE, "the fixed filter's beam shadow");
    stage.finish();
}

#[test]
fn contact_hardening_noise_stays_still_without_taa_and_averages_out_with_it() {
    let Some(mut stage) = new_stage(MEASURE) else { return };
    pole(&mut stage, ShadowFilter::ContactHardening);
    let first = stage.settled(2);
    let second = stage.image();
    let d = diff(&first, &second, 0);
    assert_eq!(d.max_channel, 0, "without TAA the lookup pattern stays still: {d}");
    // The noise along the soft edge near the tip: pixels next to each
    // other along the edge, where the shadow itself doesn't change.
    let noise = |stage: &Stage, image: &Image| {
        let mut sum = 0.0;
        let mut n = 0.0;
        for k in 0..60 {
            let z = -0.25 + (k % 6) as f32 * 0.02 - 0.05;
            let x = 2.4 + (k / 6) as f32 * 0.05;
            let a = mask_at(stage, image, vec3(x, 0.0, z));
            let b = mask_at(stage, image, vec3(x + 0.02, 0.0, z));
            sum += (a - b).abs();
            n += 1.0;
        }
        sum / n
    };
    let still = noise(&stage, &second);
    if !needs(&stage.device, GpuFeature::TemporalAntialiasing) {
        stage.finish();
        return;
    }
    stage.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    let averaged = stage.settled(32);
    let smooth = noise(&stage, &averaged);
    eprintln!("noise along the soft edge: {still:.4} without TAA, {smooth:.4} with it");
    assert!(
        smooth < still * 0.7,
        "TAA averages the turning pattern: {still} without, {smooth} with"
    );
    stage.finish();
}

#[test]
fn contact_hardening_follows_the_held_sun_direction() {
    let Some(mut stage) = new_stage((SIZE, SIZE)) else {
        return;
    };
    pole(&mut stage, ShadowFilter::ContactHardening);
    stage.ctx.scene3d.lighting.shadow_sun_step = 0.08;
    let before = stage.settled(3);
    // Turned less than the step: the shadows keep the held direction.
    stage.ctx.scene3d.lighting.sun_direction = Quat::from_rotation_z(0.06) * vec3(1.0, -1.0, 0.0);
    let held = stage.settled(3);
    assert_similar(&before, &held, SAME_PICTURE, "a sun turned less than the step");
    // Negative control: without a step the shadow follows the turn.
    stage.ctx.scene3d.lighting.shadow_sun_step = 0.0;
    let followed = stage.settled(3);
    assert_differs(&before, &followed, SAME_PICTURE, "the shadow of the turned sun");
    stage.finish();
}

#[test]
fn contact_hardening_shadows_of_cutout_casters_keep_their_holes() {
    let Some(mut stage) = Stage::new() else { return };
    let (_, [_, _, hole_floor, sheet_floor]) = cutout(&mut stage, HOLES_SHADER);
    stage.ctx.scene3d.lighting.shadow_filter = ShadowFilter::ContactHardening;
    let image = stage.settled(3);
    let (lit, dark) = (stage.color_at(&image, hole_floor), stage.color_at(&image, sheet_floor));
    eprintln!("floor under a hole {lit:?}, under the sheet {dark:?}");
    assert!(
        luma(lit) > luma(dark) * 2.0 && luma(lit) > 40.0,
        "light falls through the holes: {lit:?} vs {dark:?}"
    );
    // Negative control: the same sheet without holes shadows that spot.
    let solid = stage.mesh(
        &MeshData::plane(1.0),
        MaterialDesc {
            blend: Blend::Opaque,
            double_sided: true,
            ..MaterialDesc::color(Color::rgb(0.9, 0.8, 0.1))
        },
    );
    stage.ctx.world.spawn((
        Transform3D {
            position: vec3(-1.5, 1.01, 0.0),
            scale: Vec3::splat(3.0),
            ..Default::default()
        },
        Model::new(solid),
    ));
    let covered = stage.settled(3);
    let shut = stage.color_at(&covered, hole_floor);
    assert!(
        luma(shut) < luma(lit) * 0.5,
        "a solid sheet shadows it: {shut:?} vs {lit:?}"
    );
    stage.finish();
}

#[test]
fn the_gl_backend_draws_contact_hardening_as_the_fixed_filter() {
    // The GL backend's shaders (see `shaders::without_contact_shadows`), on
    // this machine's GPU: exactly the `Softest` picture.
    FORCE_FLOAT_DEPTH.set(true);
    let made = new_stage((SIZE, SIZE));
    FORCE_FLOAT_DEPTH.set(false);
    let Some(mut stage) = made else { return };
    pole(&mut stage, ShadowFilter::ContactHardening);
    let contact = stage.settled(3);
    stage.ctx.scene3d.lighting.shadow_filter = ShadowFilter::Softest;
    let fixed = stage.settled(3);
    let d = diff(&contact, &fixed, 0);
    assert_eq!(d.max_channel, 0, "GL draws ContactHardening as Softest: {d}");
    // Negative control: elsewhere the two differ.
    let Some(mut other) = new_stage((SIZE, SIZE)) else {
        return;
    };
    pole(&mut other, ShadowFilter::ContactHardening);
    let real = other.settled(3);
    assert_differs(&real, &fixed, SAME_PICTURE, "contact-hardening off the GL backend");
    other.finish();
    stage.finish();
}

#[test]
fn the_gl_backend_shaders_keep_every_line_and_read_no_shadow_depths() {
    let material = compose_shader("");
    assert!(
        material.contains("textureLoad(shadow_map"),
        "the blocker search reads the shadow map's depths"
    );
    let gl = for_samples(&material, 1, true);
    assert!(
        !gl.contains("textureLoad(shadow_map"),
        "the GL backend's shader doesn't"
    );
    // Lines after the replaced part stay where they were, so error messages
    // point at the same lines.
    let line = |source: &str| source.lines().position(|l| l.starts_with("fn sunlight("));
    assert!(line(&material).is_some());
    assert_eq!(line(&gl), line(&material), "no line moves");
    // The other backends keep it.
    assert!(for_samples(&material, 4, false).contains("textureLoad(shadow_map"));
    assert_eq!(
        without_contact_shadows("fn f() {}"),
        "fn f() {}",
        "a shader without it is left alone"
    );
}
