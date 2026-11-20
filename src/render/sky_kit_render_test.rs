//! Offscreen GPU tests of the sky kit (`SkyKit`, `three/sky_kit.rs`): the
//! sky at four times of day against reference images, and what each part
//! must do — clouds cast shadows on the ground and light the fog, the moon
//! shows its phase, a WebGL2-class GPU gets the flat cloud layer, and a
//! game that never turns the kit on pays for none of it.
//!
//! `PONKO_RENDER_TEST_IMAGES=1` keeps every picture in
//! `target/render-test-output/`; `cloud_presets_gpu_time` (ignored, run by
//! hand) measures each [`CloudQuality`].
use super::gpu3d::{FrameFeatures, last_features};
use super::regression_stage::*;
use super::render_test_support::*;
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::{CloudQuality, DayNight, GpuTier, MaterialDesc, MeshData, SkyKit};

/// The frames drawn before a picture is read: the clouds' temporal resolve
/// settles over about this many.
const SETTLE: usize = 24;

/// The frame size of the reference pictures.
const WIDE: (u32, u32) = (192, 112);

/// A stage `size` big with the kit's test world: a wide green ground,
/// a few blocks, the default kit at `hour` with no wind (so every run draws
/// the same clouds), no bloom and the sun's shadows on.
fn stage(size: (u32, u32), tier: Option<GpuTier>) -> Option<Stage> {
    stage_with(Setup {
        size,
        tier,
        samples: if tier == Some(GpuTier::WebGl2) { 1 } else { 4 },
        ..Setup::default()
    })
}

fn stage_with(setup: Setup) -> Option<Stage> {
    let mut stage = Stage::with(setup)?;
    let ground = stage.mesh(
        &MeshData::plane(80000.0),
        MaterialDesc {
            color: Color::rgb(0.32, 0.45, 0.22),
            roughness: 0.95,
            ..Default::default()
        },
    );
    stage.ctx.world.spawn((Transform3D::default(), Model::new(ground)));
    let stone = stage.cube(Color::rgb(0.75, 0.72, 0.66));
    let red = stage.cube(Color::rgb(0.7, 0.18, 0.12));
    // A ring of blocks, so every view has something standing in it.
    for i in 0..16 {
        let mesh = if i % 2 == 0 { stone } else { red };
        let a = (i as f32 + 0.3) / 16.0 * std::f32::consts::TAU;
        let r = 24.0 + 9.0 * (i % 3) as f32;
        let (x, z) = (a.cos() * r, a.sin() * r);
        let top = 2.0 + (i % 4) as f32 * 1.5;
        stage.block(mesh, vec3(x - 1.5, 0.0, z - 1.5), vec3(x + 1.5, top, z + 1.5));
    }
    // And a few tall ones far off, for the views from a hill.
    for i in 0..8 {
        let mesh = if i % 2 == 0 { stone } else { red };
        let a = (i as f32 + 0.6) / 8.0 * std::f32::consts::TAU;
        let (x, z) = (a.cos() * 700.0, a.sin() * 700.0);
        let top = 30.0 + (i % 3) as f32 * 15.0;
        stage.block(mesh, vec3(x - 12.0, 0.0, z - 12.0), vec3(x + 12.0, top, z + 12.0));
    }
    // A game's defaults (the stage starts from a lab's: no tonemapping,
    // a black flat sky), without bloom.
    let scene = &mut stage.ctx.scene3d;
    scene.lighting = crate::render::Lighting::default();
    scene.sky = crate::render::Sky::default();
    scene.post.tonemap = crate::render::Post::default().tonemap;
    scene.post.bloom = 0.0;
    scene.camera.far = 40000.0;
    Some(stage)
}

/// Moves the clouds so the test views have both sun and shade near the
/// camera (the default field has a clear gap over the origin).
const CLOUD_OFFSET: Vec2 = Vec2::new(-2750.0, -1500.0);

/// Sets the kit to `hour` with `kit`'s clouds, wind held.
fn set_time(stage: &mut Stage, hour: f32, mut kit: SkyKit) {
    kit.clouds.wind = Vec2::ZERO;
    kit.clouds.offset += CLOUD_OFFSET;
    stage.ctx.scene3d.sky.kit = Some(kit);
    DayNight::new(hour).apply(&mut stage.ctx.scene3d);
}

/// Points the camera from `eye` along compass direction `toward` (only its
/// x and z count), `pitch` degrees above the horizon.
fn look(stage: &mut Stage, eye: Vec3, toward: Vec3, pitch: f32) {
    let flat = vec3(toward.x, 0.0, toward.z).normalize();
    let p = pitch.to_radians();
    let dir = flat * p.cos() + Vec3::Y * p.sin();
    let camera = &mut stage.ctx.scene3d.camera;
    camera.position = eye;
    camera.look_at(eye + dir * 10.0);
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// The mean luma of the pixels in the box `x` by `y` of `image`, 0..255.
fn mean_luma(image: &Image, x: std::ops::Range<u32>, y: std::ops::Range<u32>) -> f32 {
    let (mut sum, mut n) = (0.0, 0.0);
    for py in y {
        for px in x.clone() {
            let [r, g, b, _] = image.pixel(px, py);
            sum += luma([f32::from(r), f32::from(g), f32::from(b)]);
            n += 1.0;
        }
    }
    sum / n
}

/// How much the luma varies over the box: the standard deviation, 0..255.
fn luma_spread(image: &Image, x: std::ops::Range<u32>, y: std::ops::Range<u32>) -> f32 {
    let mean = mean_luma(image, x.clone(), y.clone());
    let (mut sum, mut n) = (0.0, 0.0);
    for py in y {
        for px in x.clone() {
            let [r, g, b, _] = image.pixel(px, py);
            let l = luma([f32::from(r), f32::from(g), f32::from(b)]);
            sum += (l - mean) * (l - mean);
            n += 1.0;
        }
    }
    (sum / n).sqrt()
}

fn features() -> FrameFeatures {
    last_features().expect("the frame was checked")
}

// ---------------------------------------------------------------------------
// Four times of day.

/// One reference picture: the kit at `hour`, the camera looking along
/// `toward` from `eye`.
fn time_of_day(name: &str, hour: f32, eye: Vec3, (toward, pitch): (Vec3, f32), scene: &str, check: impl Fn(&Image)) {
    let Some(mut stage) = stage(WIDE, None) else { return };
    set_time(&mut stage, hour, SkyKit::default());
    look(&mut stage, eye, toward, pitch);
    let image = stage.settled(SETTLE);
    let f = features();
    assert!(f.contains(FrameFeatures::CLOUDS), "{name}: the clouds are marched");
    let errors = stage.take_errors();
    assert!(errors.is_empty(), "{name}: the GPU reported errors: {errors:#?}");
    check(&image);
    check_reference(name, &image, EFFECT_REFERENCE, scene, &stage.info);
    // Negative control: the reference tells the clouds' absence apart.
    set_time(&mut stage, hour, SkyKit::default().with_coverage(0.0));
    let clear = stage.settled(SETTLE);
    reference_rejects(name, &clear, EFFECT_REFERENCE, "the same sky without clouds");
}

#[test]
fn sky_kit_at_dawn_matches_reference() {
    // Half past six: the sun just up in the east (+x), behind the clouds.
    time_of_day(
        "sky_kit_dawn",
        6.5,
        vec3(0.0, 2.0, 0.0),
        (Vec3::X, 8.0),
        "the sky kit at 6:30, looking east: a low sun under a half-clouded sky",
        |image| {
            let (w, h) = WIDE;
            let sky = mean_luma(image, 0..w, 0..h / 4);
            let ground = mean_luma(image, 0..w, h * 3 / 4..h);
            assert!(sky > 40.0, "the dawn sky is lit: {sky}");
            assert!(
                ground < sky,
                "the ground under a low sun is darker than the sky: {ground} vs {sky}"
            );
            assert!(luma_spread(image, 0..w, 0..h / 2) > 6.0, "the clouds break up the sky");
        },
    );
}

#[test]
fn sky_kit_at_noon_matches_reference() {
    // Noon: the sun high in the south (-z); looking away from it at the
    // clouds' sunlit sides, from a hill 250 m up so their shadows show on
    // the ground (from eye height the ground they fall on is a sliver at
    // the horizon).
    time_of_day(
        "sky_kit_noon",
        12.0,
        vec3(0.0, 250.0, 0.0),
        (Vec3::Z, -7.0),
        "the sky kit at noon, looking north: white clouds on blue, their shadows on the ground",
        |image| {
            let (w, h) = WIDE;
            let [r, g, b, _] = image.pixel(w / 2, 4);
            assert!(b > r, "the noon sky is blue: {r} {g} {b}");
            assert!(luma_spread(image, 0..w, 0..h / 3) > 8.0, "clouds stand out of the blue");
        },
    );
}

#[test]
fn sky_kit_at_dusk_matches_reference() {
    // Twenty to six in the evening: the sun low in the west (-x).
    time_of_day(
        "sky_kit_dusk",
        17.7,
        vec3(0.0, 2.0, 0.0),
        (-Vec3::X, 6.0),
        "the sky kit at 17:42, looking west: a red sunset lighting the clouds from below",
        |image| {
            let (w, h) = WIDE;
            let [r, _, b, _] = image.pixel(w / 2, h / 2 - 8);
            assert!(r > b, "the sunset sky near the sun is red: {r} vs {b}");
        },
    );
}

#[test]
fn sky_kit_at_night_matches_reference() {
    // Eleven at night, a waning gibbous moon rising in the east.
    let Some(mut stage) = stage(WIDE, None) else { return };
    let mut day = DayNight::new(23.0);
    day.first_phase = 0.6;
    stage.ctx.scene3d.sky.kit = Some({
        let mut kit = SkyKit::default().with_coverage(0.35);
        kit.clouds.wind = Vec2::ZERO;
        kit.moon.size = 5.0;
        kit
    });
    day.apply(&mut stage.ctx.scene3d);
    let kit = stage.ctx.scene3d.sky.kit.unwrap();
    let to_moon = kit.to_moon(-stage.ctx.scene3d.lighting.sun_direction);
    let elevation = to_moon.y.asin().to_degrees();
    look(&mut stage, vec3(0.0, 2.0, 0.0), to_moon, elevation * 0.5);
    let image = stage.settled(SETTLE);
    let (mx, my) = stage.pixel(stage.ctx.scene3d.camera.position + to_moon * 5000.0);
    let moon = mean_color(&image, mx, my, 1);
    let (w, _) = WIDE;
    let sky = mean_luma(&image, 0..w, 0..8);
    assert!(
        luma(moon) > 150.0,
        "the moon is bright, not black: {moon:?} at {mx},{my} ({elevation} degrees up)"
    );
    assert!(sky < 40.0, "the night sky is dark: {sky}");
    assert!(sky < luma(moon) / 4.0);
    let errors = stage.take_errors();
    assert!(errors.is_empty(), "the GPU reported errors: {errors:#?}");
    check_reference(
        "sky_kit_night",
        &image,
        EFFECT_REFERENCE,
        "the sky kit at 23:00: a waning gibbous moon over the dark land, stars, moonlit clouds",
        &stage.info,
    );
    if let Some(kit) = &mut stage.ctx.scene3d.sky.kit {
        kit.clouds.coverage = 0.0;
    }
    let clear = stage.settled(SETTLE);
    reference_rejects(
        "sky_kit_night",
        &clear,
        EFFECT_REFERENCE,
        "the same night without clouds",
    );
}

// ---------------------------------------------------------------------------
// What each part does.

#[test]
fn sky_kit_costs_nothing_until_it_is_turned_on() {
    let Some(mut stage) = stage((64, 48), None) else { return };
    stage.ctx.scene3d.lighting.sun_direction = crate::render::Sky::sun_direction_at(10.0);
    look(&mut stage, vec3(0.0, 2.0, 0.0), -Vec3::Z, 5.0);
    let (lighting, sky) = (stage.ctx.scene3d.lighting, stage.ctx.scene3d.sky);
    let plain = stage.settled(8);
    let f = features();
    assert!(!f.contains(FrameFeatures::CLOUDS) && !f.contains(FrameFeatures::CLOUD_SHADOWS));
    assert!(
        !stage.renderer.kit_made(),
        "nothing of the kit is made before it is turned on"
    );
    // Turned on: the passes run.
    set_time(&mut stage, 10.0, SkyKit::default());
    stage.settled(3);
    let f = features();
    assert!(f.contains(FrameFeatures::CLOUDS) && f.contains(FrameFeatures::CLOUD_SHADOWS));
    assert!(stage.renderer.kit_made());
    // And off again: none of them, and the picture is the plain sky's.
    (stage.ctx.scene3d.lighting, stage.ctx.scene3d.sky) = (lighting, sky);
    let again = stage.settled(8);
    let f = features();
    assert!(!f.contains(FrameFeatures::CLOUDS) && !f.contains(FrameFeatures::CLOUD_SHADOWS));
    let d = diff(&plain, &again, 3);
    assert!(
        d.within(Tolerance {
            channel: 3,
            outliers: 0.01,
            edge: 0,
        }),
        "turned off, the kit leaves nothing behind: {d:?}"
    );
    stage.finish();
}

/// The ground seen from 600 m up looking down at it through a wide lens,
/// at noon, with `kit`: over a kilometer of ground, at the edge of a
/// cloud's shadow. (From much higher the haze of the air in between would
/// wash the shadows out.)
fn ground_from_above(kit: SkyKit) -> Option<(Image, Stage)> {
    let mut stage = stage((96, 96), None)?;
    set_time(&mut stage, 12.0, kit);
    stage.ctx.scene3d.camera.fov_y = 100f32.to_radians();
    look(&mut stage, vec3(0.0, 600.0, 0.0), -Vec3::Z, -89.0);
    let image = stage.settled(4);
    Some((image, stage))
}

#[test]
fn cloud_shadows_dapple_the_ground() {
    let kit = SkyKit::default().with_coverage(0.55);
    let Some((shaded, stage)) = ground_from_above(kit) else {
        return;
    };
    let map = stage.renderer.cloud_shadow_map(&stage.device, &stage.queue);
    let (lo, hi) = map
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &t| (lo.min(t), hi.max(t)));
    let shaded_share = map.iter().filter(|&&t| t < 0.7).count() as f32 / map.len() as f32;
    eprintln!(
        "cloud shadow map: {lo:.3}..{hi:.3}, {:.0}% shaded",
        shaded_share * 100.0
    );
    assert!(hi > 0.95, "clear sky lets the sun through: {hi}");
    assert!(lo < 0.4, "thick clouds hold most of it back: {lo}");
    assert!(
        (0.15..0.85).contains(&shaded_share),
        "about as much shade as cover: {shaded_share}"
    );
    stage.finish();
    let mut unshaded_kit = kit;
    unshaded_kit.clouds.shadows = 0.0;
    let Some((unshaded, stage)) = ground_from_above(unshaded_kit) else {
        return;
    };
    stage.finish();
    // The middle of the view: ground only (the far blocks are at its edges).
    let (x, y) = (12..84, 12..84);
    let (shaded_spread, unshaded_spread) = (
        luma_spread(&shaded, x.clone(), y.clone()),
        luma_spread(&unshaded, x.clone(), y.clone()),
    );
    let (shaded_mean, unshaded_mean) = (mean_luma(&shaded, x.clone(), y.clone()), mean_luma(&unshaded, x, y));
    // Negative control: without the shadows the ground is evenly lit.
    assert!(unshaded_spread < 2.0, "unshaded ground is even: {unshaded_spread}");
    assert!(
        shaded_spread > unshaded_spread + 1.0,
        "cloud shadows make light and dark patches: {shaded_spread} vs {unshaded_spread}"
    );
    assert!(
        shaded_mean < unshaded_mean - 8.0,
        "and darken it: {shaded_mean} vs {unshaded_mean}"
    );
}

/// Fog toward a low morning sun under `coverage` clouds casting `shadows`,
/// with the fog's sun glow `glow`: the mean color of the fogged ground in
/// the middle of the frame.
fn fog_toward_sun(coverage: f32, shadows: f32, glow: f32, density: f32) -> Option<[f32; 3]> {
    let mut stage = stage((64, 48), None)?;
    let mut kit = SkyKit::default().with_coverage(coverage);
    kit.clouds.shadows = shadows;
    set_time(&mut stage, 8.0, kit);
    let sky = &mut stage.ctx.scene3d.sky;
    sky.fog_density = density;
    sky.fog_height = 20.0;
    sky.fog_sun_glow = glow;
    let to_sun = -stage.ctx.scene3d.lighting.sun_direction;
    look(&mut stage, vec3(0.0, 2.0, 0.0), to_sun, -2.0);
    let image = stage.settled(8);
    stage.finish();
    let (mut sum, mut n) = ([0.0f32; 3], 0.0);
    for y in 28..40 {
        for x in 16..48 {
            let p = image.pixel(x, y);
            for c in 0..3 {
                sum[c] += f32::from(p[c]);
            }
            n += 1.0;
        }
    }
    Some(sum.map(|s| s / n))
}

#[test]
fn clouds_color_the_fog_and_dim_its_glow() {
    // The glow: the fog with it, less the fog without.
    let glow = |coverage, shadows| -> Option<f32> {
        let on = fog_toward_sun(coverage, shadows, 0.3, 0.01)?;
        let off = fog_toward_sun(coverage, shadows, 0.0, 0.01)?;
        Some(luma(on) - luma(off))
    };
    let Some(clear) = glow(0.0, 1.0) else { return };
    let Some(overcast) = glow(1.0, 1.0) else { return };
    let Some(unshaded) = glow(1.0, 0.0) else { return };
    eprintln!("fog glow: clear {clear:.1}, overcast {overcast:.1}, overcast without shadows {unshaded:.1}");
    assert!(clear > 5.0, "the fog glows toward the sun: {clear}");
    assert!(
        overcast < clear * 0.5,
        "an overcast sky dims the glow: {overcast} vs {clear}"
    );
    // Negative control: clouds that cast no shadow leave the glow be.
    assert!(
        unshaded > clear * 0.7,
        "the dimming is the clouds' shadow: {unshaded} vs {clear}"
    );
    // Thick fog takes the light of the sky behind it: toward a clear
    // morning sun bright, under an overcast the clouds' dimmer grey.
    let (Some(clear), Some(overcast)) = (fog_toward_sun(0.0, 1.0, 0.0, 0.08), fog_toward_sun(1.0, 1.0, 0.0, 0.08))
    else {
        return;
    };
    eprintln!("fog: clear {clear:?}, overcast {overcast:?}");
    assert!(
        luma(overcast) < luma(clear) * 0.85,
        "clouds dim the fog: {overcast:?} vs {clear:?}"
    );
}

#[test]
fn webgl2_class_gpus_get_the_flat_cloud_layer() {
    let Some(mut stage) = stage((96, 64), Some(GpuTier::WebGl2)) else {
        return;
    };
    set_time(&mut stage, 11.0, SkyKit::default().with_coverage(0.6));
    look(&mut stage, vec3(0.0, 2.0, 0.0), -Vec3::Z, 25.0);
    let cloudy = stage.settled(3);
    let f = features();
    assert!(!f.contains(FrameFeatures::CLOUDS), "no cloud march on WebGL2");
    assert!(f.contains(FrameFeatures::CLOUD_SHADOWS), "the shadows stay");
    let cloudy_spread = luma_spread(&cloudy, 0..96, 0..40);
    // Negative control: a clear sky is smooth.
    set_time(&mut stage, 11.0, SkyKit::default().with_coverage(0.0));
    let clear = stage.settled(3);
    let clear_spread = luma_spread(&clear, 0..96, 0..40);
    assert!(
        cloudy_spread > clear_spread + 6.0,
        "the flat layer shows clouds: {cloudy_spread} vs {clear_spread}"
    );
    stage.finish();
}

/// The moon at `phase`, filling much of a narrow view; returns the luma
/// of its left and right halves and the sky beside it.
fn moon_halves(phase: f32) -> Option<(f32, f32, f32)> {
    let mut stage = stage((64, 64), None)?;
    let mut kit = SkyKit::default().with_coverage(0.0);
    kit.moon.size = 14.0;
    kit.moon.phase = phase;
    // Midnight, the moon placed high in the south whatever the phase
    // (`direction` is the way its light travels).
    kit.moon.direction = Some(-vec3(0.0, 0.7, -0.7).normalize());
    stage.ctx.scene3d.sky.kit = Some(kit);
    stage.ctx.scene3d.lighting.sun_direction = crate::render::Sky::sun_direction_at(0.0);
    let to_moon = vec3(0.0, 0.7, -0.7).normalize();
    let camera = &mut stage.ctx.scene3d.camera;
    camera.position = vec3(0.0, 2.0, 0.0);
    camera.fov_y = 30f32.to_radians();
    camera.look_at(camera.position + to_moon);
    let image = stage.settled(3);
    stage.finish();
    // 14 of 30 degrees: the disk spans about 30 pixels around the middle.
    let left = mean_luma(&image, 22..30, 28..36);
    let right = mean_luma(&image, 34..42, 28..36);
    let sky = mean_luma(&image, 0..8, 0..8);
    Some((left, right, sky))
}

#[test]
fn the_moon_shows_its_phase() {
    let Some((full_l, full_r, sky)) = moon_halves(0.5) else {
        return;
    };
    let Some((new_l, new_r, _)) = moon_halves(0.0) else {
        return;
    };
    let Some((first_l, first_r, _)) = moon_halves(0.25) else {
        return;
    };
    let Some((last_l, last_r, _)) = moon_halves(0.75) else {
        return;
    };
    assert!(
        full_l > 120.0 && full_r > 120.0,
        "a full moon is lit all over: {full_l} {full_r}"
    );
    assert!(full_l > sky * 4.0);
    // New, its face is no brighter than the night sky around it.
    assert!(
        new_l < sky * 1.5 + 5.0 && new_r < sky * 1.5 + 5.0,
        "a new moon is dark: {new_l} {new_r} (sky {sky})"
    );
    // A waxing moon is lit on the right (as seen from the north), a
    // waning one on the left.
    assert!(
        first_r > first_l * 3.0,
        "first quarter: lit on the right: {first_l} {first_r}"
    );
    assert!(
        last_l > last_r * 3.0,
        "last quarter: lit on the left: {last_l} {last_r}"
    );
}

#[test]
fn resolved_clouds_hold_still_from_frame_to_frame() {
    let Some(mut stage) = stage((96, 64), None) else { return };
    set_time(&mut stage, 15.0, SkyKit::default());
    look(&mut stage, vec3(0.0, 2.0, 0.0), -Vec3::Z, 30.0);
    let a = stage.settled(SETTLE);
    let b = stage.image();
    let d = diff(&a, &b, 3);
    assert!(
        d.within(Tolerance {
            channel: 4,
            outliers: 0.02,
            edge: 0,
        }),
        "the temporal resolve keeps the marched clouds from flickering: {d:?}"
    );
    stage.finish();
}

// ---------------------------------------------------------------------------
// Cost.

/// The GPU time of the kit's passes at each [`CloudQuality`], at
/// 1920 x 1080 (the numbers in `CloudQuality`'s docs). Run by hand:
/// `cargo test -p ponko --lib --features 3d --release cloud_presets_gpu_time -- --ignored --nocapture`.
#[test]
#[ignore]
fn cloud_presets_gpu_time() {
    let Some(mut stage) = stage_with(Setup {
        size: (1920, 1080),
        timestamps: true,
        ..Setup::default()
    }) else {
        return;
    };
    let Some(mut timer) = super::timer::GpuTimer::new(&stage.device, &stage.queue) else {
        skip("timestamp queries unavailable");
        return;
    };
    look(&mut stage, vec3(0.0, 2.0, 0.0), -Vec3::Z, 20.0);
    stage.ctx.scene3d.lighting.sun_direction = crate::render::Sky::sun_direction_at(14.0);
    // The whole frame's GPU time (every pass's), over 40 frames after 20 to
    // settle: what the kit adds is the difference to the frame without it.
    // (Per-pass stamps on a tiled GPU charge a pass with work queued before
    // it, so the kit's own passes are listed for orientation only.)
    let kit_passes = ["sky kit lookup", "cloud shadows", "clouds", "cloud resolve"];
    let mut run = |stage: &mut Stage| {
        let (mut totals, mut each) = (Vec::new(), vec![Vec::new(); kit_passes.len()]);
        for frame in 0..60 {
            stage.render_timed(Some(&mut timer));
            if frame < 20 || timer.last_frame.is_empty() {
                continue;
            }
            totals.push(timer.last_frame.iter().map(|p| p.ms).sum::<f32>());
            for (i, name) in kit_passes.iter().enumerate() {
                each[i].push(timer.last_frame.iter().find(|p| p.name == *name).map_or(0.0, |p| p.ms));
            }
        }
        totals.sort_by(f32::total_cmp);
        for v in &mut each {
            v.sort_by(f32::total_cmp);
        }
        let at = |v: &Vec<f32>, q: f32| v[((v.len() - 1) as f32 * q) as usize];
        let parts: Vec<String> = kit_passes
            .iter()
            .zip(&each)
            .map(|(name, v)| format!("{name} {:.3}", at(v, 0.5)))
            .collect();
        (at(&totals, 0.5), at(&totals, 0.1), at(&totals, 0.9), parts.join(", "))
    };
    let (base, lo, hi, _) = run(&mut stage);
    eprintln!("1920x1080, frame without the kit (atmosphere): {base:.3} ms median ({lo:.3}..{hi:.3})");
    for quality in CloudQuality::ALL {
        set_time(&mut stage, 14.0, SkyKit::default().with_quality(quality));
        let (total, lo, hi, parts) = run(&mut stage);
        eprintln!(
            "{}: frame {total:.3} ms median ({lo:.3}..{hi:.3}), kit +{:.3} ms; {parts}",
            quality.name(),
            total - base,
        );
    }
    stage.finish();
}
