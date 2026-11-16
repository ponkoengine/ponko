//! Cooked assets (`ponko cook`) load through the same calls as their
//! sources and give the game the same content.
//!
//! `tests/fixtures/cook/source/` holds a small texture, sound and model, and
//! `tests/fixtures/cook/cooked/` what `ponko cook` makes of them, under the
//! same names (the CLI's test `the_shared_cook_fixture_is_what_cook_makes_of_its_source`
//! checks it still is): a KTX2 texture named `tile.png`, a FLAC sound named
//! `beep.wav` and an optimized, quantized GLB.

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
    for name in ["tile.png", "beep.wav", "grid.glb"] {
        for folder in ["source", "cooked"] {
            memory.insert(format!("{folder}/{name}"), fixture(folder, name));
        }
    }
    memory
}

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

#[cfg(feature = "3d")]
#[test]
fn a_cooked_model_loads_by_its_source_name_with_the_same_triangles() {
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(sources());
        let source = ctx.gfx.try_load_mesh_data("source/grid.glb").expect("the model");
        let cooked = ctx.gfx.try_load_mesh_data("cooked/grid.glb").expect("the cooked model");
        let (a, b) = (triangles(&source), triangles(&cooked));
        assert_eq!(a.len(), b.len());
        assert!(!a.is_empty());
        for (ta, tb) in a.iter().zip(&b) {
            for (ca, cb) in ta.iter().zip(tb) {
                assert_eq!(ca[..3], cb[..3], "positions are exact");
                for k in 3..8 {
                    assert!((ca[k] - cb[k]).abs() < 1e-4, "{} vs {}", ca[k], cb[k]);
                }
            }
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
