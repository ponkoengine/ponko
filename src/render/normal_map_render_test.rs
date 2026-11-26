//! Normal maps tilt a surface the way glTF says: the map's +X (red) along
//! the direction its texture coordinate u grows, +Y (green) up the image,
//! which is where v shrinks (glTF's v runs down the image), +Z (blue) out
//! of the surface. "OpenGL style", green up.
//!
//! Two checks, each lit only by the sun:
//!
//! - Flat squares with texture coordinates laid out three ways (as
//!   `MeshData::plane` has them, turned a quarter, and mirrored) and 1x1
//!   maps that lean 30 degrees toward +X or +Y: each square is brighter
//!   than a flat one when the sun stands on the side the map should lean
//!   to, and darker when it stands on the other side (the negative
//!   control: a map leaning the wrong way passes neither).
//! - The Khronos glTF sample models `NormalTangentTest` and
//!   `NormalTangentMirrorTest`: rows of plates, one with a real hemisphere
//!   and the others with a hemisphere drawn only in the normal map (with
//!   texture coordinates turned and mirrored, and in the second model,
//!   tangents given in the file, which the engine doesn't read). Every
//!   drawn hemisphere is lit on the same side as the real one. The models
//!   are fetched once, at a pinned revision and checksum, into the
//!   git-ignored cache of well-known content (`.cache/known-content`, or
//!   the folder `PONKO_KNOWN_CONTENT` names; the known-content fetch script
//!   fills the same cache); without them and without a network the tests
//!   are skipped, which is not a pass.
//!
//! Skips without a GPU (a skip is not a pass).
use super::gpu::GpuTexture;
use super::regression_stage::*;
use super::render_test_support::{Image, skip};
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::{MaterialDesc, MeshData, Model};
use crate::texture::Texture;

/// A linear RGBA8 texture of `width` x `height` with its mipmaps, in the
/// stage's texture list (normal maps are data, not colors).
fn data_texture(stage: &mut Stage, width: u32, height: u32, rgba: &[u8]) -> Texture {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = stage.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("normal map"),
        size,
        mip_level_count: super::mips::levels(width, height),
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    stage.queue.write_texture(
        texture.as_image_copy(),
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: None,
        },
        size,
    );
    let mut encoder = stage.device.create_command_encoder(&Default::default());
    super::mips::MipMaker::new(&stage.device).generate(&stage.device, &mut encoder, &texture, true);
    stage.queue.submit([encoder.finish()]);
    let id = stage.textures.len() as u32;
    let bind_group = stage.textures[0].bind_group.clone();
    stage.textures.push(GpuTexture {
        texture,
        nearest: false,
        mips: Some(true),
        bind_group,
    });
    Texture::new(id, width, height)
}

/// A normal map's texel for the unit vector `m` (tangent space).
fn texel(m: Vec3) -> [u8; 4] {
    let unorm = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
    [unorm(m.x), unorm(m.y), unorm(m.z), 255]
}

/// How a square's texture coordinates lie on it: the world directions
/// (along the ground) in which u and v grow.
#[derive(Clone, Copy, Debug)]
struct Layout {
    name: &'static str,
    u: Vec3,
    v: Vec3,
}

const LAYOUTS: [Layout; 3] = [
    // As `MeshData::plane` and the top of `MeshData::cube` have them: the
    // image lies on the ground as seen from above with -z up the screen.
    Layout {
        name: "plane",
        u: Vec3::X,
        v: Vec3::Z,
    },
    Layout {
        name: "turned a quarter",
        u: Vec3::NEG_Z,
        v: Vec3::X,
    },
    Layout {
        name: "mirrored",
        u: Vec3::NEG_X,
        v: Vec3::Z,
    },
];

/// A square 1 across on the ground facing up, centered on the origin,
/// with its texture coordinates laid out as `layout` says (0 to 1).
fn square(layout: Layout) -> MeshData {
    let corners = [
        vec3(-0.5, 0.0, 0.5),
        vec3(0.5, 0.0, 0.5),
        vec3(0.5, 0.0, -0.5),
        vec3(-0.5, 0.0, -0.5),
    ];
    MeshData {
        positions: corners.to_vec(),
        normals: vec![Vec3::Y; 4],
        uvs: corners
            .iter()
            .map(|p| vec2(p.dot(layout.u) + 0.5, p.dot(layout.v) + 0.5))
            .collect(),
        indices: vec![0, 1, 2, 0, 2, 3],
        ..Default::default()
    }
}

#[test]
fn a_normal_map_leans_the_surface_toward_u_for_red_and_up_the_image_for_green() {
    let Some(mut stage) = Stage::with(Setup {
        size: (256, 256),
        ..Setup::default()
    }) else {
        return;
    };
    let lean = 30f32.to_radians();
    let (sin, cos) = lean.sin_cos();
    // The flat square, then each layout with a red-up and a green-up map,
    // and where the map should lean each one: toward +u and toward -v.
    let flat = data_texture(&mut stage, 1, 1, &texel(Vec3::Z));
    let mut cases = Vec::new();
    for layout in LAYOUTS {
        for (map, m, toward) in [
            ("red up", vec3(sin, 0.0, cos), layout.u),
            ("green up", vec3(0.0, sin, cos), -layout.v),
        ] {
            cases.push((layout, map, data_texture(&mut stage, 1, 1, &texel(m)), toward));
        }
    }
    let mut squares = Vec::new();
    for (i, normal_map) in std::iter::once(flat).chain(cases.iter().map(|c| c.2)).enumerate() {
        let layout = if i == 0 { LAYOUTS[0] } else { cases[i - 1].0 };
        let mesh = stage.mesh(
            &square(layout),
            MaterialDesc {
                color: Color::WHITE,
                roughness: 1.0,
                metallic: 0.0,
                normal_map: Some(normal_map),
                ..Default::default()
            },
        );
        // Seven squares in a row, 1.5 apart.
        let at = vec3((i as f32 - 3.0) * 1.5, 0.0, 0.0);
        stage.ctx.world.spawn((Transform3D::at(at), Model::new(mesh)));
        squares.push(at);
    }
    stage.ctx.scene3d.lighting.ambient_intensity = 0.0;
    stage.ctx.scene3d.sky.sky_color = Color::BLACK;
    stage.ctx.scene3d.sky.ground_color = Color::BLACK;
    stage.ctx.scene3d.camera.position = vec3(0.0, 9.0, 3.0);
    stage.ctx.scene3d.camera.look_at(Vec3::ZERO);
    // The brightness of every square with the sun 45 degrees up on the
    // side `from` (along the ground).
    let mut lit = |from: Vec3| {
        stage.ctx.scene3d.lighting.sun_direction = -(from + Vec3::Y).normalize();
        let image = stage.settled(2);
        squares
            .iter()
            .map(|at| luma(stage.color_at(&image, *at)))
            .collect::<Vec<_>>()
    };
    let sides = [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z];
    let brightness: Vec<Vec<f32>> = sides.iter().map(|side| lit(*side)).collect();
    let under = |side: Vec3| &brightness[sides.iter().position(|s| *s == side).unwrap()];
    for (i, (layout, map, _, toward)) in cases.iter().enumerate() {
        let (near, far) = (under(*toward), under(-*toward));
        let (flat_near, mapped_near, flat_far, mapped_far) = (near[0], near[i + 1], far[0], far[i + 1]);
        eprintln!(
            "{} layout, {map}: should lean toward {toward}; sun on that side {mapped_near:.1} (flat {flat_near:.1}), \
             on the other side {mapped_far:.1} (flat {flat_far:.1})",
            layout.name
        );
        assert!(flat_near > 40.0, "the sun lights the flat square: {flat_near}");
        assert!(
            mapped_near > flat_near + 8.0,
            "{} layout, {map}: the square leans toward {toward}, so the sun on that side lights it more than a \
             flat one ({mapped_near} against {flat_near})",
            layout.name
        );
        assert!(
            mapped_far < flat_far - 8.0,
            "{} layout, {map}: the square leans away from {}, so the sun on that side lights it less than a flat \
             one ({mapped_far} against {flat_far})",
            layout.name,
            -*toward
        );
    }
    stage.finish();
}

// ---- The Khronos glTF sample models ----

/// The Khronos glTF Sample Assets revision the models are fetched at (the
/// one the known-content fetch script pins).
const SAMPLES_REV: &str = "e124e8d8d7330a5372bc2b4d00d39a4a9d7997ea";

/// The cache the known-content tool fills: `PONKO_KNOWN_CONTENT`, else
/// `.cache/known-content` in the checkout (git-ignored).
fn known_content() -> std::path::PathBuf {
    std::env::var_os("PONKO_KNOWN_CONTENT").map_or_else(
        || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".cache/known-content"),
        std::path::PathBuf::from,
    )
}

/// The sample model `name` (its binary glTF) from the cache, fetched into
/// it at the pinned revision if it isn't there yet. `size` and `checksum`
/// (FNV-1a) pin the file: anything else is an error, not a skip. `None`
/// (logged as a skip) when it can't be fetched.
fn sample_model(name: &str, size: usize, checksum: &str) -> Option<Vec<u8>> {
    let path = known_content().join(format!("samples/{name}/glTF-Binary/{name}.glb"));
    if !path.is_file() {
        let url = format!(
            "https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/{SAMPLES_REV}/Models/{name}/glTF-Binary/{name}.glb"
        );
        let partial = path.with_extension("glb.part");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let fetched = std::process::Command::new("curl")
            .args(["-fsSL", "--max-time", "120", "-o"])
            .arg(&partial)
            .arg(&url)
            .status();
        if !matches!(fetched, Ok(s) if s.success()) {
            let _ = std::fs::remove_file(&partial);
            skip(format!(
                "the sample model {name} isn't in {} and fetching {url} failed ({fetched:?}); put it \
                 there, or set PONKO_KNOWN_CONTENT to a cache that has it",
                known_content().display()
            ));
            return None;
        }
        std::fs::rename(&partial, &path).unwrap();
    }
    let bytes = std::fs::read(&path).unwrap();
    let sum = super::render_test_support::checksum(&bytes);
    assert!(
        bytes.len() == size && sum == checksum,
        "{} is not the pinned file ({} bytes, FNV-1a {sum}; expected {size}, {checksum}): delete it to fetch it again",
        path.display(),
        bytes.len()
    );
    Some(bytes)
}

/// A plate of a sample model: its center (on the model's front, z = 0) and
/// whether its hemisphere is real geometry (else it is in the normal map).
#[derive(Clone, Copy, Debug)]
struct Plate {
    center: Vec2,
    real: bool,
}

/// The model's plates: the connected pieces of its mesh 0.3 across (its
/// labels are narrower), a real hemisphere where one rises off the front.
fn plates(data: &MeshData) -> Vec<Plate> {
    // Corners at the same place are one corner (the file splits them where
    // normals or texture coordinates differ).
    let key = |p: Vec3| (p * 1e4).round().as_ivec3();
    let mut ids = std::collections::HashMap::new();
    let corner: Vec<usize> = data
        .positions
        .iter()
        .map(|p| {
            let n = ids.len();
            *ids.entry(key(*p)).or_insert(n)
        })
        .collect();
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for t in data.indices.chunks(3) {
        let a = root(&mut parent, corner[t[0] as usize]);
        for &i in &t[1..] {
            let b = root(&mut parent, corner[i as usize]);
            parent[b] = a;
        }
    }
    let mut pieces: std::collections::HashMap<usize, (Vec3, Vec3)> = std::collections::HashMap::new();
    for (i, p) in data.positions.iter().enumerate() {
        let r = root(&mut parent, corner[i]);
        let e = pieces.entry(r).or_insert((*p, *p));
        e.0 = e.0.min(*p);
        e.1 = e.1.max(*p);
    }
    let mut plates: Vec<Plate> = pieces
        .values()
        .filter(|(lo, hi)| ((hi.x - lo.x) - 0.3).abs() < 0.01 && ((hi.y - lo.y) - 0.3).abs() < 0.01)
        .map(|(lo, hi)| Plate {
            center: ((*lo + *hi) * 0.5).truncate(),
            real: hi.z > 0.05,
        })
        .collect();
    plates.sort_by(|a, b| {
        (-a.center.y, a.center.x)
            .partial_cmp(&(-b.center.y, b.center.x))
            .unwrap()
    });
    plates
}

/// Which way a plate's bump is lit on screen: from the centroid of the
/// pixels darker than the plate's flat part to the centroid of the
/// brighter ones (x right, y up, in pixels). Zero for a flat plate.
fn lit_side(stage: &Stage, image: &Image, center: Vec2) -> Vec2 {
    // Inside the plate's border, around its hemisphere (radius 0.09).
    let half = 0.12;
    let (x0, y0) = stage.pixel(center.extend(0.0) + vec3(-half, half, 0.0));
    let (x1, y1) = stage.pixel(center.extend(0.0) + vec3(half, -half, 0.0));
    let mut lumas = Vec::new();
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = image.pixel(x, y);
            lumas.push((x, y, luma([f32::from(p[0]), f32::from(p[1]), f32::from(p[2])])));
        }
    }
    let mut sorted: Vec<f32> = lumas.iter().map(|l| l.2).collect();
    sorted.sort_by(f32::total_cmp);
    let flat = sorted[sorted.len() / 2];
    let (mut bright, mut bright_w, mut dark, mut dark_w) = (Vec2::ZERO, 0.0, Vec2::ZERO, 0.0);
    for (x, y, l) in lumas {
        let at = vec2(x as f32, -(y as f32));
        let d = l - flat;
        if d > 2.0 {
            bright += at * d;
            bright_w += d;
        } else if d < -2.0 {
            dark -= at * d;
            dark_w -= d;
        }
    }
    if bright_w < 1.0 || dark_w < 1.0 {
        return Vec2::ZERO;
    }
    bright / bright_w - dark / dark_w
}

/// Draws the sample model `name` (pinned by `size` and `checksum`), white
/// and lit only by a sun from the upper right in front, and checks that
/// every hemisphere drawn by its normal map is lit on the side its row's
/// real hemisphere is.
fn sample_hemispheres_are_lit_as_the_real_ones(name: &str, size: usize, checksum: &str) {
    let Some(bytes) = sample_model(name, size, checksum) else {
        return;
    };
    let Some(mut stage) = Stage::with(Setup {
        size: (800, 640),
        ..Setup::default()
    }) else {
        return;
    };
    let loaded = crate::three::gltf::load(name, &bytes, &mut |f| Err(format!("no {f}")), &|_| false).unwrap();
    let material = &loaded.materials[loaded.parts[0].1];
    let (width, height, rgba) = &loaded.images[material.normal_map.expect("the model has a normal map")];
    let normal_map = data_texture(&mut stage, *width, *height, rgba);
    let plates = plates(&loaded.data);
    let real = plates.iter().filter(|p| p.real).count();
    assert!(
        real >= 10 && plates.len() >= 2 * real,
        "{name}: {} plates, {real} with a real hemisphere",
        plates.len()
    );
    let desc = |normal_map| MaterialDesc {
        color: Color::WHITE,
        roughness: 0.6,
        metallic: 0.0,
        normal_map,
        double_sided: true,
        ..Default::default()
    };
    let mapped = stage.mesh(&loaded.data, desc(Some(normal_map)));
    let bare = stage.mesh(&loaded.data, desc(None));
    let model = stage.ctx.world.spawn((Transform3D::default(), Model::new(mapped)));
    stage.ctx.scene3d.lighting.ambient_intensity = 0.1;
    stage.ctx.scene3d.lighting.sun_direction = -vec3(1.0, 1.0, 1.0).normalize();
    stage.ctx.scene3d.camera.position = vec3(0.0, -0.08, 2.6);
    stage.ctx.scene3d.camera.look_at(vec3(0.0, -0.08, 0.0));
    let image = stage.settled(2);
    // The same model without its normal map: the drawn hemispheres are
    // gone (the negative control of the measure).
    stage.ctx.world.insert(model, Model::new(bare));
    let without = stage.settled(2);
    let mut checked = 0;
    for plate in plates.iter().filter(|p| !p.real) {
        // The real hemisphere of its row: the nearest on its left.
        let real = plates
            .iter()
            .filter(|r| r.real && (r.center.y - plate.center.y).abs() < 0.01 && r.center.x < plate.center.x)
            .max_by(|a, b| a.center.x.total_cmp(&b.center.x))
            .expect("every row starts with a real hemisphere");
        let (expected, drawn) = (
            lit_side(&stage, &image, real.center),
            lit_side(&stage, &image, plate.center),
        );
        let bare = lit_side(&stage, &without, plate.center);
        let agreement = expected.normalize_or_zero().dot(drawn.normalize_or_zero());
        eprintln!(
            "{name}: plate at {:.2}: lit toward {drawn:.1} px, its row's real hemisphere toward {expected:.1} px \
             (agreement {agreement:.2}); without the normal map {bare:.1} px",
            plate.center
        );
        assert!(
            expected.x > 2.0 && expected.y > 2.0,
            "{name}: the real hemisphere at {:.2} is lit from the upper right: {expected}",
            real.center
        );
        assert!(
            bare.length() < 0.25 * expected.length(),
            "{name}: without the normal map the plate at {:.2} is flat: {bare}",
            plate.center
        );
        assert!(
            agreement > 0.8 && drawn.length() > 0.4 * expected.length(),
            "{name}: the hemisphere the normal map draws at {:.2} is lit toward {drawn}, the real one toward \
             {expected}",
            plate.center
        );
        checked += 1;
    }
    assert!(checked >= 10, "{name}: {checked} drawn hemispheres checked");
    stage.finish();
}

#[test]
fn the_khronos_normal_tangent_test_lights_drawn_hemispheres_as_real_ones() {
    sample_hemispheres_are_lit_as_the_real_ones("NormalTangentTest", 1_796_996, "91126674e08c420d");
}

#[test]
fn the_khronos_normal_tangent_mirror_test_lights_drawn_hemispheres_as_real_ones() {
    sample_hemispheres_are_lit_as_the_real_ones("NormalTangentMirrorTest", 1_567_084, "aeca454737add90c");
}
