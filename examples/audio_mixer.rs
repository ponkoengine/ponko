//! A mixer from a data file: music ducks under dialogue, and U blends to
//! an underwater snapshot. The sounds are made in code. Build only in CI;
//! running it opens a window and plays audio.
//!
//! D plays a line of "dialogue", U toggles underwater, M mutes the music.
//! The mixer state is saved with the settings and restored at the next start.
use ponko::audio::MixerState;
use ponko::prelude::*;

/// What the example keeps between frames.
struct Mixer {
    dialogue: Sound,
    footsteps: Sound,
    music: AudioBus,
    voice: AudioBus,
    world: AudioBus,
    underwater: bool,
}

/// A mono tone (with a few harmonics) that fades in and out at its ends.
fn tone(rate: u32, seconds: f32, hz: &[f32], volume: f32) -> Vec<f32> {
    let n = (rate as f32 * seconds) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let edge = (t / 0.02).min((seconds - t) / 0.02).clamp(0.0, 1.0);
            let sum: f32 = hz.iter().map(|f| (t * f * std::f32::consts::TAU).sin()).sum();
            sum / hz.len() as f32 * volume * edge
        })
        .collect()
}

// ANCHOR: mixer_setup
fn setup(ctx: &mut Ctx) -> Mixer {
    // Buses, effects, ducking and snapshots come from the file, which
    // hot reloads: edit it while the example runs.
    ctx.audio.load_mixer("audio_mixer.json");
    // The player's volumes and the snapshots that were on, from last time.
    if let Ok(saved) = ctx.storage.read_json::<MixerState>("mixer.json") {
        ctx.audio.restore_mixer_state(&saved);
    }
    let (music, voice, world) = (
        ctx.audio.bus("music"),
        ctx.audio.bus("dialogue"),
        ctx.audio.bus("world"),
    );

    let rate = 44_100;
    let song = ctx.audio.create(rate, &tone(rate, 4.0, &[220.0, 277.2, 329.6], 0.3));
    let looping = SoundParams {
        looping: true,
        ..Default::default()
    };
    ctx.audio.play_on(song, looping, music, Default::default());
    let footsteps = ctx.audio.create(rate, &tone(rate, 0.15, &[90.0, 3_000.0], 0.5));
    let dialogue = ctx.audio.create(rate, &tone(rate, 1.5, &[180.0, 360.0, 540.0], 0.6));
    Mixer {
        dialogue,
        footsteps,
        music,
        voice,
        world,
        underwater: ctx.audio.snapshot_weight("underwater") > 0.0,
    }
}
// ANCHOR_END: mixer_setup

// ANCHOR: mixer_frame
fn frame(ctx: &mut Ctx, m: &mut Mixer) {
    if ctx.input.key_pressed(Key::D) {
        // Music ducks while the line plays, and comes back after it.
        ctx.audio
            .play_on(m.dialogue, SoundParams::default(), m.voice, Default::default());
    }
    if ctx.input.key_pressed(Key::U) {
        m.underwater = !m.underwater;
        if m.underwater {
            ctx.audio.start_snapshot("underwater", 1.0);
        } else {
            ctx.audio.stop_snapshot("underwater", 1.0);
        }
    }
    if ctx.input.key_pressed(Key::M) {
        let muted = ctx.audio.bus_muted(m.music);
        ctx.audio.mute_bus(m.music, !muted);
    }
    if ctx.time.frame().is_multiple_of(30) {
        ctx.audio
            .play_on(m.footsteps, SoundParams::default(), m.world, Default::default());
    }
    if ctx.input.key_pressed(Key::U) || ctx.input.key_pressed(Key::M) {
        // Saved with the other settings whenever the player changes it.
        let _ = ctx.storage.write_json("mixer.json", &ctx.audio.mixer_state());
    }
    let lines = [
        "D: dialogue (ducks the music)   U: underwater   M: mute music".to_string(),
        format!("music {:.2}", ctx.audio.bus_gain(m.music)),
        format!("world {:.2}", ctx.audio.bus_gain(m.world)),
        format!("underwater {:.2}", ctx.audio.snapshot_weight("underwater")),
    ];
    for (i, line) in lines.iter().enumerate() {
        ctx.gfx
            .draw_text(line, vec2(20.0, 20.0 + 28.0 * i as f32), 22.0, Color::WHITE);
    }
}
// ANCHOR_END: mixer_frame

fn main() {
    Game::new("Audio mixer", 800, 240)
        .app_id("dev.ponko.audio-mixer")
        .run(|ctx| {
            let mut mixer = setup(ctx);
            move |ctx: &mut Ctx| frame(ctx, &mut mixer)
        });
}
