//! `ctx.storage`: the player's files through a game, save slots for the
//! world, and storage that is off without an app id.

use ponko::prelude::*;
use ponko::storage::Storage;
use std::path::PathBuf;

/// A fresh, empty folder for one test.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ponko-storage-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[derive(Component, Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
struct Health(i32);

#[test]
fn files_written_in_one_run_are_read_back_in_the_next() {
    let dir = temp_dir("runs");
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        assert!(ctx.storage.is_available() && ctx.storage.is_persistent());
        ctx.storage.write("settings.json", b"{}").unwrap();
        ctx.storage.write_json("saves/a.json", &[1, 2, 3]).unwrap();
    });
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        assert_eq!(ctx.storage.list().unwrap(), ["saves/a.json", "settings.json"]);
        assert_eq!(ctx.storage.read_json::<Vec<i32>>("saves/a.json").unwrap(), [1, 2, 3]);
        assert!(ctx.storage.exists("settings.json"));
        ctx.storage.delete("settings.json").unwrap();
        ctx.storage.delete("settings.json").unwrap(); // already gone: fine
        assert!(!ctx.storage.exists("settings.json"));
        assert_eq!(
            ctx.storage.read("settings.json").unwrap_err().kind(),
            ErrorKind::NotFound
        );
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_app_id_names_the_folder_inside_the_data_folder() {
    // Nothing is written: the player's real data folder stays untouched.
    Game::new("t", 1, 1).app_id("com.example.crabs").run_headless(0, |ctx| {
        assert!(ctx.storage.is_available(), "the test machine has a home folder");
        let dir = ctx.storage.dir().expect("a folder on the desktop");
        assert!(dir.ends_with("com.example.crabs"), "{}", dir.display());
        assert!(dir.is_absolute(), "{}", dir.display());
    });
}

#[test]
fn a_game_without_an_app_id_keeps_no_files_and_says_why() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        assert!(!ctx.storage.is_available());
        assert_eq!(ctx.storage.dir(), None);
        let err = ctx.storage.write("a.txt", b"x").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Unsupported);
        assert!(err.message().contains("app_id"), "{err}");
        assert_eq!(ctx.storage.list().unwrap_err().kind(), ErrorKind::Unsupported);
        assert_eq!(ctx.load_world("a").unwrap_err().kind(), ErrorKind::Unsupported);
    });
}

#[test]
#[should_panic(expected = "app id \"../saves\" must be 1 to 128 letters")]
fn an_app_id_that_is_not_a_plain_name_is_refused() {
    let _ = Game::new("t", 1, 1).app_id("../saves");
}

#[test]
fn a_world_saved_in_a_slot_loads_back_with_its_entities() {
    let dir = temp_dir("slots");
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.world.reflect::<Health>();
        ctx.world.spawn((Health(7),));
        ctx.storage.save_world(&ctx.world, "slot 1").unwrap();
        ctx.storage.save_world(&ctx.world, "autosave").unwrap();
        assert_eq!(ctx.storage.world_slots().unwrap(), ["autosave", "slot 1"]);
        assert!(dir.join("saves").join("slot 1.json").is_file());
    });
    let mut world = Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.world.reflect::<Health>();
        ctx.world.spawn((Health(1),)); // replaced by the load
        ctx.load_world("slot 1").unwrap();
        ctx.storage.delete_world("autosave").unwrap();
        assert_eq!(ctx.storage.world_slots().unwrap(), ["slot 1"]);
    });
    let mut healths: Vec<Health> = world.query::<&Health>().iter().copied().collect();
    healths.sort_by_key(|h| h.0);
    assert_eq!(healths, [Health(7)]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn loading_a_missing_or_damaged_slot_leaves_the_world_as_it_was() {
    let dir = temp_dir("bad-slot");
    let mut files = Storage::in_folder(&dir);
    files.write("saves/broken.json", b"{ not json").unwrap();
    let mut world = Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.world.reflect::<Health>();
        ctx.world.spawn((Health(3),));
        assert_eq!(ctx.load_world("nothing").unwrap_err().kind(), ErrorKind::NotFound);
        let err = ctx.load_world("broken").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Json);
        assert_eq!(err.path(), Some("saves/broken.json"));
        assert_eq!(ctx.load_world("a/b").unwrap_err().kind(), ErrorKind::Invalid);
    });
    assert_eq!(
        world.query::<&Health>().iter().copied().collect::<Vec<_>>(),
        [Health(3)]
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_write_the_disk_refuses_leaves_the_old_file() {
    let dir = temp_dir("refused");
    let mut files = Storage::in_folder(&dir);
    files.write("a.json", b"old").unwrap();
    // The new file's place is taken by a folder: the rename can't happen.
    std::fs::create_dir_all(dir.join("b.json")).unwrap();
    assert_eq!(files.write("b.json", b"new").unwrap_err().kind(), ErrorKind::Io);
    assert!(!dir.join("b.json.tmp").exists(), "the temporary file is cleaned up");
    // A file where the storage folder should be.
    let blocked = Storage::in_folder(dir.join("a.json"));
    assert_eq!(blocked.list().unwrap_err().kind(), ErrorKind::Io);
    // A file in the way of the storage folder's path. Windows reports that
    // path as not found (a folder not made yet, so no files), Unix as "not a
    // directory".
    #[cfg(unix)]
    {
        let below = Storage::in_folder(dir.join("a.json").join("below"));
        assert_eq!(below.list().unwrap_err().kind(), ErrorKind::Io);
    }
    assert_eq!(files.read("a.json").unwrap(), b"old");
    let _ = std::fs::remove_dir_all(dir);
}

// ANCHOR: storage
/// The player's choices, kept between runs.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)] // a file from an older version still loads
struct Options {
    volume: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self { volume: 1.0 }
    }
}

fn load_options(ctx: &mut Ctx) -> Options {
    // A first run has no file (and a damaged one isn't JSON): the defaults.
    ctx.storage.read_json("options.json").unwrap_or_default()
}

fn keep_options(ctx: &mut Ctx, options: &Options) {
    // The old file stays whole until the new one is, then becomes the backup.
    if let Err(e) = ctx.storage.write_json("options.json", options) {
        eprintln!("{e}"); // a full or read-only disk: the game goes on
    }
}

fn quick_save(ctx: &mut Ctx) -> Result<(), Error> {
    ctx.storage.save_world(&ctx.world, "quick") // saves/quick.json
}

fn quick_load(ctx: &mut Ctx) -> Result<(), Error> {
    ctx.load_world("quick")
}

fn space_crab() -> Game {
    // The folder in the player's data folder, and the browser database's name.
    Game::new("Space Crab", 1280, 720).app_id("space-crab")
}
// ANCHOR_END: storage

#[test]
fn the_guide_example_keeps_options_and_a_quick_save_between_runs() {
    let dir = temp_dir("guide");
    let first = space_crab().data_dir(&dir); // a test keeps its files apart
    first.run_headless(0, |ctx| {
        assert_eq!(load_options(ctx).volume, 1.0, "first run: the defaults");
        keep_options(ctx, &Options { volume: 0.25 });
        ctx.world.reflect::<Health>();
        ctx.world.spawn((Health(9),));
        quick_save(ctx).unwrap();
    });
    let mut world = space_crab().data_dir(&dir).run_headless(0, |ctx| {
        assert_eq!(load_options(ctx).volume, 0.25);
        ctx.world.reflect::<Health>();
        quick_load(ctx).unwrap();
    });
    assert_eq!(
        world.query::<&Health>().iter().copied().collect::<Vec<_>>(),
        [Health(9)]
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_save_slot_keeps_an_animations_frame() {
    let dir = temp_dir("animation");
    let mut crab = None;
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        let sheet = SpriteSheet::new(ctx.gfx.load_texture("crab.png"), 16, 16);
        let mut walk = ponko::sheet::Animation::new(sheet, 0..4, 10.0);
        walk.time = 0.25; // frame 2
        crab = Some(
            ctx.world
                .spawn((ponko::d2::Transform::default(), sheet.sprite(2), walk)),
        );
        ctx.storage.save_world(&ctx.world, "slot1").unwrap();
    });
    let crab = crab.unwrap();
    let world = Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.load_world("slot1").unwrap();
    });
    let walk = world
        .get::<ponko::sheet::Animation>(crab)
        .expect("the animation is in the save");
    assert_eq!((walk.time, walk.current_frame(), walk.looping), (0.25, 2, true));
    assert_eq!(walk.sheet.frame_size, vec2(16.0, 16.0));
    let _ = std::fs::remove_dir_all(dir);
}

// ---- Binary saves and background saves ----------------------------------------

use ponko::reflect::SaveFormat;

/// The healths in `world`, sorted.
fn healths(world: &mut World) -> Vec<i32> {
    let mut all: Vec<i32> = world.query::<&Health>().iter().map(|h| h.0).collect();
    all.sort_unstable();
    all
}

#[test]
fn a_binary_slot_loads_back_like_a_json_slot() {
    let dir = temp_dir("binary-slot");
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        assert_eq!(ctx.storage.save_format(), SaveFormat::Json, "JSON stays the default");
        ctx.storage.set_save_format(SaveFormat::Binary);
        ctx.world.reflect::<Health>();
        ctx.world.spawn((Health(7),));
        ctx.world.spawn((Health(-2),));
        ctx.storage.save_world(&ctx.world, "slot 1").unwrap();
        assert_eq!(ctx.storage.world_slots().unwrap(), ["slot 1"]);
    });
    let bytes = std::fs::read(dir.join("saves").join("slot 1.save")).unwrap();
    assert_eq!(SaveFormat::detect(&bytes), SaveFormat::Binary);
    assert!(!dir.join("saves").join("slot 1.json").exists());
    // A game that writes JSON still reads it.
    let mut world = Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.world.reflect::<Health>();
        ctx.load_world("slot 1").unwrap();
    });
    assert_eq!(healths(&mut world), [-2, 7]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn json_slots_saved_before_switching_to_binary_still_load_and_are_replaced_on_save() {
    let dir = temp_dir("switch-format");
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.world.reflect::<Health>();
        ctx.world.spawn((Health(1),));
        ctx.storage.save_world(&ctx.world, "old").unwrap();
    });
    let mut world = Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.storage.set_save_format(SaveFormat::Binary);
        ctx.world.reflect::<Health>();
        ctx.load_world("old").unwrap();
        assert_eq!(ctx.storage.world_slots().unwrap(), ["old"]);
        // Saving the slot again writes it in binary and drops the JSON file.
        ctx.world.spawn((Health(2),));
        ctx.storage.save_world(&ctx.world, "old").unwrap();
        assert_eq!(ctx.storage.list().unwrap(), ["saves/old.save"]);
        ctx.load_world("old").unwrap();
        ctx.storage.delete_world("old").unwrap();
        assert!(ctx.storage.world_slots().unwrap().is_empty());
    });
    assert_eq!(healths(&mut world), [1, 2]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_slot_left_in_both_formats_loads_the_one_the_game_writes() {
    let dir = temp_dir("both-formats");
    let mut world = World::new();
    world.reflect::<Health>();
    world.spawn((Health(10),));
    let json = world.save();
    world.spawn((Health(20),));
    let binary = world.save();
    // What a save cut off between writing the new file and removing the
    // old one leaves.
    let mut files = Storage::in_folder(&dir);
    files.write("saves/s.json", &SaveFormat::Json.encode(&json)).unwrap();
    files
        .write("saves/s.save", &SaveFormat::Binary.encode(&binary))
        .unwrap();
    assert_eq!(files.world_slots().unwrap(), ["s"]);
    for (format, expected) in [(SaveFormat::Binary, vec![10, 20]), (SaveFormat::Json, vec![10])] {
        let mut world = Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
            ctx.storage.set_save_format(format);
            ctx.world.reflect::<Health>();
            ctx.load_world("s").unwrap();
        });
        assert_eq!(healths(&mut world), expected, "{format:?}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_damaged_binary_slot_is_refused_and_leaves_the_world_as_it_was() {
    let dir = temp_dir("bad-binary");
    let mut world = World::new();
    world.reflect::<Health>();
    world.spawn((Health(5),));
    let mut bytes = SaveFormat::Binary.encode(&world.save());
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0x55;
    Storage::in_folder(&dir).write("saves/bad.save", &bytes).unwrap();
    let mut world = Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.storage.set_save_format(SaveFormat::Binary);
        ctx.world.reflect::<Health>();
        ctx.world.spawn((Health(3),));
        let err = ctx.load_world("bad").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Decode, "{err}");
        assert_eq!(err.path(), Some("saves/bad.save"));
    });
    assert_eq!(healths(&mut world), [3]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_background_save_holds_the_world_as_it_was_at_the_call() {
    for format in [SaveFormat::Json, SaveFormat::Binary] {
        let dir = temp_dir(&format!("background-{format:?}"));
        let mut world = Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
            ctx.storage.set_save_format(format);
            ctx.world.reflect::<Health>();
            let e = ctx.world.spawn((Health(1),));
            let job = ctx.storage.save_world_in_background(&ctx.world, "bg").unwrap();
            // The game goes on at once; the save already has its world.
            ctx.world.get_mut::<Health>(e).unwrap().0 = 99;
            ctx.world.spawn((Health(50),));
            job.wait().unwrap();
            assert!(!ctx.storage.is_saving());
            ctx.load_world("bg").unwrap();
        });
        assert_eq!(healths(&mut world), [1], "{format:?}");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn background_saves_are_written_in_order_and_reads_wait_for_them() {
    let dir = temp_dir("background-order");
    let mut world = Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.storage.set_save_format(SaveFormat::Binary);
        ctx.world.reflect::<Health>();
        // Big saves first, then small ones, so a later save would finish
        // encoding first if nothing kept them in order.
        for i in 0..2000 {
            ctx.world.spawn((Health(i),));
        }
        let mut jobs = Vec::new();
        for _ in 0..4 {
            jobs.push(ctx.storage.save_world_in_background(&ctx.world, "slot").unwrap());
        }
        ctx.world.clear();
        ctx.world.spawn((Health(-1),));
        jobs.push(ctx.storage.save_world_in_background(&ctx.world, "slot").unwrap());
        // A read of the slot waits for every save of it: it sees the last.
        let bytes = ctx.storage.read("saves/slot.save").unwrap();
        let save = SaveFormat::decode(&bytes).unwrap();
        assert_eq!(save["entities"].as_array().unwrap().len(), 1);
        assert!(!ctx.storage.is_saving());
        for job in jobs {
            job.wait().unwrap();
        }
        // The backup is the save before the last one.
        let backup = SaveFormat::decode(&ctx.storage.read_backup("saves/slot.save").unwrap()).unwrap();
        assert_eq!(backup["entities"].as_array().unwrap().len(), 2000);
        ctx.load_world("slot").unwrap();
    });
    assert_eq!(healths(&mut world), [-1]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn background_saves_finish_before_the_game_ends() {
    let dir = temp_dir("background-exit");
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.world.reflect::<Health>();
        for i in 0..5000 {
            ctx.world.spawn((Health(i),));
        }
        // The job is dropped: the save is written anyway.
        drop(ctx.storage.save_world_in_background(&ctx.world, "last").unwrap());
    });
    let mut world = Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.world.reflect::<Health>();
        ctx.load_world("last").unwrap();
    });
    assert_eq!(healths(&mut world).len(), 5000);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_background_save_without_storage_or_with_a_bad_slot_fails_at_once() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        let err = ctx.storage.save_world_in_background(&ctx.world, "a").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Unsupported);
    });
    let dir = temp_dir("background-bad-slot");
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        let err = ctx.storage.save_world_in_background(&ctx.world, "a/b").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Invalid);
        assert!(!ctx.storage.is_saving());
    });
    let _ = std::fs::remove_dir_all(dir);
}

/// How many entities the save file `saves/<slot>.json` in `dir` holds, read straight from the
/// disk: storage's own reads wait for a save in flight, which would hide a `finish_saves` that
/// returned early.
fn entities_on_disk(dir: &std::path::Path, slot: &str) -> Option<usize> {
    let bytes = std::fs::read(dir.join("saves").join(format!("{slot}.json"))).ok()?;
    let save: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some(save["entities"].as_array()?.len())
}

#[test]
fn finish_saves_waits_for_saves_of_different_slots_and_both_files_load_back() {
    let dir = temp_dir("finish-two-slots");
    let mut world = Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.world.reflect::<Health>();
        for i in 0..3000 {
            ctx.world.spawn((Health(i),));
        }
        ctx.storage.save_world_in_background(&ctx.world, "big").unwrap();
        ctx.world.clear();
        ctx.world.spawn((Health(-7),));
        ctx.storage.save_world_in_background(&ctx.world, "small").unwrap();
        ctx.storage.finish_saves();
        assert!(!ctx.storage.is_saving(), "nothing is left in flight");
        assert_eq!(entities_on_disk(&dir, "big"), Some(3000), "the first slot is on disk");
        assert_eq!(entities_on_disk(&dir, "small"), Some(1), "so is the second");
        ctx.load_world("small").unwrap();
        assert_eq!(
            ctx.world.query::<&Health>().iter().map(|h| h.0).collect::<Vec<_>>(),
            [-7]
        );
        ctx.load_world("big").unwrap(); // loading replaces the world
    });
    assert_eq!(healths(&mut world).len(), 3000, "the first slot loads back whole");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn finish_saves_leaves_the_last_of_several_saves_to_one_slot_on_disk() {
    let dir = temp_dir("finish-same-slot");
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.world.reflect::<Health>();
        // Big worlds first and a small one last, so a later save would finish encoding
        // first if nothing kept them in the order they were made.
        for i in 0..2000 {
            ctx.world.spawn((Health(i),));
        }
        for _ in 0..3 {
            ctx.storage.save_world_in_background(&ctx.world, "slot").unwrap();
        }
        ctx.world.clear();
        ctx.world.spawn((Health(-1),));
        ctx.storage.save_world_in_background(&ctx.world, "slot").unwrap();
        ctx.storage.finish_saves();
        assert!(!ctx.storage.is_saving());
        assert_eq!(entities_on_disk(&dir, "slot"), Some(1), "the file holds the last save");
        ctx.world.clear();
        ctx.load_world("slot").unwrap();
        assert_eq!(
            ctx.world.query::<&Health>().iter().map(|h| h.0).collect::<Vec<_>>(),
            [-1]
        );
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn finish_saves_with_nothing_queued_returns_at_once() {
    let dir = temp_dir("finish-nothing");
    // On a thread, so that a call that waits for ever fails the test instead of hanging it.
    let (done, finished) = std::sync::mpsc::channel();
    let folder = dir.clone();
    std::thread::spawn(move || {
        let storage = Storage::in_folder(&folder);
        storage.finish_saves();
        storage.finish_saves(); // and again: still nothing to wait for
        assert!(!storage.is_saving());
        let _ = done.send(());
    });
    finished
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("finish_saves did not return");
    Game::new("t", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.storage.finish_saves();
        ctx.storage.finish_saves();
        assert!(!ctx.storage.is_saving());
    });
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        ctx.storage.finish_saves(); // storage off: nothing to wait for either
    });
    let _ = std::fs::remove_dir_all(dir);
}
