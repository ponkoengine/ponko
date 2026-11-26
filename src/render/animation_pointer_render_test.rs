//! glTF `KHR_animation_pointer` animations drawn offscreen through a
//! game's own frame (`Ctx::render_frame`): a node's visibility, a
//! material's base color, a texture transform's offset and an alpha cutoff,
//! each keyed from one value at 0 s to another at 1 s. The picture is the
//! proof: each square changes where the clip says, and the same clip
//! without its property tracks (the negative control) draws the squares
//! as they rest. A static texture transform draws as the texture
//! coordinates moved by hand did when the loader baked it into them.
//! The adapter is logged ("render test ran [...] on ..."). Skips without a
//! GPU (a skip is not a pass).
use serde_json::{Value, json};

use super::render_test_support::{Image, keep_image, offscreen_ctx};
use crate::anim::{AnimationClip, Skeleton};
use crate::assets::MemorySource;
use crate::ctx::Ctx;
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::{Model, Tonemap};

const SIZE: (u32, u32) = (160, 64);

/// Where the squares stand, left to right: blinking, tinted, sliding, cut.
const BLINK: f32 = -1.5;
const TINT: f32 = -0.5;
const SLIDE: f32 = 0.5;
const CUT: f32 = 1.5;

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A 2 × 1 texture: black on the left, white on the right.
fn halves() -> String {
    let image = image::RgbaImage::from_fn(2, 1, |x, _| {
        let v = if x == 0 { 0 } else { 255 };
        image::Rgba([v, v, v, 255])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    format!("data:image/png;base64,{}", base64(&bytes.into_inner()))
}

/// The binary part of a file and its views and accessors.
#[derive(Default)]
struct Bin {
    bytes: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    fn add(&mut self, data: &[u8], component: u32, count: usize, ty: &str, extra: Value) -> usize {
        self.bytes.resize(self.bytes.len().next_multiple_of(4), 0);
        self.views
            .push(json!({ "buffer": 0, "byteOffset": self.bytes.len(), "byteLength": data.len() }));
        self.bytes.extend_from_slice(data);
        let mut a =
            json!({ "bufferView": self.views.len() - 1, "componentType": component, "count": count, "type": ty });
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            a[k] = v;
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    fn floats(&mut self, values: &[f32], n: usize) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|f| f.to_le_bytes()).collect();
        let ty = ["", "SCALAR", "VEC2", "VEC3", "VEC4"][n];
        self.add(&bytes, 5126, values.len() / n, ty, json!({}))
    }

    fn finish(self, mut doc: Value) -> Vec<u8> {
        doc["asset"] = json!({ "version": "2.0" });
        doc["buffers"] = json!([{
            "byteLength": self.bytes.len(),
            "uri": format!("data:application/octet-stream;base64,{}", base64(&self.bytes)),
        }]);
        doc["bufferViews"] = json!(self.views);
        doc["accessors"] = json!(self.accessors);
        doc.to_string().into_bytes()
    }
}

/// A square 0.8 across facing +Z, its texture coordinates shifted by `du`.
fn square(bin: &mut Bin, du: f32) -> Value {
    let h = 0.4;
    let positions = [-h, -h, 0.0, h, -h, 0.0, h, h, 0.0, -h, h, 0.0];
    let bytes: Vec<u8> = positions.iter().flat_map(|f: &f32| f.to_le_bytes()).collect();
    let position = bin.add(
        &bytes,
        5126,
        4,
        "VEC3",
        json!({ "min": [-h, -h, 0.0], "max": [h, h, 0.0] }),
    );
    let uv = bin.floats(&[du, 1.0, 1.0 + du, 1.0, 1.0 + du, 0.0, du, 0.0], 2);
    let indices: Vec<u8> = [0u16, 1, 2, 0, 2, 3].iter().flat_map(|i| i.to_le_bytes()).collect();
    let indices = bin.add(&indices, 5123, 6, "SCALAR", json!({}));
    json!({ "attributes": { "POSITION": position, "TEXCOORD_0": uv }, "indices": indices })
}

/// An unlit material of `color`, with the black-and-white texture when
/// `textured`, and `extra` merged in.
fn unlit(color: [f32; 4], textured: bool, extra: Value) -> Value {
    let mut m = json!({
        "pbrMetallicRoughness": { "baseColorFactor": color },
        "extensions": { "KHR_materials_unlit": {} },
    });
    if textured {
        m["pbrMetallicRoughness"]["baseColorTexture"] = json!({ "index": 0 });
    }
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        m[k] = v;
    }
    m
}

fn pointer(path: &str) -> Value {
    json!({ "path": "pointer", "extensions": { "KHR_animation_pointer": { "pointer": path } } })
}

/// Four squares in a row, each with a value keyed from 0 s to 1 s:
/// a red one shown then hidden; a white one turning green; a textured one
/// whose texture slides half its width; and a half-transparent cutout one
/// whose alpha cutoff rises from 0.2 to 0.8, cutting it away.
fn animated() -> Vec<u8> {
    let mut bin = Bin::default();
    let primitive = square(&mut bin, 0.0);
    let times = bin.floats(&[0.0, 1.0], 1);
    let flags = bin.add(&[1, 0], 5121, 2, "SCALAR", json!({}));
    let colors = bin.floats(&[1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0], 4);
    let offsets = bin.floats(&[0.0, 0.0, 0.5, 0.0], 2);
    let cutoffs = bin.floats(&[0.2, 0.8], 1);
    let mesh = |material: usize| {
        let mut p = primitive.clone();
        p["material"] = json!(material);
        json!({ "primitives": [p] })
    };
    let node = |mesh: usize, x: f32| json!({ "mesh": mesh, "translation": [x, 0.0, 0.0] });
    bin.finish(json!({
        "extensionsUsed": ["KHR_animation_pointer", "KHR_node_visibility", "KHR_materials_unlit", "KHR_texture_transform"],
        "scene": 0,
        "scenes": [{ "nodes": [0, 1, 2, 3] }],
        "nodes": [node(0, BLINK), node(1, TINT), node(2, SLIDE), node(3, CUT)],
        "meshes": [mesh(0), mesh(1), mesh(2), mesh(3)],
        "materials": [
            unlit([1.0, 0.0, 0.0, 1.0], false, json!({})),
            unlit([1.0, 1.0, 1.0, 1.0], false, json!({})),
            unlit([1.0, 1.0, 1.0, 1.0], true, json!({})),
            unlit([1.0, 1.0, 1.0, 0.5], false, json!({ "alphaMode": "MASK", "alphaCutoff": 0.2 })),
        ],
        "textures": [{ "source": 0, "sampler": 0 }],
        "samplers": [{ "magFilter": 9728, "minFilter": 9728 }],
        "images": [{ "uri": halves() }],
        "animations": [{
            "name": "change",
            "samplers": [
                { "input": times, "output": flags, "interpolation": "STEP" },
                { "input": times, "output": colors },
                { "input": times, "output": offsets },
                { "input": times, "output": cutoffs },
            ],
            "channels": [
                { "sampler": 0, "target": pointer("/nodes/0/extensions/KHR_node_visibility/visible") },
                { "sampler": 1, "target": pointer("/materials/1/pbrMetallicRoughness/baseColorFactor") },
                { "sampler": 2, "target": pointer("/materials/2/pbrMetallicRoughness/baseColorTexture/extensions/KHR_texture_transform/offset") },
                { "sampler": 3, "target": pointer("/materials/3/alphaCutoff") },
            ],
        }],
    }))
}

/// One textured square: with a static texture transform of offset
/// (`offset`, 0), or none and its texture coordinates shifted by `du`.
fn slid(offset: Option<f32>, du: f32) -> Vec<u8> {
    let mut bin = Bin::default();
    let mut primitive = square(&mut bin, du);
    primitive["material"] = json!(0);
    let mut material = unlit([1.0; 4], true, json!({}));
    if let Some(offset) = offset {
        material["pbrMetallicRoughness"]["baseColorTexture"]["extensions"] =
            json!({ "KHR_texture_transform": { "offset": [offset, 0.0] } });
    }
    bin.finish(json!({
        "extensionsUsed": ["KHR_materials_unlit", "KHR_texture_transform"],
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0, "translation": [SLIDE, 0.0, 0.0] }],
        "meshes": [{ "primitives": [primitive] }],
        "materials": [material],
        "textures": [{ "source": 0, "sampler": 0 }],
        "samplers": [{ "magFilter": 9728, "minFilter": 9728 }],
        "images": [{ "uri": halves() }],
    }))
}

/// A game drawing offscreen on black, the files in memory, the camera in
/// front of the squares; colors as they are (no curve, no glow).
fn scene() -> Option<Ctx> {
    let mut ctx = offscreen_ctx(SIZE)?;
    ctx.assets.add_source(
        MemorySource::new("mem")
            .with("animated.gltf", animated())
            .with("static.gltf", slid(Some(0.5), 0.0))
            .with("plain.gltf", slid(None, 0.0))
            .with("shifted.gltf", slid(None, 0.5)),
    );
    ctx.scene3d.sky.atmosphere = false;
    ctx.scene3d.lighting.shadows = false;
    ctx.scene3d.post.tonemap = Tonemap::None;
    ctx.scene3d.post.bloom = 0.0;
    ctx.gfx.clear_color = Color::BLACK;
    ctx.scene3d.camera.position = vec3(0.0, 0.0, 3.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    Some(ctx)
}

/// What a drawing is sampled at: its clip at a time, with or without its
/// property tracks.
#[derive(Clone, Copy)]
enum Pose {
    Rest,
    At(f32),
    Unkeyed(f32),
}

/// Draws `file` in `pose` in a fresh game (no temporal history from
/// another drawing) and reads the frame back.
fn draw(file: &str, pose: Pose) -> Option<(image::RgbaImage, Ctx)> {
    let mut ctx = scene()?;
    let mesh = ctx.gfx.load_mesh(file);
    let model = ctx.world.spawn((Transform3D::default(), Model::new(mesh)));
    if let Ok(mut skeleton) = ctx.gfx.try_load_skeleton(file) {
        let clips: Vec<AnimationClip> = ctx.gfx.load_clips(file);
        for clip in clips {
            match pose {
                Pose::Rest => {}
                Pose::At(t) => clip.sample(t, &mut skeleton),
                Pose::Unkeyed(t) => clip.with_property_tracks(Vec::new()).sample(t, &mut skeleton),
            }
        }
        ctx.world.insert::<Skeleton>(model, skeleton);
    }
    // The first frames put the meshes and textures on the GPU.
    for _ in 0..3 {
        ctx.render_frame();
    }
    let path = std::env::temp_dir().join(format!(
        "ponko-pointer-{}-{:?}.png",
        std::process::id(),
        std::thread::current().id()
    ));
    ctx.gfx.queue_screenshot(path.clone(), 0, None);
    ctx.render_frame();
    let image = image::open(&path).expect("the frame was captured").to_rgba8();
    let _ = std::fs::remove_file(&path);
    // With PONKO_RENDER_TEST_IMAGES=1, kept for looking at.
    keep_image(&Image {
        width: image.width(),
        height: image.height(),
        rgba: image.as_raw().clone(),
    });
    Some((image, ctx))
}

/// The color the camera sees at the world point (x, y, 0), 0 to 255.
fn color(ctx: &Ctx, image: &image::RgbaImage, x: f32, y: f32) -> [f32; 3] {
    let ndc = ctx.scene3d.camera.view_proj().project_point3(vec3(x, y, 0.0));
    let px = ((ndc.x * 0.5 + 0.5) * SIZE.0 as f32) as u32;
    let py = ((0.5 - ndc.y * 0.5) * SIZE.1 as f32) as u32;
    let p = image.get_pixel(px.min(SIZE.0 - 1), py.min(SIZE.1 - 1));
    [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])]
}

fn dark(c: [f32; 3]) -> bool {
    c.iter().all(|v| *v < 30.0)
}

fn bright(c: [f32; 3]) -> bool {
    c.iter().all(|v| *v > 200.0)
}

#[test]
fn pointer_animations_change_what_is_drawn() {
    let Some((rest, ctx)) = draw("animated.gltf", Pose::Rest) else {
        return;
    };
    let (start, _) = draw("animated.gltf", Pose::At(0.0)).unwrap();
    let (end, _) = draw("animated.gltf", Pose::At(1.0)).unwrap();
    let (half, _) = draw("animated.gltf", Pose::At(0.5)).unwrap();
    let (unkeyed, _) = draw("animated.gltf", Pose::Unkeyed(1.0)).unwrap();
    let at = |image: &image::RgbaImage, x: f32| color(&ctx, image, x, 0.0);
    let left = |image: &image::RgbaImage| color(&ctx, image, SLIDE - 0.2, 0.0);
    let right = |image: &image::RgbaImage| color(&ctx, image, SLIDE + 0.2, 0.0);
    for (image, when) in [
        (&rest, "at rest"),
        (&start, "at 0 s"),
        (&unkeyed, "at 1 s without its tracks"),
    ] {
        // Shown and red; white; black then white; cut out but kept.
        let red = at(image, BLINK);
        assert!(
            red[0] > 200.0 && red[1] < 30.0,
            "{when}: the blinking square is {red:?}"
        );
        assert!(
            bright(at(image, TINT)),
            "{when}: the tinted square is {:?}",
            at(image, TINT)
        );
        assert!(
            dark(left(image)) && bright(right(image)),
            "{when}: the sliding texture is {:?} | {:?}",
            left(image),
            right(image)
        );
        assert!(
            bright(at(image, CUT)),
            "{when}: the cutout square is {:?}",
            at(image, CUT)
        );
    }
    // At 1 s: hidden, green, slid half across (white | black), cut away.
    assert!(dark(at(&end, BLINK)), "hidden: {:?}", at(&end, BLINK));
    let green = at(&end, TINT);
    assert!(
        green[1] > 200.0 && green[0] < 30.0 && green[2] < 30.0,
        "green: {green:?}"
    );
    assert!(
        bright(left(&end)) && dark(right(&end)),
        "slid: {:?} | {:?}",
        left(&end),
        right(&end)
    );
    assert!(dark(at(&end, CUT)), "cut away: {:?}", at(&end, CUT));
    // Half way: still shown (a step), red and blue half of white in linear
    // light (about 186 as sRGB bytes), the cutoff at 0.5 keeping alpha 0.5.
    assert!(at(&half, BLINK)[0] > 200.0, "a step holds: {:?}", at(&half, BLINK));
    let mid = at(&half, TINT);
    assert!(
        (mid[0] - 186.0).abs() < 16.0 && mid[1] > 240.0,
        "half way to green: {mid:?}"
    );
}

#[test]
fn a_static_texture_transform_draws_as_moved_texture_coordinates() {
    let Some((transformed, ctx)) = draw("static.gltf", Pose::Rest) else {
        return;
    };
    let (shifted, _) = draw("shifted.gltf", Pose::Rest).unwrap();
    let (plain, _) = draw("plain.gltf", Pose::Rest).unwrap();
    let sides = |image: &image::RgbaImage| {
        (
            color(&ctx, image, SLIDE - 0.2, 0.0),
            color(&ctx, image, SLIDE + 0.2, 0.0),
        )
    };
    // The transform moves the texture: not the plain square (the control).
    let (l, r) = sides(&plain);
    assert!(dark(l) && bright(r), "plain: {l:?} | {r:?}");
    let (l, r) = sides(&transformed);
    assert!(bright(l) && dark(r), "transformed: {l:?} | {r:?}");
    // And the whole picture is the one the shifted coordinates draw.
    let different = transformed
        .pixels()
        .zip(shifted.pixels())
        .filter(|(a, b)| (0..3).any(|c| a[c].abs_diff(b[c]) > 2))
        .count();
    assert_eq!(different, 0, "pixels that differ from the shifted coordinates'");
}
