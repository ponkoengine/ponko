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
    assert_eq!(layers.len() as u32, 1 + 2 * baked.pages());
    let side = baked.page_size() as usize;
    assert!(layers.iter().all(|l| l.len() == side * side * 4));
    let texel = |i: usize| -> [f32; 4] { std::array::from_fn(|k| f16_to_f32(layers[0][i * 4 + k])) };
    assert_eq!(texel(0), [-1.0, baked.pages() as f32, 1.0, 1.0]);
    let r = baked.rect(ids[3]).unwrap();
    let at = HEADER_TEXELS as usize + 2 * ids[3];
    assert_eq!(texel(at), [r.x as f32, r.y as f32, r.width as f32, r.height as f32]);
    assert_eq!(texel(at + 1)[0], r.page as f32);
    // The probe grid's origin, from its two parts.
    let origin = Vec3::from_slice(&texel(1)[..3]) + Vec3::from_slice(&texel(2)[..3]);
    assert!((origin - baked.probes.as_ref().unwrap().origin).abs().max_element() < 1e-3);
    assert_eq!(baked.gpu_bytes(), (side * side * 8 * layers.len()) as u64);
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
