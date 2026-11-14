//! Asset packs: many asset files in one archive (`PackSource`), as `ponko
//! pack` writes them.
//!
//! `tests/fixtures/packs/sample.pack` is made by the CLI's writer (its test
//! `the_shared_fixture_is_what_the_writer_makes` checks it still is), so
//! these tests read exactly what `ponko pack` writes.

use ponko::prelude::*;

fn sample() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/packs/sample.pack")).unwrap()
}

fn game() -> Game {
    Game::new("asset packs", 1, 1).hot_reload(false)
}

#[test]
fn a_pack_written_by_ponko_pack_holds_its_files_by_name() {
    let pack = PackSource::from_bytes("sample", sample()).unwrap();
    assert_eq!(
        pack.names().collect::<Vec<_>>(),
        [
            "empty.bin",
            "notes/copy.txt",
            "notes/long.txt",
            "notes/same.txt",
            "rules.json",
            "sprites/dot.png"
        ]
    );
    assert_eq!(pack.read("notes/same.txt").unwrap(), b"the same bytes twice");
    assert_eq!(pack.read("notes/copy.txt").unwrap(), b"the same bytes twice");
    let long = String::from_utf8(pack.read("notes/long.txt").unwrap()).unwrap();
    assert_eq!(long.lines().count(), 40, "a deflated file unpacks whole");
    assert!(pack.read("empty.bin").unwrap().is_empty());
}

// ANCHOR: pack_source
#[derive(serde::Deserialize)]
struct Rules {
    speed: f32,
}

#[test]
fn textures_and_data_load_by_name_from_an_added_pack() {
    game().run_headless(0, |ctx| {
        // A pack made with `ponko pack`, compiled into the executable.
        let pack = PackSource::from_bytes("built-in", include_bytes!("fixtures/packs/sample.pack").as_slice())
            .expect("a pack");
        ctx.assets.add_source(pack);

        // Loads by name find its files.
        let dot = ctx.gfx.load_texture("sprites/dot.png");
        ctx.assets.load_resource::<Rules>(&mut ctx.world, "rules.json");
        // ANCHOR_END: pack_source
        assert_eq!((dot.width(), dot.height()), (1, 1));
        assert_eq!(ctx.gfx.load_texture("sprites/dot.png"), dot, "loaded once");
        assert_eq!(ctx.world.resource::<Rules>().speed, 30.0);
    });
}

#[test]
fn a_pack_file_opens_and_reads_a_file_at_a_time() {
    let dir = std::env::temp_dir().join(format!("ponko-asset-packs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("dlc.pack");
    std::fs::write(&path, sample()).unwrap();
    game().run_headless(0, |ctx| {
        ctx.assets.add_source(PackSource::open(&path).unwrap());
        assert!(ctx.assets.has_source("dlc.pack"));
        let dot = ctx.gfx.load_texture("sprites/dot.png");
        assert_eq!((dot.width(), dot.height()), (1, 1));
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_file_that_is_not_a_pack_is_a_decode_error_naming_it() {
    let err = PackSource::from_bytes("page", b"<!doctype html><title>Not found</title>".to_vec())
        .err()
        .unwrap();
    assert_eq!(err.kind(), ErrorKind::Decode);
    assert!(
        err.message().contains("asset pack \"page\": not an asset pack"),
        "{err}"
    );
    let err = PackSource::open("no/such/file.pack").err().unwrap();
    assert_eq!(err.kind(), ErrorKind::Io);
    assert!(err.message().contains("no/such/file.pack"), "{err}");
}

#[test]
fn a_pack_whose_header_sizes_overflow_is_a_decode_error() {
    for (index_len, data_len) in [(u64::MAX, 0), (u64::MAX - 31, 0), (1, u64::MAX - 32)] {
        let mut bytes = b"PONKOPAK".to_vec();
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(index_len.to_le_bytes());
        bytes.extend(data_len.to_le_bytes());
        let err = PackSource::from_bytes("forged", bytes).err().unwrap();
        assert_eq!(err.kind(), ErrorKind::Decode);
        assert!(err.message().contains("past 2^64 bytes"), "{err}");
    }
}
