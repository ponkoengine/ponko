//! Well-known content drawn offscreen by the production renderer
//! (`Ctx::render_frame`, as a game draws), from the cache that
//! `scripts/fetch-known-content.py` fills (`PONKO_KNOWN_CONTENT`, else
//! `.cache/known-content`; see the guide's "Well-known content"):
//!
//! - Every Khronos glTF sample that loads is drawn and compared with the
//!   screenshot that ships with it. It is seen through the file's first
//!   camera, or else framed by the rule in [`frame`]; the background is the
//!   screenshot's. The comparison is perceptual ([`similarity`]): how well
//!   the drawn silhouettes overlap and how far apart the colors are, never
//!   bytes. Each sample must keep the verdict that
//!   `tests/fixtures/known-content/draws.txt` records for the pinned
//!   revision. The pictures, side by side, and the numbers go to
//!   `results/` in the cache.
//! - `known_scenes_benchmark` (ignored: run it in release on purpose)
//!   measures Sponza and the Bistro: load time, memory, GPU memory and the
//!   GPU time of each pass at 1080p and 1440p for each quality preset.
//!
//! Without the cache these tests say so and pass; without a GPU they skip
//! (a skip is not a pass).
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};

use super::render_test_support::{TestGpu, describe_adapter, gpu_with};
use crate::ctx::Ctx;
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::{EnvironmentMap, GraphicsQuality, Model, ModelLightKind};

/// Reads asset names as files under a folder (the cache).
struct Folder(PathBuf);

impl crate::assets::AssetSource for Folder {
    fn id(&self) -> &str {
        "known-content"
    }

    fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        if name.split(['/', '\\']).any(|part| part == "..") {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        std::fs::read(self.0.join(name))
    }
}

fn cache() -> PathBuf {
    std::env::var_os("PONKO_KNOWN_CONTENT").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join(".cache/known-content"),
        PathBuf::from,
    )
}

/// The cache's manifest, or `None` (logged) without one.
fn manifest(root: &Path) -> Option<Value> {
    let Ok(text) = std::fs::read_to_string(root.join("manifest.json")) else {
        eprintln!(
            "skipped: no well-known content in {} (python3 scripts/fetch-known-content.py fetches it)",
            root.display()
        );
        return None;
    };
    Some(serde_json::from_str(&text).expect("the cache's manifest.json is JSON"))
}

/// A game context drawing offscreen at `size`, on a device opened as a
/// game's window opens its own: GPU timestamps and the block-compressed
/// formats where the adapter has them. `None` without a GPU (logged).
fn game_ctx(size: (u32, u32)) -> Option<(Ctx, wgpu::AdapterInfo)> {
    let gpu = gpu_with(|adapter| {
        #[cfg_attr(not(feature = "compressed-textures"), allow(unused_mut))]
        let mut features = adapter.features()
            & (wgpu::Features::TIMESTAMP_QUERY
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
                | wgpu::Features::INDIRECT_FIRST_INSTANCE);
        #[cfg(feature = "compressed-textures")]
        {
            features |= adapter.features() & super::blocks::FEATURES;
        }
        Ok(wgpu::DeviceDescriptor {
            required_features: features,
            required_limits: wgpu::Limits {
                max_sampled_textures_per_shader_stage: adapter
                    .limits()
                    .max_sampled_textures_per_shader_stage
                    .clamp(16, 24),
                ..wgpu::Limits::default()
            },
            ..Default::default()
        })
    })?;
    let TestGpu { info, .. } = &gpu;
    let info = info.clone();
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::new(size.0 as f32, size.1 as f32));
    ctx.audio.headless = true;
    ctx.gfx.renderer = Some(super::gpu::Renderer::offscreen(gpu, size));
    ctx.assets.add_source(Folder(cache()));
    Some((ctx, info))
}

/// Waits until the GPU has finished what was submitted.
fn wait(ctx: &Ctx) {
    if let Some(renderer) = &ctx.gfx.renderer {
        let (device, _, _) = renderer.texture_for_test(0);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
    }
}

/// Draws a frame and reads it back (RGBA8 sRGB, as a screenshot saves it).
fn capture(ctx: &mut Ctx) -> image::RgbaImage {
    let path = std::env::temp_dir().join(format!(
        "ponko-known-{}-{:?}.png",
        std::process::id(),
        std::thread::current().id()
    ));
    ctx.gfx.queue_screenshot(path.clone(), 0, None);
    assert_eq!(ctx.render_frame(), crate::render::FrameStatus::Offscreen);
    let shot = image::open(&path).expect("the frame was captured").to_rgba8();
    let _ = std::fs::remove_file(&path);
    shot
}

/// The framing rule for a sample without a camera of its own, after the
/// glTF Sample Viewer's default view: a 45° vertical field of view, the
/// camera on the +Z side of the center of the model's bounds looking down
/// -Z, as far away as the bounding sphere (`center`, `radius`) needs to
/// fit the narrower of the two fields of view.
fn frame(camera: &mut crate::three::Camera3D, center: Vec3, radius: f32, aspect: f32) {
    let radius = radius.max(1e-3);
    camera.fov_y = 45f32.to_radians();
    let half = (camera.fov_y * 0.5).min(((camera.fov_y * 0.5).tan() * aspect).atan());
    let distance = radius / half.sin();
    camera.position = center + Vec3::Z * distance;
    camera.rotation = Quat::IDENTITY;
    camera.near = (distance - radius * 1.5).max(distance * 1e-3);
    camera.far = distance + radius * 2.0;
}

/// The median color of an image's outermost pixels: its background.
fn border_color(image: &image::RgbaImage) -> [u8; 3] {
    let (w, h) = image.dimensions();
    let mut channels = [Vec::new(), Vec::new(), Vec::new()];
    let mut push = |x: u32, y: u32| {
        let p = image.get_pixel(x, y);
        for c in 0..3 {
            channels[c].push(p[c]);
        }
    };
    for x in 0..w {
        push(x, 0);
        push(x, h - 1);
    }
    for y in 0..h {
        push(0, y);
        push(w - 1, y);
    }
    channels.map(|mut v| {
        v.sort_unstable();
        v[v.len() / 2]
    })
}

/// sRGB bytes to CIE L*a*b* (D65).
fn lab(p: [f32; 3]) -> [f32; 3] {
    let lin = |c: f32| {
        let c = c / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b] = p.map(lin);
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// How alike a drawing is to a reference picture of the same size, the
/// way a person comparing them would look: whether the same shapes stand
/// in the same places, and whether they have the same colors.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Similarity {
    /// Intersection over union of the two pictures' foregrounds (pixels
    /// away from each picture's own background color), once
    /// [`registered`]: 1 when the silhouettes coincide, 0 when they don't
    /// touch.
    overlap: f32,
    /// The mean color difference (CIE76 ΔE) of 8x8 pixel blocks where
    /// either picture has its foreground: about 2 is just visible, 10 a
    /// clear shift, 30 another color.
    color: f32,
    /// Of that, the mean difference in lightness (L*, drawing minus
    /// reference): positive when the drawing is lit brighter.
    lightness: f32,
}

impl Similarity {
    /// The tolerance: a drawing matches its reference when the
    /// registered silhouettes overlap by 70% and the colors are within ΔE 20 (lit
    /// by another viewer's lights, tone curve and environment, the same
    /// materials land about there).
    const OVERLAP: f32 = 0.7;
    const COLOR: f32 = 20.0;

    fn matches(&self) -> bool {
        self.overlap >= Self::OVERLAP && self.color <= Self::COLOR
    }

    /// What doesn't match, in words, for the table.
    fn difference(&self) -> String {
        let mut out = Vec::new();
        if self.overlap < Self::OVERLAP {
            out.push(format!("silhouettes overlap {:.0}%", self.overlap * 100.0));
        }
        if self.color > Self::COLOR {
            out.push(format!("colors ΔE {:.0}", self.color));
        }
        out.join(", ")
    }
}

/// Foreground: a channel more than this away from the background.
const FOREGROUND: u8 = 24;
const BLOCK: u32 = 8;

/// The smallest rectangle holding every pixel away from `bg`, as
/// (x, y, width, height); the whole picture when there is none.
fn foreground_box(image: &image::RgbaImage, bg: [u8; 3]) -> (u32, u32, u32, u32) {
    let (w, h) = image.dimensions();
    let (mut lo, mut hi) = ((w, h), (0, 0));
    for (x, y, p) in image.enumerate_pixels() {
        if (0..3).any(|c| p[c].abs_diff(bg[c]) > FOREGROUND) {
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        }
    }
    if lo.0 > hi.0 {
        return (0, 0, w, h);
    }
    (lo.0, lo.1, hi.0 - lo.0 + 1, hi.1 - lo.1 + 1)
}

/// The two pictures registered: each cropped to its foreground, the
/// drawing scaled uniformly so its longer side matches the reference's
/// and centered on a canvas the reference crop's size (on its own
/// background). A model framed closer or further, or placed elsewhere in
/// the frame, compares as the same; one seen from another side doesn't.
fn registered(
    reference: &image::RgbaImage,
    drawn: &image::RgbaImage,
    (rb, db): ([u8; 3], [u8; 3]),
) -> (image::RgbaImage, image::RgbaImage) {
    use image::imageops::{FilterType, crop_imm, overlay, resize};
    let (rx, ry, rw, rh) = foreground_box(reference, rb);
    let (dx, dy, dw, dh) = foreground_box(drawn, db);
    // The reference crop at 128 pixels on its long side.
    let scale = 128.0 / rw.max(rh) as f32;
    let (tw, th) = (
        ((rw as f32 * scale).round() as u32).max(8),
        ((rh as f32 * scale).round() as u32).max(8),
    );
    let reference = resize(
        &crop_imm(reference, rx, ry, rw, rh).to_image(),
        tw,
        th,
        FilterType::Triangle,
    );
    let s = tw.max(th) as f32 / dw.max(dh) as f32;
    let (sw, sh) = (
        ((dw as f32 * s).round() as u32).clamp(1, tw.max(th)),
        ((dh as f32 * s).round() as u32).clamp(1, tw.max(th)),
    );
    let scaled = resize(
        &crop_imm(drawn, dx, dy, dw, dh).to_image(),
        sw,
        sh,
        FilterType::Triangle,
    );
    let mut canvas = image::RgbaImage::from_pixel(tw, th, image::Rgba([db[0], db[1], db[2], 255]));
    overlay(
        &mut canvas,
        &scaled,
        (i64::from(tw) - i64::from(sw)) / 2,
        (i64::from(th) - i64::from(sh)) / 2,
    );
    (reference, canvas)
}

fn similarity(reference: &image::RgbaImage, drawn: &image::RgbaImage) -> Similarity {
    let backgrounds = (border_color(reference), border_color(drawn));
    let (rb, db) = backgrounds;
    let (reference, drawn) = registered(reference, drawn, backgrounds);
    let (reference, drawn) = (&reference, &drawn);
    let (w, h) = reference.dimensions();
    let fore = |p: &image::Rgba<u8>, bg: [u8; 3]| (0..3).any(|c| p[c].abs_diff(bg[c]) > FOREGROUND);
    let (mut both, mut either) = (0usize, 0usize);
    let mut masks = vec![false; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let (r, d) = (fore(reference.get_pixel(x, y), rb), fore(drawn.get_pixel(x, y), db));
            both += usize::from(r && d);
            either += usize::from(r || d);
            masks[(y * w + x) as usize] = r || d;
        }
    }
    let overlap = if either == 0 { 1.0 } else { both as f32 / either as f32 };
    let (mut sum, mut light, mut blocks) = (0.0f32, 0.0f32, 0usize);
    for by in (0..h).step_by(BLOCK as usize) {
        for bx in (0..w).step_by(BLOCK as usize) {
            let (mut a, mut b, mut n, mut any) = ([0f32; 3], [0f32; 3], 0.0f32, false);
            for y in by..(by + BLOCK).min(h) {
                for x in bx..(bx + BLOCK).min(w) {
                    any |= masks[(y * w + x) as usize];
                    let (p, q) = (reference.get_pixel(x, y), drawn.get_pixel(x, y));
                    for c in 0..3 {
                        a[c] += f32::from(p[c]);
                        b[c] += f32::from(q[c]);
                    }
                    n += 1.0;
                }
            }
            if any {
                let (la, lb) = (lab(a.map(|v| v / n)), lab(b.map(|v| v / n)));
                sum += ((la[0] - lb[0]).powi(2) + (la[1] - lb[1]).powi(2) + (la[2] - lb[2]).powi(2)).sqrt();
                light += lb[0] - la[0];
                blocks += 1;
            }
        }
    }
    let n = blocks.max(1) as f32;
    Similarity {
        overlap,
        color: sum / n,
        lightness: light / n,
    }
}

/// The size a sample is drawn at: its screenshot's shape, 256 pixels on
/// the long side.
fn drawn_size(reference: (u32, u32)) -> (u32, u32) {
    let (w, h) = (reference.0.max(1) as f32, reference.1.max(1) as f32);
    let scale = 256.0 / w.max(h);
    (
        ((w * scale).round() as u32).max(16),
        ((h * scale).round() as u32).max(16),
    )
}

// Keep the comparison rig for samples outside the lighting group so the
// existing material, geometry and texture comparisons stay comparable.
const RIG_SUN: f32 = 2.0;

fn sample_lighting(name: &str) -> Option<(f32, f32)> {
    // Fixed EV100 and environment intensity for the reference shots.
    // glTF files carry no camera exposure.
    let lighting = match name {
        "PlaysetLightTest" => (11.0, 0.35),
        "LightsPunctualLamp" => (5.0, 0.20),
        "PointLightIntensityTest" => (0.0, 0.04),
        "DirectionalLight" => (0.0, 0.10),
        "CarConcept"
        | "CompareEmissiveStrength"
        | "EmissiveStrengthTest"
        | "EnvironmentTest"
        | "PotOfCoals"
        | "USDShaderBallForGltf"
        | "VirtualCity" => (0.0, 1.0),
        _ => return None,
    };
    Some(lighting)
}

/// Draws the sample through its first camera or frames its bounds, with
/// the screenshot's background and the test's lighting for that sample.
fn draw_sample(
    name: &str,
    file: &str,
    size: (u32, u32),
    background: [u8; 3],
) -> Result<Option<(image::RgbaImage, bool)>, String> {
    let Some((mut ctx, _)) = game_ctx(size) else {
        return Ok(None);
    };
    let mesh = ctx.gfx.try_load_mesh(file).map_err(|e| e.to_string())?;
    let at = Transform3D::default();
    let model = ctx.world.spawn((at, Model::new(mesh)));
    // A file with skins stands in its rest pose with its skeleton, as the
    // guide spawns one ("Skinned meshes"); without it, a skinned mesh
    // stays in the space it was bound in.
    if let Ok(skeleton) = ctx.gfx.try_load_skeleton(file) {
        ctx.world.insert(model, skeleton);
    }
    let bg = |c: u8| f32::from(c) / 255.0;
    ctx.gfx.clear_color = Color::rgb(bg(background[0]), bg(background[1]), bg(background[2]));
    let sky = &mut ctx.scene3d.sky;
    sky.atmosphere = false;
    let camera = &mut ctx.scene3d.camera;
    let aspect = size.0 as f32 / size.1 as f32;
    let cameras = ctx.gfx.try_load_cameras(file).map_err(|e| e.to_string())?;
    let own_camera = if let Some(shot) = cameras.first() {
        shot.apply(camera, &at);
        true
    } else {
        let (center, radius) = ctx.gfx.assets3d.bounds[mesh.id as usize];
        frame(camera, center, radius, aspect);
        false
    };
    if let Some((ev100, ambient)) = sample_lighting(name) {
        ctx.scene3d.lighting.ambient_intensity = ambient;
        let lights = ctx.gfx.try_load_lights(file).map_err(|e| e.to_string())?;
        ctx.scene3d.light_gltf(&mut ctx.world, &lights, &at, ev100);
        if name == "DirectionalLight" {
            // The reference shows the direct BRDF of three spheres in isolation.
            ctx.scene3d.lighting.shadows = false;
        }
        if name == "EmissiveStrengthTest" {
            ctx.scene3d.post.exposure = -1.25;
            ctx.scene3d.post.bloom = 0.15;
        }
    } else {
        ctx.scene3d.sky.sky_color = Color::rgb(0.6, 0.6, 0.6);
        ctx.scene3d.sky.ground_color = Color::rgb(0.3, 0.3, 0.3);
        let (forward, up, right) = (
            camera.rotation * Vec3::NEG_Z,
            camera.rotation * Vec3::Y,
            camera.rotation * Vec3::X,
        );
        let lighting = &mut ctx.scene3d.lighting;
        lighting.sun_direction = (forward * 0.7 - up * 0.8 + right * 0.5).normalize();
        lighting.sun_intensity = RIG_SUN;
        for light in ctx.gfx.try_load_lights(file).map_err(|e| e.to_string())? {
            if let ModelLightKind::Sun { color, intensity } = light.kind {
                lighting.sun_direction = light.placed(&at).forward();
                (lighting.sun_color, lighting.sun_intensity) = (color, intensity);
            } else {
                light.spawn(&mut ctx.world, &at);
            }
        }
    }
    // Uploads, shadows and the temporal effects settle over a few frames.
    for _ in 0..8 {
        ctx.render_frame();
    }
    Ok(Some((capture(&mut ctx), own_camera)))
}

/// `a` and `b` side by side (each fitted into `cell` x `cell`), on grey.
fn side_by_side(a: &image::RgbaImage, b: &image::RgbaImage, cell: u32) -> image::RgbaImage {
    let mut out = image::RgbaImage::from_pixel(cell * 2 + 4, cell, image::Rgba([128, 128, 128, 255]));
    for (i, img) in [a, b].into_iter().enumerate() {
        let (w, h) = img.dimensions();
        let s = cell as f32 / w.max(h) as f32;
        let (nw, nh) = (((w as f32 * s) as u32).max(1), ((h as f32 * s) as u32).max(1));
        let small = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle);
        let x = i as u32 * (cell + 4) + (cell - nw) / 2;
        image::imageops::overlay(&mut out, &small, i64::from(x), i64::from((cell - nh) / 2));
    }
    out
}

const DRAWS: &str = "tests/fixtures/known-content/draws.txt";

/// The recorded verdicts: the revision they are for, and each sample's.
fn recorded() -> (String, std::collections::BTreeMap<String, String>) {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(DRAWS)).unwrap_or_default();
    let mut revision = String::new();
    let mut verdicts = std::collections::BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(rev) = line.strip_prefix("# revision ") {
            revision = rev.trim().to_string();
        } else if !line.starts_with('#')
            && let Some((name, verdict)) = line.split_once(": ")
        {
            verdicts.insert(name.to_string(), verdict.to_string());
        }
    }
    (revision, verdicts)
}

#[test]
fn the_similarity_tells_a_drawing_from_an_empty_frame_and_from_another_shape() {
    // A white square on grey, as a reference; the same drawn a little
    // darker (another viewer's light); nothing; the square moved away.
    let picture = |at: u32, shade: u8| {
        image::RgbaImage::from_fn(64, 64, |x, y| {
            if (at..at + 24).contains(&x) && (20..44).contains(&y) {
                image::Rgba([shade, shade, shade, 255])
            } else {
                image::Rgba([90, 90, 90, 255])
            }
        })
    };
    let reference = picture(20, 240);
    let same = similarity(&reference, &picture(20, 220));
    assert!(same.matches(), "{same:?}");
    let empty = similarity(
        &reference,
        &image::RgbaImage::from_pixel(64, 64, image::Rgba([90, 90, 90, 255])),
    );
    assert!(!empty.matches() && empty.overlap == 0.0, "{empty:?}");
    // Framed elsewhere and smaller: the same picture once registered.
    let elsewhere = similarity(
        &reference,
        &image::RgbaImage::from_fn(64, 64, |x, y| {
            if (40..52).contains(&x) && (4..16).contains(&y) {
                image::Rgba([235, 235, 235, 255])
            } else {
                image::Rgba([90, 90, 90, 255])
            }
        }),
    );
    assert!(elsewhere.matches(), "{elsewhere:?}");
    // Another shape (a bar, as a model seen from another side): no match.
    let bar = similarity(
        &reference,
        &image::RgbaImage::from_fn(64, 64, |x, y| {
            if (4..60).contains(&x) && (28..36).contains(&y) {
                image::Rgba([240, 240, 240, 255])
            } else {
                image::Rgba([90, 90, 90, 255])
            }
        }),
    );
    assert!(!bar.matches() && bar.overlap < 0.5, "{bar:?}");
    let recolored = similarity(
        &reference,
        &image::RgbaImage::from_fn(64, 64, |x, y| {
            if (20..44).contains(&x) && (20..44).contains(&y) {
                image::Rgba([240, 40, 40, 255])
            } else {
                image::Rgba([90, 90, 90, 255])
            }
        }),
    );
    assert!(!recolored.matches() && recolored.overlap > 0.9, "{recolored:?}");
    assert!(recolored.difference().contains("colors"), "{}", recolored.difference());
}

#[test]
fn the_framing_rule_fits_the_bounds_in_view() {
    let mut camera = crate::three::Camera3D::new(Vec2::new(100.0, 100.0));
    for aspect in [0.5f32, 1.0, 2.0] {
        frame(&mut camera, vec3(1.0, 2.0, 3.0), 2.0, aspect);
        camera.set_viewport(Vec2::new(100.0 * aspect, 100.0));
        let view_proj = camera.view_proj();
        // The sphere's extreme points stay on screen.
        for d in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y] {
            let p = view_proj.project_point3(vec3(1.0, 2.0, 3.0) + d * 2.0);
            assert!(
                p.x.abs() <= 1.0 + 1e-4 && p.y.abs() <= 1.0 + 1e-4,
                "aspect {aspect}: {d} at {p}"
            );
        }
        assert_eq!(camera.forward(), Vec3::NEG_Z);
    }
}

#[test]
fn known_samples_are_drawn_like_their_screenshots() {
    let root = cache();
    let Some(manifest) = manifest(&root) else { return };
    let Some((_, adapter)) = game_ctx((16, 16)) else {
        return;
    };
    let revision = manifest["samples"]["revision"].as_str().unwrap_or_default().to_string();
    let only = std::env::var("PONKO_KNOWN_ONLY").ok();
    let out = root.join("results");
    std::fs::create_dir_all(out.join("draw")).unwrap();
    let mut rows = Vec::new();
    let mut found = std::collections::BTreeMap::new();
    let mut sheet: Vec<(String, image::RgbaImage)> = Vec::new();
    let samples = manifest["assets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["set"] == "samples");
    for asset in samples {
        let name = asset["name"].as_str().unwrap().to_string();
        if only.as_deref().is_some_and(|o| !o.split(',').any(|n| n == name)) {
            continue;
        }
        let file = asset["file"].as_str().unwrap();
        let Some(screenshot) = asset["screenshot"].as_str() else {
            rows.push(json!({ "name": name, "drawn": false, "why": "no screenshot" }));
            continue;
        };
        // Some screenshots are animated GIFs, which this build doesn't read.
        let reference = match image::open(root.join(screenshot)) {
            Ok(image) => image.to_rgba8(),
            Err(e) => {
                rows.push(json!({ "name": name, "drawn": false, "why": format!("its screenshot can't be read: {e}") }));
                continue;
            }
        };
        let size = drawn_size(reference.dimensions());
        let reference = image::imageops::resize(&reference, size.0, size.1, image::imageops::FilterType::Triangle);
        let background = border_color(&reference);
        let drawn = std::panic::catch_unwind(|| draw_sample(&name, file, size, background));
        let (drawn, own_camera) = match drawn {
            Ok(Ok(Some(drawn))) => drawn,
            Ok(Ok(None)) => return,
            // Not loading is the load sweep's (tests/known_content.rs).
            Ok(Err(e)) => {
                rows.push(json!({ "name": name, "drawn": false, "why": e }));
                continue;
            }
            Err(panic) => {
                let text = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_default();
                rows.push(json!({ "name": name, "drawn": false, "why": format!("panicked: {text}") }));
                found.insert(name.clone(), "panicked".to_string());
                continue;
            }
        };
        let s = similarity(&reference, &drawn);
        let verdict = if s.matches() { "matches" } else { "differs" };
        eprintln!(
            "{name:40} {verdict:8} overlap {:.2} ΔE {:5.1}{}",
            s.overlap,
            s.color,
            if own_camera { " (its camera)" } else { "" }
        );
        let pair = side_by_side(&reference, &drawn, 256);
        pair.save(out.join("draw").join(format!("{name}.png"))).unwrap();
        sheet.push((name.clone(), side_by_side(&reference, &drawn, 120)));
        rows.push(json!({
            "name": name, "drawn": true, "matches": s.matches(), "overlap": s.overlap, "color": s.color,
            "lightness": s.lightness,
            "difference": s.difference(), "own_camera": own_camera, "size": [size.0, size.1],
        }));
        found.insert(name, verdict.to_string());
    }
    // Contact sheets of 16 pairs (reference left, drawing right), in the
    // order of sheets.txt, for looking through them.
    let mut index = String::new();
    for (n, chunk) in sheet.chunks(16).enumerate() {
        let (cw, ch) = (chunk[0].1.width(), chunk[0].1.height());
        let mut page = image::RgbaImage::from_pixel(cw * 4 + 12, ch * 4 + 12, image::Rgba([40, 40, 40, 255]));
        for (i, (name, pair)) in chunk.iter().enumerate() {
            let (col, row) = (i as u32 % 4, i as u32 / 4);
            image::imageops::overlay(&mut page, pair, i64::from(col * (cw + 4)), i64::from(row * (ch + 4)));
            index.push_str(&format!("sheet-{n:02}.png {row} {col} {name}\n"));
        }
        page.save(out.join(format!("sheet-{n:02}.png"))).unwrap();
    }
    std::fs::write(out.join("sheets.txt"), index).unwrap();
    std::fs::write(
        out.join("draw.json"),
        serde_json::to_string_pretty(&json!({
            "revision": revision,
            "adapter": describe_adapter(&adapter),
            "tolerance": { "overlap": Similarity::OVERLAP, "color": Similarity::COLOR },
            "assets": rows,
        }))
        .unwrap(),
    )
    .unwrap();
    let matching = found.values().filter(|v| *v == "matches").count();
    eprintln!("{} samples drawn: {matching} match their screenshots", found.len());

    let (recorded_revision, mut recorded) = recorded();
    if std::env::var_os("PONKO_UPDATE_KNOWN_CONTENT").is_some() && only.is_none() {
        recorded.extend(found);
        let mut text = format!(
            "# Whether each Khronos glTF sample is drawn like its screenshot\n\
             # (src/render/known_content_render_test.rs). Rewritten by PONKO_UPDATE_KNOWN_CONTENT=1.\n\
             # revision {revision}\n"
        );
        for (name, verdict) in &recorded {
            text.push_str(&format!("{name}: {verdict}\n"));
        }
        std::fs::write(Path::new(env!("CARGO_MANIFEST_DIR")).join(DRAWS), text).unwrap();
        return;
    }
    if recorded_revision != revision {
        eprintln!("the cache holds revision {revision}, {DRAWS} is for {recorded_revision}: verdicts not compared");
        return;
    }
    // A sample that matched must still match, and nothing may panic.
    let worse: Vec<String> = found
        .iter()
        .filter(|(name, verdict)| {
            *verdict == "panicked"
                || (recorded.get(*name).map(String::as_str) == Some("matches") && *verdict != "matches")
        })
        .map(|(name, verdict)| format!("{name}: {verdict}"))
        .collect();
    assert!(
        worse.is_empty(),
        "samples drawn worse than {DRAWS} records (see results/draw in the cache):\n{}",
        worse.join("\n")
    );
}

/// The process's resident memory now, in bytes, as `ps` reports it (no
/// figure where there is no `ps`).
fn resident_memory() -> Option<u64> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let kib: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kib * 1024)
}

/// A scene's camera when its file has none: position, a point looked at.
type Place = Option<(Vec3, Vec3)>;

/// The two scenes: name, file, and the camera for a scene without one.
const SCENES: [(&str, &str, Place); 2] = [
    // Down the length of the atrium, from one end at head height.
    (
        "Sponza",
        "samples/Sponza/glTF/Sponza.gltf",
        Some((Vec3::new(9.0, 1.6, -0.4), Vec3::new(-9.0, 3.0, -0.4))),
    ),
    // The file's own camera, looking down the street.
    ("Bistro", "bistro/bistro.gltf", None),
];

const SIZES: [(u32, u32); 2] = [(1920, 1080), (2560, 1440)];
const WARM_UP: usize = 20;
const FRAMES: usize = 60;

/// Load time, memory, GPU memory and the GPU time of each pass of the two
/// scenes, at each size and quality preset. Run explicitly, in release:
/// `cargo test --release --lib --features 3d,compressed-textures known_scenes_benchmark -- --ignored --nocapture`
#[test]
#[ignore = "a measurement: run in release on purpose"]
fn known_scenes_benchmark() {
    let root = cache();
    let Some(manifest) = manifest(&root) else { return };
    let have = |file: &str| {
        manifest["assets"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x["file"] == file))
    };
    let mut report = Vec::new();
    let only = std::env::var("PONKO_KNOWN_SCENE").ok();
    let studio = std::env::var_os("PONKO_KNOWN_STUDIO").is_some();
    for (name, file, place) in SCENES {
        if only.as_deref().is_some_and(|only| only != name) {
            continue;
        }
        if !have(file) {
            eprintln!("{name}: not in the cache (fetch it with --set all)");
            continue;
        }
        let rss_before = resident_memory().unwrap_or(0);
        let Some((mut ctx, info)) = game_ctx(SIZES[0]) else {
            return;
        };
        let started = Instant::now();
        let mesh = ctx.gfx.try_load_mesh(file).unwrap_or_else(|e| panic!("{name}: {e}"));
        let at = Transform3D::default();
        ctx.world.spawn((at, Model::new(mesh)));
        let mut lights = 0;
        for light in ctx.gfx.load_lights(file) {
            // The Bistro sun is 6830 lux, exposed at EV100 11.
            if light
                .place_exposed(&mut ctx.world, &mut ctx.scene3d.lighting, &at, 11.0)
                .is_some()
            {
                lights += 1;
            }
        }
        if studio {
            ctx.scene3d.environment = Some(EnvironmentMap::studio());
        }
        match (ctx.gfx.load_cameras(file).first(), place) {
            (Some(shot), _) => shot.apply(&mut ctx.scene3d.camera, &at),
            (None, Some((position, target))) => {
                ctx.scene3d.camera.position = position;
                ctx.scene3d.camera.look_at(target);
            }
            (None, None) => {}
        }
        let parsed = started.elapsed().as_secs_f64();
        ctx.render_frame();
        wait(&ctx);
        let loaded = started.elapsed().as_secs_f64();
        let rss = resident_memory().unwrap_or(0);
        // What was measured, to look at.
        std::fs::create_dir_all(root.join("results")).unwrap();
        capture(&mut ctx)
            .save(root.join(format!("results/{name}.png")))
            .unwrap();
        eprintln!(
            "{name}: parsed in {parsed:.2} s, first frame drawn {loaded:.2} s after the load began; \
             resident memory {:.0} MB (+{:.0} MB); {lights} lights; on {}",
            rss as f64 / 1e6,
            rss.saturating_sub(rss_before) as f64 / 1e6,
            describe_adapter(&info)
        );
        for size in SIZES {
            if let Some(renderer) = ctx.gfx.renderer.as_mut() {
                renderer.resize(size.0, size.1);
            }
            ctx.set_window_size(Vec2::new(size.0 as f32, size.1 as f32));
            for quality in GraphicsQuality::ALL {
                ctx.scene3d.set_quality(quality);
                for _ in 0..WARM_UP {
                    ctx.render_frame();
                }
                wait(&ctx);
                let mut frame_ms = Vec::with_capacity(FRAMES);
                let mut passes: Vec<(&'static str, f64)> = Vec::new();
                let mut timed = 0u32;
                let mut seen = ctx.gfx.renderer.as_ref().and_then(|r| r.gpu_frame()).map_or(0, |f| f.1);
                for _ in 0..FRAMES {
                    let t = Instant::now();
                    ctx.render_frame();
                    wait(&ctx);
                    frame_ms.push(t.elapsed().as_secs_f64() * 1e3);
                    if let Some((spans, frames)) = ctx.gfx.renderer.as_ref().and_then(|r| r.gpu_frame())
                        && frames != seen
                    {
                        seen = frames;
                        timed += 1;
                        for span in spans {
                            match passes.iter_mut().find(|p| p.0 == span.name) {
                                Some(p) => p.1 += f64::from(span.ms),
                                None => passes.push((span.name, f64::from(span.ms))),
                            }
                        }
                    }
                }
                frame_ms.sort_by(f64::total_cmp);
                let gpu = ctx
                    .gfx
                    .renderer
                    .as_ref()
                    .and_then(|r| r.gpu_memory())
                    .unwrap_or_default();
                let passes: Vec<(&str, f64)> = passes
                    .iter()
                    .map(|(n, ms)| (*n, ms / f64::from(timed.max(1))))
                    .collect();
                let gpu_total: f64 = passes.iter().map(|p| p.1).sum();
                eprintln!(
                    "{name} {}x{} {quality:?}: frame median {:.2} ms, p95 {:.2} ms, GPU {gpu_total:.2} ms; \
                     GPU memory {:.0} MB (textures {:.0}, meshes {:.0}, targets {:.0}, shadows {:.0})",
                    size.0,
                    size.1,
                    frame_ms[FRAMES / 2],
                    frame_ms[FRAMES * 95 / 100],
                    gpu.total as f64 / 1e6,
                    gpu.textures as f64 / 1e6,
                    gpu.meshes as f64 / 1e6,
                    gpu.frame_targets as f64 / 1e6,
                    gpu.shadows as f64 / 1e6,
                );
                report.push(json!({
                    "scene": name, "studio_environment": studio, "width": size.0, "height": size.1, "quality": format!("{quality:?}"),
                    "parse_s": parsed, "first_frame_s": loaded, "resident_mb": rss as f64 / 1e6, "resident_added_mb": rss.saturating_sub(rss_before) as f64 / 1e6,
                    "frame_ms_median": frame_ms[FRAMES / 2], "frame_ms_p95": frame_ms[FRAMES * 95 / 100],
                    "gpu_ms": gpu_total, "gpu_frames_timed": timed,
                    "passes": passes.iter().map(|(n, ms)| json!({ "pass": n, "ms": ms })).collect::<Vec<_>>(),
                    "gpu_memory_mb": {
                        "total": gpu.total as f64 / 1e6, "textures": gpu.textures as f64 / 1e6,
                        "meshes": gpu.meshes as f64 / 1e6, "frame_targets": gpu.frame_targets as f64 / 1e6,
                        "shadows": gpu.shadows as f64 / 1e6,
                    },
                    "adapter": describe_adapter(&info),
                }));
            }
        }
    }
    let out = root.join("results");
    std::fs::create_dir_all(&out).unwrap();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    std::fs::write(
        out.join(format!("scenes-{stamp}.json")),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
}
