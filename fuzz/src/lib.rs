//! What each fuzz target does with the fuzzer's bytes (`fuzz_targets/`
//! are one line each). A target must never crash, hang or allocate without
//! bound, whatever the bytes; errors are fine. Where a load succeeds, the
//! targets also check what must hold for any file the engine accepted
//! (a loaded world saves and loads again, an action map written out reads
//! back).
//!
//! Everything goes through the engine's public API, as a game would load
//! the file, except the network protocols, whose packet parsers sit behind
//! connected sessions: those use the hidden `ponko::__fuzz` entry points
//! (only built with `--cfg fuzzing`, which `cargo fuzz` sets).

use ponko::assets::MemorySource;
use ponko::prelude::*;
use ponko::reflect::WorldReflect;

/// Runs `f` in a headless game whose assets are `files`, then `ticks`
/// ticks, so systems see whatever `f` loaded.
fn headless(ticks: u64, files: MemorySource, f: impl FnOnce(&mut Ctx)) {
    Game::new("fuzz", 1, 1).run_headless(ticks, |ctx| {
        ctx.assets.add_source(files);
        f(ctx);
    });
}

/// A world save or scene (`load_scene`, the `World::load` format), then
/// two ticks of the engine's systems over it.
pub fn scene(data: &[u8]) {
    headless(2, MemorySource::new("fuzz").with("scene.json", data), |ctx| {
        if ctx.load_scene("scene.json").is_ok() {
            let saved = ctx.world.save();
            if let Err(e) = ctx.world.load(&saved) {
                panic!("a loaded scene must load again after a save: {e}\n{saved}");
            }
        }
    });
}

/// A prefab (one entity or a tree, possibly including itself), spawned and
/// despawned, then two ticks.
pub fn prefab(data: &[u8]) {
    headless(2, MemorySource::new("fuzz").with("p.json", data), |ctx| {
        if let Ok(root) = ctx.spawn_prefab("p.json", Vec2::ZERO) {
            let _ = ctx.spawn_prefab("p.json", vec3(1.0, 2.0, 3.0));
            ctx.despawn_prefab(root);
        }
    });
}

/// A glTF model (`.glb`, or `.gltf` JSON; buffers and images it names by
/// file resolve to the same bytes): mesh data, the mesh with its materials
/// and images, its skeleton and clips.
pub fn gltf(data: &[u8]) {
    let files = MemorySource::new("fuzz")
        .with("m.glb", data)
        .with("m.gltf", data)
        .with("m.bin", data)
        .with("m.png", data);
    headless(0, files, |ctx| {
        for name in ["m.glb", "m.gltf"] {
            let _ = ctx.gfx.try_load_mesh_data(name);
            let _ = ctx.gfx.try_load_mesh(name);
            let _ = ctx.gfx.try_load_skeleton(name);
            let _ = ctx.gfx.try_load_clips(name);
        }
    });
}

/// An empty Unity project, for resolving references (none resolve).
fn empty_project() -> &'static ponko_unity::Project {
    static PROJECT: std::sync::OnceLock<ponko_unity::Project> = std::sync::OnceLock::new();
    PROJECT.get_or_init(|| {
        let root = std::env::temp_dir().join(format!("ponko-fuzz-project-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Assets")).expect("a temporary folder");
        ponko_unity::Project::open(&root).expect("an empty project")
    })
}

/// Everything the importer reads out of a parsed Unity file.
fn read_unity_file(file: &ponko_unity::UnityFile) {
    for object in &file.objects {
        let _ = ponko_unity::UnityMesh::from_object(object);
        let _ = ponko_unity::UnityTexture::from_object(object);
        let _ = ponko_unity::UnityMaterial::from_object(object);
        let _ = ponko_unity::UnityTerrain::from_object(object);
    }
    let _ = ponko_unity::Hierarchy::build(file, empty_project());
}

/// A Unity text (YAML) file: scene, prefab, mesh, material, clip.
pub fn unity_yaml(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Ok(file) = ponko_unity::UnityFile::parse(text) {
        read_unity_file(&file);
    }
    let _ = ponko_unity::AnimationClip::parse(text);
}

/// A Unity binary serialized file.
pub fn unity_binary(data: &[u8]) {
    if let Ok(file) = ponko_unity::UnityFile::parse_binary(data) {
        read_unity_file(&file);
    }
    let _ = ponko_unity::AnimationClip::parse_binary(data);
}

/// Action bindings from JSON (players' saved key bindings), installed and
/// queried for a tick.
pub fn action_map(data: &[u8]) {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let Ok(map) = ActionMap::from_json(text) else { return };
    let json = map.to_json().expect("an action map writes out");
    if let Err(e) = ActionMap::from_json(&json) {
        panic!("an action map that was read must read back: {e}\n{json}");
    }
    let names: Vec<String> = map.iter().map(|(name, _)| name.to_string()).collect();
    headless(1, MemorySource::new("fuzz"), |ctx| {
        ctx.input.set_actions(map);
        for name in &names {
            let _ = ctx.input.action_down(name.as_str());
            let _ = ctx.input.action_pressed(name.as_str());
            let _ = ctx.input.action_axis(name.as_str());
            let _ = ctx.input.action_axis2(name.as_str());
        }
    });
}

/// An asset pack (`ponko pack`, downloadable content): every file in it read.
pub fn asset_pack(data: &[u8]) {
    let Ok(pack) = ponko::assets::PackSource::from_bytes("fuzz", data.to_vec()) else {
        return;
    };
    let names: Vec<String> = pack.names().take(64).map(str::to_string).collect();
    for name in &names {
        assert!(pack.contains(name), "a pack has the files it names");
        let _ = pack.read(name);
    }
}

/// Lockstep datagrams to a running two-player session (`ponko::net::Net`).
pub fn lockstep(data: &[u8]) {
    ponko::__fuzz::lockstep_packets(data);
}

/// Message-protocol datagrams to a connected host and client
/// (`ponko::net::NetMessages`).
pub fn messages(data: &[u8]) {
    ponko::__fuzz::message_packets(data);
}
