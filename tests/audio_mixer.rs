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
