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

use ponko::assets::{AssetSource as _, MemorySource};
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

/// Replication messages (snapshots, inputs, events) to a server and a
/// client that joined it (`ponko::net::Replication`).
pub fn replication(data: &[u8]) {
    ponko::__fuzz::replication_packets(data);
}

/// Splits fuzzer bytes into `(flags, bytes)` records, `[flags: u8][length:
/// u16 LE][bytes]` each, the last one cut short where the data ends.
fn records(mut data: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    std::iter::from_fn(move || {
        let (&flags, rest) = data.split_first()?;
        let len = match rest {
            [a, b, ..] => usize::from(u16::from_le_bytes([*a, *b])),
            _ => return None,
        };
        let rest = &rest[2..];
        let (bytes, rest) = rest.split_at(len.min(rest.len()));
        data = rest;
        Some((flags, bytes))
    })
}

/// The lobby protocol both ways: frames from a hostile client to a running
/// lobby server (`ponko_lobby`, with an honest client in a room), and from a
/// hostile server to a lobby client (`LobbyClient`). Records as for the
/// network targets; flags: bit 0 picks the direction (to the server, to the
/// client), bit 1 the lane (reliable, unreliable). Every frame is also
/// decoded as a request and as a reply.
pub fn lobby(data: &[u8]) {
    use ponko::net::lobby::{LobbyClient, protocol};
    use ponko::net::{Lane, MessageEvent, NetMessages, PeerId};
    use std::time::Duration;

    const STEP: Duration = Duration::from_millis(10);
    let mut limits = ponko_lobby::LobbyLimits::default();
    limits.address_key = ponko_lobby::AddressKey::Connection;
    let mut server = ponko_lobby::LobbyServer::new(limits).seed(1);
    let mut endpoints = NetMessages::loopback(3).into_iter();
    server.listen(endpoints.next().expect("three endpoints"));
    let mut raw = endpoints.next().expect("three endpoints");
    let mut honest = LobbyClient::new(endpoints.next().expect("three endpoints"));
    let mut endpoints = NetMessages::loopback(2).into_iter();
    let mut fake_host = endpoints.next().expect("two endpoints");
    let mut client = LobbyClient::new(endpoints.next().expect("two endpoints"));
    let client_peer = std::cell::Cell::new(None);
    let step = |server: &mut ponko_lobby::LobbyServer,
                raw: &mut NetMessages,
                honest: &mut LobbyClient,
                fake_host: &mut NetMessages,
                client: &mut LobbyClient| {
        server.poll_elapsed(STEP);
        let _ = raw.poll_elapsed(STEP);
        while raw.next_event().is_some() {}
        let _ = honest.poll_elapsed(STEP);
        while honest.next_event().is_some() {}
        let _ = fake_host.poll_elapsed(STEP);
        while let Some(event) = fake_host.next_event() {
            if let MessageEvent::Connected(peer) = event {
                client_peer.set(Some(peer));
            }
        }
        let _ = client.poll_elapsed(STEP);
        while client.next_event().is_some() {}
    };
    for i in 0..60 {
        if i == 30 {
            let _ = honest.create_room(4);
        }
        step(&mut server, &mut raw, &mut honest, &mut fake_host, &mut client);
    }
    let Some(peer) = client_peer.get() else {
        panic!("the lobby client connects to its fake host")
    };
    for (flags, bytes) in records(data) {
        let _ = protocol::decode::<protocol::Request>(bytes);
        let _ = protocol::decode::<protocol::Reply>(bytes);
        let lane = if flags & 2 == 0 {
            Lane::Reliable
        } else {
            Lane::Unreliable
        };
        let _ = if flags & 1 == 0 {
            raw.send(PeerId::HOST, lane, bytes)
        } else {
            fake_host.send(peer, lane, bytes)
        };
        step(&mut server, &mut raw, &mut honest, &mut fake_host, &mut client);
    }
}

/// Control endpoint request lines (`PONKO_CONTROL`): refused without the
/// token, then run by the game's dispatcher as if the token were right.
pub fn control(data: &[u8]) {
    ponko::__fuzz::control_requests(data);
}

/// A tile image for the level and Aseprite targets' sheets.
const TILES_PNG: &[u8] = include_bytes!("../../tests/fixtures/tilemaps/tiles.png");

/// An Aseprite file read directly (`aseprite-source`), and a cooked
/// Aseprite export (the JSON Array sheet `ponko cook` writes, with a valid
/// PNG beside it), each loaded split into layers and not, then every frame,
/// animation and slice asked for.
pub fn aseprite(data: &[u8]) {
    let files = MemorySource::new("fuzz")
        .with("a.aseprite", data)
        .with("c.json", data)
        .with("c.png", TILES_PNG)
        .with("layers.png", TILES_PNG);
    headless(0, files, |ctx| {
        for name in ["a.aseprite", "c.aseprite"] {
            for split in [false, true] {
                let Ok(sprite) = ctx.gfx.try_load_aseprite(name, split) else {
                    continue;
                };
                let layers = sprite.layers();
                for frame in 0..sprite.frame_count().min(16) {
                    let _ = sprite.frame_duration(frame);
                    let _ = sprite.sprite(frame, None);
                    for layer in &layers {
                        let _ = sprite.sprite(frame, Some(layer));
                    }
                    let _ = sprite.slice("s", frame);
                }
                if let Ok(run) = sprite.animation("Run", None) {
                    let _ = run.sprite_at(1.5);
                }
            }
        }
    });
}

/// A level: the bytes as an LDtk project, a Tiled JSON map and a Tiled XML
/// map (external levels, tilesets and templates they name read the same
/// bytes), loaded, its solid rectangles merged, and spawned.
pub fn levels(data: &[u8]) {
    use ponko::tilemap::LevelOptions;
    let mut files = MemorySource::new("fuzz").with("tiles.png", TILES_PNG);
    for name in ["l.ldtk", "l.tmj", "l.tmx", "l.ldtkl", "t.tsx", "t.tsj", "o.tx"] {
        files = files.with(name, data);
    }
    headless(0, files, |ctx| {
        for name in ["l.ldtk", "l.tmj", "l.tmx"] {
            if let Ok(level) = ctx.assets.load_level(name, &LevelOptions::default()) {
                let _ = level.solid_rects(&LevelOptions::default());
                if let Ok(root) = ctx.spawn_level(name, Vec2::ZERO) {
                    ctx.despawn_level(root);
                }
            }
        }
    });
}

/// A texture as `ponko cook` ships it (KTX2 with BCn or UASTC blocks,
/// Zstandard or zlib supercompressed; DDS) or any image file, decoded for a
/// GPU with every block format and for one with none, and streamed a slice
/// at a time.
pub fn cooked_texture(data: &[u8]) {
    ponko::__fuzz::texture(data);
}

/// A sound file in any format the engine plays (WAV, AIFF, ADPCM, OGG
/// Vorbis, FLAC, MP3): decoded whole, in slices, streamed from memory and
/// played as a stream, which must refuse absurd headers and stop at the
/// length limit.
pub fn sound(data: &[u8]) {
    ponko::__fuzz::sound(data);
}
