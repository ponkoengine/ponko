//! Mixer buses, effects, streams and positional sound from C. Headless runs
//! use the silent backend: nothing is heard, but handles, listener state and
//! argument checks behave as in a real run.
use ponko_ffi::*;
use std::ffi::{CString, c_void};
use std::ptr::{null, null_mut};

use PonkoStatus::*;

const OK: PonkoStatus = PONKO_OK;
const BAD: PonkoStatus = PONKO_INVALID_ARGUMENT;

fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}

/// A mono 16-bit PCM WAV file of `samples` samples at 8 kHz, silent.
fn wav(samples: u32) -> Vec<u8> {
    let data = samples * 2;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36 + data).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes()); // PCM
    b.extend(1u16.to_le_bytes()); // mono
    b.extend(8000u32.to_le_bytes());
    b.extend(16000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(data.to_le_bytes());
    b.resize(b.len() + data as usize, 0);
    b
}

struct Run {
    stream: CString,
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let run = &*user.cast::<Run>();
        buses(ctx);
        let beep = sound(ctx);
        effects_and_streams(ctx, run, beep);
        positional_sounds(ctx, beep);
        sources_on_entities(ctx, beep);
        listener(ctx);
    }
}

unsafe fn sound(ctx: *mut PonkoCtx) -> PonkoSound {
    let samples = vec![0.1f32; 8000];
    let mut beep = PonkoSound { id: 0, seconds: 0.0 };
    assert_eq!(
        unsafe { ponko_ctx_create_sound(ctx, 8000, samples.as_ptr(), samples.len(), &mut beep) },
        OK
    );
    beep
}

unsafe fn buses(ctx: *mut PonkoCtx) {
    unsafe {
        // Defaults are what the Rust types say.
        let e = ponko_audio_effects_default();
        assert_eq!((e.low_pass_hz, e.reverb), (20000.0, 0.0));
        let s = ponko_audio_settings3d_default();
        assert_eq!((s.min_distance, s.max_distance, s.doppler), (1.0, 100.0, 1.0));
        assert_eq!(s.rolloff, PonkoAudioRolloff::PONKO_ROLLOFF_LOGARITHMIC);
        assert_eq!(s.bus, PonkoAudioBus::default(), "ordinary sounds play on sfx, bus 0");
        assert_eq!((s.sound.volume, s.sound.speed, s.sound.looping), (1.0, 1.0, false));
        assert_eq!(s.effects, e);

        // A name gives one handle, the same every time; names are separate.
        let (mut music, mut again, mut voices, mut sfx) = <[PonkoAudioBus; 4]>::default().into();
        assert_eq!(ponko_ctx_audio_bus(ctx, c"music".as_ptr(), &mut music), OK);
        assert_eq!(ponko_ctx_audio_bus(ctx, c"music".as_ptr(), &mut again), OK);
        assert_eq!(ponko_ctx_audio_bus(ctx, c"voices".as_ptr(), &mut voices), OK);
        assert_eq!(ponko_ctx_audio_bus(ctx, c"sfx".as_ptr(), &mut sfx), OK);
        assert_eq!(music, again);
        assert_ne!(music, voices);
        assert_ne!(music, sfx);
        assert_eq!(sfx, PonkoAudioBus::default());

        assert_eq!(ponko_ctx_set_audio_bus_volume(ctx, music, 0.25), OK);
        assert_eq!(ponko_ctx_set_audio_bus_volume(ctx, music, 0.0), OK);
        assert_eq!(
            ponko_ctx_set_audio_bus_volume(ctx, music, 2.0),
            OK,
            "more than 1 is allowed"
        );
        assert_eq!(
            ponko_ctx_set_audio_bus_volume(ctx, music, f32::NAN),
            OK,
            "NaN becomes silence"
        );
        assert_eq!(ponko_ctx_mute_audio_bus(ctx, music, true), OK);
        assert_eq!(ponko_ctx_mute_audio_bus(ctx, music, false), OK);
        // A made-up handle does nothing.
        let ghost = PonkoAudioBus { index: 9999 };
        assert_eq!(ponko_ctx_set_audio_bus_volume(ctx, ghost, 0.5), OK);
        assert_eq!(ponko_ctx_mute_audio_bus(ctx, ghost, true), OK);

        // Names must be present, and the output writable.
        assert_eq!(ponko_ctx_audio_bus(ctx, null(), &mut music), BAD);
        assert_eq!(ponko_ctx_audio_bus(ctx, c"music".as_ptr(), null_mut()), BAD);
        assert_eq!(ponko_ctx_audio_bus(null_mut(), c"music".as_ptr(), &mut music), BAD);
        assert_eq!(ponko_ctx_set_audio_bus_volume(null_mut(), music, 1.0), BAD);
        assert_eq!(ponko_ctx_mute_audio_bus(null_mut(), music, true), BAD);
    }
}

unsafe fn effects_and_streams(ctx: *mut PonkoCtx, run: &Run, beep: PonkoSound) {
    unsafe {
        let mut music = PonkoAudioBus::default();
        assert_eq!(ponko_ctx_audio_bus(ctx, c"music".as_ptr(), &mut music), OK);

        // Playing on a bus: with and without params, effects and output.
        let mut p = PonkoPlaying { id: 77 };
        assert_eq!(ponko_ctx_play_sound_on(ctx, beep, null(), music, null(), &mut p), OK);
        assert_ne!(p.id, 77, "headless voices get a handle of their own");
        let mut params = ponko_sound_params_default();
        params.looping = true;
        params.volume = 0.5;
        let fx = PonkoAudioEffects {
            low_pass_hz: 800.0,
            reverb: 0.3,
        };
        assert_eq!(ponko_ctx_play_sound_on(ctx, beep, &params, music, &fx, &mut p), OK);
        assert_eq!(ponko_ctx_play_sound_on(ctx, beep, &params, music, &fx, null_mut()), OK);
        assert_eq!(
            ponko_ctx_play_sound_on(null_mut(), beep, null(), music, null(), &mut p),
            BAD
        );

        // Changing the effects of a voice: a value, the defaults, a stopped voice.
        assert_eq!(ponko_ctx_set_sound_effects(ctx, p, &fx), OK);
        assert_eq!(ponko_ctx_set_sound_effects(ctx, p, null()), OK);
        assert_eq!(ponko_ctx_set_sound_effects(ctx, PonkoPlaying { id: 4242 }, &fx), OK);
        assert_eq!(ponko_ctx_set_sound_effects(null_mut(), p, &fx), BAD);

        // Streams: a missing file is an error (and doesn't abort); so are
        // a null path and a null output.
        let mut s = PonkoSound { id: 0, seconds: 0.0 };
        assert_ne!(ponko_ctx_load_stream(ctx, c"/no/such/music.ogg".as_ptr(), &mut s), OK);
        assert_eq!(ponko_ctx_load_stream(ctx, null(), &mut s), BAD);
        assert_eq!(ponko_ctx_load_stream(ctx, run.stream.as_ptr(), null_mut()), BAD);
        assert_eq!(ponko_ctx_load_stream(null_mut(), run.stream.as_ptr(), &mut s), BAD);
        // A real file opens, and plays like any sound.
        assert_eq!(ponko_ctx_load_stream(ctx, run.stream.as_ptr(), &mut s), OK);
        assert!(s.seconds >= 0.0 && s.seconds <= 1.0 + 1e-3, "{}", s.seconds);
        assert_eq!(ponko_ctx_play_sound_on(ctx, s, null(), music, null(), &mut p), OK);
    }
}

unsafe fn positional_sounds(ctx: *mut PonkoCtx, beep: PonkoSound) {
    unsafe {
        // Each positional voice gets its own non-zero id, even headless.
        let mut a = PonkoPlaying { id: 0 };
        let mut b = PonkoPlaying { id: 0 };
        assert_eq!(
            ponko_ctx_play_sound_at(ctx, beep, v3(3.0, 0.0, -5.0), null(), &mut a),
            OK
        );
        let mut settings = ponko_audio_settings3d_default();
        settings.rolloff = PonkoAudioRolloff::PONKO_ROLLOFF_LINEAR;
        settings.max_distance = 20.0;
        settings.doppler = 0.0;
        settings.sound.looping = true;
        assert_eq!(
            ponko_ctx_play_sound_at(ctx, beep, v3(0.0, 0.0, 0.0), &settings, &mut b),
            OK
        );
        assert_ne!(a.id, 0);
        assert_ne!(b.id, 0);
        assert_ne!(a, b);
        // The output is optional, a null context is not.
        assert_eq!(
            ponko_ctx_play_sound_at(ctx, beep, v3(0.0, 0.0, 0.0), &settings, null_mut()),
            OK
        );
        assert_eq!(
            ponko_ctx_play_sound_at(null_mut(), beep, v3(0.0, 0.0, 0.0), null(), &mut a),
            BAD
        );
        // Out of range, on another bus: still accepted.
        let mut far = settings;
        far.bus = PonkoAudioBus { index: 9999 };
        assert_eq!(ponko_ctx_play_sound_at(ctx, beep, v3(1e6, 0.0, 0.0), &far, &mut a), OK);
        // Its effects can be changed and it can be stopped like any voice.
        assert_eq!(ponko_ctx_set_sound_effects(ctx, b, null()), OK);
        assert_eq!(ponko_ctx_stop_sound(ctx, b), OK);
    }
}

unsafe fn sources_on_entities(ctx: *mut PonkoCtx, beep: PonkoSound) {
    unsafe {
        let w = ponko_ctx_world(ctx);
        let mut transform = 0u32;
        assert_eq!(ponko_transform3d_component(w, &mut transform), OK);
        let t = ponko_transform3d_at(v3(2.0, 0.0, 0.0));
        let part = PonkoValue {
            component: transform,
            data: (&raw const t).cast(),
            size: size_of::<PonkoTransform3D>(),
        };
        let mut e = 0u64;
        assert_eq!(ponko_spawn(w, &part, 1, &mut e), OK);

        assert_eq!(ponko_ctx_set_audio_source3d(ctx, e, beep, null()), OK);
        let mut settings = ponko_audio_settings3d_default();
        settings.sound.looping = true;
        assert_eq!(
            ponko_ctx_set_audio_source3d(ctx, e, beep, &settings),
            OK,
            "setting it again replaces it"
        );
        assert_eq!(ponko_ctx_remove_audio_source3d(ctx, e), OK);
        assert_eq!(ponko_ctx_remove_audio_source3d(ctx, e), OK, "removing twice is fine");

        // Gone entities and null contexts.
        assert_eq!(ponko_ctx_set_audio_source3d(ctx, 12345, beep, null()), BAD);
        assert_eq!(ponko_ctx_remove_audio_source3d(ctx, 12345), BAD);
        assert_eq!(ponko_ctx_set_audio_source3d(null_mut(), e, beep, null()), BAD);
        assert_eq!(ponko_ctx_remove_audio_source3d(null_mut(), e), BAD);
    }
}

unsafe fn listener(ctx: *mut PonkoCtx) {
    unsafe {
        // With none set, the ears are at the camera.
        let mut camera = PonkoAudioListener3D {
            position: v3(9.0, 9.0, 9.0),
            rotation: PonkoQuat {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 0.0,
            },
        };
        assert_eq!(ponko_ctx_audio_listener(ctx, &mut camera), OK);
        let r = camera.rotation;
        let length = (r.x * r.x + r.y * r.y + r.z * r.z + r.w * r.w).sqrt();
        assert!(
            (length - 1.0).abs() < 1e-5,
            "the camera's rotation is a unit quaternion: {r:?}"
        );
        assert_ne!(camera.position.x, 9.0, "the output was written");

        let ears = PonkoAudioListener3D {
            position: v3(1.0, 2.0, 3.0),
            rotation: PonkoQuat {
                x: 0.0,
                y: 1.0,
                z: 0.0,
                w: 0.0,
            },
        };
        assert_eq!(ponko_ctx_set_audio_listener(ctx, &ears), OK);
        let mut got = camera;
        assert_eq!(ponko_ctx_audio_listener(ctx, &mut got), OK);
        assert_eq!((got.position.x, got.position.y, got.position.z), (1.0, 2.0, 3.0));
        assert_eq!(
            (got.rotation.x, got.rotation.y, got.rotation.z, got.rotation.w),
            (0.0, 1.0, 0.0, 0.0)
        );

        // Null goes back to the camera.
        assert_eq!(ponko_ctx_set_audio_listener(ctx, null()), OK);
        assert_eq!(ponko_ctx_audio_listener(ctx, &mut got), OK);
        assert_eq!(got, camera);

        assert_eq!(ponko_ctx_audio_listener(ctx, null_mut()), BAD);
        assert_eq!(ponko_ctx_audio_listener(null_mut(), &mut got), BAD);
        assert_eq!(ponko_ctx_set_audio_listener(null_mut(), &ears), BAD);
    }
}

#[test]
fn buses_streams_and_positional_sound_work_from_c() {
    let file = std::env::temp_dir().join("ponko-ffi-audio3d-stream.wav");
    std::fs::write(&file, wav(8000)).unwrap();
    let mut run = Run {
        stream: CString::new(file.to_str().unwrap()).unwrap(),
    };
    unsafe {
        let g = ponko_game_new(c"audio3d".as_ptr(), 1, 1);
        let mut world = null_mut();
        let user = (&raw mut run).cast();
        assert_eq!(ponko_game_run_headless(g, 1, Some(setup), user, &mut world), OK);
        ponko_world_free(world);
    }
    let _ = std::fs::remove_file(&file);
}
