use super::*;
use crate::three::Transform3D;

/// A box from `lo` to `hi`, its own lightmap coordinates at `tpu`.
fn block(lo: Vec3, hi: Vec3, tpu: f32) -> (MeshData, Mat4) {
    let size = hi - lo;
    let mut mesh = MeshData::cube(1.0);
    // Coordinates for the box as it stands: generated on the scaled mesh,
    // then the mesh is scaled back (the coordinates stay).
    for p in &mut mesh.positions {
        *p *= size;
    }
    mesh.generate_lightmap_uvs(tpu);
    for p in &mut mesh.positions {
        *p /= size;
    }
    let t = Transform3D {
        position: (lo + hi) * 0.5,
        scale: size,
        ..Default::default()
    };
    (mesh, t.matrix())
}

fn gray_sky(level: f32, sun: f32) -> LightBakeScene {
    let lighting = Lighting {
        sun_intensity: sun,
        sun_direction: Vec3::new(-1.0, -1.0, 0.0),
        ambient_intensity: 1.0,
        ..Default::default()
    };
    let sky = Sky {
        sky_color: Color::rgb(level, level, level),
        ground_color: Color::rgb(level, level, level),
        equator_color: None,
        ..Default::default()
    };
    LightBakeScene::new(&lighting, &sky)
}

fn settings() -> LightBakeSettings {
    LightBakeSettings::default()
        .with_samples(24)
        .with_texels_per_unit(2.0)
        .with_page_size(128)
        .with_probe_spacing(0.0)
}

/// The baked light at world point `p` of model `index` (by its texel
/// nearest `p`, searched over the rectangle).
fn light_near(baked: &BakedLighting, scene: &LightBakeScene, index: usize, p: Vec3) -> Vec3 {
    let model = &scene.models[index];
    let rect = baked.rect(index).unwrap();
    let mut best = (f32::INFINITY, Vec3::ZERO);
    for [a, b, c] in model.triangles() {
        let q = [a, b, c].map(|i| model.uvs1[i]);
        for k in 0..64 {
            let w1 = (k % 8) as f32 / 8.0 + 1.0 / 16.0;
            let w2 = (k / 8) as f32 / 8.0 + 1.0 / 16.0;
            if w1 + w2 > 1.0 {
                continue;
            }
            let w0 = 1.0 - w1 - w2;
            let world = model.positions[a] * w0 + model.positions[b] * w1 + model.positions[c] * w2;
            let d = world.distance(p);
            if d < best.0 {
                let uv = q[0] * w0 + q[1] * w1 + q[2] * w2;
                let tx = rect.x + ((uv.x * rect.width as f32) as u32).min(rect.width - 1);
                let ty = rect.y + ((uv.y * rect.height as f32) as u32).min(rect.height - 1);
                best = (d, baked.texel(rect.page, tx, ty).unwrap().0);
            }
        }
    }
    best.1
}

/// A closed room (inside -2..2 across, 0..3 high, walls 0.3 thick), its
/// west wall red; with `opening`, a hole in the roof the sun shines in by.
fn room(opening: bool, scene: &mut LightBakeScene, tpu: f32) -> Vec<usize> {
    let (i, o) = (2.0, 2.3);
    let mut blocks = vec![
        (Color::WHITE, Vec3::new(-o, -0.3, -o), Vec3::new(o, 0.0, o)),
        (
            Color::rgb(0.9, 0.05, 0.05),
            Vec3::new(-o, 0.0, -o),
            Vec3::new(-i, 3.0, o),
        ),
        (Color::WHITE, Vec3::new(i, 0.0, -o), Vec3::new(o, 3.0, o)),
        (Color::WHITE, Vec3::new(-i, 0.0, -o), Vec3::new(i, 3.0, -i)),
        (Color::WHITE, Vec3::new(-i, 0.0, i), Vec3::new(i, 3.0, o)),
    ];
    if opening {
        blocks.push((Color::WHITE, Vec3::new(-o, 3.0, -o), Vec3::new(-1.0, 3.3, o)));
        blocks.push((Color::WHITE, Vec3::new(0.5, 3.0, -o), Vec3::new(o, 3.3, o)));
        blocks.push((Color::WHITE, Vec3::new(-1.0, 3.0, -o), Vec3::new(0.5, 3.3, -1.0)));
        blocks.push((Color::WHITE, Vec3::new(-1.0, 3.0, 1.0), Vec3::new(0.5, 3.3, o)));
    } else {
        blocks.push((Color::WHITE, Vec3::new(-o, 3.0, -o), Vec3::new(o, 3.3, o)));
    }
    blocks
        .into_iter()
        .map(|(color, lo, hi)| {
            let (mesh, m) = block(lo, hi, tpu);
            scene.add(BakeModel::new(&mesh, m).with_albedo(color))
        })
        .collect()
}

#[test]
fn a_closed_room_bakes_dark_inside_however_bright_outside() {
    let mut scene = gray_sky(1.0, 10.0);
    let ids = room(false, &mut scene, 2.0);
    let baked = bake_lighting(&scene, &settings()).unwrap();
    for p in [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.5, 0.0, 1.5),
        Vec3::new(-1.9, 0.0, 0.0),
    ] {
        let l = light_near(&baked, &scene, ids[0], p);
        assert!(l.max_element() < 1e-3, "light leaked into the room at {p}: {l}");
    }
    // The roof's top, outside, takes the sky's light (white: 1).
    let top = light_near(&baked, &scene, ids[5], Vec3::new(0.0, 3.3, 0.0));
    assert!(top.min_element() > 0.9, "{top}");
}

#[test]
fn bounced_light_tints_the_floor_beside_a_red_wall() {
    let mut scene = gray_sky(0.2, 6.0);
    let ids = room(true, &mut scene, 2.0);
    let baked = bake_lighting(&scene, &settings()).unwrap();
    let floor = light_near(&baked, &scene, ids[0], Vec3::new(-1.6, 0.0, 0.0));
    assert!(
        floor.x > floor.y * 1.3,
        "the floor beside the sunlit red wall is red: {floor}"
    );
    // The east wall, across the room, takes some bounced light too.
    let east = light_near(&baked, &scene, ids[2], Vec3::new(2.0, 1.5, 0.0));
    assert!(east.max_element() > 0.01, "{east}");
}

#[test]
fn bakes_are_the_same_on_any_number_of_threads() {
    let mut scene = gray_sky(0.4, 4.0);
    room(true, &mut scene, 1.5);
    let s = settings()
        .with_texels_per_unit(1.5)
        .with_samples(8)
        .with_probe_spacing(1.5);
    let bake = |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| bake_lighting(&scene, &s).unwrap())
    };
    let (one, four) = (bake(1), bake(4));
    assert_eq!(one.to_bytes(), four.to_bytes());
}

#[test]
fn every_texel_a_lightmap_lookup_reads_is_baked() {
    let mut scene = gray_sky(0.5, 3.0);
    let ids = room(true, &mut scene, 2.0);
    let mut ball = MeshData::sphere(0.6, 12);
    ball.generate_lightmap_uvs(2.0);
    let ball_id = scene.add(BakeModel::new(&ball, Mat4::from_translation(Vec3::new(0.0, 0.6, 0.0))));
    let baked = bake_lighting(&scene, &settings()).unwrap();
    let mut checked = 0;
    for &m in ids.iter().chain([&ball_id]) {
        let model = &scene.models[m];
        let rect = baked.rect(m).unwrap();
        for [a, b, c] in model.triangles() {
            let q = [a, b, c].map(|i| model.uvs1[i] * Vec2::new(rect.width as f32, rect.height as f32));
            // The corners, the edges' middles and the middle: what the GPU's
            // bilinear filter reads there (the four texels around each).
            let points = [
                q[0],
                q[1],
                q[2],
                (q[0] + q[1]) * 0.5,
                (q[1] + q[2]) * 0.5,
                (q[2] + q[0]) * 0.5,
                (q[0] + q[1] + q[2]) / 3.0,
            ];
            for p in points {
                let p = p.clamp(Vec2::splat(0.5), Vec2::new(rect.width as f32, rect.height as f32) - 0.5);
                let base = (p - 0.5).floor();
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let x = rect.x + (base.x as u32 + dx).min(rect.width - 1);
                    let y = rect.y + (base.y as u32 + dy).min(rect.height - 1);
                    let (light, baked_here) = baked.texel(rect.page, x, y).unwrap();
                    assert!(baked_here, "model {m}: texel ({x}, {y}) is read but was never baked");
                    assert!(light.is_finite());
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 1000);
    let stats = baked.stats();
    assert!(stats.texels > 0 && stats.rays > stats.texels * 24);
    assert!(stats.inside_texels > 0, "texels under the walls are found inside them");
}

#[test]
fn an_open_floor_takes_the_skys_light_and_white_sky_bounces_nothing_extra() {
    let mut scene = gray_sky(1.0, 0.0);
    let mut floor = MeshData::plane(6.0);
    floor.generate_lightmap_uvs(2.0);
    let f = scene.add(BakeModel::new(&floor, Mat4::IDENTITY));
    let baked = bake_lighting(&scene, &settings().with_probe_spacing(1.0)).unwrap();
    let l = light_near(&baked, &scene, f, Vec3::ZERO);
    assert!((l - Vec3::ONE).abs().max_element() < 0.02, "{l}");
    // A probe above the floor: the sky above, the floor (white, lit by the
    // white sky: as bright) below; all 1 within the probes' accuracy.
    for n in [Vec3::Y, Vec3::NEG_Y, Vec3::X] {
        let p = baked.probe_light(Vec3::new(0.3, 0.7, -0.2), n).unwrap();
        assert!((p - Vec3::ONE).abs().max_element() < 0.15, "{n}: {p}");
    }
    assert!(baked.probe_light(Vec3::new(50.0, 0.0, 0.0), Vec3::Y).is_none());
}

#[test]
fn probes_see_which_way_the_light_comes_from() {
    // A red floor under a black sky, lit by a sun from straight above:
    // the probes' light comes up from the floor.
    let lighting = Lighting {
        sun_direction: Vec3::NEG_Y,
        sun_intensity: 3.0,
        ..Default::default()
    };
    let sky = Sky {
        sky_color: Color::BLACK,
        ground_color: Color::BLACK,
        equator_color: None,
        ..Default::default()
    };
    let mut scene = LightBakeScene::new(&lighting, &sky);
    let mut floor = MeshData::plane(8.0);
    floor.generate_lightmap_uvs(1.0);
    scene.add(BakeModel::new(&floor, Mat4::IDENTITY).with_albedo(Color::rgb(1.0, 0.0, 0.0)));
    let baked = bake_lighting(&scene, &settings().with_probe_spacing(1.0)).unwrap();
    let down = baked.probe_light(Vec3::new(0.0, 0.5, 0.0), Vec3::NEG_Y).unwrap();
    let up = baked.probe_light(Vec3::new(0.0, 0.5, 0.0), Vec3::Y).unwrap();
    assert!(down.x > 0.5 && down.y < 1e-3, "{down}");
    assert!(up.x < down.x * 0.25, "facing away from the floor: {up} vs {down}");
}

#[test]
fn a_lightmapped_model_needs_lightmap_coordinates() {
    let mut scene = gray_sky(1.0, 1.0);
    scene.add(BakeModel::new(&MeshData::cube(1.0), Mat4::IDENTITY));
    let err = bake_lighting(&scene, &settings()).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Invalid);
    assert!(err.to_string().contains("generate_lightmap_uvs"), "{err}");
    // As an occluder alone it needs none.
    let mut scene = gray_sky(1.0, 1.0);
    let i = scene.add(BakeModel::new(&MeshData::cube(1.0), Mat4::IDENTITY).with_lightmapped(false));
    let baked = bake_lighting(&scene, &settings()).unwrap();
    assert!(baked.lightmap(i).is_none());
    assert_eq!(baked.pages(), 0);
    assert!(bake_lighting(&scene, &settings().with_page_size(16)).is_err());
}

#[test]
fn baked_lighting_survives_bytes() {
    let mut scene = gray_sky(0.4, 4.0);
    room(true, &mut scene, 1.0);
    let baked = bake_lighting(
        &scene,
        &settings()
            .with_texels_per_unit(1.0)
            .with_samples(4)
            .with_probe_spacing(2.0),
    )
    .unwrap();
    let mut back = BakedLighting::from_bytes(&baked.to_bytes()).unwrap();
    back.stats = baked.stats;
    assert_eq!(back, baked);
    let bytes = baked.to_bytes();
    assert_eq!(
        BakedLighting::from_bytes(&bytes[..bytes.len() - 1]).unwrap_err().kind(),
        ErrorKind::Decode
    );
}

#[test]
fn files_of_version_1_still_load() {
    let mut scene = gray_sky(0.4, 4.0);
    room(true, &mut scene, 1.0);
    let baked = bake_lighting(
        &scene,
        &settings()
            .with_texels_per_unit(1.0)
            .with_samples(4)
            .with_probe_spacing(2.0)
            .with_compress(false),
    )
    .unwrap();
    let mut back = BakedLighting::from_bytes(&baked.to_bytes_v1()).unwrap();
    back.stats = baked.stats;
    assert_eq!(back, baked);
    // A later version is refused.
    let mut later = baked.to_bytes();
    later[8] = 3;
    assert_eq!(BakedLighting::from_bytes(&later).unwrap_err().kind(), ErrorKind::Decode);
}

#[test]
fn compressed_bakes_take_an_eighth_of_the_space_and_stay_within_two_percent() {
    let mut scene = gray_sky(0.4, 4.0);
    room(true, &mut scene, 1.0);
    let plain = bake_lighting(
        &scene,
        &settings()
            .with_texels_per_unit(4.0)
            .with_samples(16)
            .with_probe_spacing(2.0)
            .with_compress(false),
    )
    .unwrap();
    let packed = plain.clone().compress();
    assert!(!plain.is_compressed() && packed.is_compressed());
    // Compressing again changes nothing, and a compressed bake survives
    // bytes exactly.
    assert_eq!(packed.clone().compress(), packed);
    let mut back = BakedLighting::from_bytes(&packed.to_bytes()).unwrap();
    back.stats = packed.stats;
    assert_eq!(back, packed);
    // The pages: a byte a texel on disk (with a bit for which are baked)
    // instead of 8, and on a GPU with BC6H.
    let side = u64::from(plain.page_size());
    let pages = 2 * u64::from(plain.pages());
    let (v1, disk_plain, disk_packed) = (
        plain.to_bytes_v1().len() as u64,
        plain.to_bytes().len() as u64,
        packed.to_bytes().len() as u64,
    );
    assert!(
        disk_packed * 8 <= v1 && disk_packed * 2 < disk_plain,
        "on disk: {disk_packed} compressed, {disk_plain} not, {v1} in version 1 ({pages} pages of {side})"
    );
    let (a, b) = (plain.gpu_layers().unwrap(), packed.gpu_layers().unwrap());
    use super::PageFormat;
    let header = a.bytes(PageFormat::Half) - pages.max(2) * side * side * 8;
    assert_eq!(b.bytes(PageFormat::Bc6h), header + pages.max(2) * side * side);
    assert_eq!(
        b.bytes(PageFormat::SharedExponent),
        header + pages.max(2) * side * side * 4
    );
    // The light it holds: within 2 % on average (relative mean absolute).
    let (mut diff, mut total) = (0.0, 0.0);
    for page in 0..plain.pages() {
        for y in 0..plain.page_size() {
            for x in 0..plain.page_size() {
                let (l, baked) = plain.texel(page, x, y).unwrap();
                let (m, kept) = packed.texel(page, x, y).unwrap();
                assert_eq!(baked, kept);
                if baked {
                    diff += (l - m).abs().element_sum();
                    total += l.element_sum();
                }
            }
        }
    }
    let error = diff / total;
    assert!(error < 0.02, "{error}");
    assert!(
        plain.direction_error(&packed) < 0.02,
        "{}",
        plain.direction_error(&packed)
    );
    // A file whose data claims to unpack to more than any bake is refused
    // before it unpacks.
    let mut huge = packed.to_bytes();
    huge[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(BakedLighting::from_bytes(&huge).unwrap_err().kind(), ErrorKind::Decode);
}

#[test]
fn the_gpu_layers_start_with_the_header_and_the_rectangle_table() {
    let mut scene = gray_sky(0.4, 0.0);
    let ids = room(true, &mut scene, 1.0);
    let baked = bake_lighting(
        &scene,
        &settings()
            .with_texels_per_unit(1.0)
            .with_samples(4)
            .with_probe_spacing(2.0),
    )
    .unwrap();
    let layers = baked.gpu_layers().unwrap();
    assert_eq!(layers.pages.len() as u32, 2 * baked.pages());
    let side = baked.page_size() as usize;
    assert!(layers.pages.iter().all(|l| l.len() == side * side * 4));
    let (header, rows) = (layers.header_side as usize, layers.header_rows as usize);
    assert!(rows <= header);
    assert_eq!(layers.header.len(), header * rows * 4);
    let texel = |i: usize| -> [f32; 4] { std::array::from_fn(|k| f16_to_f32(layers.header[i * 4 + k])) };
    assert_eq!(texel(0), [-1.0, baked.pages() as f32, 1.0, 1.0]);
    assert_eq!(texel(4), [side as f32, 2.0 * baked.pages() as f32, 0.0, 0.0]);
    let r = baked.rect(ids[3]).unwrap();
    let at = HEADER_TEXELS as usize + 2 * ids[3];
    assert_eq!(texel(at), [r.x as f32, r.y as f32, r.width as f32, r.height as f32]);
    assert_eq!(texel(at + 1)[0], r.page as f32);
    // The probe grid's origin, from its two parts.
    let origin = Vec3::from_slice(&texel(1)[..3]) + Vec3::from_slice(&texel(2)[..3]);
    assert!((origin - baked.probes.as_ref().unwrap().origin).abs().max_element() < 1e-3);
    // Compressed: the two-layer header, and a byte a texel for the pages
    // (two layers at least).
    assert!(baked.is_compressed());
    let pages = (2 * baked.pages()).max(2) as usize;
    assert_eq!(baked.gpu_bytes(), (header * rows * 8 * 2 + side * side * pages) as u64);
}

#[test]
fn half_floats_round_to_nearest() {
    for v in [0.0f32, 1.0, -1.0, 0.5, 1000.25, 65504.0, 1e-5, 2048.0, 2.345_678_9] {
        let back = f16_to_f32(f32_to_f16(v));
        assert!((back - v).abs() <= v.abs() * 1e-3 + 1e-7, "{v} -> {back}");
    }
    assert_eq!(f32_to_f16(1.0), 0x3c00);
    assert_eq!(f32_to_f16(-2.0), 0xc000);
    assert!(f16_to_f32(f32_to_f16(1e6)).is_infinite());
    for i in 0..=2048 {
        assert_eq!(
            f16_to_f32(f32_to_f16(i as f32)),
            i as f32,
            "whole numbers to 2048 are exact"
        );
    }
}

#[test]
fn each_texel_bakes_the_spot_its_lightmap_coordinates_point_at() {
    // A flat floor: its texels form an even grid on it, each texel one
    // step from the next.
    let mut floor = MeshData::plane(6.0);
    floor.generate_lightmap_uvs(4.0);
    let model = BakeModel::new(&floor, Mat4::from_translation(Vec3::new(1.0, 0.5, -2.0)));
    let side = 64;
    let rect = LightmapRect {
        page: 0,
        x: 1,
        y: 1,
        width: 40,
        height: 40,
    };
    let mut page = vec![None; (side * side) as usize];
    for [a, b, c] in model.triangles() {
        rasterize(&model, [a, b, c], Vec3::Y, 0, rect, side, &mut page);
    }
    let at = |x: u32, y: u32| {
        page[(y * side + x) as usize]
            .filter(|t: &Texel| t.exact)
            .map(|t| t.position)
    };
    let (mut steps, mut checked) = (Vec::new(), 0);
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width - 1 {
            if let (Some(p), Some(q)) = (at(x, y), at(x + 1, y)) {
                assert!((p.y - 0.5).abs() < 1e-5, "on the floor: {p}");
                steps.push(q - p);
                checked += 1;
            }
        }
    }
    assert!(checked > 500, "{checked}");
    let first = steps[0];
    assert!(first.length() > 0.05, "texels apart: {first}");
    for s in &steps {
        assert!((*s - first).length() < 1e-3, "an even grid: {s} vs {first}");
    }
}

#[test]
fn a_small_bake_takes_the_smallest_page_that_holds_it() {
    let mut scene = gray_sky(1.0, 0.0);
    let mut floor = MeshData::plane(4.0);
    floor.generate_lightmap_uvs(4.0);
    scene.add(BakeModel::new(&floor, Mat4::IDENTITY));
    let small = settings().with_page_size(1024).with_samples(4);
    // 16 x 16 texels of floor: the smallest page there is.
    let baked = bake_lighting(&scene, &small).unwrap();
    assert_eq!((baked.page_size(), baked.pages()), (64, 1));
    // Too many texels for 128: 256, still below the largest.
    let baked = bake_lighting(&scene, &small.with_texels_per_unit(40.0)).unwrap();
    assert_eq!((baked.page_size(), baked.pages()), (256, 1));
    // More than the largest page holds: pages of the largest (a model
    // larger than a page shrinks to fit one).
    scene.add(BakeModel::new(&floor, Mat4::from_translation(Vec3::new(5.0, 0.0, 0.0))));
    let big = small.with_texels_per_unit(40.0).with_page_size(128);
    let baked = bake_lighting(&scene, &big).unwrap();
    assert_eq!((baked.page_size(), baked.pages()), (128, 2));
    assert_eq!(baked.rect(0).unwrap().width, 126);
}

/// The room of [`room`] (with the opening) as entities drawn with meshes
/// made while `gfx` keeps bake sources, a level of detail group, an
/// instance batch of crates, a lamp and a ball that moves ([`Unbaked`]);
/// and the same listed by hand, in the order the world bake adds them.
fn world_room(ctx: &mut crate::ctx::Ctx, tpu: f32) -> (LightBakeScene, Vec<ponko_ecs::Entity>) {
    use crate::render::{InstanceBatchOptions, LodLevel, LodRenderer, Model};
    let mut hand = LightBakeScene::new(&ctx.scene3d.lighting, &ctx.scene3d.sky);
    let (i, o) = (2.0, 2.3);
    let white = Color::rgb(0.8, 0.8, 0.8);
    let red = Color::rgb(0.8, 0.1, 0.1);
    let boxes = [
        (white, Vec3::new(-o, -0.3, -o), Vec3::new(o, 0.0, o)),
        (red, Vec3::new(-o, 0.0, -o), Vec3::new(-i, 3.0, o)),
        (white, Vec3::new(i, 0.0, -o), Vec3::new(o, 3.0, o)),
        (white, Vec3::new(-i, 0.0, -o), Vec3::new(i, 3.0, -i)),
        (white, Vec3::new(-i, 0.0, i), Vec3::new(i, 3.0, o)),
        (white, Vec3::new(-o, 3.0, -o), Vec3::new(0.5, 3.3, o)),
    ];
    let mut entities = Vec::new();
    for (color, lo, hi) in boxes {
        let (mesh, m) = block(lo, hi, tpu);
        let material = ctx.gfx.create_material(MaterialDesc::color(color));
        let handle = ctx.gfx.create_mesh(&mesh, material);
        let size = hi - lo;
        let t = Transform3D {
            position: (lo + hi) * 0.5,
            scale: size,
            ..Default::default()
        };
        entities.push(ctx.world.spawn((t, Model::new(handle))));
        hand.add(BakeModel::new(&mesh, m).with_albedo(color));
    }
    // A column without lightmap coordinates: it only blocks and bounces.
    let column = MeshData::cube(0.4);
    let stone = ctx.gfx.create_material(MaterialDesc::color(white));
    let handle = ctx.gfx.create_mesh(&column, stone);
    let at = Transform3D::at(Vec3::new(1.0, 0.2, 1.0));
    entities.push(ctx.world.spawn((at, Model::new(handle))));
    hand.add(
        BakeModel::new(&column, at.matrix())
            .with_albedo(white)
            .with_lightmapped(false),
    );
    // A crate as a level of detail group: its first level is baked.
    let (near, _) = block(Vec3::splat(-0.3), Vec3::splat(0.3), tpu);
    let near_mesh = ctx.gfx.create_mesh(&near, stone);
    let far_mesh = ctx.gfx.create_mesh(&MeshData::cube(0.6), stone);
    let offset = Transform3D::at(Vec3::new(0.0, 0.3, 0.0));
    let group = crate::render::LodGroup::new(
        0.6,
        vec![
            LodLevel::new(0.2, vec![LodRenderer::new(near_mesh).with_offset(offset)]),
            LodLevel::new(0.05, vec![LodRenderer::new(far_mesh).with_offset(offset)]),
        ],
    );
    let place = Transform3D::at(Vec3::new(-1.0, 0.0, -1.0));
    entities.push(ctx.world.spawn((place, group)));
    hand.add(BakeModel::new(&near, place.matrix() * offset.matrix()).with_albedo(white));
    // Crates in an instance batch: they block and bounce.
    let small = MeshData::cube(0.3);
    let small_mesh = ctx.gfx.create_mesh(&small, stone);
    let crates = [
        Mat4::from_translation(Vec3::new(1.5, 0.15, -1.5)),
        Mat4::from_translation(Vec3::new(1.5, 0.15, -1.0)),
    ];
    ctx.gfx
        .create_instance_batch(small_mesh, &crates, InstanceBatchOptions::default());
    for m in crates {
        hand.add(BakeModel::new(&small, m).with_albedo(white).with_lightmapped(false));
    }
    // A ball that rolls around: lit by the probes, not baked.
    let ball = ctx.gfx.create_mesh(&MeshData::sphere(0.3, 12), stone);
    ctx.world
        .spawn((Transform3D::at(Vec3::new(0.0, 0.3, 0.0)), Model::new(ball), Unbaked));
    let lamp = PointLight3D {
        intensity: 4.0,
        range: 6.0,
        ..Default::default()
    };
    ctx.world.spawn((Transform3D::at(Vec3::new(0.0, 2.5, 0.0)), lamp));
    hand.add_lights_from(&mut ctx.world);
    (hand, entities)
}

#[test]
fn a_world_bake_bakes_the_same_as_its_models_listed_by_hand() {
    crate::game::Game::new("world bake", 1, 1).run_headless(0, |ctx| {
        ctx.scene3d.lighting.sun_direction = Vec3::new(-1.0, -1.0, 0.0);
        ctx.gfx.keep_bake_sources(true);
        let (hand, entities) = world_room(ctx, 2.0);
        let mut scene = LightBakeScene::new(&ctx.scene3d.lighting, &ctx.scene3d.sky);
        let added = scene.add_world(&mut ctx.world, &ctx.gfx).unwrap();
        assert!(added.skipped().is_empty());
        assert_eq!(
            scene.len(),
            hand.len(),
            "every model, the group's first level and each crate"
        );
        assert_eq!(scene.lamps.len(), 1);
        // The boxes and the group get lightmaps; the column has no
        // lightmap coordinates and the ball moves.
        let with_lightmaps: Vec<_> = added.entries().iter().map(|e| e.0).collect();
        let expected: Vec<_> = entities.iter().copied().filter(|&e| e != entities[6]).collect();
        assert_eq!(with_lightmaps, expected);
        let settings = settings().with_samples(8).with_probe_spacing(2.0);
        let from_world = bake_lighting(&scene, &settings).unwrap();
        let by_hand = bake_lighting(&hand, &settings).unwrap();
        assert!(
            from_world.to_bytes() == by_hand.to_bytes(),
            "the world's models bake as the same models listed by hand"
        );
        assert_eq!(added.insert_lightmaps(&from_world, &mut ctx.world), 7);
        let floor = ctx.world.get::<Lightmapped>(entities[0]).copied();
        assert_eq!(floor, from_world.lightmap(0));
        assert_eq!(
            ctx.world.get::<Lightmapped>(entities[7]).copied(),
            Some(Lightmapped(7)),
            "the group's entry is its first renderer's"
        );
    });
}

#[test]
fn a_world_bake_needs_the_meshes_kept_and_skips_the_ones_made_before() {
    crate::game::Game::new("world bake", 1, 1).run_headless(0, |ctx| {
        let mut scene = LightBakeScene::new(&ctx.scene3d.lighting, &ctx.scene3d.sky);
        let err = scene.add_world(&mut ctx.world, &ctx.gfx).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Invalid);
        assert!(err.to_string().contains("keep_bake_sources(true)"), "{err}");
        let early = ctx.gfx.create_plain_mesh(&MeshData::cube(1.0));
        let early = ctx
            .world
            .spawn((Transform3D::default(), crate::render::Model::new(early)));
        ctx.gfx.keep_bake_sources(true);
        let added = scene.add_world(&mut ctx.world, &ctx.gfx).unwrap();
        assert_eq!(added.skipped(), &[early][..]);
        assert!(scene.is_empty());
        // Off again: the copies are dropped.
        ctx.gfx.keep_bake_sources(false);
        assert!(ctx.gfx.assets3d.bake_sources.is_none());
    });
}

/// A glTF file in the engine's `assets/` folder, deleted when dropped.
struct AssetFile(String);

impl AssetFile {
    fn new(stem: &str, ext: &str, bytes: &[u8]) -> Self {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = format!("test-{}-{n}-{stem}.{ext}", std::process::id());
        std::fs::write(Self::path(&name), bytes).expect("the test writes its asset");
        Self(name)
    }

    fn path(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(name)
    }
}

impl Drop for AssetFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(Self::path(&self.0));
    }
}

/// A glTF level: a 4 x 4 floor with lightmap coordinates (`TEXCOORD_1`), a
/// red pillar on it without them, and a spawn point with a mesh (not
/// level geometry). Returns the `.gltf` and its buffer.
fn gltf_level() -> (AssetFile, AssetFile) {
    let mut bin = Vec::new();
    let floor = [
        [-2.0f32, 0.0, -2.0],
        [2.0, 0.0, -2.0],
        [2.0, 0.0, 2.0],
        [-2.0, 0.0, 2.0],
    ];
    let uv1 = [[0.0f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let floor_indices: [u16; 6] = [0, 2, 1, 0, 3, 2];
    let (lo, hi) = ([-0.3f32, 0.0, -0.3], [0.3f32, 2.0, 0.3]);
    let pillar: Vec<[f32; 3]> = (0..8)
        .map(|k| {
            [
                if k & 1 == 0 { lo[0] } else { hi[0] },
                if k & 2 == 0 { lo[1] } else { hi[1] },
                if k & 4 == 0 { lo[2] } else { hi[2] },
            ]
        })
        .collect();
    // Corners k: bit 0 x, bit 1 y, bit 2 z; outward faces.
    let pillar_indices: [u16; 36] = [
        0, 2, 3, 0, 3, 1, 4, 5, 7, 4, 7, 6, 0, 1, 5, 0, 5, 4, 2, 6, 7, 2, 7, 3, 0, 4, 6, 0, 6, 2, 1, 3, 7, 1, 7, 5,
    ];
    for v in floor
        .iter()
        .flatten()
        .chain(uv1.iter().flatten())
        .chain(pillar.iter().flatten())
    {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in floor_indices.iter().chain(&pillar_indices) {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let bin_file = AssetFile::new("level", "bin", &bin);
    let json = serde_json::json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": [0, 1, 2] }],
        "nodes": [
            { "name": "Floor", "mesh": 0 },
            { "name": "Pillar", "mesh": 1, "translation": [0.5, 0.0, 0.0] },
            { "name": "Start", "mesh": 1, "translation": [1.5, 0.0, 1.5], "extras": { "ponko:spawn": true } }
        ],
        "materials": [
            { "pbrMetallicRoughness": { "baseColorFactor": [0.8, 0.8, 0.8, 1.0] } },
            { "pbrMetallicRoughness": { "baseColorFactor": [0.9, 0.05, 0.05, 1.0] } }
        ],
        "meshes": [
            { "primitives": [{ "attributes": { "POSITION": 0, "TEXCOORD_1": 1 }, "indices": 3, "material": 0 }] },
            { "primitives": [{ "attributes": { "POSITION": 2 }, "indices": 4, "material": 1 }] }
        ],
        "buffers": [{ "byteLength": bin.len(), "uri": bin_file.0 }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": 48 },
            { "buffer": 0, "byteOffset": 48, "byteLength": 32 },
            { "buffer": 0, "byteOffset": 80, "byteLength": 96 },
            { "buffer": 0, "byteOffset": 176, "byteLength": 12 },
            { "buffer": 0, "byteOffset": 188, "byteLength": 72 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [-2.0, 0.0, -2.0], "max": [2.0, 0.0, 2.0] },
            { "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC2" },
            { "bufferView": 2, "componentType": 5126, "count": 8, "type": "VEC3", "min": lo, "max": hi },
            { "bufferView": 3, "componentType": 5123, "count": 6, "type": "SCALAR" },
            { "bufferView": 4, "componentType": 5123, "count": 36, "type": "SCALAR" }
        ]
    });
    (AssetFile::new("level", "gltf", json.to_string().as_bytes()), bin_file)
}

#[test]
fn a_gltf_bake_takes_the_level_geometry_with_its_materials_and_lightmaps_the_spawned_scene() {
    let (level, _bin) = gltf_level();
    crate::game::Game::new("gltf bake", 1, 1).run_headless(0, |ctx| {
        ctx.scene3d.lighting.sun_intensity = 0.0;
        ctx.scene3d.lighting.ambient_intensity = 1.0;
        ctx.scene3d.sky.sky_color = Color::WHITE;
        ctx.scene3d.sky.ground_color = Color::WHITE;
        ctx.scene3d.sky.equator_color = None;
        let at = Transform3D::at(Vec3::new(10.0, 0.0, 0.0));
        let mut scene = LightBakeScene::new(&ctx.scene3d.lighting, &ctx.scene3d.sky);
        let added = scene.add_gltf(ctx, &level.0, at.matrix()).unwrap();
        // The floor and the pillar; the spawn point isn't level geometry.
        assert_eq!(scene.len(), 2);
        assert_eq!(
            added.nodes(),
            &[("Floor".to_string(), 0)][..],
            "only the floor has lightmap coordinates"
        );
        // glTF's base color factor is linear already.
        let red = scene.models[1].parts[0].1.albedo;
        assert!((red - Vec3::new(0.9, 0.05, 0.05)).abs().max_element() < 1e-3, "{red}");
        let baked = bake_lighting(&scene, &settings().with_samples(32).with_texels_per_unit(4.0)).unwrap();
        // Placed where the scene is: the floor beside the red pillar takes
        // red light from it, the far corner hardly any.
        let beside = light_near(&baked, &scene, 0, Vec3::new(10.95, 0.0, 0.0));
        let corner = light_near(&baked, &scene, 0, Vec3::new(8.2, 0.0, 1.8));
        let tint = |l: Vec3| l.x - l.y;
        assert!(
            tint(beside) > 0.02 && tint(beside) > tint(corner) * 3.0,
            "beside {beside}, corner {corner}"
        );
        let root = crate::d3::GltfRoot::spawn(ctx, &level.0, at, Default::default()).unwrap();
        assert_eq!(added.insert_lightmaps(&baked, &mut ctx.world, root), 1);
        let floor = ctx
            .world
            .get::<crate::d3::GltfRoot>(root)
            .unwrap()
            .entity("Floor")
            .unwrap();
        assert_eq!(ctx.world.get::<Lightmapped>(floor), Some(&Lightmapped(0)));
    });
}

#[test]
fn an_albedo_map_of_one_color_bakes_as_a_flat_material_of_that_color() {
    let red = [200u8, 40, 30];
    let with = |textured: bool| {
        let mut scene = gray_sky(0.2, 6.0);
        let ids = room(true, &mut scene, 2.0);
        let wall = &mut scene.models[ids[1]];
        if textured {
            wall.albedo = Color::WHITE;
            wall.albedo_map = Some(std::sync::Arc::new(
                BakeTexture::from_rgba(1, 1, &[red[0], red[1], red[2], 255]).unwrap(),
            ));
            // Coordinates for the map (the cube's own).
            wall.uvs = vec![Vec2::new(0.3, 0.6); wall.positions.len()];
        } else {
            wall.albedo = Color::rgb(
                f32::from(red[0]) / 255.0,
                f32::from(red[1]) / 255.0,
                f32::from(red[2]) / 255.0,
            );
        }
        bake_lighting(&scene, &settings()).unwrap().to_bytes()
    };
    assert!(with(true) == with(false), "a one-color map bounces as its color");
}

/// The floor's red minus blue light 0.4 units in front of a wall lit by
/// the sun, at z -1 and z +1: the wall's albedo map is red on its z < 0
/// half and blue on the other (or white with `white`).
fn floor_tints_by_a_textured_wall(white: bool) -> (f32, f32) {
    let lighting = Lighting {
        sun_intensity: 6.0,
        sun_color: Color::WHITE,
        sun_direction: Vec3::new(-1.0, -1.0, 0.0),
        ambient_intensity: 1.0,
        ..Default::default()
    };
    let gray = Color::rgb(0.1, 0.1, 0.1);
    let sky = Sky {
        sky_color: gray,
        ground_color: gray,
        equator_color: None,
        ..Default::default()
    };
    let mut scene = LightBakeScene::new(&lighting, &sky);
    let mut floor = MeshData::plane(6.0);
    floor.generate_lightmap_uvs(4.0);
    let f = scene.add(BakeModel::new(&floor, Mat4::IDENTITY));
    // A 4 x 4 wall facing +x at x = -1: its texture's v runs along z.
    let wall = Mat4::from_translation(Vec3::new(-1.0, 2.0, 0.0)) * Mat4::from_rotation_z(-std::f32::consts::FRAC_PI_2);
    let pixels: Vec<u8> = if white {
        vec![255; 8]
    } else {
        vec![230, 20, 20, 255, 20, 20, 230, 255]
    };
    let map = BakeTexture::from_rgba(1, 2, &pixels).unwrap();
    scene.add(
        BakeModel::new(&MeshData::plane(4.0), wall)
            .with_albedo_map(map)
            .with_lightmapped(false),
    );
    let baked = bake_lighting(&scene, &settings().with_texels_per_unit(4.0).with_samples(64)).unwrap();
    let tint = |z: f32| {
        let l = light_near(&baked, &scene, f, Vec3::new(-0.6, 0.0, z));
        l.x - l.z
    };
    (tint(-1.0), tint(1.0))
}

#[test]
fn each_half_of_a_textured_wall_bounces_its_own_color() {
    let (red_side, blue_side) = floor_tints_by_a_textured_wall(false);
    let (white_a, white_b) = floor_tints_by_a_textured_wall(true);
    eprintln!(
        "red minus blue on the floor: {red_side} by the red half, {blue_side} by the blue half; white wall {white_a}, {white_b}"
    );
    // The negative control: a white map tints neither side.
    assert!(white_a.abs() < 0.01 && white_b.abs() < 0.01, "{white_a} {white_b}");
    assert!(red_side > 0.03, "red by the red half: {red_side}");
    assert!(blue_side < -0.03, "blue by the blue half: {blue_side}");
}

#[test]
fn a_world_bake_reads_its_materials_textures() {
    crate::game::Game::new("world bake", 1, 1).run_headless(0, |ctx| {
        ctx.gfx.keep_bake_sources(true);
        let texture = ctx.gfx.create_texture(1, 1, &[10, 200, 10, 255]);
        let green = ctx.gfx.create_material(MaterialDesc::default().with_texture(texture));
        let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), green);
        ctx.world
            .spawn((Transform3D::default(), crate::render::Model::new(mesh)));
        let mut scene = LightBakeScene::new(&ctx.scene3d.lighting, &ctx.scene3d.sky);
        scene.add_world(&mut ctx.world, &ctx.gfx).unwrap();
        let map = scene.models[0].parts[0].1.map.as_ref().expect("the material's texture");
        let c = map.sample(Vec2::splat(0.5));
        assert!(c.y > 0.5 && c.x < 0.01, "{c}");
    });
}

#[test]
fn bakes_run_on_the_frame_pool_inside_their_scope() {
    // The scope the web's threaded build checks for (`threads.mjs`): it opens
    // inside the frame pool's runner, where the bake's parallel loops start.
    let (_, report) = crate::game::Game::new("bake", 1, 1)
        .sim_system(|_ctx| {
            let mut scene = gray_sky(0.4, 4.0);
            room(true, &mut scene, 1.0);
            let s = settings().with_texels_per_unit(1.0).with_samples(2);
            std::hint::black_box(bake_lighting(&scene, &s).unwrap());
        })
        .profile_headless(0, 2, |_| {});
    let scope = report.scope("three/shared_bake");
    assert!(
        scope.is_some_and(|s| s.calls_per_frame >= 1.0),
        "three/shared_bake didn't run: {report}"
    );
}
