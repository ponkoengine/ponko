//! The mistakes with files keep messages that name the mistake, where it is
//! and the fix: a wrong kind of file given to a loader, an asset path with
//! the folder in it, a save slot that is missing, damaged, empty or from a
//! newer version, and a scene or prefab that names a component the game
//! never registered. The wording is pinned here; the guide's
//! "When something fails" chapter lists the same messages.

use ponko::prelude::*;

/// A folder of this test's own, so parallel tests never share saves.
fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ponko-error-files-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `bytes` as a file called `name` in the test's own folder; returns its path.
fn file(dir: &std::path::Path, name: &str, bytes: &[u8]) -> String {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize, Component)]
struct Health {
    hp: i32,
}

// A wrong kind of file given to a loader.

#[test]
fn a_sound_given_to_the_texture_loader_says_what_it_is_and_what_loads_it() {
    let dir = folder("sound-as-texture");
    let path = file(&dir, "crab.png", b"OggS\0\x02\0\0\0\0\0\0\0\0");
    Game::new("t", 1, 1).run_headless(0, move |ctx| {
        let err = ctx.gfx.try_load_texture(&path).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Decode);
        assert_eq!(err.path(), Some(path.as_str()));
        let message = err.message();
        assert!(
            message.contains("is not a valid image: its bytes are an Ogg sound, not an image"),
            "{err}"
        );
        assert!(message.contains("load it with `ctx.audio.load` instead"), "{err}");
    });
}

#[test]
fn an_empty_texture_a_text_file_and_a_git_lfs_pointer_each_say_so() {
    let dir = folder("textures");
    let empty = file(&dir, "empty.png", b"");
    let text = file(&dir, "notes.png", b"these are my notes, not a picture");
    let lfs = file(
        &dir,
        "lfs.png",
        b"version https://git-lfs.github.com/spec/v1\noid sha256:4d7a\nsize 1234\n",
    );
    let cut = file(&dir, "cut.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
    Game::new("t", 1, 1).run_headless(0, move |ctx| {
        let said = |ctx: &mut Ctx, path: &str| ctx.gfx.try_load_texture(path).unwrap_err().to_string();
        assert!(said(ctx, &empty).contains("the file is empty (0 bytes)"));
        assert!(said(ctx, &text).contains("it is a text file, not an image"));
        assert!(said(ctx, &lfs).contains("a Git LFS pointer"));
        assert!(said(ctx, &lfs).contains("run `git lfs pull`"));
        assert!(said(ctx, &cut).contains("starts like a PNG image but can't be read"));
    });
}

#[test]
fn a_texture_given_to_the_font_loader_says_what_it_is() {
    let dir = folder("texture-as-font");
    let path = file(&dir, "pixel.ttf", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0");
    Game::new("t", 1, 1).run_headless(0, move |ctx| {
        let err = ctx.gfx.try_load_font(&path).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Decode);
        assert!(
            err.message()
                .contains("is not a valid font: its bytes are a PNG image, not a font"),
            "{err}"
        );
        assert!(
            err.message().contains("load it with `ctx.gfx.load_texture` instead"),
            "{err}"
        );
    });
}

#[cfg(feature = "audio")]
#[test]
fn a_texture_given_to_the_sound_loader_says_what_it_is_and_which_sounds_load() {
    let dir = folder("texture-as-sound");
    let path = file(&dir, "hit.ogg", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0");
    let empty = file(&dir, "silence.wav", b"");
    let noise = file(&dir, "noise.ogg", &[0xde, 0xad, 0xbe, 0xef, 1, 2, 3, 4, 5]);
    Game::new("t", 1, 1).run_headless(0, move |ctx| {
        let err = ctx.audio.try_load(&path).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Decode);
        assert!(
            err.message()
                .contains("is not a valid sound: its bytes are a PNG image, not a sound"),
            "{err}"
        );
        assert!(
            err.message().contains("load it with `ctx.gfx.load_texture` instead"),
            "{err}"
        );
        let err = ctx.audio.try_load(&empty).unwrap_err();
        assert!(err.message().contains("the file is empty (0 bytes)"), "{err}");
        // A file that starts like nothing ponko reads names the formats that load.
        let err = ctx.audio.try_load(&noise).unwrap_err();
        assert!(err.message().contains("(de ad be ef 01 02 03 04)"), "{err}");
        assert!(
            err.message()
                .contains("ponko reads Ogg Vorbis, MP3, FLAC, WAV and AIFF"),
            "{err}"
        );
    });
}

// An asset path with the folder in it.

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn an_asset_name_that_starts_with_the_assets_folder_says_to_drop_it() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        // `crab.png` is among the engine's own test assets.
        assert!(ctx.gfx.try_load_texture("crab.png").is_ok());
        let err = ctx.gfx.try_load_texture("assets/crab.png").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert!(
            err.message().contains("Hint: names are relative to the assets/ folder, so drop the leading `assets/`: try \"crab.png\" (it exists there)"),
            "{err}"
        );
        let err = ctx.gfx.try_load_texture("sprites\\crab.png").unwrap_err();
        assert!(err.message().contains("names use `/` as the separator on every platform"), "{err}");
        // A plain misspelling gets no hint of this kind.
        let err = ctx.gfx.try_load_texture("crab.pgn").unwrap_err();
        assert!(!err.message().contains("Hint:"), "{err}");
    });
}

// Save slots.

#[test]
fn a_missing_slot_lists_the_slots_there_are() {
    let dir = folder("missing-slot");
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        let err = ctx.load_world("slot2").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert_eq!(err.path(), Some("saves/slot2.json"));
        assert_eq!(
            err.message(),
            "there is no save in slot \"slot2\" (looked for saves/slot2.json); \
             no slot has been saved yet; save one with `Storage::save_world`"
        );
        ctx.storage.save_world(&ctx.world, "slot1").unwrap();
        let err = ctx.load_world("slot2").unwrap_err();
        assert!(err.message().ends_with("saved slots: slot1"), "{err}");
    });
}

#[test]
fn a_damaged_or_empty_slot_says_which_and_how_to_get_on() {
    let dir = folder("damaged-slot");
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.storage
            .write("saves/torn.json", b"{ \"format\": \"ponko-wor")
            .unwrap();
        let err = ctx.load_world("torn").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Json);
        assert!(
            err.message()
                .starts_with("save slot \"torn\" (saves/torn.json) is damaged or isn't a ponko save: "),
            "{err}"
        );
        assert!(
            err.message().contains("`Storage::read_backup(\"saves/torn.json\")`"),
            "{err}"
        );
        assert!(err.message().contains("`Storage::delete_world`"), "{err}");
        ctx.storage.write("saves/none.json", b"").unwrap();
        let err = ctx.load_world("none").unwrap_err();
        assert!(
            err.message()
                .starts_with("save slot \"none\" (saves/none.json) is empty (0 bytes); "),
            "{err}"
        );
    });
}

#[test]
fn a_slot_from_a_newer_engine_or_another_kind_of_file_says_so() {
    let dir = folder("kinds-of-slot");
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.storage
            .write(
                "saves/newer.json",
                br#"{"format":"ponko-world","version":99,"entities":[]}"#,
            )
            .unwrap();
        let err = ctx.load_world("newer").unwrap_err();
        assert!(
            err.message()
                .contains("unsupported save version 99 (this engine reads 1)"),
            "{err}"
        );
        assert!(
            err.message()
                .contains("the save was written by a newer ponko: update the engine"),
            "{err}"
        );
        ctx.storage.write("saves/other.json", br#"{"hello":"world"}"#).unwrap();
        let err = ctx.load_world("other").unwrap_err();
        assert!(
            err.message()
                .contains("it has no \"format\" field, so it isn't a file ponko wrote"),
            "{err}"
        );
        let prefab = br#"{"format":"ponko-prefab","version":1,"components":{"Health":{"hp":1}}}"#;
        ctx.storage.write("saves/prefab.json", prefab).unwrap();
        let err = ctx.load_world("prefab").unwrap_err();
        assert!(
            err.message()
                .contains("it is a prefab: spawn it with `ctx.spawn_prefab` instead"),
            "{err}"
        );
    });
}

#[test]
fn a_slot_naming_an_unregistered_component_says_which_entity_and_what_to_call() {
    let dir = folder("unregistered-in-slot");
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.world.reflect::<Health>();
        let save = br#"{"format":"ponko-world","version":1,"entities":[
            {"entity":7,"components":{"Health":{"hp":3}}},
            {"entity":9,"components":{"Helth":{"hp":3}}}]}"#;
        ctx.storage.write("saves/typo.json", save).unwrap();
        let err = ctx.load_world("typo").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        let message = err.message();
        assert!(
            message.starts_with("save slot \"typo\" (saves/typo.json): no reflected component named `Helth`"),
            "{err}"
        );
        assert!(
            message.contains("register the type with `world.reflect::<Helth>()` in setup"),
            "{err}"
        );
        assert!(
            message.contains("entity 9 is the first in the file to name it"),
            "{err}"
        );
        assert!(message.contains("did you mean `Health`?"), "{err}");
        // The world is unchanged by the failed load.
        assert_eq!(ctx.world.query::<&Health>().iter().count(), 0);
    });
}

#[test]
fn reading_a_missing_stored_file_names_the_folder_and_a_wrong_shape_names_the_type() {
    let dir = folder("stored-files");
    let expected = dir.display().to_string();
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, move |ctx| {
        let err = ctx.storage.read("settings.json").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert_eq!(err.message(), format!("no stored file \"settings.json\" in {expected}"));
        ctx.storage.write("settings.json", br#"{"volume": 3}"#).unwrap();
        let err = ctx.storage.read_json::<Vec<u8>>("settings.json").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Json);
        assert!(
            err.message().starts_with("settings.json: the file doesn't fit `"),
            "{err}"
        );
        assert!(err.message().contains("Vec<u8>"), "{err}");
    });
}

// Scenes and prefabs.

#[test]
fn a_scene_naming_an_unregistered_component_says_which_entity_and_the_call() {
    let dir = folder("scene-unregistered");
    let scene = file(
        &dir,
        "level.json",
        br#"{"format":"ponko-world","version":1,"entities":[{"entity":4,"components":{"Wizard":{"mana":3}}}]}"#,
    );
    Game::new("t", 1, 1).run_headless(0, move |ctx| {
        ctx.world.reflect::<Health>();
        let err = ctx.load_scene(&scene).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert_eq!(err.path(), Some(scene.as_str()));
        let message = err.message();
        assert!(
            message.starts_with(&format!("{scene}: no reflected component named `Wizard`: ")),
            "{err}"
        );
        assert!(message.contains("`world.reflect::<Wizard>()`"), "{err}");
        assert!(message.contains("names are case-sensitive"), "{err}");
        assert!(
            message.contains("entity 4 is the first in the file to name it"),
            "{err}"
        );
        assert!(
            message.contains("`world.reflected_components()` lists the names registered"),
            "{err}"
        );
    });
}

#[test]
fn a_scene_value_that_does_not_fit_names_the_component_and_the_entity() {
    let dir = folder("scene-bad-value");
    let scene = file(
        &dir,
        "level.json",
        br#"{"format":"ponko-world","version":1,"entities":[
            {"entity":1,"components":{"Health":{"hp":5}}},
            {"entity":2,"components":{"Health":{"hp":"lots"}}}]}"#,
    );
    Game::new("t", 1, 1).run_headless(0, move |ctx| {
        ctx.world.reflect::<Health>();
        let err = ctx.load_scene(&scene).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Json);
        assert!(
            err.message()
                .ends_with("bad value for `Health` on entity 2: invalid type: string \"lots\", expected i32"),
            "{err}"
        );
    });
}

#[test]
fn a_prefab_given_to_the_scene_loader_and_back_say_what_each_is() {
    let dir = folder("prefab-or-scene");
    let prefab = file(
        &dir,
        "crab.json",
        br#"{"format":"ponko-prefab","version":1,"components":{"Health":{"hp":1}}}"#,
    );
    let scene = file(
        &dir,
        "level.json",
        br#"{"format":"ponko-world","version":1,"entities":[]}"#,
    );
    let newer = file(
        &dir,
        "newer.json",
        br#"{"format":"ponko-prefab","version":9,"components":{}}"#,
    );
    Game::new("t", 1, 1).run_headless(0, move |ctx| {
        ctx.world.reflect::<Health>();
        let err = ctx.load_scene(&prefab).unwrap_err();
        assert!(
            err.message()
                .contains("it is a prefab: spawn it with `ctx.spawn_prefab` instead"),
            "{err}"
        );
        let err = ctx.spawn_prefab(&scene, Vec2::ZERO).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Json);
        assert!(err.message().contains("not a prefab"), "{err}");
        assert!(
            err.message()
                .contains("it is a scene or save: load it with `ctx.load_scene`"),
            "{err}"
        );
        let err = ctx.spawn_prefab(&newer, Vec2::ZERO).unwrap_err();
        assert!(
            err.message()
                .contains("unsupported prefab version 9 (this engine reads 1 and 2)"),
            "{err}"
        );
        assert!(err.message().contains("written by a newer ponko"), "{err}");
    });
}

#[test]
fn a_prefab_naming_an_unregistered_component_suggests_the_registered_one() {
    let dir = folder("prefab-typo");
    let prefab = file(
        &dir,
        "crab.json",
        br#"{"format":"ponko-prefab","version":1,"components":{"Healt":{"hp":1}}}"#,
    );
    Game::new("t", 1, 1).run_headless(0, move |ctx| {
        ctx.world.reflect::<Health>();
        let err = ctx.spawn_prefab(&prefab, Vec2::ZERO).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert!(err.message().contains("no reflected component named `Healt`"), "{err}");
        assert!(err.message().contains("did you mean `Health`?"), "{err}");
    });
}
