//! Fades, voice limits, loop points, pause groups, music, output devices
//! and the offline mix from C. The run mixes offline, so sound is rendered
//! into memory and checked; no device is opened.
use ponko_ffi::*;
use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};

use PonkoQuantize::*;
use PonkoStatus::*;

const OK: PonkoStatus = PONKO_OK;
const BAD: PonkoStatus = PONKO_INVALID_ARGUMENT;
const RATE: u32 = 48_000;

fn bus(ctx: *mut PonkoCtx, name: &std::ffi::CStr) -> PonkoAudioBus {
    let mut b = PonkoAudioBus::default();
    assert_eq!(unsafe { ponko_ctx_audio_bus(ctx, name.as_ptr(), &mut b) }, OK);
    b
}

fn tone(ctx: *mut PonkoCtx, hz: f32, seconds: f32) -> PonkoSound {
    let samples: Vec<f32> = (0..(RATE as f32 * seconds) as usize)
        .map(|i| (i as f32 * hz * std::f32::consts::TAU / RATE as f32).sin() * 0.4)
        .collect();
    let mut sound = PonkoSound { id: 0, seconds: 0.0 };
    assert_eq!(
        unsafe { ponko_ctx_create_sound(ctx, RATE, samples.as_ptr(), samples.len(), &mut sound) },
        OK
    );
    sound
}

fn looping(ctx: *mut PonkoCtx, sound: PonkoSound) -> PonkoPlaying {
    let mut params = ponko_sound_params_default();
    params.looping = true;
    let mut playing = PonkoPlaying::default();
    assert_eq!(unsafe { ponko_ctx_play_sound(ctx, sound, &params, &mut playing) }, OK);
    playing
}

/// A text from one of the two-call text functions.
unsafe fn text(f: impl Fn(*mut c_char, usize, *mut usize) -> PonkoStatus) -> String {
    let mut len = 0;
    assert_eq!(f(null_mut(), 0, &mut len), OK);
    let mut buffer = vec![0u8; len + 1];
    assert_eq!(f(buffer.as_mut_ptr().cast(), buffer.len(), &mut len), OK);
    buffer.truncate(len);
    String::from_utf8(buffer).unwrap()
}

unsafe fn voices(ctx: *mut PonkoCtx) {
    unsafe {
        let (a, b) = (tone(ctx, 300.0, 1.0), tone(ctx, 500.0, 1.0));
        assert_eq!(ponko_ctx_set_audio_voice_limit(ctx, 1), OK);
        let mut limit = 0;
        assert_eq!(ponko_ctx_audio_voice_limit(ctx, &mut limit), OK);
        assert_eq!(limit, 1);
        let first = looping(ctx, a);
        let second = looping(ctx, b);
        let mut is_virtual = false;
        assert_eq!(ponko_ctx_sound_is_virtual(ctx, second, &mut is_virtual), OK);
        assert!(is_virtual, "over the limit of one");
        assert_eq!(ponko_ctx_sound_is_virtual(ctx, first, &mut is_virtual), OK);
        assert!(!is_virtual);
        let (mut real, mut virt) = (0, 0);
        assert_eq!(ponko_ctx_audio_voice_count(ctx, &mut real, &mut virt), OK);
        assert_eq!((real, virt), (1, 1));
        assert_eq!(ponko_ctx_set_sound_priority(ctx, second, 250), OK);
        assert_eq!(ponko_ctx_set_sound_volume(ctx, first, 0.5, 0.0), OK);
        assert_eq!(ponko_ctx_set_sound_speed(ctx, first, 1.5, 0.2), OK);
        assert_eq!(ponko_ctx_fade_out_sound(ctx, first, 0.1), OK);
        assert_eq!(ponko_ctx_set_audio_voice_limit(ctx, 64), OK);
        assert_eq!(ponko_ctx_audio_voice_count(ctx, null_mut(), &mut virt), BAD);

        let steps = bus(ctx, c"footsteps");
        assert_eq!(ponko_ctx_set_audio_bus_voice_limit(ctx, steps, 8), OK);
        assert_eq!(ponko_ctx_audio_bus_voice_limit(ctx, steps, &mut limit), OK);
        assert_eq!(limit, 8);
        assert_eq!(ponko_ctx_set_audio_bus_voice_limit(ctx, steps, 0), OK);
        assert_eq!(ponko_ctx_audio_bus_voice_limit(ctx, steps, &mut limit), OK);
        assert_eq!(limit, 0, "0: no limit of its own");

        let (mut start, mut end) = (-1.0, -1.0);
        assert_eq!(ponko_ctx_sound_loop(ctx, a, &mut start, &mut end), OK);
        assert_eq!((start, end), (0.0, -1.0), "the whole sound");
        assert_eq!(ponko_ctx_set_sound_loop(ctx, a, 0.25, 0.75), OK);
        assert_eq!(ponko_ctx_sound_loop(ctx, a, &mut start, &mut end), OK);
        assert_eq!((start, end), (0.25, 0.75));
        assert_eq!(ponko_ctx_set_sound_loop(ctx, a, 0.25, -1.0), OK);
        assert_eq!(ponko_ctx_sound_loop(ctx, a, &mut start, &mut end), OK);
        assert_eq!((start, end), (0.25, -1.0));

        // A positional source's priority.
        let w = ponko_ctx_world(ctx);
        let mut transform = 0u32;
        assert_eq!(ponko_transform3d_component(w, &mut transform), OK);
        let t = ponko_transform3d_at(PonkoVec3 { x: 2.0, y: 0.0, z: 0.0 });
        let part = PonkoValue {
            component: transform,
            data: (&raw const t).cast(),
            size: size_of::<PonkoTransform3D>(),
        };
        let mut e = 0u64;
        assert_eq!(ponko_spawn(w, &part, 1, &mut e), OK);
        assert_eq!(ponko_ctx_set_audio_source3d_priority(ctx, e, 200), BAD, "no source yet");
        assert_eq!(ponko_ctx_set_audio_source3d(ctx, e, a, null()), OK);
        assert_eq!(ponko_ctx_set_audio_source3d_priority(ctx, e, 200), OK);
    }
}

unsafe fn pause_and_pitch(ctx: *mut PonkoCtx) {
    unsafe {
        let (world, sfx) = (bus(ctx, c"world"), bus(ctx, c"sfx"));
        assert_eq!(ponko_ctx_set_audio_bus_parent(ctx, sfx, &world), OK);
        assert_eq!(ponko_ctx_pause_audio_bus(ctx, world, true), OK);
        let mut paused = false;
        assert_eq!(ponko_ctx_audio_bus_paused(ctx, sfx, &mut paused), OK);
        assert!(paused, "children pause with their parent");
        assert_eq!(ponko_ctx_pause_audio_bus(ctx, world, false), OK);
        assert_eq!(ponko_ctx_audio_bus_paused(ctx, sfx, &mut paused), OK);
        assert!(!paused);

        let mut pitch = ponko_bus_effect_default(PonkoBusEffectKind::PONKO_BUS_EFFECT_PITCH_SHIFT);
        assert_eq!((pitch.ratio, pitch.mix), (1.0, 1.0));
        pitch.ratio = 9.0; // clamped to 4
        assert_eq!(ponko_ctx_set_audio_bus_effects(ctx, sfx, &pitch, 1), OK);
        let mut back = ponko_bus_effect_default(PonkoBusEffectKind::PONKO_BUS_EFFECT_DELAY);
        let mut n = 0;
        assert_eq!(ponko_ctx_audio_bus_effects(ctx, sfx, &mut back, 1, &mut n), OK);
        assert_eq!(n, 1);
        assert_eq!(back.kind, PonkoBusEffectKind::PONKO_BUS_EFFECT_PITCH_SHIFT);
        assert_eq!(back.ratio, 4.0);
    }
}

unsafe fn music(ctx: *mut PonkoCtx) {
    unsafe {
        let layers = [tone(ctx, 200.0, 2.0), tone(ctx, 700.0, 2.0)];
        let volumes = [1.0, 0.0];
        assert_eq!(
            ponko_ctx_play_music(ctx, layers.as_ptr(), null(), 0, 120.0, 4, 0.0, PONKO_QUANTIZE_NOW),
            BAD
        );
        assert_eq!(
            ponko_ctx_play_music(
                ctx,
                layers.as_ptr(),
                volumes.as_ptr(),
                2,
                120.0,
                4,
                0.0,
                PONKO_QUANTIZE_NOW
            ),
            OK
        );
        let (mut beat, mut playing) = (-1.0, false);
        assert_eq!(ponko_ctx_music_beat(ctx, &mut beat, &mut playing), OK);
        assert!(playing);
        assert_eq!(beat, 0.0);
        let mut volume = -1.0;
        assert_eq!(ponko_ctx_music_layer_volume(ctx, 1, &mut volume), OK);
        assert_eq!(volume, 0.0);
        assert_eq!(ponko_ctx_set_music_layer(ctx, 1, 1.0, 0.0, PONKO_QUANTIZE_NOW), OK);
        assert_eq!(ponko_ctx_music_layer_volume(ctx, 1, &mut volume), OK);
        assert_eq!(volume, 1.0);
        let mut stinger = PonkoPlaying::default();
        assert_eq!(
            ponko_ctx_play_stinger(ctx, layers[0], PONKO_QUANTIZE_BEAT, &mut stinger),
            OK
        );
        assert_eq!(ponko_ctx_set_music_speed(ctx, 1.1, 0.5), OK);
        let mut music_bus = PonkoAudioBus::default();
        assert_eq!(ponko_ctx_music_bus(ctx, &mut music_bus), OK);
        assert_eq!(music_bus, bus(ctx, c"music"));
        let score = bus(ctx, c"score");
        assert_eq!(ponko_ctx_set_music_bus(ctx, score), OK);
        assert_eq!(ponko_ctx_music_bus(ctx, &mut music_bus), OK);
        assert_eq!(music_bus, score);
        assert_eq!(ponko_ctx_set_music_bus(ctx, bus(ctx, c"music")), OK);
    }
}

unsafe fn devices_and_offline(ctx: *mut PonkoCtx) {
    unsafe {
        let devices = text(|b, c, n| ponko_ctx_audio_output_devices(ctx, b, c, n));
        assert!(devices.contains(r#""id":"offline""#), "{devices}");
        assert_eq!(
            ponko_ctx_set_audio_output_device(ctx, c"no such device".as_ptr()),
            PONKO_NOT_FOUND
        );
        assert_eq!(ponko_ctx_set_audio_output_device(ctx, c"offline".as_ptr()), OK);
        assert_eq!(ponko_ctx_set_audio_output_device(ctx, null()), OK);
        let current = text(|b, c, n| ponko_ctx_audio_output_device(ctx, b, c, n));
        assert_eq!(current, "offline");
        let events = text(|b, c, n| ponko_ctx_audio_device_events(ctx, b, c, n));
        assert!(events.starts_with('['), "{events}");

        let mut count = 0;
        let mut mix = vec![0.0f32; 4800 * 2];
        assert_eq!(
            ponko_ctx_render_audio_offline(ctx, 0.1, mix.as_mut_ptr(), mix.len(), &mut count),
            OK
        );
        assert_eq!(count, 4800 * 2);
        assert!(mix.iter().any(|s| s.abs() > 0.1), "the music and voices are heard");
        assert_eq!(ponko_ctx_render_audio_offline(ctx, 0.1, null_mut(), 0, &mut count), OK);
        assert_eq!(ponko_ctx_render_audio_offline(ctx, 0.1, null_mut(), 0, null_mut()), BAD);
    }
}

struct Run {
    ticks: u32,
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        assert_eq!(ponko_ctx_open_audio_offline(ctx, RATE), OK);
        voices(ctx);
        pause_and_pitch(ctx);
        music(ctx);
        devices_and_offline(ctx);
        assert_eq!(ponko_ctx_stop_music(ctx, 0.5, PONKO_QUANTIZE_BAR), OK);
    }
}

unsafe extern "C" fn tick(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let run = &mut *user.cast::<Run>();
        run.ticks += 1;
        let mut paused = false;
        assert_eq!(ponko_ctx_audio_bus_paused(ctx, bus(ctx, c"world"), &mut paused), OK);
        assert_eq!(
            paused,
            run.ticks > 2,
            "tick {}: paused from the pause state on",
            run.ticks
        );
        if run.ticks == 2 {
            assert_eq!(ponko_ctx_push_state(ctx, 1), OK);
        }
    }
}

#[test]
fn fades_limits_music_pause_states_devices_and_the_offline_mix_work_from_c() {
    let mut run = Run { ticks: 0 };
    unsafe {
        let g = ponko_game_new(c"music".as_ptr(), 1, 1);
        let names = [c"world".as_ptr()];
        assert_eq!(
            ponko_game_pause_audio_in_state(g, 1, names.as_ptr(), 1),
            BAD,
            "no state machine yet"
        );
        assert_eq!(ponko_game_state(g, 0), OK);
        assert_eq!(ponko_game_pause_audio_in_state(g, 1, null(), 1), BAD);
        assert_eq!(ponko_game_pause_audio_in_state(g, 1, names.as_ptr(), 1), OK);
        assert_eq!(
            ponko_game_sim_system(g, c"tick".as_ptr(), 1, Some(tick), (&raw mut run).cast()),
            OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(g, 4, Some(setup), null_mut(), &mut world), OK);
        ponko_world_free(world);
    }
    assert_eq!(run.ticks, 4);
}
