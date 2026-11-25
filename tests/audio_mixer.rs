//! The mixer from a data file: loading, hot reload, and saving its state
//! with the settings.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ponko::audio::MixerState;
use ponko::prelude::*;

fn temp_file(name: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ponko-mixer-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn saved_mixer_file_reloads_into_the_running_game() {
    let path = temp_file("reload.json", r#"{"buses": [{"name": "music", "volume": 0.5}]}"#);
    let name = path.to_str().unwrap().to_string();
    let edited = path.clone();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    Game::new("mixer", 1, 1)
        .hot_reload(true)
        .sim_system(move |ctx: &mut Ctx| {
            let music = ctx.audio.bus("music");
            log.lock().unwrap().push(ctx.audio.bus_volume(music));
            if ctx.time.sim_tick() == 2 {
                std::thread::sleep(Duration::from_millis(50));
                std::fs::write(
                    &edited,
                    r#"{"buses": [{"name": "master"}, {"name": "music", "parent": "master", "volume": 0.25}]}"#,
                )
                .unwrap();
                std::thread::sleep(Duration::from_millis(400));
            }
        })
        .run_headless(5, |ctx| ctx.audio.load_mixer(&name));
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen, [0.5, 0.5, 0.5, 0.25, 0.25], "applied on the tick after the save");
}

#[test]
fn a_broken_save_keeps_the_mixer_as_it_was() {
    let path = temp_file("broken.json", r#"{"buses": [{"name": "music", "volume": 0.5}]}"#);
    let name = path.to_str().unwrap().to_string();
    let edited = path.clone();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    Game::new("mixer", 1, 1)
        .hot_reload(true)
        .sim_system(move |ctx: &mut Ctx| {
            let music = ctx.audio.bus("music");
            log.lock().unwrap().push(ctx.audio.bus_volume(music));
            if ctx.time.sim_tick() == 2 {
                std::thread::sleep(Duration::from_millis(50));
                std::fs::write(
                    &edited,
                    r#"{"buses": [{"name": "music", "parent": "nowhere", "volume": 0.1}]}"#,
                )
                .unwrap();
                std::thread::sleep(Duration::from_millis(400));
            }
        })
        .run_headless(4, |ctx| ctx.audio.load_mixer(&name));
    assert_eq!(seen.lock().unwrap().clone(), [0.5, 0.5, 0.5, 0.5]);
}

#[test]
fn mixer_state_is_saved_and_restored_with_the_settings() {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Settings {
        fullscreen: bool,
        mixer: MixerState,
    }
    let mixer = r#"{
        "buses": [{"name": "music"}, {"name": "world", "effects": [{"filter": {}}]}],
        "snapshots": [{"name": "underwater", "buses": [{"bus": "world", "volume": 0.5}]}]
    }"#;
    let dir = std::env::temp_dir().join(format!("ponko-mixer-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Game::new("mixer", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.audio.apply_mixer_json(mixer).unwrap();
        let music = ctx.audio.bus("music");
        ctx.audio.set_bus_volume(music, 0.35);
        ctx.audio.mute_bus(AudioBus::default(), true);
        ctx.audio.start_snapshot("underwater", 0.0);
        let settings = Settings {
            fullscreen: true,
            mixer: ctx.audio.mixer_state(),
        };
        ctx.storage.write_json("settings.json", &settings).unwrap();
    });
    Game::new("mixer", 1, 1).data_dir(&dir).run_headless(0, |ctx| {
        ctx.audio.apply_mixer_json(mixer).unwrap();
        let settings: Settings = ctx.storage.read_json("settings.json").unwrap();
        assert!(settings.fullscreen);
        ctx.audio.restore_mixer_state(&settings.mixer);
        let (music, world) = (ctx.audio.bus("music"), ctx.audio.bus("world"));
        assert_eq!(ctx.audio.bus_volume(music), 0.35);
        assert!(ctx.audio.bus_muted(AudioBus::default()));
        assert_eq!(ctx.audio.snapshot_weight("underwater"), 1.0);
        assert_eq!(ctx.audio.bus_gain(world), 0.5);
    });
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn find_bus_resolves_only_existing_buses_and_names_them_on_a_typo() {
    Game::new("buses", 1, 1).run_headless(0, |ctx| {
        assert_eq!(
            ctx.audio.find_bus("sfx").unwrap(),
            AudioBus::default(),
            "the default bus exists from the start"
        );
        ctx.audio
            .apply_mixer_json(r#"{"buses": [{"name": "master"}, {"name": "music", "parent": "master"}]}"#)
            .unwrap();
        let music = ctx.audio.find_bus("music").unwrap();
        assert_eq!(music, ctx.audio.bus("music"));
        let err = ctx.audio.find_bus("musci").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert_eq!(
            err.message(),
            r#"unknown audio bus "musci" (known: master, music, sfx)"#
        );
        assert!(ctx.audio.find_bus("musci").is_err(), "finding never makes a bus");
    });
}

#[test]
fn snapshot_ids_act_like_names_and_survive_the_snapshot_being_replaced() {
    use ponko::audio::{MixerSnapshot, SnapshotId};
    Game::new("snapshots", 1, 1).run_headless(0, |ctx| {
        ctx.audio.add_snapshot("paused", MixerSnapshot::default());
        ctx.audio.add_snapshot("underwater", MixerSnapshot::default());
        let underwater = ctx.audio.snapshot_id("underwater").unwrap();
        assert!(ctx.audio.start_snapshot(underwater, 0.0));
        assert_eq!(ctx.audio.snapshot_weight(underwater), 1.0);
        assert_eq!(ctx.audio.snapshot_weight("underwater"), 1.0);
        assert_eq!(ctx.audio.snapshot_weight("paused"), 0.0);
        // Replaced (a reloaded mixer file does the same): the id still names it.
        ctx.audio.add_snapshot("underwater", MixerSnapshot::default());
        assert_eq!(ctx.audio.snapshot_id("underwater").unwrap(), underwater);
        ctx.audio.stop_snapshot(underwater, 0.0);
        assert_eq!(ctx.audio.snapshot_weight(underwater), 0.0);
        assert!(!ctx.audio.start_snapshot(SnapshotId::NONE, 0.0));
        assert_eq!(ctx.audio.snapshot_weight(SnapshotId::default()), 0.0);
        let err = ctx.audio.snapshot_id("under water").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert_eq!(
            err.message(),
            r#"unknown audio snapshot "under water" (known: paused, underwater)"#
        );
    });
}
