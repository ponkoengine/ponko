//! Auto exposure (`Post::auto_exposure`), drawn offscreen: a dark and a
//! bright scene settle at the same middle grey under every tone map, on every
//! GPU tier and with HDR output; the exposure moves no faster than its
//! speeds; the first frame and a reset start at the target; min, max,
//! compensation, the center weight and the mask move the target; each camera
//! of a split screen adapts on its own; and off, it makes no pass or image
//! and changes no pixel. Semantic checks on the exposure the renderer adapted
//! to (read back from the GPU) and on the picture; no reference images.
//!
//! Skips without a GPU adapter.

use super::gpu::GpuTexture;
use super::regression_stage::*;
use super::render_test_support::*;
use super::views::WindowViews;
use crate::camera::Viewport;
use crate::prelude::*;
use crate::render::{AutoExposure, GpuTier, MaterialDesc, MeshData, Tonemap};
use crate::texture::Texture;
use crate::three::linear;

/// The glow of the dark and the bright wall: about ten stops apart.
const DARK: f32 = 0.03;
const BRIGHT: f32 = 1.5;
/// The stage's frame time.
const DT: f32 = 1.0 / 60.0;

/// The light a wall glowing `level` sends to the camera.
fn light(level: f32) -> f32 {
    linear(Color::rgb(level, level, level))[0]
}

/// The exposure, in stops, that brings a wall glowing `level` to middle grey.
fn needs(level: f32) -> f32 {
    (0.18 / light(level)).log2()
}

/// A stage lit by nothing but two walls glowing grey, each filling the
/// picture: a dark one toward -Z and a bright one toward +Z, the camera at
/// the origin looking at the dark one. Auto exposure on, with a wide range
/// and fast speeds (half a stop a frame); bloom off.
fn walls(setup: Setup) -> Option<Stage> {
    let mut stage = Stage::with(setup)?;
    let dark = stage.mesh(&MeshData::cube(1.0), glow(DARK));
    let bright = stage.mesh(&MeshData::cube(1.0), glow(BRIGHT));
    stage.block(dark, vec3(-40.0, -40.0, -60.0), vec3(40.0, 40.0, -5.0));
    stage.block(bright, vec3(-40.0, -40.0, 5.0), vec3(40.0, 40.0, 60.0));
    let lighting = &mut stage.ctx.scene3d.lighting;
    (lighting.sun_intensity, lighting.ambient_intensity) = (0.0, 0.0);
    stage.ctx.scene3d.camera = facing(&stage, false);
    stage.ctx.scene3d.post.auto_exposure = AutoExposure {
        enabled: true,
        min: -12.0,
        max: 12.0,
        brighten_speed: 30.0,
        darken_speed: 30.0,
        ..Default::default()
    };
    Some(stage)
}

fn glow(level: f32) -> MaterialDesc {
    MaterialDesc {
        color: Color::BLACK,
        emissive: Color::rgb(level, level, level),
        ..Default::default()
    }
}

/// The stage's camera at the origin, facing the bright wall or the dark one.
fn facing(stage: &Stage, bright: bool) -> crate::three::Camera3D {
    let mut camera = stage.ctx.scene3d.camera;
    camera.position = Vec3::ZERO;
    camera.look_at(if bright { Vec3::Z } else { Vec3::NEG_Z });
    camera
}

/// The main camera's adapted exposure after the last frame, in stops.
fn stops(stage: &Stage) -> f32 {
    stage
        .renderer
        .auto_exposure_stops(&stage.device, &stage.queue, None)
        .expect("auto exposure is on")
}

/// `frames` frames facing the bright wall or the dark one: the middle
/// pixel of the last and the exposure it was drawn with.
fn settle(stage: &mut Stage, bright: bool, frames: usize) -> ([u8; 4], f32) {
    stage.ctx.scene3d.camera = facing(stage, bright);
    let image = stage.settled(frames);
    (image.pixel(SIZE / 2, SIZE / 2), stops(stage))
}

fn srgb8(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// The shader's ACES fit.
fn aces(x: f32) -> f32 {
    ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
}

fn close(a: [u8; 4], b: [u8; 4], within: u8) -> bool {
    (0..3).all(|c| a[c].abs_diff(b[c]) <= within)
}

#[test]
fn a_dark_and_a_bright_scene_settle_at_the_same_middle_grey_under_every_tone_map() {
    let Some(mut stage) = walls(Setup::default()) else {
        return;
    };
    for tonemap in [Tonemap::Aces, Tonemap::AgX, Tonemap::Neutral, Tonemap::None] {
        stage.ctx.scene3d.post.tonemap = tonemap;
        let (dark, dark_stops) = settle(&mut stage, false, 40);
        let (bright, bright_stops) = settle(&mut stage, true, 40);
        assert!(
            close(dark, bright, 2),
            "{tonemap:?}: the dark wall {dark:?} and the bright one {bright:?} settle alike"
        );
        assert!(
            (dark_stops - needs(DARK)).abs() < 0.05 && (bright_stops - needs(BRIGHT)).abs() < 0.05,
            "{tonemap:?}: {dark_stops} and {bright_stops} stops bring {} and {} to middle grey ({} and {})",
            light(DARK),
            light(BRIGHT),
            needs(DARK),
            needs(BRIGHT)
        );
        // Middle grey through the curve, as the tone mapping draws it.
        let grey = match tonemap {
            Tonemap::Aces => Some(srgb8(aces(0.18))),
            Tonemap::None => Some(srgb8(0.18)),
            _ => None,
        };
        if let Some(grey) = grey {
            assert!(
                close(dark, [grey; 4], 3),
                "{tonemap:?}: middle grey is {grey}, the wall {dark:?}"
            );
        }
    }
    // Negative control: without it the walls are ten stops apart.
    stage.ctx.scene3d.post.auto_exposure.enabled = false;
    stage.ctx.scene3d.post.tonemap = Tonemap::Aces;
    let mut fixed = |bright| {
        stage.ctx.scene3d.camera = facing(&stage, bright);
        stage.settled(2).pixel(SIZE / 2, SIZE / 2)
    };
    let (dark, bright) = (fixed(false), fixed(true));
    assert!(
        dark[0] < 40 && bright[0] > 200,
        "fixed exposure: the dark wall {dark:?}, the bright one {bright:?}"
    );
    stage.finish();
}

#[test]
fn it_settles_on_every_gpu_tier() {
    for tier in GpuTier::ALL {
        let Some(mut stage) = walls(Setup {
            tier: Some(tier),
            ..Default::default()
        }) else {
            return;
        };
        stage.ctx.scene3d.post.tonemap = Tonemap::Aces;
        let (dark, dark_stops) = settle(&mut stage, false, 40);
        let (bright, bright_stops) = settle(&mut stage, true, 40);
        assert!(
            close(dark, bright, 2) && close(dark, [srgb8(aces(0.18)); 4], 3),
            "{tier}: the dark wall {dark:?} and the bright one {bright:?} settle at middle grey"
        );
        assert!(
            (dark_stops - needs(DARK)).abs() < 0.05 && (bright_stops - needs(BRIGHT)).abs() < 0.05,
            "{tier}: {dark_stops} and {bright_stops} stops"
        );
        stage.finish();
    }
}

/// The middle pixel of an `Rgba16Float` output, as linear light.
fn hdr_pixel(stage: &Stage) -> [f32; 3] {
    let bytes = read_texture(&stage.device, &stage.queue, &stage.output);
    let at = ((SIZE / 2 * SIZE + SIZE / 2) * 8) as usize;
    let half = |i: usize| {
        let bits = u16::from_le_bytes([bytes[at + i * 2], bytes[at + i * 2 + 1]]);
        let (sign, exp, frac) = ((bits >> 15) as i32, ((bits >> 10) & 31) as i32, f32::from(bits & 1023));
        let v = if exp == 0 {
            frac * 2f32.powi(-24)
        } else {
            (1.0 + frac / 1024.0) * 2f32.powi(exp - 15)
        };
        if sign == 1 { -v } else { v }
    };
    [half(0), half(1), half(2)]
}

#[test]
fn with_hdr_output_middle_grey_stays_middle_grey() {
    let Some(mut stage) = walls(Setup {
        format: wgpu::TextureFormat::Rgba16Float,
        ..Default::default()
    }) else {
        return;
    };
    stage.renderer.set_hdr_headroom(Some(4.0));
    stage.ctx.scene3d.post.tonemap = Tonemap::Aces;
    let settle = |stage: &mut Stage, bright| {
        stage.ctx.scene3d.camera = facing(stage, bright);
        for _ in 0..40 {
            stage.render();
        }
        hdr_pixel(stage)
    };
    let (dark, bright) = (settle(&mut stage, false), settle(&mut stage, true));
    // Below the knee the HDR frame keeps the SDR curve: middle grey is
    // ACES's middle grey, in linear light.
    for c in 0..3 {
        assert!(
            (dark[c] - bright[c]).abs() < 0.01 && (dark[c] - aces(0.18)).abs() < 0.01,
            "HDR output: the dark wall {dark:?}, the bright one {bright:?}, middle grey {}",
            aces(0.18)
        );
    }
    stage.finish();
}

#[test]
fn the_exposure_moves_no_faster_than_its_speeds() {
    let Some(mut stage) = walls(Setup::default()) else {
        return;
    };
    let auto = &mut stage.ctx.scene3d.post.auto_exposure;
    (auto.brighten_speed, auto.darken_speed) = (1.0, 3.0);
    let (_, mut last) = settle(&mut stage, false, 1);
    assert!(
        (last - needs(DARK)).abs() < 0.05,
        "the first frame starts at the target"
    );
    // Into the light: three stops a second darker, a frame at a time, all
    // the way (the target is ten stops away).
    for frame in 0..30 {
        let (_, now) = settle(&mut stage, true, 1);
        assert!(
            (now - (last - 3.0 * DT)).abs() < 1e-4,
            "darkening, frame {frame}: {last} -> {now}"
        );
        last = now;
    }
    // Back into the dark: one stop a second brighter.
    for frame in 0..30 {
        let (_, now) = settle(&mut stage, false, 1);
        assert!(
            (now - (last + DT)).abs() < 1e-4,
            "brightening, frame {frame}: {last} -> {now}"
        );
        last = now;
    }
    // Speed 0 holds the exposure where it is.
    stage.ctx.scene3d.post.auto_exposure.brighten_speed = 0.0;
    let (_, held) = settle(&mut stage, false, 10);
    assert_eq!(held, last, "speed 0 holds");
    // An infinite speed jumps to the target.
    stage.ctx.scene3d.post.auto_exposure.brighten_speed = f32::INFINITY;
    let (_, jumped) = settle(&mut stage, false, 1);
    assert!((jumped - needs(DARK)).abs() < 0.05, "an infinite speed jumps: {jumped}");
    stage.finish();
}

#[test]
fn the_first_frame_and_a_reset_start_at_the_target() {
    let Some(mut stage) = walls(Setup::default()) else {
        return;
    };
    let auto = &mut stage.ctx.scene3d.post.auto_exposure;
    (auto.brighten_speed, auto.darken_speed) = (1.0, 3.0);
    let (_, first) = settle(&mut stage, true, 1);
    assert!((first - needs(BRIGHT)).abs() < 0.05, "the first frame: {first}");
    let (_, adapting) = settle(&mut stage, false, 1);
    assert!(
        (adapting - (first + DT)).abs() < 1e-4,
        "without a reset it adapts: {first} -> {adapting}"
    );
    stage.ctx.gfx.reset_temporal_history();
    let (_, reset) = settle(&mut stage, false, 1);
    assert!(
        (reset - needs(DARK)).abs() < 0.05,
        "a reset jumps to the target: {reset}"
    );
    // Off and on again starts over too.
    stage.ctx.scene3d.camera = facing(&stage, true);
    stage.ctx.scene3d.post.auto_exposure.enabled = false;
    stage.render();
    stage.ctx.scene3d.post.auto_exposure.enabled = true;
    let (_, again) = settle(&mut stage, true, 1);
    assert!((again - needs(BRIGHT)).abs() < 0.05, "turned on again: {again}");
    stage.finish();
}

#[test]
fn min_max_and_compensation_move_the_target_and_exposure_still_applies() {
    let Some(mut stage) = walls(Setup::default()) else {
        return;
    };
    stage.ctx.scene3d.post.tonemap = Tonemap::None;
    stage.ctx.scene3d.post.auto_exposure.max = 2.0;
    let (_, held) = settle(&mut stage, false, 30);
    assert_eq!(held, 2.0, "the dark wall needs {} stops: held at max", needs(DARK));
    stage.ctx.scene3d.post.auto_exposure.min = -1.0;
    let (_, held) = settle(&mut stage, true, 30);
    assert_eq!(held, -1.0, "the bright wall needs {} stops: held at min", needs(BRIGHT));
    let auto = &mut stage.ctx.scene3d.post.auto_exposure;
    (auto.min, auto.max, auto.compensation) = (-12.0, 12.0, 1.0);
    let (lifted, compensated) = settle(&mut stage, false, 40);
    assert!(
        (compensated - (needs(DARK) + 1.0)).abs() < 0.05,
        "a stop of compensation: {compensated}"
    );
    assert!(
        close(lifted, [srgb8(0.36); 4], 3),
        "a stop above middle grey: {lifted:?}"
    );
    // The fixed exposure applies on top, and the meter doesn't see it.
    stage.ctx.scene3d.post.auto_exposure.compensation = 0.0;
    stage.ctx.scene3d.post.exposure = -1.0;
    let (lowered, adapted) = settle(&mut stage, false, 40);
    assert!((adapted - needs(DARK)).abs() < 0.05, "{adapted}");
    assert!(
        close(lowered, [srgb8(0.09); 4], 3),
        "a stop below middle grey: {lowered:?}"
    );
    stage.finish();
}

/// A mask for the stage's textures, 2 x 1: `left` and `right`, the left
/// and the right half of the picture (white counts, black doesn't).
fn mask(stage: &mut Stage, left: u8, right: u8) -> Texture {
    let (layout, _, _) = white_textures(&stage.device, &stage.queue);
    let texture = stage.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 2,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    stage.queue.write_texture(
        texture.as_image_copy(),
        &[left, left, left, 255, right, right, right, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(8),
            rows_per_image: None,
        },
        texture.size(),
    );
    let view = texture.create_view(&Default::default());
    let sampler = stage.device.create_sampler(&Default::default());
    let bind_group = stage.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let id = stage.textures.len() as u32;
    stage.textures.push(GpuTexture {
        texture,
        nearest: true,
        mips: None,
        bind_group,
    });
    Texture::new(id, 2, 1)
}

#[test]
fn the_center_weight_and_the_mask_choose_what_is_metered() {
    let Some(mut stage) = walls(Setup::default()) else {
        return;
    };
    let auto = &mut stage.ctx.scene3d.post.auto_exposure;
    (auto.brighten_speed, auto.darken_speed) = (f32::INFINITY, f32::INFINITY);
    // A bright panel over the right half of the dark wall, and a small
    // bright one in the middle.
    let bright = stage.mesh(&MeshData::cube(1.0), glow(BRIGHT));
    let panel = stage.block(bright, vec3(0.0, -40.0, -4.9), vec3(40.0, 40.0, -4.8));
    let (_, half) = settle(&mut stage, false, 1);
    let between = |s: f32| s < needs(DARK) - 1.0 && s > needs(BRIGHT) + 1.0;
    assert!(between(half), "half dark, half bright meters between them: {half}");
    // Only the right half counts: the bright panel's exposure. Only the
    // left: the dark wall's.
    stage.ctx.scene3d.post.auto_exposure.center_weight = 0.0;
    let right = mask(&mut stage, 0, 255);
    stage.ctx.scene3d.post.auto_exposure.mask = Some(right);
    let (_, bright_side) = settle(&mut stage, false, 1);
    let left = mask(&mut stage, 255, 0);
    stage.ctx.scene3d.post.auto_exposure.mask = Some(left);
    let (_, dark_side) = settle(&mut stage, false, 1);
    assert!(
        (bright_side - needs(BRIGHT)).abs() < 0.3 && (dark_side - needs(DARK)).abs() < 0.3,
        "masked to the right half: {bright_side} (the bright panel needs {}), to the left: {dark_side} \
         (the dark wall needs {})",
        needs(BRIGHT),
        needs(DARK)
    );
    // Negative control: a white mask is no mask.
    let white = mask(&mut stage, 255, 255);
    stage.ctx.scene3d.post.auto_exposure.mask = Some(white);
    let (_, unmasked) = settle(&mut stage, false, 1);
    stage.ctx.scene3d.post.auto_exposure.mask = None;
    let (_, no_mask) = settle(&mut stage, false, 1);
    assert!((unmasked - no_mask).abs() < 1e-3, "{unmasked} vs {no_mask}");

    // The center weight: a small bright square in the middle of the dark
    // wall counts more the more the middle does.
    stage.ctx.world.despawn(panel);
    stage.block(bright, vec3(-0.5, -0.5, -4.9), vec3(0.5, 0.5, -4.8));
    let weighted = |stage: &mut Stage, weight: f32| {
        stage.ctx.scene3d.post.auto_exposure.center_weight = weight;
        settle(stage, false, 1).1
    };
    let (even, middle) = (weighted(&mut stage, 0.0), weighted(&mut stage, 1.0));
    assert!(
        middle < even - 0.2,
        "the middle counts more: {even} stops metered evenly, {middle} toward the middle"
    );
    stage.finish();
}

#[test]
fn each_camera_of_a_split_screen_adapts_on_its_own() {
    let Some(mut stage) = walls(Setup::default()) else {
        return;
    };
    let auto = &mut stage.ctx.scene3d.post.auto_exposure;
    (auto.brighten_speed, auto.darken_speed) = (1.0, 3.0);
    let camera = |stage: &Stage, bright, rect| {
        let mut c = facing(stage, bright);
        c.rect = rect;
        c
    };
    // Player one faces the dark wall, player two the bright one.
    stage.ctx.scene3d.camera = camera(&stage, false, Viewport::split(2, 0));
    let two = camera(&stage, true, Viewport::split(2, 1));
    stage.ctx.scene3d.views.push(d3::View::new(two));
    let mut windows = WindowViews::default();
    let read = |stage: &Stage| {
        let at = |view| {
            stage
                .renderer
                .auto_exposure_stops(&stage.device, &stage.queue, view)
                .expect("on in both")
        };
        (at(None), at(Some(0)))
    };
    super::views_render_test::frame(&mut stage, &mut windows, false);
    let (one, two_stops) = read(&stage);
    assert!(
        (one - needs(DARK)).abs() < 0.05 && (two_stops - needs(BRIGHT)).abs() < 0.05,
        "each starts at its own target: {one}, {two_stops}"
    );
    // They turn around: player one adapts to the light at three stops a
    // second, player two to the dark at one, each from its own exposure.
    stage.ctx.scene3d.camera = camera(&stage, true, Viewport::split(2, 0));
    stage.ctx.scene3d.views[0].camera = camera(&stage, false, Viewport::split(2, 1));
    let (mut last_one, mut last_two) = (one, two_stops);
    for frame in 0..20 {
        super::views_render_test::frame(&mut stage, &mut windows, false);
        let (now_one, now_two) = read(&stage);
        assert!(
            (now_one - (last_one - 3.0 * DT)).abs() < 1e-4 && (now_two - (last_two + DT)).abs() < 1e-4,
            "frame {frame}: player one {last_one} -> {now_one}, player two {last_two} -> {now_two}"
        );
        (last_one, last_two) = (now_one, now_two);
    }
    stage.finish();
}

#[test]
fn off_it_makes_no_pass_or_image_and_changes_no_pixel() {
    let Some(mut stage) = walls(Setup::default()) else {
        return;
    };
    stage.ctx.scene3d.post.auto_exposure.enabled = false;
    let off = stage.image();
    assert_eq!(stage.renderer.auto_exposure_meter_sizes(), None, "no meter while off");
    let calls = stage.renderer.draw_calls();
    // On: an eighth of the frame, a quarter of that, one texel.
    stage.ctx.scene3d.post.auto_exposure.enabled = true;
    let on = stage.image();
    assert_eq!(
        stage.renderer.auto_exposure_meter_sizes(),
        Some(vec![(16, 16), (4, 4), (1, 1)])
    );
    assert_eq!(
        stage.renderer.draw_calls(),
        calls + 4,
        "three meter passes and the adaptation"
    );
    assert!(diff(&off, &on, 0).max_channel > 50, "on, the dark wall brightens");
    // Off again: the images are freed, and the frame is the one before.
    stage.ctx.scene3d.post.auto_exposure.enabled = false;
    let again = stage.image();
    assert_eq!(stage.renderer.auto_exposure_meter_sizes(), None);
    assert_eq!(stage.renderer.draw_calls(), calls);
    assert_eq!(diff(&off, &again, 0).max_channel, 0, "off changes no pixel");
    stage.finish();
}

/// What auto exposure costs on the GPU at 1920x1080 and 2560x1440, from
/// timestamp queries: its passes (meter, reductions, adaptation) and the
/// tone mapping pass with it off and on. For numbers, not a check:
/// `cargo test --release --lib --features 3d auto_exposure_cost -- --ignored --nocapture`
#[test]
#[ignore = "requires a GPU with timestamps; run explicitly for numbers"]
fn auto_exposure_cost() {
    use crate::d3::Transform as Transform3D;
    use crate::render::static_batch_render_test::{BatchRig, RigSetup};
    use crate::render::{AntiAliasing, Model};
    for size in [(1920, 1080), (2560, 1440)] {
        let Some(mut rig) = BatchRig::new(RigSetup {
            size,
            timestamps: true,
            ..Default::default()
        }) else {
            return;
        };
        if rig.timer.is_none() {
            skip("no timestamp queries");
            return;
        }
        rig.ctx.scene3d.camera.position = Vec3::ZERO;
        rig.ctx.scene3d.camera.rotation = Quat::IDENTITY;
        rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::None;
        rig.ctx.scene3d.post.bloom = 0.0;
        for i in 0..60 {
            let at = vec3((i % 10) as f32 - 4.5, (i / 10) as f32 - 2.5, -3.0 - i as f32 * 0.5);
            let level = 0.2 + (i % 5) as f32 * 0.2;
            let mesh = rig.mesh(&MeshData::cube(0.6), glow(level));
            rig.ctx.world.spawn((Transform3D::at(at), Model::new(mesh)));
        }
        println!("adapter: {}", describe_adapter(&rig.gpu.info));
        let measure = |rig: &mut BatchRig, pass: &str| {
            let mut times = Vec::new();
            let mut read = rig.timer.as_ref().unwrap().frames_read;
            for frame in 0..120 {
                rig.frame();
                let timer = rig.timer.as_ref().unwrap();
                if frame >= 30 && timer.frames_read != read {
                    read = timer.frames_read;
                    times.push(
                        timer
                            .last_frame
                            .iter()
                            .filter(|p| p.name == pass)
                            .map(|p| p.ms)
                            .sum::<f32>(),
                    );
                }
            }
            times.sort_by(f32::total_cmp);
            times[times.len() / 2]
        };
        // Other work on a shared machine only adds time: the best of a few
        // runs' medians.
        let measure = |rig: &mut BatchRig, pass: &str| (0..3).map(|_| measure(rig, pass)).fold(f32::MAX, f32::min);
        let tonemap_off = measure(&mut rig, "post");
        rig.ctx.scene3d.post.auto_exposure.enabled = true;
        let passes = measure(&mut rig, "auto exposure");
        let tonemap_on = measure(&mut rig, "post");
        let timer = rig.timer.as_ref().unwrap();
        let each: Vec<String> = timer
            .last_frame
            .iter()
            .filter(|p| p.name == "auto exposure")
            .map(|p| format!("{:.3}", p.ms))
            .collect();
        println!("each auto exposure pass of the last frame: {}", each.join(", "));
        println!(
            "{}x{}: auto exposure passes {passes:.3} ms; tone mapping {tonemap_off:.3} ms off, {tonemap_on:.3} ms on",
            size.0, size.1
        );
    }
}
