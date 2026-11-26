//! Real opaque-scene refraction, compared with air and a thin wall.
use super::super::regression_stage::{Setup, Stage, assert_similar};
use super::super::render_test_support::Tolerance;
use super::{frame_checks::last_features, passes::Features};
use crate::d3::Transform;
use crate::prelude::*;
use crate::render::{GpuTier, MaterialDesc, MaterialLayers, MeshData, Model, Transmission};

fn stage(setup: Setup) -> Option<Stage> {
    let mut stage = Stage::with(setup)?;
    stage.ctx.scene3d.camera.position = vec3(0.0, 0.0, 4.0);
    stage.ctx.scene3d.camera.look_at(Vec3::ZERO);
    stage.ctx.scene3d.lighting.sun_intensity = 0.0;
    stage.ctx.scene3d.lighting.ambient_intensity = 0.0;
    Some(stage)
}

fn backdrop(stage: &mut Stage) {
    for (x, color) in [(-2.0, Color::rgb(0.75, 0.0, 0.0)), (0.0, Color::rgb(0.0, 0.0, 0.75))] {
        let mesh = stage.mesh(
            &MeshData::cube(1.0),
            MaterialDesc::color(Color::BLACK).with_emissive(color),
        );
        stage.block(mesh, vec3(x, -2.0, -1.1), vec3(x + 2.0, 2.0, -1.0));
    }
}

fn glass(internal: Transmission, ior: f32) -> MaterialDesc {
    MaterialDesc::color(Color::WHITE)
        .with_roughness(0.03)
        .with_layers(MaterialLayers::NONE.with_ior(ior).with_transmission(internal))
}

/// A pane with a tilted normal, so a ray at the center is bent sideways.
fn pane(stage: &mut Stage, desc: MaterialDesc) -> Entity {
    let mesh = stage.mesh(&MeshData::plane(1.7), desc);
    stage.ctx.world.spawn((
        Transform {
            rotation: Quat::from_rotation_y(-0.6) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            ..Default::default()
        },
        Model::new(mesh),
    ))
}

#[test]
fn transmission_bends_background_rays_and_skips_the_copy_when_out_of_view() {
    let Some(mut stage) = stage(Setup::default()) else {
        return;
    };
    backdrop(&mut stage);
    let air = stage.image();
    assert!(!last_features().unwrap().contains(Features::SPLIT));
    let thin = pane(&mut stage, glass(Transmission::new(1.0), 1.5));
    let through_thin = stage.image();
    assert!(last_features().unwrap().contains(Features::REFRACTION_MIPS));
    stage.ctx.world.despawn(thin);
    let rough = pane(&mut stage, glass(Transmission::new(1.0), 1.5).with_roughness(1.0));
    let through_rough = stage.image();
    stage.ctx.world.despawn(rough);

    let thick = Transmission::new(1.0).with_volume(1.25, f32::INFINITY, Color::WHITE);
    let unit_ior = pane(&mut stage, glass(thick, 1.0));
    let through_unit_ior = stage.image();
    stage.ctx.world.despawn(unit_ior);
    let volume = pane(&mut stage, glass(thick, 1.8));
    let through_volume = stage.image();

    let at = vec3(-0.12, 0.0, 0.0);
    let air_pixel = stage.color_at(&air, at);
    let thin_pixel = stage.color_at(&through_thin, at);
    let rough_pixel = stage.color_at(&through_rough, at);
    let unit_pixel = stage.color_at(&through_unit_ior, at);
    let volume_pixel = stage.color_at(&through_volume, at);
    assert!(air_pixel[0] > 150.0 && air_pixel[2] < 10.0, "air {air_pixel:?}");
    assert!(thin_pixel[0] > 140.0 && thin_pixel[2] < 10.0, "thin {thin_pixel:?}");
    assert!(rough_pixel[0] > 50.0 && rough_pixel[2] > 45.0, "rough {rough_pixel:?}");
    assert!(unit_pixel[0] > 140.0 && unit_pixel[2] < 10.0, "IOR 1 {unit_pixel:?}");
    assert!(
        volume_pixel[2] > 120.0 && volume_pixel[0] < 25.0,
        "IOR 1.8 {volume_pixel:?}"
    );

    stage.ctx.world.get_mut::<Transform>(volume).unwrap().position.x = 40.0;
    let outside = stage.image();
    assert!(!last_features().unwrap().contains(Features::SPLIT));
    assert_similar(
        &air,
        &outside,
        Tolerance {
            channel: 1,
            outliers: 0.0,
            edge: 0,
        },
        "glass outside the frustum",
    );
    stage.finish();
}

#[test]
fn transmission_is_tinted_by_path_length() {
    let Some(mut stage) = stage(Setup::default()) else {
        return;
    };
    let background = stage.mesh(
        &MeshData::cube(1.0),
        MaterialDesc::color(Color::BLACK).with_emissive(Color::WHITE),
    );
    stage.block(background, vec3(-2.0, -2.0, -1.1), vec3(2.0, 2.0, -1.0));
    let distance = 0.5;
    let tint = Color::rgb(0.25, 0.65, 0.9);
    let thin = pane(
        &mut stage,
        glass(Transmission::new(1.0).with_volume(0.0, distance, tint), 1.0),
    );
    let untinted = stage.image();
    stage.ctx.world.despawn(thin);
    pane(
        &mut stage,
        glass(Transmission::new(1.0).with_volume(distance, distance, tint), 1.0),
    );
    let tinted = stage.image();
    assert!(stage.color_at(&untinted, Vec3::ZERO)[0] > 245.0);
    let [r, g, b] = stage.color_at(&tinted, Vec3::ZERO);
    assert!(
        (r - 64.0).abs() < 12.0 && (g - 166.0).abs() < 12.0 && (b - 230.0).abs() < 12.0,
        "{r} {g} {b}"
    );
    stage.finish();
}

#[test]
fn transmission_webgl2_fallback_reads_the_unblurred_copy() {
    // Exercise the GL depth-copy path on the same adapter as the main test,
    // with the same binding limits as a WebGL2 device.
    super::shaders::FORCE_FLOAT_DEPTH.set(true);
    let stage = stage(Setup {
        tier: Some(GpuTier::WebGl2),
        samples: 1,
        ..Setup::default()
    });
    super::shaders::FORCE_FLOAT_DEPTH.set(false);
    let Some(mut stage) = stage else { return };
    backdrop(&mut stage);
    let thick = Transmission::new(1.0).with_volume(1.25, f32::INFINITY, Color::WHITE);
    pane(&mut stage, glass(thick, 1.8));
    let bent = stage.image();
    let features = last_features().unwrap();
    assert!(features.contains(Features::SPLIT));
    assert!(!features.contains(Features::REFRACTION_MIPS));
    let pixel = stage.color_at(&bent, vec3(-0.12, 0.0, 0.0));
    assert!(pixel[2] > 120.0 && pixel[0] < 25.0, "WebGL2 fallback {pixel:?}");
    stage.finish();
}

#[test]
fn diffuse_transmission_takes_backlight_without_a_scene_copy() {
    let Some(mut stage) = stage(Setup::default()) else {
        return;
    };
    stage.ctx.scene3d.lighting.sun_intensity = 2.0;
    stage.ctx.scene3d.lighting.sun_direction = Vec3::Z;
    let solid = pane(&mut stage, MaterialDesc::color(Color::WHITE));
    let unlit = stage.image();
    stage.ctx.world.despawn(solid);
    let mut transmitted = Transmission::NONE;
    transmitted.diffuse = 1.0;
    transmitted.diffuse_color = Color::rgb(0.1, 0.8, 0.1);
    pane(&mut stage, glass(transmitted, 1.5));
    let lit = stage.image();
    let center = stage.color_at(&lit, Vec3::ZERO);
    assert!(stage.color_at(&unlit, Vec3::ZERO)[1] < 10.0);
    assert!(center[1] > 90.0 && center[0] < 35.0, "backlit {center:?}");
    assert!(!last_features().unwrap().contains(Features::SPLIT));
    stage.finish();
}

/// Run this offscreen in release. Each case draws the same two opaque walls;
/// glass adds one nearly screen-height plane, 4x MSAA, and the HDR mip chain.
#[test]
#[ignore = "offscreen GPU benchmark"]
fn transmission_gpu_cost() {
    let Some(mut stage) = stage(Setup {
        size: (1920, 1080),
        timestamps: true,
        ..Setup::default()
    }) else {
        return;
    };
    backdrop(&mut stage);
    let glass = pane(
        &mut stage,
        glass(
            Transmission::new(1.0).with_volume(0.6, f32::INFINITY, Color::WHITE),
            1.5,
        )
        .with_roughness(0.4),
    );
    let mut timer =
        super::super::timer::GpuTimer::new(&stage.device, &stage.queue).expect("benchmark needs GPU timestamp queries");
    for run in 1..=3 {
        for (case, x) in [("no glass", 40.0), ("glass", 0.0)] {
            stage.ctx.world.get_mut::<Transform>(glass).unwrap().position.x = x;
            let mut frames = Vec::new();
            let mut mips = Vec::new();
            for frame in 0..80 {
                let read = timer.frames_read;
                stage.render_timed(Some(&mut timer));
                stage.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                timer.collect(&stage.device);
                if frame >= 20 && timer.frames_read > read {
                    frames.push(timer.latest_frame().unwrap().0);
                    mips.push(
                        timer
                            .last_frame
                            .iter()
                            .filter(|span| span.name == "refraction mips")
                            .map(|span| span.ms)
                            // An empty sum is -0.0.
                            .sum::<f32>()
                            + 0.0,
                    );
                }
            }
            assert!(frames.len() >= 50, "only {} samples", frames.len());
            frames.sort_by(f32::total_cmp);
            mips.sort_by(f32::total_cmp);
            eprintln!(
                "run {run} ({case}): {:.3} ms GPU frame, {:.3} ms mips, {} samples",
                frames[frames.len() / 2],
                mips[mips.len() / 2],
                frames.len(),
            );
        }
    }
    stage.finish();
}
