//! Loops, fades, speed and pitch, pause groups, voice limits, music on the
//! beat and output devices, heard: Kira renders into buffers (a fake set
//! of devices for the device tests), never to a real device, and the tests
//! measure each tone in the mix.

use super::backend::fake::{Devices, FakeOutputs};
use super::backend::{DeviceIssue, Outputs};
use super::dsp::buffer::{Output, rms};
use super::{
    Audio, AudioBus, AudioDeviceEvent, AudioEffects, Backend, BusEffect, PitchShiftEffect, Quantize, Song, Sound,
    SoundParams,
};
use std::sync::{Arc, Mutex};

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

/// `seconds` of a sine at `hz`.
fn sine(audio: &mut Audio, hz: f32, amplitude: f32, seconds: f32) -> Sound {
    let n = (RATE as f32 * seconds) as usize;
    let samples: Vec<f32> = (0..n)
        .map(|i| (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin() * amplitude)
        .collect();
    audio.create(RATE, &samples)
}

/// `first` seconds at `a` Hz, then `then` seconds at `b` Hz, as one sound.
fn two_tones(audio: &mut Audio, a: f32, first: f32, b: f32, then: f32) -> Sound {
    let tone = |hz: f32, seconds: f32| {
        (0..(RATE as f32 * seconds) as usize)
            .map(move |i| (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin() * 0.4)
    };
    let samples: Vec<f32> = tone(a, first).chain(tone(b, then)).collect();
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

/// Renders `seconds` in 1/60 s frames, moving the game side along as a
/// game would.
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
fn a_looping_sound_plays_its_intro_once_then_loops_the_rest() {
    let (mut audio, out) = mixer();
    // 0.3 s of intro at 400 Hz, then 0.4 s of loop at 600 Hz.
    let song = two_tones(&mut audio, 400.0, 0.3, 600.0, 0.4);
    audio.set_loop(song, 0.3, None);
    audio.play_ex(song, LOOP);
    let intro = run(&mut audio, &out, 0.2);
    assert!(amplitude(&intro, 400.0) > 0.35, "the intro first");
    // Past the end (0.7 s) and around the loop twice: no intro again.
    for _ in 0..8 {
        let block = run(&mut audio, &out, 0.15);
        assert!(amplitude(&block, 400.0) < 0.05, "{}", amplitude(&block, 400.0));
    }
    let late = run(&mut audio, &out, 0.1);
    assert!(amplitude(&late, 600.0) > 0.35, "the loop goes on");
}

#[test]
fn loop_end_turns_around_before_the_end_of_the_sound() {
    let (mut audio, out) = mixer();
    // A tail at 900 Hz after the loop body: never heard while looping.
    let song = two_tones(&mut audio, 600.0, 0.4, 900.0, 0.3);
    audio.set_loop(song, 0.0, Some(0.4));
    audio.play_ex(song, LOOP);
    for _ in 0..10 {
        let block = run(&mut audio, &out, 0.1);
        assert!(amplitude(&block, 900.0) < 0.05);
    }
}

#[test]
fn volume_fades_over_the_time_given_then_holds() {
    let (mut audio, out) = mixer();
    let tone = sine(&mut audio, 400.0, 0.5, 1.0);
    let voice = audio.play_ex(tone, LOOP);
    run(&mut audio, &out, 0.1);
    audio.set_volume(voice, 0.0, 0.5);
    let half = amplitude(&run(&mut audio, &out, 0.25), 400.0);
    assert!((half - 0.25).abs() < 0.05, "halfway down: {half}");
    let gone = amplitude(&run(&mut audio, &out, 0.4), 400.0);
    assert!(gone < 0.01, "{gone}");
    audio.set_volume(voice, 1.0, 0.0);
    let back = amplitude(&run(&mut audio, &out, 0.1), 400.0);
    assert!((back - 0.5).abs() < 0.03, "at once: {back}");
}

#[test]
fn a_crossfade_hands_one_sound_over_to_the_other() {
    let (mut audio, out) = mixer();
    let (a, b) = (sine(&mut audio, 400.0, 0.4, 1.0), sine(&mut audio, 600.0, 0.4, 1.0));
    let old = audio.play_ex(a, LOOP);
    run(&mut audio, &out, 0.1);
    audio.fade_out(old, 0.5);
    let new = audio.play_ex(b, LOOP);
    audio.set_volume(new, 0.0, 0.0);
    audio.set_volume(new, 1.0, 0.5);
    let middle = run(&mut audio, &out, 0.25);
    let (ma, mb) = (amplitude(&middle, 400.0), amplitude(&middle, 600.0));
    assert!(ma > 0.1 && ma < 0.3 && mb > 0.1 && mb < 0.3, "both halfway: {ma} {mb}");
    let end = run(&mut audio, &out, 0.5);
    assert!(amplitude(&end, 400.0) < 0.01);
    assert!((amplitude(&end, 600.0) - 0.4).abs() < 0.03);
    assert_eq!(audio.voice_count().real, 1, "the faded voice ended");
}

#[test]
fn speed_raises_pitch_and_tempo_together() {
    let (mut audio, out) = mixer();
    let tone = sine(&mut audio, 300.0, 0.4, 1.0);
    let voice = audio.play_ex(tone, LOOP);
    run(&mut audio, &out, 0.1);
    audio.set_speed(voice, 2.0, 0.0);
    let fast = run(&mut audio, &out, 0.1);
    assert!(amplitude(&fast, 600.0) > 0.35, "an octave up");
    assert!(amplitude(&fast, 300.0) < 0.05);
}

#[test]
fn a_pitch_shift_changes_the_pitch_and_not_the_tempo() {
    let (mut audio, out) = mixer();
    let bus = audio.bus("shifted");
    audio.set_bus_effects(bus, &[BusEffect::PitchShift(PitchShiftEffect::semitones(12.0))]);
    // A 0.5 s tone: still 0.5 s long after the shift.
    let tone = sine(&mut audio, 300.0, 0.4, 0.5);
    audio.play_on(tone, SoundParams::default(), bus, AudioEffects::default());
    let heard = run(&mut audio, &out, 0.3);
    let up = amplitude(&heard, 600.0);
    assert!(up > 0.2, "an octave up: {up}");
    assert!(amplitude(&heard, 300.0) < 0.1, "{}", amplitude(&heard, 300.0));
    let still = run(&mut audio, &out, 0.15);
    assert!(amplitude(&still, 600.0) > 0.2, "still playing at 0.45 s");
    let after = run(&mut audio, &out, 0.2);
    assert!(rms(&after[RATE as usize / 10..]) < 0.01, "and over by 0.6 s");
    // Speed with the shift undone: faster, same pitch.
    let plain = audio.bus("plain");
    audio.set_bus_effects(plain, &[BusEffect::PitchShift(PitchShiftEffect::undo_speed(1.5))]);
    let long = sine(&mut audio, 300.0, 0.4, 2.0);
    audio.play_on(long, SoundParams { speed: 1.5, ..LOOP }, plain, AudioEffects::default());
    let kept = run(&mut audio, &out, 0.3);
    assert!(amplitude(&kept, 300.0) > 0.2, "{}", amplitude(&kept, 300.0));
    assert!(amplitude(&kept, 450.0) < 0.1);
}

#[test]
fn a_paused_bus_holds_its_sounds_where_they_are() {
    let (mut audio, out) = mixer();
    let (world, menu) = (audio.bus("world"), audio.bus("menu"));
    let rain = sine(&mut audio, 400.0, 0.4, 2.0);
    let click = sine(&mut audio, 1000.0, 0.4, 2.0);
    let voice = audio.play_on(rain, LOOP, world, AudioEffects::default());
    audio.play_on(click, LOOP, menu, AudioEffects::default());
    run(&mut audio, &out, 0.5);
    audio.pause_bus(world, true);
    let at = audio.backend.voice_position(voice.0).unwrap();
    let paused = run(&mut audio, &out, 0.5);
    assert!(amplitude(&paused, 400.0) < 0.01, "silent while paused");
    assert!(amplitude(&paused, 1000.0) > 0.35, "the menu bus plays on");
    let held = audio.backend.voice_position(voice.0).unwrap();
    assert!((held - at).abs() < 0.03, "held at {at}: {held}");
    // A sound started on a paused bus waits for it too.
    let late = sine(&mut audio, 600.0, 0.4, 2.0);
    audio.play_on(late, LOOP, world, AudioEffects::default());
    assert!(amplitude(&run(&mut audio, &out, 0.2), 600.0) < 0.01);
    audio.pause_bus(world, false);
    let resumed = run(&mut audio, &out, 0.2);
    assert!(amplitude(&resumed, 400.0) > 0.35);
    assert!(amplitude(&resumed, 600.0) > 0.35);
    let moved = audio.backend.voice_position(voice.0).unwrap();
    assert!((moved - held - 0.2).abs() < 0.03, "carried on from {held}: {moved}");
}

#[test]
fn only_the_loudest_voices_up_to_the_limit_are_heard() {
    let (mut audio, out) = mixer();
    audio.set_voice_limit(2);
    let tones: Vec<Sound> = [300.0, 500.0, 700.0, 900.0]
        .into_iter()
        .map(|hz| sine(&mut audio, hz, 0.2, 2.0))
        .collect();
    let volumes = [0.4, 1.0, 0.7, 0.2];
    let voices: Vec<_> = tones
        .iter()
        .zip(volumes)
        .map(|(&t, volume)| audio.play_ex(t, SoundParams { volume, ..LOOP }))
        .collect();
    let mix = run(&mut audio, &out, 0.2);
    assert!(amplitude(&mix, 500.0) > 0.15 && amplitude(&mix, 700.0) > 0.1);
    assert!(amplitude(&mix, 300.0) < 0.01 && amplitude(&mix, 900.0) < 0.01);
    assert_eq!((audio.voice_count().real, audio.voice_count().virtualized), (2, 2));
    assert!(audio.is_virtual(voices[0]) && !audio.is_virtual(voices[1]));
    // The loudest ends: the next one is heard, from where it would be by now.
    audio.stop(voices[1]);
    let after = run(&mut audio, &out, 0.1);
    assert!(amplitude(&after, 300.0) > 0.06, "{}", amplitude(&after, 300.0));
    let position = audio.backend.voice_position(voices[0].0).unwrap();
    assert!((position - 0.3).abs() < 0.04, "it kept time while virtual: {position}");
}

#[test]
fn priority_wins_over_loudness() {
    let (mut audio, out) = mixer();
    audio.set_voice_limit(1);
    let (quiet, loud) = (sine(&mut audio, 300.0, 0.2, 1.0), sine(&mut audio, 500.0, 0.4, 1.0));
    let important = audio.play_ex(quiet, LOOP);
    audio.set_priority(important, 250);
    audio.play_ex(loud, LOOP);
    let mix = run(&mut audio, &out, 0.2);
    assert!(amplitude(&mix, 300.0) > 0.15);
    assert!(amplitude(&mix, 500.0) < 0.01);
}

#[test]
fn a_bus_limit_caps_its_voices_and_inaudible_ones_go_virtual() {
    let (mut audio, out) = mixer();
    let steps = audio.bus("footsteps");
    audio.set_bus_voice_limit(steps, Some(1));
    let (a, b) = (sine(&mut audio, 300.0, 0.3, 1.0), sine(&mut audio, 500.0, 0.3, 1.0));
    audio.play_on(a, LOOP, steps, AudioEffects::default());
    let second = audio.play_on(b, SoundParams { volume: 0.5, ..LOOP }, steps, AudioEffects::default());
    let silent = audio.play_ex(b, LOOP);
    audio.set_volume(silent, 0.0, 0.0);
    let mix = run(&mut audio, &out, 0.2);
    assert!(amplitude(&mix, 300.0) > 0.25);
    assert!(audio.is_virtual(second));
    assert!(audio.is_virtual(silent), "volume 0: virtual even with room");
    audio.set_volume(silent, 1.0, 0.0);
    run(&mut audio, &out, 0.05);
    assert!(!audio.is_virtual(silent), "audible again: heard");
}

#[test]
fn a_virtual_one_shot_ends_on_time() {
    let (mut audio, out) = mixer();
    audio.set_voice_limit(1);
    let (long, short) = (sine(&mut audio, 300.0, 0.4, 2.0), sine(&mut audio, 500.0, 0.1, 0.3));
    audio.play_ex(long, LOOP);
    let shot = audio.play_ex(short, SoundParams::default());
    run(&mut audio, &out, 0.1);
    assert!(audio.is_virtual(shot));
    run(&mut audio, &out, 0.3);
    assert_eq!(audio.voice_count().virtualized, 0, "it ended after 0.3 s");
}

fn song(audio: &mut Audio) -> (Song, AudioBus) {
    // 8 s stems at 120 bpm: 4 bars.
    let base = sine(audio, 200.0, 0.3, 8.0);
    let drums = sine(audio, 700.0, 0.3, 8.0);
    (Song::new(base).layer(drums, 0.0).tempo(120.0, 4), audio.music_bus())
}

#[test]
fn a_layer_comes_in_on_the_next_bar_line() {
    let (mut audio, out) = mixer();
    let (song, _) = song(&mut audio);
    audio.play_music(song, 0.0, Quantize::Now);
    run(&mut audio, &out, 0.5);
    audio.set_music_layer(1, 1.0, 0.0, Quantize::Bar);
    // The bar line is at 2 s (four beats at 120 bpm).
    let before = run(&mut audio, &out, 1.4);
    assert!(amplitude(&before, 700.0) < 0.01, "not before the bar");
    assert!(amplitude(&before, 200.0) > 0.25);
    let after = run(&mut audio, &out, 0.25);
    assert!(amplitude(&after, 700.0) > 0.25, "on it");
    assert_eq!(audio.music_layer_volume(1), 1.0);
    let beat = audio.backend.music_clock_beat().unwrap();
    assert!((beat - 4.3).abs() < 0.1, "the clock and the music agree: {beat}");
    assert!(
        (audio.music_beat() - beat).abs() < 0.1,
        "and the game side: {}",
        audio.music_beat()
    );
}

#[test]
fn layers_stay_in_sync_sample_for_sample() {
    let (mut audio, out) = mixer();
    // The same tone twice, the second inverted: in sync they cancel.
    let tone: Vec<f32> = (0..RATE * 2)
        .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() * 0.3)
        .collect();
    let inverted: Vec<f32> = tone.iter().map(|s| -s).collect();
    let (a, b) = (audio.create(RATE, &tone), audio.create(RATE, &inverted));
    audio.play_music(Song::new(a).layer(b, 1.0).tempo(120.0, 4), 0.0, Quantize::Now);
    let mix = run(&mut audio, &out, 0.5);
    assert!(rms(&mix) < 1e-4, "{}", rms(&mix));
}

#[test]
fn a_song_change_crossfades_on_the_bar_line() {
    let (mut audio, out) = mixer();
    let (calm, _) = song(&mut audio);
    let fight = sine(&mut audio, 440.0, 0.3, 8.0);
    audio.play_music(calm, 0.0, Quantize::Now);
    run(&mut audio, &out, 0.7);
    audio.play_music(Song::new(fight).tempo(140.0, 4), 0.0, Quantize::Bar);
    let before = run(&mut audio, &out, 1.2);
    assert!(amplitude(&before, 440.0) < 0.01, "not before the bar line at 2 s");
    let after = run(&mut audio, &out, 0.3);
    assert!(amplitude(&after, 440.0) > 0.25);
    assert!(amplitude(&after, 200.0) < 0.01);
    assert_eq!(audio.music().unwrap().bpm(), 140.0);
    // The new grid runs at 140 bpm from the bar line, 0.2 s ago.
    let beat = audio.backend.music_clock_beat().unwrap();
    assert!(
        (beat - 0.2 * 140.0 / 60.0).abs() < 0.02,
        "0.2 s into the new song: {beat}"
    );
    assert!(
        (audio.music_beat() - beat).abs() < 0.1,
        "the game side agrees: {}",
        audio.music_beat()
    );
}

#[test]
fn a_stinger_waits_for_the_beat() {
    let (mut audio, out) = mixer();
    let (song, _) = song(&mut audio);
    let fanfare = sine(&mut audio, 1100.0, 0.3, 0.3);
    audio.play_music(song, 0.0, Quantize::Now);
    run(&mut audio, &out, 0.1);
    audio.play_stinger(fanfare, Quantize::Beat);
    // The next beat is at 0.5 s.
    assert!(amplitude(&run(&mut audio, &out, 0.3), 1100.0) < 0.01);
    assert!(amplitude(&run(&mut audio, &out, 0.2), 1100.0) > 0.25);
}

#[test]
fn faster_music_moves_its_grid_with_it() {
    let (mut audio, out) = mixer();
    let (song, _) = song(&mut audio);
    audio.play_music(song, 0.0, Quantize::Now);
    audio.set_music_speed(2.0, 0.0);
    let mix = run(&mut audio, &out, 0.5);
    assert!(amplitude(&mix, 400.0) > 0.25, "an octave up");
    let beat = audio.backend.music_clock_beat().unwrap();
    assert!((beat - 2.0).abs() < 0.1, "two beats in 0.5 s: {beat}");
}

#[test]
fn music_pauses_with_its_bus_and_keeps_its_grid() {
    let (mut audio, out) = mixer();
    let (song, music) = song(&mut audio);
    audio.play_music(song, 0.0, Quantize::Now);
    run(&mut audio, &out, 0.5);
    audio.pause_bus(music, true);
    run(&mut audio, &out, 1.0);
    let beat = audio.backend.music_clock_beat().unwrap();
    assert!((beat - 1.0).abs() < 0.1, "the clock held: {beat}");
    assert!((audio.music_beat() - 1.0).abs() < 0.05);
}

/// A backend on fake devices A (default) and B.
fn devices() -> (Audio, Arc<Mutex<Devices>>) {
    let fake = FakeOutputs::default();
    {
        let mut d = fake.0.lock().unwrap();
        d.list = vec![FakeOutputs::device("A", true), FakeOutputs::device("B", false)];
    }
    let shared = fake.0.clone();
    let audio = Audio {
        backend: Backend::with_outputs(Box::new(fake)),
        ..Default::default()
    };
    (audio, shared)
}

/// The output most recently opened, and the device it is on.
fn latest(devices: &Arc<Mutex<Devices>>) -> (String, Output) {
    let d = devices.lock().unwrap();
    let (id, out) = d.opened.last().expect("an output was opened");
    (
        id.clone(),
        Output {
            renderer: out.renderer.clone(),
            sample_rate: out.sample_rate,
        },
    )
}

#[test]
fn the_device_list_and_choice_reach_the_output() {
    let (mut audio, devices) = devices();
    let names: Vec<String> = audio.output_devices().into_iter().map(|d| d.id).collect();
    assert_eq!(names, ["A", "B"]);
    assert!(audio.set_output_device(Some("C")).is_err());
    audio.set_output_device(Some("B speakers")).unwrap();
    let tone = sine(&mut audio, 400.0, 0.4, 2.0);
    audio.play_ex(tone, LOOP);
    let (id, out) = latest(&devices);
    assert_eq!(id, "B", "opened on the chosen device");
    assert!(amplitude(&run(&mut audio, &out, 0.2), 400.0) > 0.35);
    assert_eq!(audio.output_device().unwrap().id, "B");
    // Back to the default while playing: the sound moves over.
    audio.set_output_device(None).unwrap();
    let (id, out) = latest(&devices);
    assert_eq!(id, "A");
    assert!(amplitude(&run(&mut audio, &out, 0.2), 400.0) > 0.35);
}

#[test]
fn a_held_output_closes_and_its_sounds_go_on_from_where_they_were() {
    let (mut audio, devices) = devices();
    let tone = sine(&mut audio, 400.0, 0.4, 4.0);
    let voice = audio.play_ex(tone, LOOP);
    let (_, a) = latest(&devices);
    run(&mut audio, &out_of(&a), 0.5);
    // The app goes to the background: the output closes.
    audio.hold_output(true);
    assert!(audio.backend.output_held());
    assert_eq!(audio.backend.device_state(), "failed", "no stream while held");
    // Frames that still run (a call in front) open nothing, and a sound
    // played meanwhile waits.
    audio.advance_mixer(1.0 / 60.0);
    audio.play_ex(tone, LOOP);
    assert_eq!(devices.lock().unwrap().opened.len(), 1, "nothing opened while held");
    // Back in front: opened again, the voice from where it was.
    audio.hold_output(false);
    assert!(!audio.backend.output_held());
    assert_eq!(devices.lock().unwrap().opened.len(), 2);
    let position = audio.backend.voice_position(voice.0).unwrap();
    assert!((position - 0.5).abs() < 0.05, "from where it was: {position}");
    let (_, again) = latest(&devices);
    assert!(
        amplitude(&run(&mut audio, &out_of(&again), 0.2), 400.0) > 0.35,
        "heard again"
    );
    // Held before anything opened: it stays closed until used.
    let (mut fresh, devices) = self::devices();
    fresh.hold_output(true);
    fresh.hold_output(false);
    assert!(devices.lock().unwrap().opened.is_empty());
}

#[test]
fn a_muted_output_plays_silence_and_everything_else_runs_as_usual() {
    let (mut audio, devices) = devices();
    devices.lock().unwrap().muted = true;
    let tone = sine(&mut audio, 400.0, 0.4, 4.0);
    let voice = audio.play_ex(tone, LOOP);
    let (_, a) = latest(&devices);
    let heard = run(&mut audio, &out_of(&a), 0.5);
    assert!(amplitude(&heard, 400.0) < 1e-4, "muted: silence");
    let position = audio.backend.voice_position(voice.0).unwrap();
    assert!((position - 0.5).abs() < 0.05, "the voice plays on: {position}");
    // The negative control: the same tone on an output that isn't muted.
    devices.lock().unwrap().muted = false;
    audio.hold_output(true);
    audio.hold_output(false);
    let (_, b) = latest(&devices);
    assert!(amplitude(&run(&mut audio, &out_of(&b), 0.2), 400.0) > 0.35, "heard");
}

#[test]
fn a_lost_device_moves_sound_to_the_default_and_back_when_it_returns() {
    let (mut audio, devices) = devices();
    audio.set_output_device(Some("B")).unwrap();
    let tone = sine(&mut audio, 400.0, 0.4, 4.0);
    let voice = audio.play_ex(tone, LOOP);
    let (_, b) = latest(&devices);
    run(&mut audio, &out_of(&b), 0.5);
    // Unplugged.
    {
        let mut d = devices.lock().unwrap();
        d.list.retain(|d| d.id != "B");
        d.issues.push_back(DeviceIssue::Lost);
    }
    audio.advance_mixer(1.0 / 60.0);
    assert_eq!(
        audio.device_events(),
        [
            AudioDeviceEvent::Lost("B speakers".into()),
            AudioDeviceEvent::Switched("A speakers".into())
        ]
    );
    let (id, a) = latest(&devices);
    assert_eq!(id, "A");
    let on_a = run(&mut audio, &a, 0.2);
    assert!(amplitude(&on_a, 400.0) > 0.35, "it plays on, on the default");
    let position = audio.backend.voice_position(voice.0).unwrap();
    assert!((position - 0.72).abs() < 0.05, "from where it was: {position}");
    assert!(audio.device_events().is_empty(), "events last one frame");
    assert_eq!(audio.chosen_output_device(), Some("B"), "the choice stays");
    // Plugged back in: within two seconds sound goes back to it.
    devices.lock().unwrap().list.push(FakeOutputs::device("B", false));
    let mut back = false;
    for _ in 0..150 {
        audio.advance_mixer(1.0 / 60.0);
        back |= audio.device_events() == [AudioDeviceEvent::Switched("B speakers".into())];
    }
    assert!(back);
    let (id, b) = latest(&devices);
    assert_eq!(id, "B");
    assert!(amplitude(&run(&mut audio, &b, 0.2), 400.0) > 0.35);
}

fn out_of(o: &Output) -> Output {
    Output {
        renderer: o.renderer.clone(),
        sample_rate: o.sample_rate,
    }
}

#[test]
fn losing_every_device_keeps_trying_and_the_music_comes_back_in_time() {
    let (mut audio, devices) = devices();
    let (song, _) = song(&mut audio);
    audio.play_music(song, 0.0, Quantize::Now);
    let (_, a) = latest(&devices);
    run(&mut audio, &a, 1.0);
    {
        let mut d = devices.lock().unwrap();
        d.list.clear();
        d.issues.push_back(DeviceIssue::Lost);
    }
    audio.advance_mixer(1.0 / 60.0);
    assert_eq!(audio.device_events(), [AudioDeviceEvent::Lost("A speakers".into())]);
    for _ in 0..30 {
        audio.advance_mixer(1.0 / 60.0);
    }
    devices.lock().unwrap().list.push(FakeOutputs::device("A", true));
    let mut back = false;
    for _ in 0..150 {
        audio.advance_mixer(1.0 / 60.0);
        back |= !audio.device_events().is_empty();
    }
    assert!(back, "opened again once a device is there");
    let (_, a) = latest(&devices);
    let mix = run(&mut audio, &a, 0.2);
    assert!(amplitude(&mix, 200.0) > 0.25, "the music plays again");
    let beat = audio.backend.music_clock_beat().unwrap();
    let heard = audio
        .backend
        .voice_position(audio.backend.music_layer_id(0).unwrap())
        .unwrap()
        * 2.0;
    assert!(
        (beat - heard).abs() < 0.1,
        "its grid follows where it is: {beat} {heard}"
    );
}

#[test]
fn the_fake_outputs_open_only_listed_devices() {
    let mut fake = FakeOutputs::default();
    assert!(fake.open(None).is_none(), "no devices: nothing opens");
}
