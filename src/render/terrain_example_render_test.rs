//! The terrain example (`examples/terrain`) drawn offscreen by the
//! production renderer: the example's own scene code builds its tiles and
//! its lake, which comes from the water kit (`ponko::water`), and the
//! frames run as a game's (the kit's update, then the frame). Only the
//! window is missing.
//!
//! What must hold: the lake is the water kit's (with its planar mirror
//! following the camera), it shows where the valleys dip under its level
//! and nowhere else, and diving under it turns on the kit's underwater
//! view. The negative control is the same view with the lake despawned.
use super::render_test_support::*;
use super::water_kit_render_test::{frame, image, offscreen};
use crate::prelude::*;
use crate::water::{Water, despawn_water};

#[allow(dead_code, clippy::all)]
#[path = "../../examples/terrain/scene.rs"]
mod scene;

const SIZE: (u32, u32) = (192, 108);

/// Where the ray through pixel (`x`, `y`) of the camera first meets the
/// lake's level, if it does before 2500 m.
fn lake_hit(ctx: &Ctx, x: u32, y: u32) -> Option<Vec2> {
    let camera = &ctx.scene3d.camera;
    let inverse = camera.view_proj().inverse();
    let ndc = vec2(
        (x as f32 + 0.5) / SIZE.0 as f32 * 2.0 - 1.0,
        1.0 - (y as f32 + 0.5) / SIZE.1 as f32 * 2.0,
    );
    let far = inverse.project_point3(ndc.extend(0.5));
    let dir = (far - camera.position).normalize();
    if dir.y >= -1e-4 {
        return None;
    }
    let along = (scene::WATER - camera.position.y) / dir.y;
    (0.0..2500.0).contains(&along).then(|| {
        let p = camera.position + dir * along;
        vec2(p.x, p.z)
    })
}

/// Whether the ray through pixel (`x`, `y`) reaches the lake before the
/// ground (marched in 2 m steps up to where it meets the lake's level).
fn sees_lake(ctx: &Ctx, x: u32, y: u32) -> Option<bool> {
    let hit = lake_hit(ctx, x, y)?;
    let eye = ctx.scene3d.camera.position;
    let to = vec3(hit.x, scene::WATER, hit.y) - eye;
    let steps = (to.length() / 2.0) as usize;
    for i in 1..steps {
        let p = eye + to * (i as f32 / steps as f32);
        if scene::height(p.x, p.z) > p.y {
            return Some(false); // a hill in the way
        }
    }
    Some(scene::height(hit.x, hit.y) < scene::WATER - 0.5)
}

#[test]
fn the_terrain_example_draws_its_lake_with_the_water_kit() {
    let Some(mut ctx) = offscreen(SIZE, None, false) else {
        return;
    };
    let terrain = scene::setup(&mut ctx, "", 16.0);
    let water = ctx
        .world
        .get::<Water>(terrain.lake)
        .expect("the lake is a water kit body");
    let mirror = water.reflection_target().expect("the lake mirrors the hills");
    for _ in 0..2 {
        frame(&mut ctx);
    }
    assert!(
        ctx.scene3d.passes.target_cameras.iter().any(|c| c.target == mirror),
        "the kit's mirror camera follows the camera"
    );
    let with = image(&mut ctx, "terrain-lake");
    // The negative control: the same view without the lake.
    despawn_water(&mut ctx, terrain.lake);
    for _ in 0..2 {
        frame(&mut ctx);
    }
    let without = image(&mut ctx, "terrain-no-lake");
    let (mut lake, mut lake_changed, mut land, mut land_changed) = (0, 0, 0, 0);
    for y in 0..SIZE.1 {
        for x in 0..SIZE.0 {
            let (a, b) = (with.pixel(x, y), without.pixel(x, y));
            let changed = (0..3).any(|i| a[i].abs_diff(b[i]) > 12);
            match sees_lake(&ctx, x, y) {
                Some(true) => {
                    lake += 1;
                    lake_changed += usize::from(changed);
                }
                // Clearly dry ground (not near the shore: the edges may
                // be a pixel off, and shore foam reaches up the beach).
                Some(false) if lake_hit(&ctx, x, y).is_some_and(|p| scene::height(p.x, p.y) > scene::WATER + 3.0) => {
                    land += 1;
                    land_changed += usize::from(changed);
                }
                None => {
                    land += 1;
                    land_changed += usize::from(changed);
                }
                _ => {}
            }
        }
    }
    let share = |n: usize, of: usize| n as f32 / of.max(1) as f32;
    eprintln!(
        "terrain example: {lake} pixels see the lake ({:.0}% drawn differently with it), \
         {land} see dry land or sky ({:.1}% differently)",
        share(lake_changed, lake) * 100.0,
        share(land_changed, land) * 100.0
    );
    assert!(lake > 500, "the view looks over the lake: {lake} pixels");
    assert!(
        share(lake_changed, lake) > 0.9,
        "the lake shows where the valleys dip under it: {lake_changed} of {lake}"
    );
    assert!(
        share(land_changed, land) < 0.02,
        "and nowhere else: {land_changed} of {land} dry or sky pixels changed"
    );
}

#[test]
fn diving_into_the_terrain_examples_lake_turns_on_the_underwater_view() {
    let Some(mut ctx) = offscreen(SIZE, None, false) else {
        return;
    };
    let mut terrain = scene::setup(&mut ctx, "under", 16.0);
    assert!(
        ctx.scene3d.camera.position.y < scene::WATER,
        "the camera starts under the lake"
    );
    frame(&mut ctx);
    let water = ctx.world.get::<Water>(terrain.lake).unwrap();
    assert!(water.shows_underwater(), "the kit's underwater view is on");
    let under = image(&mut ctx, "terrain-under");
    // U makes the lake again without its underwater view.
    scene::switch_underwater(&mut ctx, &mut terrain);
    frame(&mut ctx);
    assert!(!ctx.world.get::<Water>(terrain.lake).unwrap().shows_underwater());
    let plain = image(&mut ctx, "terrain-under-off");
    let d = diff(&under, &plain, 16);
    eprintln!("under the lake, with and without the underwater view: {d}");
    assert!(
        d.outliers as f32 > d.pixels as f32 * 0.3,
        "the underwater view tints the picture: {d}"
    );
}
