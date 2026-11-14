//! The mixer heard: Kira renders into a buffer instead of a device, and
//! the tests measure each tone in the mix (they sit at frequencies that
//! don't leak into each other over a measuring window).

use super::dsp::buffer::{Output, rms};
use super::{Audio, AudioBus, AudioEffects, Backend, BusEffect, FilterEffect, Sound, SoundParams};

const RATE: u32 = 48_000;
/// 50 ms: a whole number of periods of every test tone.
const WINDOW: f32 = 0.05;

fn mixer() -> (Audio, Output) {
    let (backend, output) = Backend::with_buffer_output(RATE);
    (
        Audio {
            backend,
            ..Default::default()
        },
        output,
    )
}

/// One second of a sine at `hz` (whole periods, so it loops cleanly).
fn sine(audio: &mut Audio, hz: f32, amplitude: f32, seconds: f32) -> Sound {
    let n = (RATE as f32 * seconds) as usize;
    let samples: Vec<f32> = (0..n)
        .map(|i| (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin() * amplitude)
        .collect();
    audio.create(RATE, &samples)
}

const LOOP: SoundParams = SoundParams {
    volume: 1.0,
    speed: 1.0,
    looping: true,
};

/// The amplitude of the `hz` component of the last `WINDOW` of `frames`.
fn amplitude(frames: &[[f32; 2]], hz: f32) -> f32 {
    let n = (WINDOW * RATE as f32) as usize;
    let tail = &frames[frames.len() - n..];
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, f) in tail.iter().enumerate() {
        let phase = f64::from(hz) * std::f64::consts::TAU * i as f64 / f64::from(RATE);
        re += f64::from(f[0]) * phase.cos();
        im += f64::from(f[0]) * phase.sin();
    }
    (2.0 * (re * re + im * im).sqrt() / n as f64) as f32
}

/// Renders `seconds`, then lets frame-clock work (snapshot fades) catch up
/// in 1/60 s frames, as a game would.
fn run(audio: &mut Audio, out: &Output, seconds: f32) -> Vec<[f32; 2]> {
    let mut frames = Vec::new();
    let mut left = seconds;
    while left > 1e-6 {
        let step = left.min(1.0 / 60.0);
        audio.advance_mixer(step);
        frames.extend(out.render(step));
        left -= step;
    }
    frames
}

#[test]
fn nested_buses_multiply_their_volumes_in_the_mix() {
    let (mut audio, out) = mixer();
    let tone = sine(&mut audio, 400.0, 0.5, 1.0);
    let (master, music) = (audio.bus("master"), audio.bus("music"));
    audio.set_bus_parent(music, Some(master)).unwrap();
    audio.play_on(tone, LOOP, music, AudioEffects::default());
    let full = amplitude(&run(&mut audio, &out, 0.2), 400.0);
    assert!((full - 0.5).abs() < 0.02, "{full}");
    audio.set_bus_volume(master, 0.5);
    audio.set_bus_volume(music, 0.5);
    let quarter = amplitude(&run(&mut audio, &out, 0.2), 400.0);
    assert!((quarter - 0.125).abs() < 0.01, "{quarter}");
    audio.mute_bus(master, true);
    let muted = rms(&run(&mut audio, &out, 0.2)[(RATE / 10) as usize..]);
    assert!(muted < 1e-4, "a muted parent silences its children: {muted}");
}

#[test]
fn ducking_lowers_music_while_dialogue_plays_and_lets_it_back_up() {
    let (mut audio, out) = mixer();
    let (music, dialogue) = (audio.bus("music"), audio.bus("dialogue"));
    audio
        .set_bus_ducking(
            music,
            &[super::Ducking {
                gain: 0.25,
                attack: 0.05,
                release: 0.2,
                ..super::Ducking::new(dialogue)
            }],
        )
        .unwrap();
    let song = sine(&mut audio, 200.0, 0.4, 1.0);
    let line = sine(&mut audio, 1000.0, 0.4, 0.5);
    audio.play_on(song, LOOP, music, AudioEffects::default());
    let before = amplitude(&run(&mut audio, &out, 0.3), 200.0);
    assert!((before - 0.4).abs() < 0.02, "{before}");
    audio.play_on(line, SoundParams::default(), dialogue, AudioEffects::default());
    let during = run(&mut audio, &out, 0.3);
    let ducked = amplitude(&during, 200.0);
    assert!(
        (ducked - 0.1).abs() < 0.015,
        "music at a quarter under the line: {ducked}"
    );
    assert!(
        (amplitude(&during, 1000.0) - 0.4).abs() < 0.02,
        "the dialogue itself is not ducked"
    );
    // The line ends at 0.5 s; 0.2 s of release later the music is back.
    let after = amplitude(&run(&mut audio, &out, 0.6), 200.0);
    assert!((after - 0.4).abs() < 0.02, "{after}");
}

#[test]
fn a_muted_key_does_not_duck() {
    let (mut audio, out) = mixer();
    let (music, dialogue) = (audio.bus("music"), audio.bus("dialogue"));
    audio.set_bus_ducking(music, &[super::Ducking::new(dialogue)]).unwrap();
    audio.mute_bus(dialogue, true);
    let song = sine(&mut audio, 200.0, 0.4, 1.0);
    let line = sine(&mut audio, 1000.0, 0.4, 1.0);
    audio.play_on(song, LOOP, music, AudioEffects::default());
    audio.play_on(line, LOOP, dialogue, AudioEffects::default());
    let level = amplitude(&run(&mut audio, &out, 0.3), 200.0);
    assert!((level - 0.4).abs() < 0.02, "{level}");
}

#[test]
fn bus_filter_follows_a_snapshot_fade() {
    let (mut audio, out) = mixer();
    let world = audio.bus("world");
    audio.set_bus_effects(world, &[BusEffect::Filter(FilterEffect::default())]);
    audio.add_snapshot(
        "underwater",
        super::MixerSnapshot {
            buses: vec![super::BusSnapshot {
                bus: world,
                volume: None,
                effects: vec![super::EffectOverride {
                    index: 0,
                    effect: BusEffect::Filter(FilterEffect {
                        cutoff_hz: 300.0,
                        ..Default::default()
                    }),
                }],
            }],
        },
    );
    let low = sine(&mut audio, 100.0, 0.3, 1.0);
    let high = sine(&mut audio, 4000.0, 0.3, 1.0);
    audio.play_on(low, LOOP, world, AudioEffects::default());
    audio.play_on(high, LOOP, world, AudioEffects::default());
    let clear = run(&mut audio, &out, 0.2);
    assert!((amplitude(&clear, 4000.0) - 0.3).abs() < 0.02);
    audio.start_snapshot("underwater", 0.5);
    let fading = amplitude(&run(&mut audio, &out, 0.25), 4000.0);
    let under = run(&mut audio, &out, 0.5);
    let muffled = amplitude(&under, 4000.0);
    assert!(fading < 0.25 && fading > muffled, "halfway through the fade: {fading}");
    assert!(muffled < 0.01, "4 kHz is gone under a 300 Hz low-pass: {muffled}");
    assert!(amplitude(&under, 100.0) > 0.25, "100 Hz stays");
    audio.stop_snapshot("underwater", 0.0);
    let back = amplitude(&run(&mut audio, &out, 0.2), 4000.0);
    assert!((back - 0.3).abs() < 0.02, "{back}");
}

#[test]
fn rebuilding_a_bus_keeps_its_voice_playing_from_where_it_was() {
    let (mut audio, out) = mixer();
    let music = audio.bus("music");
    let song = sine(&mut audio, 200.0, 0.4, 2.0);
    let voice = audio.play_on(song, LOOP, music, AudioEffects::default());
    run(&mut audio, &out, 0.5);
    // A new parent and a new chain: both rebuild the bus.
    let master = audio.bus("master");
    audio.set_bus_parent(music, Some(master)).unwrap();
    audio.set_bus_effects(
        music,
        &[BusEffect::Reverb(super::ReverbEffect {
            mix: 0.0,
            ..Default::default()
        })],
    );
    let after = run(&mut audio, &out, 0.25);
    assert!((amplitude(&after, 200.0) - 0.4).abs() < 0.02);
    let position = audio.backend.voice_position(voice.0).unwrap();
    assert!((position - 0.75).abs() < 0.02, "carried on from 0.5 s: {position}");
    audio.stop(voice);
}

#[test]
fn bus_compressor_and_delay_change_the_mix() {
    let (mut audio, out) = mixer();
    let bus = audio.bus("fx");
    let tone = sine(&mut audio, 400.0, 0.8, 1.0);
    audio.set_bus_effects(
        bus,
        &[BusEffect::Compressor(super::CompressorEffect {
            threshold_db: -30.0,
            ratio: 8.0,
            ..Default::default()
        })],
    );
    let loud = audio.play_on(tone, LOOP, bus, AudioEffects::default());
    let squashed = amplitude(&run(&mut audio, &out, 0.3), 400.0);
    assert!(squashed < 0.4, "loud tone compressed: {squashed}");
    audio.stop(loud);
    run(&mut audio, &out, 0.1);
    let echo = audio.bus("echo");
    audio.set_bus_effects(
        echo,
        &[BusEffect::Delay(super::DelayEffect {
            delay: 0.1,
            feedback_db: -6.0,
            mix: 1.0,
        })],
    );
    let click = sine(&mut audio, 1000.0, 0.5, 0.05);
    audio.play_on(click, SoundParams::default(), echo, AudioEffects::default());
    let heard = run(&mut audio, &out, 0.2);
    let start = heard.len() - (0.2 * RATE as f32) as usize;
    let early = rms(&heard[start..start + (0.04 * RATE as f32) as usize]);
    let late = rms(&heard[start + (0.11 * RATE as f32) as usize..start + (0.14 * RATE as f32) as usize]);
    assert!(early < 1e-4, "fully wet: nothing before the echo: {early}");
    // The first echo comes at the feedback level, -6 dB: half the click's 0.35 RMS.
    assert!((late - 0.177).abs() < 0.03, "the click comes 0.1 s late: {late}");
}

#[test]
fn example_mixer_file_ducks_music_under_dialogue_and_goes_underwater() {
    let (mut audio, out) = mixer();
    audio
        .apply_mixer_json(include_str!("../../assets/audio_mixer.json"))
        .unwrap();
    let (music, dialogue, world) = (audio.bus("music"), audio.bus("dialogue"), audio.bus("world"));
    let song = sine(&mut audio, 200.0, 0.4, 1.0);
    let line = sine(&mut audio, 1000.0, 0.2, 0.5);
    let steps = sine(&mut audio, 4000.0, 0.3, 1.0);
    audio.play_on(song, LOOP, music, AudioEffects::default());
    audio.play_on(steps, LOOP, world, AudioEffects::default());
    let calm = run(&mut audio, &out, 0.3);
    let music_level = amplitude(&calm, 200.0);
    assert!((music_level - 0.32).abs() < 0.02, "music bus at 0.8: {music_level}");
    let world_level = amplitude(&calm, 4000.0);
    audio.play_on(line, SoundParams::default(), dialogue, AudioEffects::default());
    let ducked = amplitude(&run(&mut audio, &out, 0.3), 200.0);
    assert!(ducked < music_level * 0.4, "music ducks under dialogue: {ducked}");
    run(&mut audio, &out, 1.0);
    audio.start_snapshot("underwater", 1.0);
    let under = run(&mut audio, &out, 1.2);
    assert!(
        amplitude(&under, 4000.0) < world_level * 0.05,
        "the world is muffled underwater"
    );
    assert!((amplitude(&under, 200.0) - 0.2).abs() < 0.02, "music at 0.5 underwater");
    assert_eq!(audio.snapshot_weight("underwater"), 1.0);
    let _ = AudioBus::default();
}
