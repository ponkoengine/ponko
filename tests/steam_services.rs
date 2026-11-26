//! Steam's store-side services against the fake backend: achievements and
//! stats, cloud saves kept in step with `Storage`, rich presence, the
//! overlay, Deck and Big Picture text input, and an absent Steam. The real
//! calls are on the person's checklist (two Steam accounts).
#![cfg(feature = "steam")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use ponko::prelude::*;
use ponko::steam::{CloudSync, Steam, SteamText, SteamTextMode};
use ponko::storage::Storage;

fn folder(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ponko-steam-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Sets the file's modification time, as if written at `at`.
fn written_at(storage: &Storage, name: &str, at: SystemTime) {
    let file = std::fs::File::options()
        .write(true)
        .open(storage.dir().unwrap().join(name))
        .unwrap();
    file.set_modified(at).unwrap();
}

#[test]
fn achievements_unlock_clear_and_read_back() {
    let (steam, fake) = Steam::fake();
    fake.define_achievements(&["FIRST_WIN", "ALL_LEVELS"]);
    assert_eq!(steam.achievement("FIRST_WIN"), Some(false));
    assert!(steam.unlock_achievement("FIRST_WIN"));
    assert!(fake.achieved("FIRST_WIN"));
    assert_eq!(steam.achievement("FIRST_WIN"), Some(true));
    assert_eq!(steam.achievement("ALL_LEVELS"), Some(false));
    assert!(steam.clear_achievement("FIRST_WIN"));
    assert!(!fake.achieved("FIRST_WIN"));
    assert!(!steam.unlock_achievement("NOT_SET_UP"), "Steam refuses unknown names");
    assert_eq!(steam.achievement("NOT_SET_UP"), None);
}

#[test]
fn stats_set_read_back_and_store() {
    let (steam, fake) = Steam::fake();
    fake.define_stats(&["wins", "distance"]);
    assert_eq!(steam.stat_i32("wins"), Some(0));
    assert!(steam.set_stat_i32("wins", 7));
    assert!(steam.set_stat_f32("distance", 1.5));
    assert_eq!(steam.stat_i32("wins"), Some(7));
    assert_eq!(steam.stat_f32("distance"), Some(1.5));
    assert!(!steam.set_stat_i32("losses", 1), "unknown stats are refused");
    assert_eq!(steam.stat_f32("losses"), None);
    assert_eq!(fake.stores(), 0, "nothing is sent until store_stats");
    assert!(steam.store_stats());
    assert_eq!(fake.stores(), 1);
    assert_eq!(fake.stat("wins"), Some(7.0));
}

#[test]
fn an_absent_steam_does_nothing_and_answers_none() {
    let steam = Steam::absent();
    assert!(!steam.is_running());
    assert!(!steam.unlock_achievement("A"));
    assert!(!steam.clear_achievement("A"));
    assert_eq!(steam.achievement("A"), None);
    assert!(!steam.set_stat_i32("s", 1));
    assert!(!steam.set_stat_f32("s", 1.0));
    assert_eq!(steam.stat_i32("s"), None);
    assert!(!steam.store_stats());
    assert!(!steam.set_rich_presence("status", "Menu"));
    steam.clear_rich_presence();
    steam.update();
    assert!(!steam.overlay_open());
    assert!(!steam.is_steam_deck());
    assert!(!steam.is_big_picture());
    assert!(!steam.show_text_input(SteamTextMode::SingleLine, "Name", "", 16));
    assert!(!steam.show_floating_keyboard(SteamTextMode::SingleLine, 0, 0, 10, 10));
    assert_eq!(steam.take_text_input(), None);
    assert!(!steam.cloud_enabled());
    assert!(steam.client().is_none());
}

#[test]
fn without_steam_the_folder_stays_the_only_copy() {
    let dir = folder("absent");
    let mut storage = Storage::in_folder(&dir);
    storage.write("saves/slot1.json", b"{}").unwrap();
    let steam = Steam::absent();
    assert_eq!(
        steam.sync_cloud(&mut storage, &["saves/"]).unwrap(),
        CloudSync::default()
    );
    steam.delete_cloud_file(&mut storage, "saves/slot1.json").unwrap();
    assert!(!storage.exists("saves/slot1.json"), "the local file still goes");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn rich_presence_sets_removes_and_clears() {
    let (steam, fake) = Steam::fake();
    assert!(steam.set_rich_presence("status", "In the menu"));
    assert!(steam.set_rich_presence("steam_display", "#Playing"));
    assert_eq!(fake.rich_presence("status").as_deref(), Some("In the menu"));
    assert!(steam.set_rich_presence("status", ""), "empty removes the key");
    assert_eq!(fake.rich_presence("status"), None);
    assert!(!steam.set_rich_presence(&"k".repeat(65), "x"), "Steam's key limit");
    assert!(!steam.set_rich_presence("status", "a\0b"), "a NUL never reaches Steam");
    steam.clear_rich_presence();
    assert_eq!(fake.rich_presence("steam_display"), None);
}

#[test]
fn the_overlay_state_follows_steam_at_each_update() {
    let (steam, fake) = Steam::fake();
    assert!(!steam.overlay_open());
    fake.set_overlay(true);
    assert!(!steam.overlay_open(), "seen at the next update only");
    steam.update();
    assert!(steam.overlay_open());
    fake.set_overlay(false);
    fake.set_overlay(true);
    fake.set_overlay(false);
    steam.update();
    assert!(!steam.overlay_open(), "the last report wins");
    steam.update();
    assert!(!steam.overlay_open());
}

#[test]
fn a_game_pauses_while_the_overlay_is_open() {
    let (steam, fake) = Steam::fake();
    let world = Game::new("steam", 64, 64)
        .headless_frames(true)
        .system(move |ctx| {
            match ctx.time.frame() {
                3 => fake.set_overlay(true),
                6 => fake.set_overlay(false),
                _ => {}
            }
            steam.update();
            if steam.overlay_open() {
                ctx.time.pause();
            } else {
                ctx.time.resume();
            }
            let paused = ctx.time.is_paused();
            ctx.world.resource_mut::<Vec<bool>>().push(paused);
        })
        .run_headless(9, |ctx| ctx.world.insert_resource(Vec::<bool>::new()));
    let seen = world.resource::<Vec<bool>>();
    assert!(seen.iter().any(|p| *p), "paused while open: {seen:?}");
    assert!(!seen.last().unwrap(), "running again once closed: {seen:?}");
}

#[test]
fn the_text_input_opens_on_a_deck_or_big_picture_and_returns_the_text() {
    let (steam, fake) = Steam::fake();
    assert!(
        !steam.show_text_input(SteamTextMode::SingleLine, "Name", "", 16),
        "on a desktop the game shows its own keyboard"
    );
    fake.set_big_picture(true);
    assert!(steam.is_big_picture());
    assert!(steam.show_text_input(SteamTextMode::SingleLine, "Name", "Crab", 16));
    assert!(fake.text_input_open());
    assert!(
        !steam.show_text_input(SteamTextMode::SingleLine, "Name", "", 16),
        "one at a time"
    );
    steam.update();
    assert_eq!(steam.take_text_input(), None, "still open");
    fake.close_text_input(Some("Crabby"));
    steam.update();
    assert_eq!(steam.take_text_input(), Some(SteamText::Submitted("Crabby".into())));
    assert_eq!(steam.take_text_input(), None, "taken once");
    assert!(steam.show_text_input(SteamTextMode::Password, "Password", "", 32));
    fake.close_text_input(None);
    steam.update();
    assert_eq!(steam.take_text_input(), Some(SteamText::Cancelled));
    assert!(!steam.show_text_input(SteamTextMode::SingleLine, "a\0b", "", 8));
}

#[test]
fn the_floating_keyboard_opens_on_a_deck_only() {
    let (steam, fake) = Steam::fake();
    assert!(!steam.show_floating_keyboard(SteamTextMode::SingleLine, 10, 10, 200, 30));
    assert!(!fake.keyboard_open());
    fake.set_steam_deck(true);
    assert!(steam.is_steam_deck());
    assert!(steam.show_floating_keyboard(SteamTextMode::Numeric, 10, 10, 200, 30));
    assert!(fake.keyboard_open());
}

#[test]
fn cloud_sync_copies_files_found_on_one_side_only() {
    let dir = folder("one-side");
    let mut storage = Storage::in_folder(&dir);
    let (steam, fake) = Steam::fake();
    storage.write("saves/here.json", b"here").unwrap();
    storage.write("settings.json", b"{}").unwrap();
    fake.put_cloud_file("saves/there.json", b"there", SystemTime::now());
    fake.put_cloud_file("other/skip.json", b"no", SystemTime::now());
    let done = steam.sync_cloud(&mut storage, &["saves/"]).unwrap();
    assert_eq!(done.uploaded, ["saves/here.json"]);
    assert_eq!(done.downloaded, ["saves/there.json"]);
    assert_eq!(storage.read("saves/there.json").unwrap(), b"there");
    assert_eq!(fake.cloud_file("saves/here.json").as_deref(), Some(&b"here"[..]));
    assert_eq!(fake.cloud_file("settings.json"), None, "only the files named");
    assert!(!storage.exists("other/skip.json"));
    assert_eq!(
        steam.sync_cloud(&mut storage, &["saves/"]).unwrap(),
        CloudSync::default(),
        "in step: a second sync copies nothing"
    );
    let done = steam.sync_cloud(&mut storage, &["settings.json"]).unwrap();
    assert_eq!(done.uploaded, ["settings.json"], "a file named exactly");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cloud_sync_keeps_the_newest_copy() {
    let dir = folder("newest");
    let mut storage = Storage::in_folder(&dir);
    let (steam, fake) = Steam::fake();
    let hour = Duration::from_secs(3600);
    let now = SystemTime::now();

    // Another computer saved later: the cloud wins.
    storage.write("saves/a.json", b"old").unwrap();
    written_at(&storage, "saves/a.json", now - hour);
    fake.put_cloud_file("saves/a.json", b"new", now);
    let done = steam.sync_cloud(&mut storage, &["saves/"]).unwrap();
    assert_eq!(done.downloaded, ["saves/a.json"]);
    assert_eq!(storage.read("saves/a.json").unwrap(), b"new");

    // This computer saved later (played while Steam was off): it wins.
    storage.write("saves/a.json", b"newer").unwrap();
    fake.put_cloud_file("saves/a.json", b"new", now - hour);
    let done = steam.sync_cloud(&mut storage, &["saves/"]).unwrap();
    assert_eq!(done.uploaded, ["saves/a.json"]);
    assert_eq!(fake.cloud_file("saves/a.json").as_deref(), Some(&b"newer"[..]));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cloud_sync_does_nothing_when_the_player_turned_the_cloud_off() {
    let dir = folder("off");
    let mut storage = Storage::in_folder(&dir);
    let (steam, fake) = Steam::fake();
    fake.set_cloud_enabled(false);
    storage.write("saves/a.json", b"x").unwrap();
    fake.put_cloud_file("saves/b.json", b"y", SystemTime::now());
    assert!(!steam.cloud_enabled());
    assert_eq!(
        steam.sync_cloud(&mut storage, &["saves/"]).unwrap(),
        CloudSync::default()
    );
    assert!(!storage.exists("saves/b.json"));
    assert_eq!(fake.cloud_file("saves/a.json"), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cloud_sync_reports_a_full_cloud_and_bad_entries() {
    let dir = folder("full");
    let mut storage = Storage::in_folder(&dir);
    let (steam, fake) = Steam::fake();
    storage.write("saves/a.json", b"x").unwrap();
    fake.set_cloud_full(true);
    let e = steam.sync_cloud(&mut storage, &["saves/"]).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Io);
    assert_eq!(e.path(), Some("saves/a.json"));
    assert_eq!(
        steam.sync_cloud(&mut storage, &[""]).unwrap_err().kind(),
        ErrorKind::Invalid
    );
    assert_eq!(
        steam
            .sync_cloud(&mut Storage::default(), &["saves/"])
            .unwrap_err()
            .kind(),
        ErrorKind::Unsupported,
        "a game without an app id keeps no files"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn deleting_a_cloud_file_keeps_it_from_coming_back() {
    let dir = folder("delete");
    let mut storage = Storage::in_folder(&dir);
    let (steam, fake) = Steam::fake();
    storage.write("saves/a.json", b"x").unwrap();
    steam.sync_cloud(&mut storage, &["saves/"]).unwrap();
    steam.delete_cloud_file(&mut storage, "saves/a.json").unwrap();
    assert_eq!(fake.cloud_file("saves/a.json"), None);
    assert_eq!(
        steam.sync_cloud(&mut storage, &["saves/"]).unwrap(),
        CloudSync::default()
    );
    assert!(!storage.exists("saves/a.json"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn clones_share_one_steam() {
    let (steam, fake) = Steam::fake();
    let other = steam.clone();
    fake.set_overlay(true);
    other.update();
    assert!(steam.overlay_open());
    let _ = format!("{steam:?} {fake:?}");
}
