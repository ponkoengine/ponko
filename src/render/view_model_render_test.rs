//! Offscreen GPU tests of the view-model pass (`gpu3d/view_model.rs`): a
//! held item stays in sight with the player's nose against a wall, where
//! it is without the pass (the negative control) hidden in the wall; it is
//! drawn where its own camera puts it; a cut-out held item is drawn with
//! its holes, the wall showing through them; a wall nearer than the depth
//! sliver the items are drawn in doesn't cut into them; under temporal
//! antialiasing an item held still stays put on screen while the eye
//! turns and walks, and one that sways moves as it sways; and with no view
//! model the pass costs nothing: no buffer, no draw. Skips without a GPU.
use super::memory::MemoryTally;
use super::regression_stage::{STRIPES_SHADER, Setup};
use super::regression_stage::{Stage, mean_color};
use super::render_test_support::needs;
use super::render_test_support::{Image, Tolerance, check_reference, diff, read_texture};
use crate::d3::{HeldItem, ViewModel};
use crate::prelude::*;
use crate::render::{AntiAliasing, GpuFeature, MaterialDesc, MeshData};
use crate::three::Blend;

/// The held item's colour and the wall's.
const RED: [f32; 3] = [0.9, 0.08, 0.05];
const GREY: [f32; 3] = [0.55, 0.55, 0.55];

/// Where the held item is in the eye's space: below and right of the
/// centre, half a metre ahead (the wall is 0.3 m ahead).
const HELD_AT: Vec3 = vec3(0.12, -0.1, -0.5);

/// The eye 1.6 m up, looking along -Z, its nose 0.3 m from a grey wall,
/// with a red 8 cm box held half a metre ahead: inside the wall. Returns
/// the held item.
fn nose_to_the_wall(stage: &mut Stage) -> Entity {
    let wall = stage.cube(Color::rgb(GREY[0], GREY[1], GREY[2]));
    stage.block(wall, vec3(-4.0, 0.0, -0.6), vec3(4.0, 4.0, -0.3));
    let red = stage.cube(Color::rgb(RED[0], RED[1], RED[2]));
    let held = stage.block(red, Vec3::ZERO, Vec3::ZERO);
    // The unit cube, 8 cm across: the held item's offset sets its scale.
    let mut item = HeldItem::at(HELD_AT);
    item.offset.scale = Vec3::splat(0.08);
    stage.ctx.world.insert(held, item);
    let camera = &mut stage.ctx.scene3d.camera;
    camera.position = vec3(0.0, 1.6, 0.0);
    camera.rotation = Quat::IDENTITY;
    camera.fov_y = 70f32.to_radians();
    camera.near = 0.1;
    // Lit from behind the eye, so both faces facing it are bright.
    stage.ctx.scene3d.lighting.sun_direction = vec3(-0.3, -0.4, -1.0);
    held
}

/// Where (in pixels, unrounded) the view model's camera sees the
/// eye-space point `p`.
fn held_point(stage: &Stage, view_model: &ViewModel, p: Vec3) -> (f32, f32) {
    let camera = &stage.ctx.scene3d.camera;
    let world = camera.position + camera.rotation * p;
    let ndc = view_model.camera(camera).view_proj().project_point3(world);
    let (w, h) = (stage.size.0 as f32, stage.size.1 as f32);
    ((ndc.x * 0.5 + 0.5) * w, (0.5 - ndc.y * 0.5) * h)
}

/// The pixel the view model's camera sees the eye-space point `p` at.
fn held_pixel(stage: &Stage, view_model: &ViewModel, p: Vec3) -> (u32, u32) {
    let (x, y) = held_point(stage, view_model, p);
    (x as u32, y as u32)
}

/// How red a colour is: red over the others.
fn redness(c: [f32; 3]) -> f32 {
    c[0] - c[1].max(c[2])
}

#[test]
fn a_held_item_stays_in_sight_against_a_wall_that_hides_it_without_the_view_model() {
    let Some(mut stage) = Stage::new() else { return };
    nose_to_the_wall(&mut stage);
    let view_model = ViewModel::default().with_fov(50f32.to_radians());
    // Negative control: no view model, and the box half a metre ahead is
    // inside the wall 0.3 m ahead: all grey.
    let hidden = stage.settled(2);
    let (x, y) = held_pixel(&stage, &view_model, HELD_AT);
    let behind = mean_color(&hidden, x, y, 1);
    assert!(
        redness(behind) < 0.05,
        "without a view model the wall hides the held item: {behind:?} at ({x}, {y})"
    );
    // With it, the box is drawn over the wall, where its own camera
    // (50°, not the world's 70°) puts it.
    stage.ctx.scene3d.view_model = Some(view_model);
    let shown = stage.settled(2);
    let held = mean_color(&shown, x, y, 1);
    assert!(
        redness(held) > 0.3,
        "the view model draws the held item over the wall: {held:?} at ({x}, {y})"
    );
    // Where and how big it is comes from the view model's camera: the
    // pixels it changes (its lit faces and the one in shade) fill the box's eight corners as that camera projects them,
    // within a pixel and a half (the world's 70° lens would make it a
    // third smaller).
    let corners: Vec<(f32, f32)> = (0..8)
        .map(|i| {
            let corner = vec3(
                if i & 1 == 0 { -0.04 } else { 0.04 },
                if i & 2 == 0 { -0.04 } else { 0.04 },
                if i & 4 == 0 { -0.04 } else { 0.04 },
            );
            held_point(&stage, &view_model, HELD_AT + corner)
        })
        .collect();
    let want = [
        corners.iter().map(|c| c.0).fold(f32::MAX, f32::min),
        corners.iter().map(|c| c.1).fold(f32::MAX, f32::min),
        corners.iter().map(|c| c.0).fold(f32::MIN, f32::max),
        corners.iter().map(|c| c.1).fold(f32::MIN, f32::max),
    ];
    let got = changed_bounds(&hidden, &shown).expect("the held item changes pixels");
    for (g, w) in got.iter().zip(want) {
        assert!(
            (g - w).abs() <= 1.5,
            "the held item covers the view model camera's projection: drawn {got:?}, projected {want:?}"
        );
    }
    let expected = (want[2] - want[0]) * (want[3] - want[1]);
    let side = (want[3] - want[1]).max(1.0);
    // The world around it is the same picture: only the box changed.
    let rest = diff(&hidden, &shown, 8);
    assert!(
        rest.outliers as f32 <= expected + 2.0 * side + 8.0,
        "only the held item's pixels change: {rest}"
    );
    check_reference(
        "view_model",
        &shown,
        Tolerance {
            channel: 12,
            outliers: 0.01,
            edge: 1,
        },
        "a red 8 cm box held half a metre ahead (below and right of the centre) over a grey wall 0.3 m ahead",
        &stage.info,
    );
    stage.finish();
}

/// The edges of the pixels where `shown` differs from `hidden` (by more
/// than 8 levels in a channel): left, top, right and bottom, the last two
/// past the last pixel (as projected edges are).
fn changed_bounds(hidden: &Image, shown: &Image) -> Option<[f32; 4]> {
    let mut bounds: Option<[f32; 4]> = None;
    let pixels = hidden.rgba.as_chunks::<4>().0.iter().zip(shown.rgba.as_chunks::<4>().0);
    for (i, (a, b)) in pixels.enumerate() {
        if (0..3).any(|c| a[c].abs_diff(b[c]) > 8) {
            let (x, y) = ((i as u32 % shown.width) as f32, (i as u32 / shown.width) as f32);
            let e = bounds.get_or_insert([x, y, x + 1.0, y + 1.0]);
            *e = [e[0].min(x), e[1].min(y), e[2].max(x + 1.0), e[3].max(y + 1.0)];
        }
    }
    bounds
}

/// The view model's GPU allocations: what the memory report lists for it.
fn view_model_bytes(stage: &Stage) -> (u64, u64) {
    let mut t = MemoryTally::listing();
    stage.renderer.tally_memory(&mut t);
    let (memory, list) = t.finish_listed();
    let ours = list
        .iter()
        .filter(|a| a.label.contains("view model"))
        .map(|a| a.bytes)
        .sum();
    (ours, memory.total)
}

#[test]
fn the_view_model_costs_nothing_while_it_is_off_and_frees_what_it_took() {
    let Some(mut stage) = Stage::new() else { return };
    nose_to_the_wall(&mut stage);
    // Off: no pass, no buffer, no draw; held items are drawn in the world
    // like any model.
    stage.render();
    stage.render();
    let off_draws = stage.renderer.take_state_changes().draws;
    let off_calls = stage.renderer.draw_calls();
    let (ours, off_total) = view_model_bytes(&stage);
    assert!(!stage.renderer.has_view_model(), "no view model, no pass");
    assert_eq!(ours, 0, "no view model, no allocation");
    // On: one uniform buffer, and the box's one draw moved from the
    // world's passes (prepass and lit) to the view model's (lit only).
    stage.ctx.scene3d.view_model = Some(ViewModel::default());
    stage.render();
    stage.render();
    let on_draws = stage.renderer.take_state_changes().draws;
    let (ours, on_total) = view_model_bytes(&stage);
    assert!(stage.renderer.has_view_model());
    assert!(ours > 0, "the view model's frame uniform is listed");
    assert_eq!(
        on_total,
        off_total + ours,
        "the view model adds its uniform, nothing else"
    );
    assert!(on_draws >= 1, "the held item is drawn: {on_draws} draws");
    assert!(
        stage.renderer.draw_calls() <= off_calls,
        "the held item takes no more draws in the pass than in the world: {} against {off_calls}",
        stage.renderer.draw_calls()
    );
    // Off again: everything it took is freed, and the frame records what
    // it did before it was ever on.
    stage.ctx.scene3d.view_model = None;
    stage.render();
    stage.render();
    let again = stage.renderer.take_state_changes().draws;
    let (ours, total) = view_model_bytes(&stage);
    assert!(!stage.renderer.has_view_model());
    assert_eq!(
        (ours, total),
        (0, off_total),
        "turned off, the view model holds nothing"
    );
    assert_eq!(again, off_draws, "turned off, the frame draws what it drew before");
    stage.finish();
}

/// The yellow of the cut-out card.
const YELLOW: [f32; 3] = [0.9, 0.8, 0.1];

/// How yellow a colour is (0 to 255): red and green over blue.
fn yellowness(c: [f32; 3]) -> f32 {
    c[0].min(c[1]) - c[2]
}

/// How long the cut-out card is, across its stripes (metres).
const CARD: f32 = 0.12;

/// The wall of [`nose_to_the_wall`] with a held card instead of the box:
/// 12 cm, facing the eye, its alpha cutting it into three solid and three
/// empty vertical stripes ([`STRIPES_SHADER`]: the card's x from -0.5 to
/// 0.5 is its u from 0 to 1, solid where `fract(3u)` is past one half).
/// Returns two eye-space points on it: the middle of a solid stripe and of
/// an empty one.
fn card_against_the_wall(stage: &mut Stage) -> (Vec3, Vec3) {
    let held = nose_to_the_wall(stage);
    stage.ctx.world.despawn(held);
    let (shader, _) = stage.shader("stripes.wgsl", STRIPES_SHADER);
    let card = stage.mesh(
        &MeshData::plane(1.0),
        MaterialDesc {
            color: Color::rgb(YELLOW[0], YELLOW[1], YELLOW[2]),
            blend: Blend::Cutout,
            shader: Some(shader),
            double_sided: true,
            ..Default::default()
        },
    );
    // The plane lies flat (normal +Y): stood up to face the eye (+Z).
    let mut item = HeldItem::at(HELD_AT).rotated(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
    item.offset.scale = Vec3::splat(CARD);
    stage
        .ctx
        .world
        .spawn((Model::new(card), item, crate::d3::Transform::default()));
    // u 0.25 is solid, u 0.75 empty (between 0.667 and 0.833).
    (
        HELD_AT + vec3(-0.25 * CARD, 0.0, 0.0),
        HELD_AT + vec3(0.25 * CARD, 0.0, 0.0),
    )
}

/// The cut-out card against the wall with `samples` per pixel: hidden in
/// the wall without the view model, drawn with its holes with it (the
/// wall through the empty stripes). Returns the picture with it.
fn cut_out_card_with(samples: u32) -> Option<(Stage, Image)> {
    let mut stage = Stage::with(Setup {
        samples,
        ..Setup::default()
    })?;
    let (solid, empty) = card_against_the_wall(&mut stage);
    let view_model = ViewModel::default().with_fov(50f32.to_radians());
    let hidden = stage.settled(2);
    let (sx, sy) = held_pixel(&stage, &view_model, solid);
    let (ex, ey) = held_pixel(&stage, &view_model, empty);
    let behind = mean_color(&hidden, sx, sy, 1);
    assert!(
        yellowness(behind) < 10.0,
        "{samples} samples: without a view model the wall hides the card: {behind:?}"
    );
    stage.ctx.scene3d.view_model = Some(view_model);
    let shown = stage.settled(2);
    let on = mean_color(&shown, sx, sy, 1);
    let through = mean_color(&shown, ex, ey, 1);
    let wall = mean_color(&hidden, ex, ey, 1);
    assert!(
        yellowness(on) > 80.0,
        "{samples} samples: the card's solid stripe is drawn over the wall: {on:?} at ({sx}, {sy})"
    );
    assert!(
        yellowness(through) < 10.0 && (0..3).all(|c| (through[c] - wall[c]).abs() < 6.0),
        "{samples} samples: the wall shows through the cut-away stripe as it was: {through:?} against {wall:?} at ({ex}, {ey})"
    );
    Some((stage, shown))
}

#[test]
fn a_cut_out_held_item_cuts_its_holes_over_the_wall_with_and_without_multisampling() {
    let Some((stage, shown)) = cut_out_card_with(4) else {
        return;
    };
    let Some((single, _)) = cut_out_card_with(1) else {
        return;
    };
    single.finish();
    check_reference(
        "view_model_cutout",
        &shown,
        Tolerance {
            channel: 12,
            outliers: 0.01,
            edge: 1,
        },
        "a yellow 12 cm card held half a metre ahead (below and right of the centre), cut into three solid and three \
         empty vertical stripes, over a grey wall 0.3 m ahead that shows through the empty ones; 4x multisampling",
        &stage.info,
    );
    stage.finish();
}

#[test]
fn a_wall_inside_the_near_sliver_does_not_cut_into_a_held_item() {
    let Some(mut stage) = Stage::new() else { return };
    nose_to_the_wall(&mut stage);
    // The wall 0.5 mm past the near plane (0.1 m): nearer than the depth
    // sliver's far edge (a hundredth of the near plane's distance).
    let wall = stage.cube(Color::rgb(GREY[0], GREY[1], GREY[2]));
    let near = stage.block(wall, vec3(-4.0, 0.0, -0.3), vec3(4.0, 4.0, -0.1005));
    let view_model = ViewModel::default().with_fov(50f32.to_radians());
    stage.ctx.scene3d.view_model = Some(view_model);
    let (x, y) = held_pixel(&stage, &view_model, HELD_AT);
    stage.render();
    // Negative control: with the world left in the sliver, it covers the
    // held item.
    stage.renderer.view_model_without(true, false);
    let cut = stage.settled(2);
    let covered = mean_color(&cut, x, y, 1);
    assert!(
        redness(covered) < 10.0,
        "without the push back the wall in the sliver covers the held item: {covered:?}"
    );
    stage.renderer.view_model_without(false, false);
    let whole = stage.settled(2);
    let held = mean_color(&whole, x, y, 1);
    assert!(
        redness(held) > 80.0,
        "the held item is drawn whole over a wall in the sliver: {held:?} at ({x}, {y})"
    );
    // The same picture as with the wall 0.3 m ahead: the item is all
    // there, nothing of the near wall is in it.
    stage.ctx.world.despawn(near);
    let far = stage.settled(3);
    assert!(
        diff(&far, &whole, 8).outliers <= 4,
        "the item looks as it does against a wall 0.3 m ahead: {}",
        diff(&far, &whole, 8)
    );
    stage.finish();
}

/// What temporal antialiasing makes of the motion at pixel `at`, where
/// the held box's face towards the eye (eye-space depth `face_z`) is: the
/// camera motion it works out from the depth there (reprojected with the
/// main camera, as `taa.wgsl` does) plus the motion image's vector, in
/// pixels. Also returns the eye-space point of the face the pixel shows.
/// `previous` is the main camera last frame.
fn taa_motion(stage: &Stage, view_model: &ViewModel, previous: Mat4, at: (u32, u32), face_z: f32) -> (Vec2, Vec3) {
    let size = Vec2::new(stage.size.0 as f32, stage.size.1 as f32);
    let camera = &stage.ctx.scene3d.camera;
    let held = view_model.camera(camera).view_proj();
    let drawn = stage.renderer.view_model_jittered().expect("a view model was drawn");
    // The jitter, a shear of x and y by w, is the same for both cameras.
    let jitter = drawn * held.inverse();
    let ndc = vec2(
        (at.0 as f32 + 0.5) / size.x * 2.0 - 1.0,
        1.0 - (at.1 as f32 + 0.5) / size.y * 2.0,
    );
    // The face's point at the pixel's centre, as drawn: along the ray
    // through it, at the face's depth in the eye's space.
    let eye_from_clip = (drawn * Mat4::from_rotation_translation(camera.rotation, camera.position)).inverse();
    let (a, b) = (
        eye_from_clip.project_point3(ndc.extend(1.0)),
        eye_from_clip.project_point3(ndc.extend(0.5)),
    );
    let point = a + (b - a) * ((face_z - a.z) / (b.z - a.z));
    let world = camera.position + camera.rotation * point;
    let depth = 0.99 + 0.01 * drawn.project_point3(world).z;
    let main = camera.view_proj();
    let reprojected = (jitter * main).inverse().project_point3(ndc.extend(depth));
    let (now, then) = (main.project_point3(reprojected), previous.project_point3(reprojected));
    let texture = stage
        .renderer
        .velocity_texture()
        .expect("temporal antialiasing has a motion image");
    let bytes = read_texture(&stage.device, &stage.queue, texture);
    let i = ((at.1 * texture.width() + at.0) * 4) as usize;
    let half = |c: usize| half::f16::from_le_bytes([bytes[i + c * 2], bytes[i + c * 2 + 1]]).to_f32();
    let uv = Vec2::new((then.x - now.x) * 0.5 + half(0), (now.y - then.y) * 0.5 + half(1));
    (uv * size, point)
}

#[test]
fn held_items_move_on_screen_as_their_own_camera_saw_them_under_temporal_antialiasing() {
    let Some(mut stage) = Stage::new() else { return };
    if !needs(&stage.device, GpuFeature::TemporalAntialiasing) {
        return;
    }
    // The wall 2.5 m off, so the eye can walk; the red box held as before.
    let wall = stage.cube(Color::rgb(GREY[0], GREY[1], GREY[2]));
    stage.block(wall, vec3(-4.0, 0.0, -3.0), vec3(4.0, 4.0, -2.5));
    let red = stage.cube(Color::rgb(RED[0], RED[1], RED[2]));
    let held = stage.block(red, Vec3::ZERO, Vec3::ZERO);
    let mut item = HeldItem::at(HELD_AT);
    item.offset.scale = Vec3::splat(0.08);
    stage.ctx.world.insert(held, item);
    let camera = &mut stage.ctx.scene3d.camera;
    camera.position = vec3(0.0, 1.6, 0.0);
    camera.rotation = Quat::IDENTITY;
    camera.fov_y = 70f32.to_radians();
    camera.near = 0.1;
    stage.ctx.scene3d.lighting.sun_direction = vec3(-0.3, -0.4, -1.0);
    stage.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    let view_model = ViewModel::default().with_fov(50f32.to_radians());
    stage.ctx.scene3d.view_model = Some(view_model);
    stage.settled(3);
    let size = Vec2::new(stage.size.0 as f32, stage.size.1 as f32);
    // The middle of the box's face towards the eye.
    let face = HELD_AT + vec3(0.0, 0.0, 0.04);
    // One frame in which the eye turns 3 degrees left, steps 5 cm ahead
    // and the item sways by `sway`. Returns the main and the view model's
    // camera before it, with the eye's place then.
    let step = |stage: &mut Stage, sway: Vec3| {
        let camera = stage.ctx.scene3d.camera;
        let eye = Mat4::from_rotation_translation(camera.rotation, camera.position);
        let camera_now = &mut stage.ctx.scene3d.camera;
        camera_now.rotation *= Quat::from_rotation_y(3f32.to_radians());
        camera_now.position += camera_now.rotation * vec3(0.0, 0.0, -0.05);
        stage.ctx.world.get_mut::<HeldItem>(held).unwrap().offset.position += sway;
        stage.render();
        (camera.view_proj(), view_model.camera(&camera).view_proj() * eye)
    };
    // Where the view model's camera sees eye-space `p` (pixels), as `clip`
    // (eye to clip space) projects it.
    let pixel = |clip: Mat4, p: Vec3| {
        let ndc = clip.project_point3(p);
        Vec2::new((ndc.x * 0.5 + 0.5) * size.x, (0.5 - ndc.y * 0.5) * size.y)
    };
    // Held still: the face stays put on screen.
    let (before, _) = step(&mut stage, Vec3::ZERO);
    assert!(
        stage.renderer.draws_view_model_motion(),
        "the held items' motion is drawn"
    );
    let (still, _) = taa_motion(
        &stage,
        &view_model,
        before,
        held_pixel(&stage, &view_model, face),
        face.z,
    );
    assert!(
        still.length() < 0.1,
        "an item held still doesn't move on screen while the eye turns and walks: {still} px"
    );
    // Swaying 2 cm right: it moves as far as its camera sees the sway.
    let sway = vec3(0.02, 0.0, 0.0);
    let (before, was) = step(&mut stage, sway);
    let (got, point) = taa_motion(
        &stage,
        &view_model,
        before,
        held_pixel(&stage, &view_model, face + sway),
        face.z,
    );
    let camera = stage.ctx.scene3d.camera;
    let is = view_model.camera(&camera).view_proj() * Mat4::from_rotation_translation(camera.rotation, camera.position);
    let want = pixel(was, point - sway) - pixel(is, point);
    assert!(want.x < -2.0, "the sway is a few pixels: {want}");
    assert!(
        (got - want).length() < 0.1,
        "a swaying item moves on screen as it sways: {got} px, its camera saw {want} px"
    );
    // Negative control: without its own vectors, temporal antialiasing
    // would move the held item with the camera.
    stage.renderer.view_model_without(false, true);
    let (before, _) = step(&mut stage, Vec3::ZERO);
    assert!(!stage.renderer.draws_view_model_motion());
    let (wrong, _) = taa_motion(
        &stage,
        &view_model,
        before,
        held_pixel(&stage, &view_model, face + sway),
        face.z,
    );
    assert!(
        wrong.length() > 2.0,
        "without the held items' vectors they would move with the camera: {wrong} px"
    );
    stage.finish();
}
