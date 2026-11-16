//! Sounds, checked headless: loading and decoding work, playing is silent.

use ponko::actions::InputEvent;
use ponko::prelude::*;
use ponko::scenario::InputScenario;

fn in_tick(f: impl FnMut(&mut Ctx) + 'static) {
    Game::new("audio", 1, 1).sim_system(f).run_headless(1, |_| {});
}

// Decoding sound files needs the `audio` feature: without it a sound only
// has to exist, and lasts zero seconds.

/// A mono 16-bit PCM WAV file.
#[cfg(feature = "audio")]
fn wav(sample_rate: u32, samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(feature = "audio")]
fn temp_file(name: &str, bytes: &[u8]) -> String {
    let path = std::env::temp_dir().join(format!("ponko-audio-{}-{name}", std::process::id()));
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn generated_sounds_know_their_length() {
    in_tick(|ctx| {
        let beep = ctx.audio.create(8000, &vec![0.0; 4000]);
        assert!((beep.seconds() - 0.5).abs() < 1e-3, "{}", beep.seconds());
        let playing = ctx.audio.play(beep);
        ctx.audio.stop(playing); // silent headless, but fine to call
    });
}

#[cfg(feature = "audio")]
#[test]
fn wav_files_load() {
    let path = temp_file("tone.wav", &wav(22_050, &vec![1000; 22_050 / 4]));
    in_tick(move |ctx| {
        let tone = ctx.audio.load(&path);
        assert!((tone.seconds() - 0.25).abs() < 0.01, "{}", tone.seconds());
        ctx.audio.play_ex(
            tone,
            SoundParams {
                volume: 0.5,
                speed: 2.0,
                looping: true,
            },
        );
    });
}

#[cfg(feature = "audio")]
#[test]
#[should_panic(expected = "could not decode the sound")]
fn files_that_are_not_sound_say_so() {
    let path = temp_file("bad.ogg", b"this is not audio");
    Game::new("audio", 1, 1).run_headless(0, move |ctx| {
        ctx.audio.load(&path);
    });
}

#[test]
#[should_panic(expected = "not found in assets/")]
fn missing_sounds_say_where_they_looked() {
    Game::new("audio", 1, 1).run_headless(0, |ctx| {
        ctx.audio.load("no-such-sound.ogg");
    });
}

#[cfg(feature = "audio")]
#[test]
fn loading_a_sound_file_twice_gives_the_same_sound() {
    let path = temp_file("twice.wav", &wav(22_050, &vec![1000; 2205]));
    in_tick(move |ctx| {
        let a = ctx.audio.load(&path);
        let b = ctx.audio.load(&path);
        assert_eq!(a, b);
        assert_ne!(a, ctx.audio.create(8000, &[0.0; 80]));
    });
}

#[test]
fn started_lists_what_was_played_with_its_bus_and_volume_and_each_gets_its_own_handle() {
    in_tick(|ctx| {
        let beep = ctx.audio.create(8000, &[0.0; 800]);
        let music = ctx.audio.bus("music");
        assert!(ctx.audio.started().is_empty());
        let a = ctx.audio.play(beep);
        let b = ctx.audio.play_on(
            beep,
            SoundParams {
                volume: 0.25,
                speed: 2.0,
                looping: true,
            },
            music,
            ponko::audio::AudioEffects::default(),
        );
        assert_ne!(a, b, "headless handles differ too");
        let sfx = ctx.audio.bus("sfx");
        let started = ctx.audio.started();
        assert_eq!(started.len(), 2);
        assert_eq!((started[0].playing, started[0].bus, started[0].volume), (a, sfx, 1.0));
        assert_eq!(
            (
                started[1].playing,
                started[1].bus,
                started[1].volume,
                started[1].speed,
                started[1].looping
            ),
            (b, music, 0.25, 2.0, true)
        );
    });
}

#[test]
fn started_starts_over_each_tick_and_each_frame() {
    #[derive(Default)]
    struct Counts(Vec<(&'static str, u64, usize)>);
    let world = Game::new("audio", 1, 1)
        .setup(|ctx| {
            let beep = ctx.audio.create(8000, &[0.0; 800]);
            ctx.world.insert_resource(beep);
            ctx.world.insert_resource(Counts::default());
        })
        .sim_system(|ctx: &mut Ctx| {
            let beep = *ctx.world.resource::<Sound>();
            ctx.audio.play(beep);
            let entry = ("tick", ctx.time.sim_tick(), ctx.audio.started().len());
            ctx.world.resource_mut::<Counts>().0.push(entry);
        })
        .system(|ctx: &mut Ctx| {
            let entry = ("frame before", ctx.time.frame(), ctx.audio.started().len());
            let beep = *ctx.world.resource::<Sound>();
            ctx.audio.play(beep);
            ctx.audio.play(beep);
            let after = ("frame after", ctx.time.frame(), ctx.audio.started().len());
            ctx.world.resource_mut::<Counts>().0.extend([entry, after]);
        })
        .headless_frames(true)
        .run_headless(3);
    let counts: Vec<usize> = world.resource::<Counts>().0.iter().map(|c| c.2).collect();
    // Each tick sees its own sound only; the frame systems after it start from none.
    assert_eq!(counts, [1, 0, 2, 1, 0, 2, 1, 0, 2]);
}

#[cfg(feature = "3d")]
#[test]
fn a_sound_played_at_a_point_is_listed_with_the_distance_fall_off_applied() {
    in_tick(|ctx| {
        let beep = ctx.audio.create(8000, &[0.0; 800]);
        let settings = ponko::d3::AudioSettings::default();
        ctx.audio.play_at(beep, ctx.scene3d.camera.position, settings);
        ctx.audio
            .play_at(beep, ctx.scene3d.camera.position + Vec3::X * 1000.0, settings);
        let started = ctx.audio.started();
        assert_eq!(started[0].volume, 1.0, "at the listener: full volume");
        assert!(started[1].volume < 0.1, "far away: {}", started[1].volume);
        assert_ne!(started[0].playing, started[1].playing);
    });
}

// ANCHOR: headless_ears
#[derive(Default)]
struct Heard(Vec<(u64, f32)>, bool);

#[test]
fn a_test_hears_what_a_click_played_and_sees_the_game_ask_to_quit() {
    let world = Game::new("t", 800, 600)
        .setup(|ctx| {
            let beep = ctx.audio.create(8000, &[0.0; 800]);
            ctx.world.insert_resource(beep);
            ctx.world.insert_resource(Heard::default());
        })
        // The game: a click plays a beep as loud as the pointer is far right; Q quits.
        .sim_system(|ctx: &mut Ctx| {
            if ctx.input.mouse_pressed(MouseButton::Left) {
                let (beep, volume) = (*ctx.world.resource::<Sound>(), ctx.input.mouse_pos().x / 800.0);
                ctx.audio.play_ex(
                    beep,
                    SoundParams {
                        volume,
                        ..Default::default()
                    },
                );
            }
            if ctx.input.key_pressed(Key::Q) {
                ctx.quit();
            }
        })
        // The test's ears.
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let heard: Vec<_> = ctx.audio.started().iter().map(|s| (tick, s.volume)).collect();
            let quit = ctx.quit_requested();
            let log = ctx.world.resource_mut::<Heard>();
            log.0.extend(heard);
            log.1 = quit;
        })
        .input_scenario(
            InputScenario::new()
                .at(1, InputEvent::MouseMove(vec2(600.0, 300.0)))
                .at(2, InputEvent::Mouse(MouseButton::Left, true))
                .at(4, InputEvent::Key(Key::Q, true)),
        )
        .run_headless(60);
    let heard = world.resource::<Heard>();
    assert_eq!(heard.0, [(2, 0.75)]);
    assert!(heard.1, "Q asked to quit");
}
// ANCHOR_END: headless_ears
