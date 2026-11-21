//! Baked lighting against realtime global illumination and a path-traced
//! reference, in the red-walled room of `gi_render_test`: the sun shines
//! through an opening in the roof onto the red west wall, and the light it
//! bounces is the only light most of the room gets. Realtime global
//! illumination also in an open scene: a red wall and a white canopy on
//! open ground, in the sun.
use super::gi_render_test::{Rig, SIZE};
use super::render_test_support::{Image, keep_image, skip};
#[allow(unused_imports)]
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::{
    BakeModel, BakedLighting, GlobalIllumination, LightBakeScene, LightBakeSettings, MaterialDesc, MeshData, Model,
    Tonemap, bake_lighting,
};

/// The room's boxes: from `lo` to `hi`, red or white.
fn room() -> Vec<(bool, Vec3, Vec3)> {
    let (inner, outer) = (3.5, 3.9);
    vec![
        (false, vec3(-outer, -0.9, -outer), vec3(outer, -0.5, outer)), // floor
        (true, vec3(-outer, -0.5, -outer), vec3(-inner, 3.5, outer)),  // west
        (false, vec3(inner, -0.5, -outer), vec3(outer, 3.5, outer)),   // east
        (false, vec3(-inner, -0.5, -outer), vec3(inner, 3.5, -inner)), // north
        (false, vec3(-inner, -0.5, inner), vec3(inner, 3.5, outer)),   // south
        // The roof, with an opening from x -2 to 0 and z -1.5 to 1.5.
        (false, vec3(-outer, 3.5, -outer), vec3(-2.0, 3.9, outer)),
        (false, vec3(0.0, 3.5, -outer), vec3(outer, 3.9, outer)),
        (false, vec3(-2.0, 3.5, -outer), vec3(0.0, 3.9, -1.5)),
        (false, vec3(-2.0, 3.5, 1.5), vec3(0.0, 3.9, outer)),
    ]
}

/// The open scene's boxes: the ground, a red wall standing in the sun
/// (its east side lit) and a white canopy east of it, whose shade the sky
/// and the wall light.
fn open_ground() -> Vec<(bool, Vec3, Vec3)> {
    vec![
        (false, vec3(-30.0, -0.9, -30.0), vec3(30.0, -0.5, 30.0)), // ground
        (true, vec3(-0.2, -0.5, -3.0), vec3(0.2, 2.5, 3.0)),       // red wall
        (false, vec3(1.5, 1.2, -1.5), vec3(4.5, 1.5, 1.5)),        // canopy
    ]
}

/// Which scene a test draws.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    /// The room lit through an opening in its roof ([`room`]).
    Room,
    /// The open scene ([`open_ground`]).
    Open,
}

impl Place {
    fn boxes(self) -> Vec<(bool, Vec3, Vec3)> {
        match self {
            Place::Room => room(),
            Place::Open => open_ground(),
        }
    }

    /// The probe grid: one probe a unit around the room, the default grid
    /// in the open. The most probes a frame either way.
    fn gi(self, enabled: bool) -> GlobalIllumination {
        let (extent, spacing) = match self {
            Place::Room => (vec3(12.0, 4.0, 8.0), 1.0),
            Place::Open => {
                let d = GlobalIllumination::default();
                (d.extent, d.spacing)
            }
        };
        GlobalIllumination {
            enabled,
            extent,
            spacing,
            probes_per_frame: GlobalIllumination::MAX_PROBES_PER_FRAME,
            intensity: 1.0,
        }
    }
}

fn material(red: bool) -> MaterialDesc {
    // Matte, so what differs between the images is the diffuse light.
    MaterialDesc::color(if red { Color::rgb(0.9, 0.04, 0.04) } else { Color::WHITE }).with_roughness(1.0)
}

/// A box from `lo` to `hi` around its center, with lightmap coordinates.
fn box_mesh(lo: Vec3, hi: Vec3, texels_per_unit: f32) -> MeshData {
    let mut data = MeshData::cube(1.0);
    for p in &mut data.positions {
        *p *= hi - lo;
    }
    data.generate_lightmap_uvs(texels_per_unit);
    data
}

/// How the room is lit.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Light {
    /// Ambient light only.
    Flat,
    Realtime,
    Baked,
}

/// The light, sky and camera of `place`.
fn set_up_in(place: Place, ctx: &mut Ctx) {
    set_up(ctx);
    if place == Place::Open {
        // Dimmer than in the room, so the sunlit ground isn't clipped.
        ctx.scene3d.lighting.sun_direction = vec3(-1.0, -1.2, 0.4);
        ctx.scene3d.lighting.sun_intensity = 2.0;
        ctx.scene3d.lighting.shadow_distance = 40.0;
        // Looking down, so no sky is in view (the reference has none).
        ctx.scene3d.camera.position = vec3(8.0, 6.0, 5.0);
        ctx.scene3d.camera.look_at(vec3(1.5, -0.5, 0.0));
    }
}

/// The light, sky and camera of the room.
fn set_up(ctx: &mut Ctx) {
    let lighting = &mut ctx.scene3d.lighting;
    lighting.sun_direction = vec3(-1.0, -1.0, 0.0);
    lighting.sun_color = Color::WHITE;
    lighting.sun_intensity = 4.0;
    lighting.ambient_intensity = 0.3;
    lighting.shadow_distance = 30.0;
    ctx.scene3d.sky.sky_color = Color::rgb(0.5, 0.5, 0.5);
    ctx.scene3d.sky.ground_color = Color::rgb(0.5, 0.5, 0.5);
    ctx.scene3d.post.tonemap = Tonemap::None;
    ctx.scene3d.post.bloom = 0.0;
    ctx.scene3d.camera.position = vec3(2.0, 1.3, 0.3);
    ctx.scene3d.camera.look_at(vec3(-3.5, -0.2, 0.0));
}

/// What the bake of the room is made from.
fn bake_scene(ctx: &Ctx, texels_per_unit: f32) -> LightBakeScene {
    bake_scene_of(Place::Room, ctx, texels_per_unit)
}

/// What the bake (and the path-traced reference) of `place` is made from.
fn bake_scene_of(place: Place, ctx: &Ctx, texels_per_unit: f32) -> LightBakeScene {
    let mut scene = LightBakeScene::new(&ctx.scene3d.lighting, &ctx.scene3d.sky);
    for (red, lo, hi) in place.boxes() {
        let data = box_mesh(lo, hi, texels_per_unit);
        scene.add(BakeModel::new(&data, Mat4::from_translation((lo + hi) * 0.5)).with_material(&material(red)));
    }
    scene
}

fn settings(texels_per_unit: f32) -> LightBakeSettings {
    LightBakeSettings::default()
        .with_texels_per_unit(texels_per_unit)
        .with_samples(256)
        .with_page_size(256)
        .with_probe_spacing(1.0)
}

/// The room drawn lit as `light` says, on a device with the texture slots
/// realtime global illumination needs (`Some(true)`), at WebGL2's limits
/// (`Some(false)`) or as the device (or forced tier) comes (`None`).
/// `None` without such a GPU.
fn draw(light: Light, slots: Option<bool>, baked: Option<&BakedLighting>, tpu: f32) -> Option<(Rig, Vec<u8>)> {
    // Enough frames for every probe to look around twice (two bounces).
    draw_in(Place::Room, light, slots, baked, tpu, 80)
}

/// [`draw`] in `place`, `frames` frames with realtime global illumination.
fn draw_in(
    place: Place,
    light: Light,
    slots: Option<bool>,
    baked: Option<&BakedLighting>,
    tpu: f32,
    frames: usize,
) -> Option<(Rig, Vec<u8>)> {
    let mut rig = match slots {
        Some(slots) => Rig::new(slots)?,
        None => Rig::any()?,
    };
    set_up_in(place, &mut rig.ctx);
    let lightmaps = light == Light::Baked;
    for (i, (red, lo, hi)) in place.boxes().into_iter().enumerate() {
        let mesh = rig.mesh(&box_mesh(lo, hi, tpu), material(red));
        let entity = rig
            .ctx
            .world
            .spawn((Transform3D::at((lo + hi) * 0.5), Model::new(mesh)));
        if lightmaps && let Some(lightmap) = baked.and_then(|b| b.lightmap(i)) {
            rig.ctx.world.insert(entity, lightmap);
        }
    }
    rig.ctx.scene3d.lighting.gi = place.gi(light == Light::Realtime);
    if lightmaps {
        let baked = baked.expect("a bake to draw with");
        let layers = baked.gpu_layers().expect("fits");
        rig.renderer.set_baked_lighting(
            &rig.device,
            &rig.queue,
            Some((baked.page_size(), &layers)),
            &rig.textures,
        );
    }
    let pixels = rig.draw(if light == Light::Realtime { frames } else { 2 });
    keep_image(&Image {
        width: SIZE,
        height: SIZE,
        rgba: pixels.clone(),
    });
    Some((rig, pixels))
}

/// sRGB (0..255) to linear.
fn linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear (clamped to 0..1) to sRGB (0..255).
fn srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// How far `pixels` are from `reference`: the mean difference of their
/// linear colors over 8 x 8 pixel tiles (so the reference's noise mostly
/// averages out), as a fraction of the reference's mean brightness.
fn error(pixels: &[u8], reference: &[Vec3]) -> f32 {
    const TILE: u32 = 8;
    let (mut diff, mut total) = (0.0, 0.0);
    for ty in 0..SIZE / TILE {
        for tx in 0..SIZE / TILE {
            let (mut a, mut b) = (Vec3::ZERO, Vec3::ZERO);
            for y in ty * TILE..(ty + 1) * TILE {
                for x in tx * TILE..(tx + 1) * TILE {
                    let i = (y * SIZE + x) as usize;
                    let p = &pixels[i * 4..i * 4 + 3];
                    a += Vec3::new(linear(p[0]), linear(p[1]), linear(p[2]));
                    b += reference[i].clamp(Vec3::ZERO, Vec3::ONE);
                }
            }
            diff += (a - b).abs().element_sum();
            total += b.element_sum();
        }
    }
    diff / total.max(1e-6)
}

/// The path-traced reference of the room as `rig`'s camera sees it.
fn reference(rig: &Rig, scene: &LightBakeScene, samples: u32) -> Vec<Vec3> {
    let camera = &rig.ctx.scene3d.camera;
    // One bounce more than the bake: the camera's ray is a bounce too.
    let image = crate::three::bake::reference_image(
        scene,
        camera.position,
        camera.view_proj(),
        (SIZE, SIZE),
        samples,
        LightBakeSettings::default().bounces + 1,
    );
    keep_image(&Image {
        width: SIZE,
        height: SIZE,
        rgba: image
            .iter()
            .flat_map(|c| [srgb(c.x), srgb(c.y), srgb(c.z), 255])
            .collect(),
    });
    image
}

/// The path-traced reference of `scene` as `rig`'s camera sees it, with
/// light bounced until it has all but run out (256 times): realtime global
/// illumination bounces light with every probe update, for as long as it
/// runs, so this is what it should settle on. (In the room, whose white
/// walls send all the light on, the floor is twice as bright as after the
/// bake's three bounces.)
fn converged_reference(rig: &Rig, scene: &LightBakeScene, samples: u32) -> Vec<Vec3> {
    let camera = &rig.ctx.scene3d.camera;
    let image =
        crate::three::bake::reference_image(scene, camera.position, camera.view_proj(), (SIZE, SIZE), samples, 256);
    keep_image(&Image {
        width: SIZE,
        height: SIZE,
        rgba: image
            .iter()
            .flat_map(|c| [srgb(c.x), srgb(c.y), srgb(c.z), 255])
            .collect(),
    });
    image
}

/// Flat ambient light and realtime global illumination after `frames`
/// frames in `place`, and their errors to the converged path-traced
/// reference. `None` without a GPU that has global illumination.
fn realtime_against_the_reference(place: Place, frames: usize) -> Option<(f32, f32)> {
    let (rig, flat) = draw_in(place, Light::Flat, Some(true), None, 4.0, frames)?;
    let (_, realtime) = draw_in(place, Light::Realtime, Some(true), None, 4.0, frames)?;
    let scene = bake_scene_of(place, &rig.ctx, 4.0);
    let truth = converged_reference(&rig, &scene, 64);
    let (flat_error, realtime_error) = (error(&flat, &truth), error(&realtime, &truth));
    eprintln!(
        "{place:?}: error to the converged path-traced reference: flat ambient {flat_error:.3}, \
         realtime GI {realtime_error:.3} after {frames} frames"
    );
    Some((flat_error, realtime_error))
}

#[test]
fn realtime_gi_settles_closer_to_the_path_traced_room_than_flat_ambient_light() {
    // Long enough for the room's light to settle: its white walls pass it
    // on for a hundred bounces and more.
    let Some((flat_error, realtime_error)) = realtime_against_the_reference(Place::Room, 1500) else {
        return;
    };
    // The negative control: the flat ambient light is far off indoors.
    assert!(flat_error > 0.4, "flat ambient light is wrong indoors: {flat_error}");
    // Before, the probes' views took the sky's ambient light where the
    // probes hadn't looked yet or faded out, and the room kept it: 0.33,
    // twice the bound, most of it sky light the room doesn't get.
    assert!(
        realtime_error < flat_error * 0.25,
        "realtime GI settles on the path-traced room: {realtime_error} vs flat {flat_error}"
    );
}

#[test]
fn realtime_gi_settles_closer_to_the_path_traced_open_scene_than_flat_ambient_light() {
    let Some((flat_error, realtime_error)) = realtime_against_the_reference(Place::Open, 600) else {
        return;
    };
    // The negative control: the flat ambient light misses the canopy's
    // shade and the wall's red light.
    assert!(flat_error > 0.05, "flat ambient light misses the shade: {flat_error}");
    assert!(
        realtime_error < flat_error * 0.6,
        "realtime GI settles on the path-traced open scene: {realtime_error} vs flat {flat_error}"
    );
}

#[test]
fn baked_lighting_comes_closer_to_the_path_traced_room_than_flat_ambient_light() {
    let tpu = 4.0;
    let Some((rig, flat)) = draw(Light::Flat, None, None, tpu) else {
        return;
    };
    let scene = bake_scene(&rig.ctx, tpu);
    let baked = bake_lighting(&scene, &settings(tpu)).expect("bakes");
    let stats = baked.stats();
    let Some((_, lit)) = draw(Light::Baked, None, Some(&baked), tpu) else {
        return;
    };
    let truth = reference(&rig, &scene, 64);
    let (flat_error, baked_error) = (error(&flat, &truth), error(&lit, &truth));
    eprintln!(
        "error to the path-traced room: flat ambient {flat_error:.3}, baked {baked_error:.3}; \
         bake: {:.2} s, {} texels, {} probes, {:.1} M rays, {} KiB on the GPU",
        stats.seconds,
        stats.texels,
        stats.probes,
        stats.rays as f64 / 1e6,
        baked.gpu_bytes() / 1024
    );
    // The negative control: the flat ambient light is far off.
    assert!(flat_error > 0.25, "flat ambient light is wrong indoors: {flat_error}");
    assert!(
        baked_error < flat_error * 0.5,
        "baked lighting is closer to the truth: {baked_error} vs {flat_error}"
    );
    // The red bounce on the floor beside the red wall, as red as in the
    // reference (averaged over 9 x 9 pixels: its noise).
    let near_red = vec3(-3.0, -0.5, -0.8);
    let (f, b) = (rig.at(&flat, near_red), rig.at(&lit, near_red));
    let t = reference_at(&rig, &truth, near_red);
    eprintln!("floor by the red wall: flat {f:?}, baked {b:?}, path traced {t:?}");
    assert!((f[0] - f[1]).abs() <= 4, "no red without bounced light: {f:?}");
    let (tint, truth_tint) = (b[0] - b[1], t[0] - t[1]);
    assert!(
        truth_tint > 8,
        "positive control: the reference's floor is red there: {t:?}"
    );
    assert!(
        (tint - truth_tint).abs() <= truth_tint / 2 + 4,
        "red bounce light on the floor as in the reference: {b:?} vs {t:?}"
    );
}

/// The reference's color (sRGB, 0 to 255) around world point `p`: the mean
/// over 9 x 9 pixels.
fn reference_at(rig: &Rig, truth: &[Vec3], p: Vec3) -> [i32; 3] {
    let c = rig.ctx.scene3d.camera.view_proj() * p.extend(1.0);
    let ndc = c.truncate() / c.w;
    let x = ((ndc.x * 0.5 + 0.5) * SIZE as f32) as i32;
    let y = ((0.5 - ndc.y * 0.5) * SIZE as f32) as i32;
    let mut sum = Vec3::ZERO;
    for dy in -4..=4 {
        for dx in -4..=4 {
            let (px, py) = ((x + dx).clamp(0, SIZE as i32 - 1), (y + dy).clamp(0, SIZE as i32 - 1));
            sum += truth[(py * SIZE as i32 + px) as usize].clamp(Vec3::ZERO, Vec3::ONE);
        }
    }
    let m = sum / 81.0;
    [srgb(m.x), srgb(m.y), srgb(m.z)].map(i32::from)
}

#[test]
fn baked_lighting_draws_the_same_at_webgl2_limits() {
    let tpu = 2.0;
    let Some((rig, _)) = draw(Light::Flat, Some(false), None, tpu) else {
        skip("no GPU at WebGL2's limits");
        return;
    };
    let scene = bake_scene(&rig.ctx, tpu);
    let baked = bake_lighting(&scene, &settings(tpu).with_samples(64)).expect("bakes");
    let Some((_, low)) = draw(Light::Baked, Some(false), Some(&baked), tpu) else {
        return;
    };
    let Some((_, high)) = draw(Light::Baked, Some(true), Some(&baked), tpu) else {
        return;
    };
    let reference: Vec<Vec3> = high
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| Vec3::new(linear(p[0]), linear(p[1]), linear(p[2])))
        .collect();
    let apart = error(&low, &reference);
    eprintln!("WebGL2 limits vs desktop: {apart:.4}");
    assert!(apart < 0.02, "the same light on 16 texture slots: {apart}");
}

/// The red tint (red minus green) of the floor 20 cm east of a 2 cm white
/// partition from floor to roof at `x` (with `None`, there in the room
/// without one), lit by baked light: `gi_render_test` measures how
/// realtime global illumination leaks through it.
fn baked_tint_past_a_partition(x: Option<f32>) -> Option<i32> {
    let tpu = 8.0;
    let mut rig = Rig::any()?;
    set_up(&mut rig.ctx);
    let mut boxes = room();
    let mut floor = -0.8;
    if let Some(x) = x {
        boxes.push((false, vec3(x - 0.01, -0.5, -3.5), vec3(x + 0.01, 3.5, 3.5)));
        floor = x + 0.2;
    }
    let mut scene = LightBakeScene::new(&rig.ctx.scene3d.lighting, &rig.ctx.scene3d.sky);
    for &(red, lo, hi) in &boxes {
        let data = box_mesh(lo, hi, tpu);
        scene.add(BakeModel::new(&data, Mat4::from_translation((lo + hi) * 0.5)).with_material(&material(red)));
    }
    let baked = bake_lighting(&scene, &settings(tpu).with_samples(64)).expect("bakes");
    for (i, &(red, lo, hi)) in boxes.iter().enumerate() {
        let mesh = rig.mesh(&box_mesh(lo, hi, tpu), material(red));
        let lightmap = baked.lightmap(i).expect("lightmapped");
        rig.ctx
            .world
            .spawn((Transform3D::at((lo + hi) * 0.5), Model::new(mesh), lightmap));
    }
    let layers = baked.gpu_layers().expect("fits");
    rig.renderer.set_baked_lighting(
        &rig.device,
        &rig.queue,
        Some((baked.page_size(), &layers)),
        &rig.textures,
    );
    let pixels = rig.draw(2);
    let c = rig.at(&pixels, vec3(floor, -0.5, -0.8));
    Some(c[0] - c[1])
}

#[test]
fn baked_light_does_not_leak_through_a_thin_partition() {
    let Some(open) = baked_tint_past_a_partition(None) else {
        return;
    };
    let Some(on_plane) = baked_tint_past_a_partition(Some(-1.0)) else {
        return;
    };
    eprintln!("red tint on the floor past a 2 cm partition, baked: {open} without it, {on_plane} with it (truth: 0)");
    assert!(open > 15, "positive control: the open room's floor is tinted: {open}");
    assert!(on_plane <= 5, "the partition holds the red light back: {on_plane}");
}

/// What baked lighting costs and how close it comes, next to realtime
/// global illumination: bake time and memory, the error to a path-traced
/// reference in the red-walled room, and GPU frame time in a town at
/// 1920x1080 on this GPU with its desktop limits, at the Low preset and at
/// WebGL2's limits.
///
/// `cargo test --release --features 3d --lib baked_lighting_cost -- --ignored --nocapture`
#[test]
#[ignore = "a benchmark: run it in release"]
fn baked_lighting_cost() {
    room_quality(4.0);
    room_quality(8.0);
    town_cost(true, false);
    town_cost(true, true);
    town_cost(false, false);
}

/// The room's error to the reference: flat ambient light, realtime global
/// illumination, and a bake at `tpu` texels per unit.
fn room_quality(tpu: f32) {
    let Some((rig, flat)) = draw(Light::Flat, Some(true), None, tpu) else {
        return;
    };
    let Some((_, realtime)) = draw(Light::Realtime, Some(true), None, tpu) else {
        return;
    };
    let scene = bake_scene(&rig.ctx, tpu);
    let baked = bake_lighting(&scene, &settings(tpu)).expect("bakes");
    let Some((_, lit)) = draw(Light::Baked, Some(true), Some(&baked), tpu) else {
        return;
    };
    let start = std::time::Instant::now();
    let truth = reference(&rig, &scene, 1024);
    let s = baked.stats();
    println!(
        "room, {tpu} texels a unit: error to the path-traced reference (1024 rays a pixel, {:.1} s): \
         flat ambient {:.3}, realtime GI {:.3}, baked {:.3}; bake {:.2} s ({} texels, {} probes, {:.1} M rays), \
         {} KiB on the GPU",
        start.elapsed().as_secs_f64(),
        error(&flat, &truth),
        error(&realtime, &truth),
        error(&lit, &truth),
        s.seconds,
        s.texels,
        s.probes,
        s.rays as f64 / 1e6,
        baked.gpu_bytes() / 1024
    );
    // Realtime GI bounces light for as long as it runs: against the
    // reference it settles on, once it has.
    let Some((_, settled)) = draw_in(Place::Room, Light::Realtime, Some(true), None, tpu, 1500) else {
        return;
    };
    let converged = converged_reference(&rig, &scene, 256);
    println!(
        "room, {tpu} texels a unit: error to the converged path-traced reference (256 bounces): \
         flat ambient {:.3}, realtime GI {:.3} after 1500 frames",
        error(&flat, &converged),
        error(&settled, &converged),
    );
}

/// GPU frame time in a town of 900 boxes at 1920x1080: on a device with
/// global illumination's texture slots (`slots`) flat, with realtime GI
/// and baked; at the Low preset (`low`) or at WebGL2's limits (no `slots`)
/// flat and baked.
fn town_cost(slots: bool, low: bool) {
    let Some(mut rig) = Rig::sized(slots, (1920, 1080)) else {
        return;
    };
    let Some(mut timer) = super::timer::GpuTimer::new(&rig.device, &rig.queue) else {
        skip("no GPU timestamps");
        return;
    };
    // One unit cube for every building (lightmap coordinates on it), each
    // building its own lightmap.
    let mut cube = MeshData::cube(1.0);
    cube.generate_lightmap_uvs(16.0);
    let white = rig.mesh(&cube, MaterialDesc::color(Color::WHITE));
    let tinted = rig.mesh(&cube, MaterialDesc::color(Color::rgb(0.8, 0.5, 0.3)));
    let mut boxes = vec![(white, vec3(-60.0, -1.0, -60.0), vec3(60.0, 0.0, 60.0))];
    let mut seed = 7u32;
    let mut random = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / (1 << 24) as f32
    };
    for i in 0..30 {
        for j in 0..30 {
            let at = vec3(i as f32 * 4.0 - 58.0, 0.0, j as f32 * 4.0 - 58.0);
            let mesh = if (i + j) % 3 == 0 { tinted } else { white };
            boxes.push((mesh, at, at + vec3(2.5, 3.0 + random() * 9.0, 2.5)));
        }
    }
    let lamp = crate::d3::PointLight {
        intensity: 30.0,
        range: 15.0,
        shadows: true,
        ..Default::default()
    };
    rig.ctx.world.spawn((Transform3D::at(vec3(6.0, 4.0, 5.0)), lamp));
    rig.ctx.scene3d.lighting.sun_direction = vec3(-0.4, -1.0, -0.3);
    rig.ctx.scene3d.camera.position = vec3(1.0, 1.7, 1.0);
    rig.ctx.scene3d.camera.look_at(vec3(20.0, 3.0, 12.0));
    rig.ctx.scene3d.camera.far = 200.0;
    if low {
        rig.ctx.scene3d.set_quality(crate::render::GraphicsQuality::Low);
    }
    let transform =
        |lo: Vec3, hi: Vec3| Mat4::from_scale_rotation_translation(hi - lo, Quat::IDENTITY, (lo + hi) * 0.5);
    let mut scene = LightBakeScene::new(&rig.ctx.scene3d.lighting, &rig.ctx.scene3d.sky);
    for &(mesh, lo, hi) in &boxes {
        let color = if mesh == tinted {
            Color::rgb(0.8, 0.5, 0.3)
        } else {
            Color::WHITE
        };
        scene.add(BakeModel::new(&cube, transform(lo, hi)).with_albedo(color));
    }
    scene.add_lights_from(&mut rig.ctx.world);
    let baked = bake_lighting(
        &scene,
        &LightBakeSettings::default()
            .with_texels_per_unit(1.0)
            .with_probe_spacing(4.0),
    )
    .expect("bakes");
    let s = baked.stats();
    let entities: Vec<_> = boxes
        .iter()
        .map(|&(mesh, lo, hi)| {
            rig.ctx.world.spawn((
                Transform3D {
                    position: (lo + hi) * 0.5,
                    scale: hi - lo,
                    ..Default::default()
                },
                Model::new(mesh),
            ))
        })
        .collect();
    let layers = baked.gpu_layers().expect("fits");
    let mut variants = vec!["flat ambient", "baked"];
    if slots && !low {
        variants.insert(1, "realtime GI (defaults)");
    }
    let mut best = vec![(f32::MAX, String::new()); variants.len()];
    for _ in 0..5 {
        for (k, &variant) in variants.iter().enumerate() {
            let bake = variant == "baked";
            rig.ctx.scene3d.lighting.gi = GlobalIllumination {
                enabled: variant.starts_with("realtime"),
                ..Default::default()
            };
            for (i, &e) in entities.iter().enumerate() {
                if bake {
                    rig.ctx.world.insert(e, baked.lightmap(i).expect("lightmapped"));
                } else {
                    let _ = rig.ctx.world.remove::<crate::render::Lightmapped>(e);
                }
            }
            rig.renderer.set_baked_lighting(
                &rig.device,
                &rig.queue,
                bake.then(|| (baked.page_size(), layers.as_slice())),
                &rig.textures,
            );
            for _ in 0..20 {
                let _ = rig.frame(Some(&mut timer));
            }
            let (mut frames, mut total) = (0, 0.0);
            let mut parts = std::collections::BTreeMap::<&str, f32>::new();
            let mut read = timer.frames_read;
            for _ in 0..60 {
                let _ = rig.frame(Some(&mut timer));
                if timer.frames_read != read {
                    read = timer.frames_read;
                    frames += 1;
                    let spans = &timer.last_frame;
                    let end = spans.iter().map(|p| p.start_ms + p.ms).fold(0.0, f32::max);
                    total += end - spans.iter().map(|p| p.start_ms).fold(f32::MAX, f32::min);
                    for p in spans {
                        *parts.entry(p.name).or_default() += p.ms;
                    }
                }
            }
            let n = frames.max(1) as f32;
            if total / n < best[k].0 {
                let shown: Vec<String> = parts
                    .iter()
                    .filter(|(name, _)| name.starts_with("gi") || **name == "opaque")
                    .map(|(name, ms)| format!("{name} {:.3}", ms / n))
                    .collect();
                best[k] = (total / n, shown.join(", "));
            }
        }
    }
    let path = match (slots, low) {
        (false, _) => "WebGL2 limits",
        (true, true) => "Low preset",
        (true, false) => "desktop limits",
    };
    println!(
        "town bake: {:.2} s, {} texels on {} page(s), {} probes, {:.1} M rays, {} KiB on the GPU",
        s.seconds,
        s.texels,
        baked.pages(),
        s.probes,
        s.rays as f64 / 1e6,
        baked.gpu_bytes() / 1024
    );
    for (variant, (gpu, parts)) in variants.iter().zip(&best) {
        println!("town, {path}, {variant}: GPU frame {gpu:.3} ms ({parts})");
    }
}

/// The room at 512 x 512 with a crate on the floor and a ball beside it
/// (no lightmap: lit by the probes), flat and baked, for looking at:
/// `PONKO_RENDER_TEST_IMAGES=1 cargo test --features 3d --lib
/// baked_room_close_up -- --ignored` saves them in
/// `target/render-test-output`. Returns the baked image's darkest
/// lightmapped floor pixel (sum of red, green and blue) next to the crate.
#[test]
#[ignore = "pictures to look at, with the benchmark"]
fn baked_room_close_up() {
    let tpu = 8.0;
    let size = 512;
    for baked_light in [false, true] {
        let Some(mut rig) = Rig::sized(true, (size, size)) else {
            return;
        };
        set_up(&mut rig.ctx);
        rig.ctx.scene3d.camera.position = vec3(1.5, 1.6, 2.0);
        rig.ctx.scene3d.camera.look_at(vec3(-2.0, -0.3, -0.5));
        let mut boxes = room();
        boxes.push((false, vec3(-2.0, -0.5, -1.2), vec3(-1.0, 0.5, -0.2)));
        let mut scene = LightBakeScene::new(&rig.ctx.scene3d.lighting, &rig.ctx.scene3d.sky);
        for &(red, lo, hi) in &boxes {
            let data = box_mesh(lo, hi, tpu);
            scene.add(BakeModel::new(&data, Mat4::from_translation((lo + hi) * 0.5)).with_material(&material(red)));
        }
        let baked = bake_lighting(&scene, &settings(tpu).with_probe_spacing(0.5)).expect("bakes");
        for (i, &(red, lo, hi)) in boxes.iter().enumerate() {
            let mesh = rig.mesh(&box_mesh(lo, hi, tpu), material(red));
            let e = rig
                .ctx
                .world
                .spawn((Transform3D::at((lo + hi) * 0.5), Model::new(mesh)));
            if baked_light {
                rig.ctx.world.insert(e, baked.lightmap(i).expect("lightmapped"));
            }
        }
        let ball = rig.mesh(&MeshData::sphere(0.4, 32), material(false));
        rig.ctx
            .world
            .spawn((Transform3D::at(vec3(-0.5, -0.1, 0.8)), Model::new(ball)));
        if baked_light {
            let layers = baked.gpu_layers().expect("fits");
            rig.renderer.set_baked_lighting(
                &rig.device,
                &rig.queue,
                Some((baked.page_size(), &layers)),
                &rig.textures,
            );
        }
        let pixels = rig.draw(2);
        keep_image(&Image {
            width: size,
            height: size,
            rgba: pixels,
        });
    }
}

/// The room's bake scene with the sun shining along `sun`, lit as
/// [`set_up`] lights it (no GPU needed).
fn room_scene_lit_from(sun: Vec3, texels_per_unit: f32) -> LightBakeScene {
    let lighting = crate::render::Lighting {
        sun_direction: sun,
        sun_color: Color::WHITE,
        sun_intensity: 4.0,
        ambient_intensity: 0.3,
        ..Default::default()
    };
    let sky = crate::render::Sky {
        sky_color: Color::rgb(0.5, 0.5, 0.5),
        ground_color: Color::rgb(0.5, 0.5, 0.5),
        ..Default::default()
    };
    let mut scene = LightBakeScene::new(&lighting, &sky);
    for (red, lo, hi) in room() {
        let data = box_mesh(lo, hi, texels_per_unit);
        scene.add(BakeModel::new(&data, Mat4::from_translation((lo + hi) * 0.5)).with_material(&material(red)));
    }
    scene
}

/// How far bake `a` is from bake `b` (of the same scene and page layout):
/// the mean absolute difference of their lightmap texels' light (red,
/// green and blue, over every texel both baked) as a share of `b`'s mean,
/// and the same for the probe volume's light toward each of the six axes
/// at every probe.
fn bake_error(a: &BakedLighting, b: &BakedLighting) -> (f32, f32) {
    assert_eq!(
        (a.pages(), a.page_size()),
        (b.pages(), b.page_size()),
        "the same layout"
    );
    let (mut diff, mut total) = (0.0f64, 0.0f64);
    for page in 0..b.pages() {
        for y in 0..b.page_size() {
            for x in 0..b.page_size() {
                if let (Some((la, true)), Some((lb, true))) = (a.texel(page, x, y), b.texel(page, x, y)) {
                    diff += f64::from((la - lb).abs().element_sum());
                    total += f64::from(lb.element_sum());
                }
            }
        }
    }
    let lightmaps = (diff / total.max(1e-9)) as f32;
    let (mut diff, mut total) = (0.0f64, 0.0f64);
    for x in -3..=3 {
        for y in 0..=3 {
            for z in -3..=3 {
                let at = vec3(x as f32, y as f32 - 0.2, z as f32);
                for n in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z] {
                    if let (Some(pa), Some(pb)) = (a.probe_light(at, n), b.probe_light(at, n)) {
                        diff += f64::from((pa - pb).abs().element_sum());
                        total += f64::from(pb.element_sum());
                    }
                }
            }
        }
    }
    (lightmaps, (diff / total.max(1e-9)) as f32)
}

/// The most the GPU baker's lightmaps and probes may differ from the CPU
/// baker's, by [`bake_error`]: 4 % for the lightmaps and 10 % for the
/// probes, and no more than 1.5 times (plus 1 %) the difference between two
/// CPU bakes with different seeds (the rays' noise: 2.3 % and 6.0 % in the
/// room at these settings on an M4 Max, where the GPU came to 2.0 % and
/// 6.0 %).
const GPU_BAKE_BOUND: (f32, f32) = (0.04, 0.10);

/// Whether `error` is within [`GPU_BAKE_BOUND`] and the noise `noise`.
fn within_noise(error: (f32, f32), noise: (f32, f32)) -> bool {
    error.0 < GPU_BAKE_BOUND.0
        && error.1 < GPU_BAKE_BOUND.1
        && error.0 <= noise.0 * 1.5 + 0.01
        && error.1 <= noise.1 * 1.5 + 0.01
}

#[test]
fn the_gpu_baker_matches_the_cpu_baker_within_the_noise() {
    let Some(gpu) = super::render_test_support::gpu() else {
        return;
    };
    let tpu = 2.0;
    let scene = room_scene_lit_from(vec3(-1.0, -1.0, 0.0), tpu);
    let settings = settings(tpu).with_samples(512);
    let start = std::time::Instant::now();
    let on_gpu = match crate::three::bake::bake_lighting_on(&gpu.device, &gpu.queue, &scene, &settings) {
        Ok(baked) => baked,
        Err(e) if e.kind() == ErrorKind::Unsupported => {
            skip(e);
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let gpu_seconds = start.elapsed().as_secs_f64();
    let cpu = bake_lighting(&scene, &settings).expect("bakes");
    let other_seed = bake_lighting(&scene, &settings.with_seed(7)).expect("bakes");
    // The negative control: the sun through the opening onto the east
    // wall instead of the red west one.
    let moved = bake_lighting(&room_scene_lit_from(vec3(1.0, -1.0, 0.0), tpu), &settings).expect("bakes");
    let noise = bake_error(&other_seed, &cpu);
    let error = bake_error(&on_gpu, &cpu);
    let control = bake_error(&on_gpu, &moved);
    eprintln!(
        "lightmaps and probes against the CPU bake: GPU {error:?}, CPU with another seed {noise:?}, \
         CPU with the sun moved {control:?}; GPU bake {gpu_seconds:.2} s, CPU {:.2} s; {} texels, {} probes, {:.1} M rays on the GPU",
        cpu.stats().seconds,
        on_gpu.stats().texels,
        on_gpu.stats().probes,
        on_gpu.stats().rays as f64 / 1e6,
    );
    assert_eq!(on_gpu.stats().texels, cpu.stats().texels);
    assert!(
        (on_gpu.stats().rays as f64 / cpu.stats().rays as f64 - 1.0).abs() < 0.05,
        "the same number of rays, within the noise: {} vs {}",
        on_gpu.stats().rays,
        cpu.stats().rays
    );
    assert!(
        within_noise(error, noise),
        "the GPU bakes the CPU's light: {error:?} (noise {noise:?})"
    );
    assert!(
        !within_noise(control, noise) && control.0 > GPU_BAKE_BOUND.0 * 3.0 && control.1 > GPU_BAKE_BOUND.1 * 3.0,
        "a bake with the sun moved is far off: {control:?}"
    );
}

#[test]
fn the_gpu_baker_reads_albedo_maps_lamps_and_two_sided_surfaces() {
    let Some(gpu) = super::render_test_support::gpu() else {
        return;
    };
    let tpu = 2.0;
    let mut scene = room_scene_lit_from(vec3(-1.0, -1.0, 0.0), tpu);
    // The red wall's color from a texture instead (red and blue halves),
    // a lamp in the corner, and a two-sided white panel across the room.
    let pixels = [230u8, 20, 20, 255, 20, 20, 230, 255];
    scene.models[1] = scene.models[1]
        .clone()
        .with_albedo(Color::WHITE)
        .with_albedo_map(crate::render::BakeTexture::from_rgba(1, 2, &pixels).unwrap());
    let lamp = crate::d3::PointLight {
        intensity: 6.0,
        range: 5.0,
        ..Default::default()
    };
    scene.add_point_light(vec3(2.5, 2.5, 2.5), &lamp);
    let spot = crate::d3::SpotLight {
        intensity: 8.0,
        range: 6.0,
        ..Default::default()
    };
    scene.add_spot_light(vec3(-2.5, 3.0, -2.5), Vec3::NEG_Y, &spot);
    let mut panel = MeshData::plane(2.0);
    panel.generate_lightmap_uvs(tpu);
    scene.add(
        BakeModel::new(
            &panel,
            Mat4::from_translation(vec3(1.5, 1.5, 0.0)) * Mat4::from_rotation_x(0.5),
        )
        .with_double_sided(true),
    );
    let settings = settings(tpu).with_samples(512);
    let on_gpu = match crate::three::bake::bake_lighting_on(&gpu.device, &gpu.queue, &scene, &settings) {
        Ok(baked) => baked,
        Err(e) if e.kind() == ErrorKind::Unsupported => {
            skip(e);
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let cpu = bake_lighting(&scene, &settings).expect("bakes");
    let noise = bake_error(&bake_lighting(&scene, &settings.with_seed(7)).expect("bakes"), &cpu);
    let error = bake_error(&on_gpu, &cpu);
    // The negative control: without the albedo map (a white wall).
    let mut flat = scene.clone();
    flat.models[1] = flat.models[1].clone().without_albedo_map();
    let control = bake_error(&on_gpu, &bake_lighting(&flat, &settings).expect("bakes"));
    eprintln!(
        "textured, lamps and a two-sided panel: GPU against CPU {error:?}, CPU with another seed {noise:?}, \
         CPU without the albedo map {control:?}"
    );
    assert!(
        within_noise(error, noise),
        "the GPU bakes the CPU's light: {error:?} (noise {noise:?})"
    );
    assert!(!within_noise(control, noise), "the map matters: {control:?}");
}

/// How [`room_with`] makes the room's floor.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Floor {
    /// A model with its lightmap.
    Model,
    /// A [`LodGroup`](crate::render::LodGroup) of two levels (both the
    /// floor's mesh), showing its first (`near`) or its second level, with
    /// the [`Lightmapped`](crate::render::Lightmapped) entry `entry`.
    Lod { near: bool, entry: Option<u32> },
}

/// The room with its bake, the floor made as `floor` says, and a small
/// white cube (not in the bake) by the red wall: a model, or (`batch`)
/// an instance batch of one on `path`.
fn room_with(
    baked: &BakedLighting,
    tpu: f32,
    floor: Floor,
    batch: Option<super::gpu3d::BatchPath>,
) -> Option<(Rig, Vec<u8>)> {
    use crate::render::{LodGroup, LodLevel, LodRenderer};
    let mut rig = Rig::any()?;
    set_up(&mut rig.ctx);
    for (i, (red, lo, hi)) in room().into_iter().enumerate() {
        let mesh = rig.mesh(&box_mesh(lo, hi, tpu), material(red));
        let at = Transform3D::at((lo + hi) * 0.5);
        match floor {
            Floor::Lod { near, entry } if i == 0 => {
                // Near: the first level whatever the size on screen; far:
                // the second (the first only above 100 screen heights).
                let first = if near { 0.0 } else { 100.0 };
                let group = LodGroup::new(
                    8.0,
                    vec![
                        LodLevel::new(first, vec![LodRenderer::new(mesh)]),
                        LodLevel::new(0.0, vec![LodRenderer::new(mesh)]),
                    ],
                );
                let e = rig.ctx.world.spawn((at, group));
                if let Some(entry) = entry {
                    rig.ctx.world.insert(e, crate::render::Lightmapped(entry));
                }
            }
            _ => {
                let lightmap = baked.lightmap(i).expect("lightmapped");
                rig.ctx.world.spawn((at, Model::new(mesh), lightmap));
            }
        }
    }
    let (lo, hi) = (vec3(-3.2, -0.5, 0.6), vec3(-2.8, -0.1, 1.0));
    let cube = rig.mesh(&box_mesh(lo, hi, tpu), material(false));
    if let Some(path) = batch {
        rig.renderer.batching.path = path;
        let transform = Mat4::from_translation((lo + hi) * 0.5);
        let made =
            rig.ctx
                .gfx
                .create_instance_batch(cube, &[transform], crate::render::InstanceBatchOptions::default());
        rig.renderer.set_instance_batch(
            &rig.device,
            &rig.queue,
            made.id,
            rig.ctx.gfx.assets3d.instance_batches[made.id as usize].as_ref(),
        );
    } else {
        rig.ctx
            .world
            .spawn((Transform3D::at((lo + hi) * 0.5), Model::new(cube)));
    }
    let layers = baked.gpu_layers().expect("fits");
    rig.renderer.set_baked_lighting(
        &rig.device,
        &rig.queue,
        Some((baked.page_size(), &layers)),
        &rig.textures,
    );
    let pixels = rig.draw(2);
    keep_image(&Image {
        width: SIZE,
        height: SIZE,
        rgba: pixels.clone(),
    });
    Some((rig, pixels))
}

/// The mean difference between two images, 0 to 255 a channel.
fn apart(a: &[u8], b: &[u8]) -> f32 {
    let sum: u64 = a.iter().zip(b).map(|(&x, &y)| u64::from(x.abs_diff(y))).sum();
    sum as f32 / a.len() as f32
}

#[test]
fn a_lightmapped_lod_group_draws_its_first_level_with_the_lightmap_and_the_others_with_probes() {
    let tpu = 4.0;
    let Some(rig) = Rig::any() else {
        skip("no GPU");
        return;
    };
    let mut ctx = rig.ctx;
    set_up(&mut ctx);
    let baked = bake_lighting(&bake_scene(&ctx, tpu), &settings(tpu).with_samples(64)).expect("bakes");
    let Some((_, model)) = room_with(&baked, tpu, Floor::Model, None) else {
        return;
    };
    let draw = |near, entry| room_with(&baked, tpu, Floor::Lod { near, entry }, None).map(|d| d.1);
    let (Some(lod), Some(unmapped), Some(wrong), Some(far), Some(far_unmapped)) = (
        draw(true, Some(0)),
        draw(true, None),
        draw(true, Some(1)),
        draw(false, Some(0)),
        draw(false, None),
    ) else {
        return;
    };
    let (same, probes, wrong, far, far_probes) = (
        apart(&lod, &model),
        apart(&unmapped, &model),
        apart(&wrong, &model),
        apart(&far, &far_unmapped),
        apart(&far, &model),
    );
    eprintln!(
        "mean difference (0-255) from the floor as a lightmapped model: as a lightmapped LOD group {same}, \
         without its lightmap {probes}, with the red wall's entry {wrong}; its second level with the \
         component against without {far}, against the model {far_probes}"
    );
    assert!(same < 0.05, "the first level reads the floor's lightmap: {same}");
    assert!(
        probes > 0.5 && wrong > 0.5,
        "negative controls: without the entry or with another's, the floor is lit otherwise: {probes}, {wrong}"
    );
    assert!(far < 0.05, "the second level takes the probes' light: {far}");
    assert!(far_probes > 0.5, "and so is not drawn as the lightmap: {far_probes}");
}

#[test]
fn instance_batches_take_the_baked_probes_light_as_models_do() {
    use super::gpu3d::BatchPath;
    let tpu = 4.0;
    let Some(rig) = Rig::any() else {
        skip("no GPU");
        return;
    };
    let mut ctx = rig.ctx;
    set_up(&mut ctx);
    let baked = bake_lighting(&bake_scene(&ctx, tpu), &settings(tpu).with_samples(64)).expect("bakes");
    let Some((rig, model)) = room_with(&baked, tpu, Floor::Model, None) else {
        return;
    };
    // The cube's top, which the bounced light from the sunlit walls and
    // the floor reaches (not the sun).
    let top = vec3(-3.0, -0.1, 0.8);
    let lit = rig.at(&model, top);
    for path in [BatchPath::Cpu, BatchPath::Gpu] {
        let Some((rig, batch)) = room_with(&baked, tpu, Floor::Model, Some(path)) else {
            return;
        };
        let got = rig.at(&batch, top);
        eprintln!(
            "{path:?}: the cube's top: {got:?} as a batch, {lit:?} as a model; images {}",
            apart(&batch, &model)
        );
        assert!(
            (0..3).all(|k| (got[k] - lit[k]).abs() <= 2),
            "{path:?}: a batch takes the probes' light: {got:?} vs {lit:?}"
        );
        assert!(apart(&batch, &model) < 0.05, "{path:?}: the same image");
    }
    // The negative control: the same cube without baked lighting.
    let Some(mut flat) = Rig::any() else { return };
    set_up(&mut flat.ctx);
    let mesh = flat.mesh(
        &box_mesh(vec3(-3.2, -0.5, 0.6), vec3(-2.8, -0.1, 1.0), tpu),
        material(false),
    );
    flat.ctx
        .world
        .spawn((Transform3D::at(vec3(-3.0, -0.3, 0.8)), Model::new(mesh)));
    for (red, lo, hi) in room() {
        let mesh = flat.mesh(&box_mesh(lo, hi, tpu), material(red));
        flat.ctx
            .world
            .spawn((Transform3D::at((lo + hi) * 0.5), Model::new(mesh)));
    }
    let pixels = flat.draw(2);
    let unbaked = flat.at(&pixels, top);
    eprintln!("the cube's top without baked lighting: {unbaked:?}");
    assert!(
        (0..3).any(|k| (unbaked[k] - lit[k]).abs() > 10),
        "negative control: without the bake the top is lit otherwise: {unbaked:?} vs {lit:?}"
    );
}
