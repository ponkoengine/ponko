//! Cooked assets (`ponko cook`) load through the same calls as their
//! sources and give the game the same content.
//!
//! `tests/fixtures/cook/source/` holds a small texture, sound and model, and
//! `tests/fixtures/cook/cooked/` what `ponko cook` makes of them, under the
//! same names (the CLI's test `the_shared_cook_fixture_is_what_cook_makes_of_its_source`
//! checks it still is): a KTX2 texture named `tile.png`, a FLAC sound named
//! `beep.wav`, an optimized, quantized GLB, and a textured one whose images
//! are KTX2 (`panel.glb`; its video memory is measured by the offscreen
//! test `model_textures_render_test`). `level.glb` is 48 m across, more
//! than one 16-bit range holds within the bound, so its cooked version has
//! a range per primitive. `hero.aseprite` is an Aseprite file, which the
//! cook makes into `hero.json` and `hero.png`, the export the engine reads.

use ponko::assets::MemorySource;
use ponko::prelude::*;

fn fixture(folder: &str, name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/cook/{folder}/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// Both versions, as `source/<name>` and `cooked/<name>`.
fn sources() -> MemorySource {
    let mut memory = MemorySource::new("cook fixture");
    for name in ["tile.png", "beep.wav", "grid.glb", "panel.glb", "level.glb"] {
        for folder in ["source", "cooked"] {
            memory.insert(format!("{folder}/{name}"), fixture(folder, name));
        }
    }
    memory.insert("source/hero.aseprite", fixture("source", "hero.aseprite"));
    for name in ["hero.json", "hero.png"] {
        memory.insert(format!("cooked/{name}"), fixture("cooked", name));
    }
    memory
}

/// How far `ponko cook` may move a position (its `POSITION_ERROR`, a tenth
/// of a millimeter), plus float rounding.
#[cfg(feature = "3d")]
const POSITION_ERROR: f32 = 1e-4 + 1e-6;

fn game() -> Game {
    Game::new("cooked assets", 1, 1).hot_reload(false)
}

// Sounds are decoded (and have a length) only with the audio feature.
#[cfg(feature = "audio")]
#[test]
fn a_cooked_sound_loads_by_its_source_name_with_the_same_length() {
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(sources());
        assert!(fixture("cooked", "beep.wav").starts_with(b"fLaC"));
        let source = ctx.audio.try_load("source/beep.wav").expect("the WAV");
        let cooked = ctx
            .audio
            .try_load("cooked/beep.wav")
            .expect("the FLAC under the WAV's name");
        assert!(source.seconds() > 0.09);
        assert!((source.seconds() - cooked.seconds()).abs() < 1e-4);
    });
}

#[cfg(feature = "compressed-textures")]
#[test]
fn a_cooked_texture_loads_by_its_source_name_at_the_same_size() {
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(sources());
        let source = ctx.gfx.try_load_texture("source/tile.png").expect("the PNG");
        let cooked = ctx
            .gfx
            .try_load_texture("cooked/tile.png")
            .expect("KTX2 under the PNG's name");
        assert_eq!((cooked.width(), cooked.height()), (source.width(), source.height()));
    });
}

#[cfg(not(feature = "compressed-textures"))]
#[test]
fn a_cooked_texture_names_the_feature_a_game_without_ktx2_needs() {
    // `ponko package` keeps the images of such a game as they are.
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(sources());
        let err = ctx.gfx.try_load_texture("cooked/tile.png").unwrap_err();
        assert!(err.to_string().contains("compressed-textures"), "{err}");
    });
}

/// A mesh's triangles as their corners (position, normal, uv), each
/// triangle starting at its smallest position, sorted.
#[cfg(feature = "3d")]
fn triangles(mesh: &ponko::render::MeshData) -> Vec<[[f32; 8]; 3]> {
    let corner = |i: u32| {
        let i = i as usize;
        let (p, n, t) = (mesh.positions[i], mesh.normals[i], mesh.uvs[i]);
        [p.x, p.y, p.z, n.x, n.y, n.z, t.x, t.y]
    };
    let mut out: Vec<[[f32; 8]; 3]> = mesh
        .indices
        .chunks(3)
        .map(|c| {
            let mut tri = [corner(c[0]), corner(c[1]), corner(c[2])];
            let first = (0..3)
                .min_by(|&a, &b| tri[a][..3].partial_cmp(&tri[b][..3]).unwrap())
                .unwrap();
            tri.rotate_left(first);
            tri
        })
        .collect();
    out.sort_by(|a, b| {
        a[0][..3]
            .partial_cmp(&b[0][..3])
            .unwrap()
            .then(a[1][..3].partial_cmp(&b[1][..3]).unwrap())
    });
    out
}

/// The two meshes have the same triangles: positions within the bound,
/// and (with `attributes`) the same normals and texture coordinates.
/// Quantizing can reorder positions that nearly tie (which triangle sorts
/// first, which corner leads), so each source triangle is matched with the
/// cooked one at its place.
#[cfg(feature = "3d")]
fn same_triangles(name: &str, a: &ponko::render::MeshData, b: &ponko::render::MeshData, attributes: bool) {
    let (a, mut b) = (triangles(a), triangles(b));
    assert_eq!(a.len(), b.len(), "{name}");
    for ta in &a {
        let mut found = None;
        for (i, tb) in b.iter().enumerate() {
            for r in 0..3 {
                let close = (0..3).all(|c| {
                    let (ca, cb) = (&ta[c], &tb[(c + r) % 3]);
                    (0..3).all(|k| (ca[k] - cb[k]).abs() <= POSITION_ERROR)
                });
                if close && found.is_none() {
                    found = Some((i, r));
                }
            }
        }
        let (i, r) = found.unwrap_or_else(|| panic!("{name}: no cooked triangle at {ta:?}"));
        let tb = b.swap_remove(i);
        for c in (0..3).filter(|_| attributes) {
            let (ca, cb) = (&ta[c], &tb[(c + r) % 3]);
            for k in 3..8 {
                assert!((ca[k] - cb[k]).abs() < 1e-4, "{name}: {} vs {}", ca[k], cb[k]);
            }
        }
    }
}

#[cfg(feature = "3d")]
#[test]
fn a_cooked_model_loads_by_its_source_name_with_the_same_triangles() {
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(sources());
        // `level.glb` has a quantization range per primitive.
        for name in ["grid.glb", "level.glb"] {
            let source = ctx
                .gfx
                .try_load_mesh_data(&format!("source/{name}"))
                .expect("the model");
            let cooked = ctx
                .gfx
                .try_load_mesh_data(&format!("cooked/{name}"))
                .expect("the cooked model");
            same_triangles(name, &source, &cooked, true);
        }
    });
}

/// The cook makes an Aseprite file into the export the engine reads: its
/// frames with their durations, its tags and its slices load as an
/// export from Aseprite itself would.
#[test]
fn a_cooked_aseprite_file_loads_as_its_export() {
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(sources());
        let hero = ctx
            .gfx
            .try_load_aseprite("cooked/hero.json", false)
            .expect("the export");
        assert_eq!(hero.frame_count(), 3);
        let durations: Vec<_> = (0..3).map(|f| hero.frame_duration(f)).collect();
        assert_eq!(durations, [Some(100), Some(150), Some(200)]);
        assert!(hero.animation("Walk", None).is_ok());
        assert!(hero.animation("Wave", None).is_ok());
        assert!(hero.animation("Run", None).is_err());
        let hitbox = hero.slice("Hitbox", 0).expect("the slice");
        assert_eq!(hitbox.bounds, Rect::new(1.0, 0.0, 5.0, 5.0));
        assert_eq!(hitbox.pivot, Some(ponko::math::IVec2::new(2, 4)));
        let panel = hero.slice("Panel", 2).expect("a slice's key holds for later frames");
        assert_eq!(panel.center, Some(Rect::new(1.0, 1.0, 5.0, 3.0)));
        assert!(hero.sprite(2, None).is_some());
        assert!(hero.sprite(3, None).is_none());
    });
}

/// What game code reads from a model node by node (`GltfScene`: the node
/// tree, transforms, extras, the properties colliders are made from, and
/// each node's mesh) is the source's: the nodes `ponko cook` adds for
/// quantized positions are folded back as the file loads, and positions
/// move by at most the bound. The textured model's cooked images are KTX2,
/// which a build without `compressed-textures` refuses, naming the feature
/// (the cook writes KTX2 only for games built with it).
#[cfg(feature = "3d")]
#[test]
fn a_cooked_models_nodes_keep_their_tree_transforms_and_extras() {
    use ponko::d3::GltfScene;
    let models: &[&str] = if cfg!(feature = "compressed-textures") {
        &["grid.glb", "level.glb", "panel.glb"]
    } else {
        &["grid.glb", "level.glb"]
    };
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(sources());
        for name in models {
            let source = GltfScene::load(ctx, &format!("source/{name}")).unwrap();
            let cooked = GltfScene::load(ctx, &format!("cooked/{name}")).unwrap();
            assert_eq!(source.nodes.len(), cooked.nodes.len(), "{name}: no node added");
            for (a, b) in source.nodes.iter().zip(&cooked.nodes) {
                assert_eq!((&a.name, &a.path, a.parent), (&b.name, &b.path, b.parent));
                assert_eq!(
                    (a.transform, a.world),
                    (b.transform, b.world),
                    "{name}: transforms are exact"
                );
                assert_eq!(a.extras, b.extras);
                assert_eq!(a.properties, b.properties, "{name}: colliders, bodies, spawn points");
                match (&a.mesh, &b.mesh) {
                    (Some(ma), Some(mb)) => same_triangles(name, ma, mb, false),
                    (None, None) => {}
                    _ => panic!("{name}: {} has a mesh in one of them", a.path),
                }
            }
        }
        match GltfScene::load(ctx, "cooked/panel.glb") {
            Ok(panel) => assert!(
                panel.node("panel").unwrap().properties.collider.is_some(),
                "a mesh collider"
            ),
            Err(e) => {
                if cfg!(feature = "compressed-textures") {
                    panic!("{e}");
                }
                assert!(e.to_string().contains("`compressed-textures`"), "{e}");
            }
        }
    });
}

/// A navigation mesh baked from a cooked level walks like the source's:
/// the same paths, the same heights, within the bake's own cell size.
#[cfg(feature = "navmesh")]
#[test]
fn a_navmesh_baked_from_a_cooked_level_matches_the_source() {
    use ponko::d3::{GltfScene, NavMesh};
    use ponko::nav::NavBakeSettings;
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(sources());
        let settings = NavBakeSettings::default();
        let bake = |path: &str| {
            let scene = GltfScene::load(ctx, path).unwrap();
            NavMesh::bake(&[scene.nav_source()], settings).unwrap()
        };
        let (source, cooked) = (bake("source/grid.glb"), bake("cooked/grid.glb"));
        let (from, to) = (Vec3::new(0.6, 0.0, 0.6), Vec3::new(3.4, 0.0, 3.4));
        let a = source.find_path(from, to).expect("a path on the source");
        let b = cooked.find_path(from, to).expect("a path on the cooked level");
        assert_eq!(a.len(), b.len(), "{a:?}\n{b:?}");
        for (p, q) in a.iter().zip(&b) {
            assert!(p.distance(*q) <= settings.cell_size, "{p} vs {q}");
        }
        for x in [0.8, 2.0, 3.2] {
            let at = Vec3::new(x, 1.0, 2.0);
            let (p, q) = (source.sample_position(at, 2.0), cooked.sample_position(at, 2.0));
            let (p, q) = (p.unwrap(), q.unwrap());
            assert!((p.y - q.y).abs() <= settings.cell_height, "{p} vs {q}");
        }
    });
}

/// Shard Grove's setup loads (its three models, the level's mesh data and
/// its eight sounds) from its source assets and from a cooked copy, timed.
/// Not a test: run it in release to measure,
/// `ponko cook assets/shard_grove --out <dir>`, then
/// `PONKO_COOKED=<dir> cargo test --release --features 3d --test cooked_assets -- --ignored --nocapture`.
#[cfg(feature = "3d")]
#[test]
#[ignore = "a measurement: needs PONKO_COOKED and a release build"]
fn measure_shard_grove_load_times() {
    use ponko::assets::{AssetSource, MemorySource};
    let Some(cooked) = std::env::var_os("PONKO_COOKED") else {
        return;
    };
    let read = |dir: &std::path::Path| {
        let mut memory = MemorySource::new("measured");
        for name in ["grove.glb", "warden.glb", "crawler.glb"] {
            memory.insert(format!("shard_grove/{name}"), std::fs::read(dir.join(name)).unwrap());
        }
        for name in ["hurt", "wind", "crawler", "gate", "select", "pulse", "shrine", "shard"] {
            let file = format!("sounds/{name}.wav");
            memory.insert(format!("shard_grove/{file}"), std::fs::read(dir.join(&file)).unwrap());
        }
        memory
    };
    let source = read(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/shard_grove"));
    let cooked = read(std::path::Path::new(&cooked));
    let bytes = |m: &MemorySource| -> usize {
        ["grove.glb", "warden.glb", "crawler.glb"]
            .iter()
            .map(|n| format!("shard_grove/{n}"))
            .chain(
                ["hurt", "wind", "crawler", "gate", "select", "pulse", "shrine", "shard"]
                    .iter()
                    .map(|n| format!("shard_grove/sounds/{n}.wav")),
            )
            .map(|n| m.read(&n).unwrap().len())
            .sum()
    };
    for (label, memory) in [("source", &source), ("cooked", &cooked)] {
        let mut times = Vec::new();
        let mut counts = (0, 0);
        for _ in 0..31 {
            let memory = memory.clone();
            game().run_headless(0, |ctx| {
                ctx.assets.add_source(memory);
                let start = std::time::Instant::now();
                for model in ["grove", "warden", "crawler"] {
                    ctx.gfx.load_mesh(&format!("shard_grove/{model}.glb"));
                }
                let level = ctx.gfx.load_mesh_data("shard_grove/grove.glb");
                for name in ["hurt", "wind", "crawler", "gate", "select", "pulse", "shrine", "shard"] {
                    ctx.audio.load(&format!("shard_grove/sounds/{name}.wav"));
                }
                times.push(start.elapsed().as_secs_f64() * 1000.0);
                counts = (level.positions.len(), level.indices.len());
            });
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{label}: {} bytes; setup loads median {:.2} ms (min {:.2}, max {:.2}) over 31 runs; level mesh {} corners, {} indices",
            bytes(memory),
            times[15],
            times[0],
            times[30],
            counts.0,
            counts.1
        );
    }
}

/// Each of Shard Grove's eight sounds loaded (read and decoded) as its
/// source WAV and as the cook's FLAC, timed one by one: whether short
/// sounds load measurably slower as FLAC. The two kinds alternate in every
/// round, and which goes first swaps each round, so a busy machine slows
/// both alike. Not a test: run it in release to measure,
/// `ponko cook assets/shard_grove --out <dir>`, then
/// `PONKO_COOKED=<dir> cargo test --release --test cooked_assets -- --ignored --nocapture measure_sound`.
#[cfg(feature = "audio")]
#[test]
#[ignore = "a measurement: needs PONKO_COOKED and a release build"]
fn measure_sound_load_times() {
    const SOUNDS: [&str; 8] = ["select", "shard", "hurt", "pulse", "shrine", "crawler", "gate", "wind"];
    const ROUNDS: usize = 41;
    let Some(cooked) = std::env::var_os("PONKO_COOKED") else {
        return;
    };
    let folders = [
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/shard_grove"),
        std::path::PathBuf::from(cooked),
    ];
    let mut memory = MemorySource::new("measured");
    let mut sizes = [[0; 2]; SOUNDS.len()];
    for (kind, folder) in folders.iter().enumerate() {
        for (i, name) in SOUNDS.iter().enumerate() {
            let bytes = std::fs::read(folder.join(format!("sounds/{name}.wav"))).unwrap();
            sizes[i][kind] = bytes.len();
            memory.insert(format!("{kind}/{name}.wav"), bytes);
        }
    }
    // times[sound][kind][round], in microseconds.
    let mut times = vec![[Vec::new(), Vec::new()]; SOUNDS.len()];
    let mut seconds = [0.0; SOUNDS.len()];
    for round in 0..ROUNDS {
        for kind in [round % 2, 1 - round % 2] {
            let memory = memory.clone();
            game().run_headless(0, |ctx| {
                ctx.assets.add_source(memory);
                for (i, name) in SOUNDS.iter().enumerate() {
                    let start = std::time::Instant::now();
                    let sound = ctx.audio.load(&format!("{kind}/{name}.wav"));
                    times[i][kind].push(start.elapsed().as_secs_f64() * 1e6);
                    seconds[i] = sound.seconds();
                }
            });
        }
    }
    let quartiles = |t: &mut Vec<f64>| {
        t.sort_by(f64::total_cmp);
        let at = |q: f64| t[((t.len() - 1) as f64 * q).round() as usize];
        (at(0.25), at(0.5), at(0.75), t[0], t[t.len() - 1])
    };
    println!("{ROUNDS} rounds; microseconds as median [quartiles] (min..max)");
    for (i, name) in SOUNDS.iter().enumerate() {
        let [wav, flac] = &mut times[i];
        let (w, f) = (quartiles(wav), quartiles(flac));
        let apart = if f.0 > w.2 || w.0 > f.2 { "apart" } else { "overlap" };
        println!(
            "{name:8} {:.2} s  WAV {:6} B: {:7.1} [{:.1}, {:.1}] ({:.1}..{:.1})  FLAC {:6} B: {:7.1} [{:.1}, {:.1}] ({:.1}..{:.1})  quartiles {apart}",
            seconds[i], sizes[i][0], w.1, w.0, w.2, w.3, w.4, sizes[i][1], f.1, f.0, f.2, f.3, f.4
        );
    }
}
