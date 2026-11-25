//! The mixer from C: bus tree, effect chains, ducking, snapshots, mixer
//! files and the saved state. Headless runs keep the mixer's model (gains,
//! snapshot weights) as a real run does; nothing is heard.
use ponko_ffi::*;
use std::ffi::{CString, c_char, c_void};
use std::ptr::{null, null_mut};

use PonkoBusEffectKind::*;
use PonkoStatus::*;

fn bus(ctx: *mut PonkoCtx, name: &std::ffi::CStr) -> PonkoAudioBus {
    let mut b = PonkoAudioBus::default();
    assert_eq!(unsafe { ponko_ctx_audio_bus(ctx, name.as_ptr(), &mut b) }, PONKO_OK);
    b
}
fn gain(ctx: *mut PonkoCtx, b: PonkoAudioBus) -> f32 {
    let mut g = -1.0;
    assert_eq!(unsafe { ponko_ctx_audio_bus_gain(ctx, b, &mut g) }, PONKO_OK);
    g
}
fn weight(ctx: *mut PonkoCtx, name: &std::ffi::CStr) -> f32 {
    let mut w = -1.0;
    assert_eq!(
        unsafe { ponko_ctx_audio_snapshot_weight(ctx, name.as_ptr(), &mut w) },
        PONKO_OK
    );
    w
}

unsafe fn tree(ctx: *mut PonkoCtx) {
    unsafe {
        let (master, music) = (bus(ctx, c"master"), bus(ctx, c"music"));
        assert_eq!(ponko_ctx_set_audio_bus_parent(ctx, music, &master), PONKO_OK);
        let (mut parent, mut has) = (PonkoAudioBus::default(), false);
        assert_eq!(ponko_ctx_audio_bus_parent(ctx, music, &mut parent, &mut has), PONKO_OK);
        assert!(has);
        assert_eq!(parent, master);
        assert_eq!(ponko_ctx_audio_bus_parent(ctx, master, &mut parent, &mut has), PONKO_OK);
        assert!(!has);
        assert_eq!(
            ponko_ctx_set_audio_bus_parent(ctx, master, &music),
            PONKO_INVALID_ARGUMENT,
            "a loop is refused"
        );
        assert_eq!(ponko_last_error_kind(), PonkoErrorKind::PONKO_ERROR_INVALID);

        assert_eq!(ponko_ctx_set_audio_bus_volume(ctx, master, 0.5), PONKO_OK);
        assert_eq!(ponko_ctx_set_audio_bus_volume(ctx, music, 0.5), PONKO_OK);
        let mut v = 0.0;
        assert_eq!(ponko_ctx_audio_bus_volume(ctx, music, &mut v), PONKO_OK);
        assert_eq!(v, 0.5);
        assert_eq!(gain(ctx, music), 0.25, "volumes multiply down the tree");
        assert_eq!(ponko_ctx_mute_audio_bus(ctx, master, true), PONKO_OK);
        let mut muted = false;
        assert_eq!(ponko_ctx_audio_bus_muted(ctx, master, &mut muted), PONKO_OK);
        assert!(muted);
        assert_eq!(gain(ctx, music), 0.0, "a muted parent silences its children");
        assert_eq!(ponko_ctx_mute_audio_bus(ctx, master, false), PONKO_OK);
        assert_eq!(ponko_ctx_set_audio_bus_parent(ctx, music, null()), PONKO_OK);
        assert_eq!(gain(ctx, music), 0.5);
    }
}

unsafe fn effects_and_ducking(ctx: *mut PonkoCtx) {
    unsafe {
        let world = bus(ctx, c"world");
        let mut filter = ponko_bus_effect_default(PONKO_BUS_EFFECT_FILTER);
        filter.cutoff_hz = 99_999.0; // clamped
        let chain = [
            filter,
            ponko_bus_effect_default(PONKO_BUS_EFFECT_REVERB),
            ponko_bus_effect_default(PONKO_BUS_EFFECT_COMPRESSOR),
            ponko_bus_effect_default(PONKO_BUS_EFFECT_DELAY),
        ];
        assert_eq!(ponko_ctx_set_audio_bus_effects(ctx, world, chain.as_ptr(), 4), PONKO_OK);
        let mut n = 0;
        assert_eq!(ponko_ctx_audio_bus_effects(ctx, world, null_mut(), 0, &mut n), PONKO_OK);
        assert_eq!(n, 4);
        let mut back = [ponko_bus_effect_default(PONKO_BUS_EFFECT_DELAY); 4];
        assert_eq!(
            ponko_ctx_audio_bus_effects(ctx, world, back.as_mut_ptr(), 4, &mut n),
            PONKO_OK
        );
        assert_eq!(back[0].kind, PONKO_BUS_EFFECT_FILTER);
        assert_eq!(back[0].cutoff_hz, 20_000.0);
        assert_eq!(&back[1..], &chain[1..]);
        assert_eq!(
            ponko_ctx_set_audio_bus_effects(ctx, world, null(), 2),
            PONKO_INVALID_ARGUMENT,
            "a null array needs a count of 0"
        );

        let (music, dialogue) = (bus(ctx, c"music"), bus(ctx, c"dialogue"));
        let mut duck = ponko_ducking_default(dialogue);
        duck.gain = 0.2;
        assert_eq!(ponko_ctx_set_audio_bus_ducking(ctx, music, &duck, 1), PONKO_OK);
        let mut got = [ponko_ducking_default(music); 2];
        assert_eq!(
            ponko_ctx_audio_bus_ducking(ctx, music, got.as_mut_ptr(), 2, &mut n),
            PONKO_OK
        );
        assert_eq!(n, 1);
        assert_eq!(got[0], duck);
        let itself = ponko_ducking_default(music);
        assert_eq!(
            ponko_ctx_set_audio_bus_ducking(ctx, music, &itself, 1),
            PONKO_INVALID_ARGUMENT
        );
        let five = [duck; 5];
        assert_eq!(
            ponko_ctx_set_audio_bus_ducking(ctx, music, five.as_ptr(), 5),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PonkoErrorKind::PONKO_ERROR_LIMIT);
        assert_eq!(ponko_ctx_set_audio_bus_ducking(ctx, music, null(), 0), PONKO_OK);
    }
}

unsafe fn snapshots(ctx: *mut PonkoCtx) {
    unsafe {
        let world = bus(ctx, c"world");
        let mut muffled = ponko_bus_effect_default(PONKO_BUS_EFFECT_FILTER);
        muffled.cutoff_hz = 400.0;
        let over = PonkoEffectOverride {
            index: 0,
            effect: muffled,
        };
        let buses = [PonkoBusSnapshot {
            bus: world,
            has_volume: true,
            volume: 0.4,
            effects: &over,
            effect_count: 1,
        }];
        assert_eq!(
            ponko_ctx_add_audio_snapshot(ctx, c"underwater".as_ptr(), buses.as_ptr(), 1),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_start_audio_snapshot(ctx, c"underwater".as_ptr(), 0.0),
            PONKO_OK
        );
        assert_eq!(weight(ctx, c"underwater"), 1.0);
        assert_eq!(gain(ctx, world), 0.4);
        assert_eq!(
            ponko_ctx_start_audio_snapshot(ctx, c"nowhere".as_ptr(), 0.0),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_stop_audio_snapshot(ctx, c"underwater".as_ptr(), 0.0),
            PONKO_OK
        );
        assert_eq!(weight(ctx, c"underwater"), 0.0);
        assert_eq!(gain(ctx, world), 1.0);
        resolved_once(ctx, world);
        // A fade runs on the frame clock: started here, checked next tick.
        assert_eq!(
            ponko_ctx_start_audio_snapshot(ctx, c"underwater".as_ptr(), 1.0),
            PONKO_OK
        );
    }
}

fn last_error() -> String {
    unsafe { std::ffi::CStr::from_ptr(ponko_last_error()) }
        .to_string_lossy()
        .into_owned()
}

/// Buses and snapshots resolved once by name, then used by handle and id;
/// a typo lists the names there are.
unsafe fn resolved_once(ctx: *mut PonkoCtx, world: PonkoAudioBus) {
    unsafe {
        let mut found = PonkoAudioBus::default();
        assert_eq!(ponko_ctx_audio_find_bus(ctx, c"world".as_ptr(), &mut found), PONKO_OK);
        assert_eq!(found, world);
        assert_eq!(
            ponko_ctx_audio_find_bus(ctx, c"wrold".as_ptr(), &mut found),
            PONKO_NOT_FOUND
        );
        assert!(
            last_error().contains(r#"unknown audio bus "wrold" (known: "#),
            "{}",
            last_error()
        );
        assert!(last_error().contains("world"));
        assert_eq!(
            ponko_ctx_audio_find_bus(ctx, c"world".as_ptr(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );

        let mut id = 0;
        assert_eq!(
            ponko_ctx_audio_snapshot_id(ctx, c"underwater".as_ptr(), &mut id),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_start_audio_snapshot_id(ctx, id, 0.0), PONKO_OK);
        let mut w = 0.0;
        assert_eq!(ponko_ctx_audio_snapshot_weight_id(ctx, id, &mut w), PONKO_OK);
        assert_eq!(w, 1.0);
        assert_eq!(ponko_ctx_stop_audio_snapshot_id(ctx, id, 0.0), PONKO_OK);
        assert_eq!(weight(ctx, c"underwater"), 0.0);
        assert_eq!(ponko_ctx_start_audio_snapshot_id(ctx, 999, 0.0), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_audio_snapshot_weight_id(ctx, 999, &mut w), PONKO_OK);
        assert_eq!(w, 0.0);
        assert_eq!(
            ponko_ctx_audio_snapshot_id(ctx, c"underwatr".as_ptr(), &mut id),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            last_error(),
            r#"unknown audio snapshot "underwatr" (known: underwater)"#
        );
    }
}

unsafe fn files_and_state(ctx: *mut PonkoCtx, path: *const c_char) {
    unsafe {
        assert_eq!(ponko_ctx_load_audio_mixer(ctx, path), PONKO_OK);
        let voices = bus(ctx, c"voices");
        assert_eq!(gain(ctx, voices), 0.6);
        assert_eq!(
            ponko_ctx_load_audio_mixer(ctx, c"no-such-mixer.json".as_ptr()),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_apply_audio_mixer_json(ctx, cr#"{"buses": [{"name": "x", "parent": "y"}]}"#.as_ptr()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_apply_audio_mixer_json(ctx, c"{\"buses\": [{\"name\": \"voices\", \"volume\": 0.3}]}".as_ptr()),
            PONKO_OK
        );
        assert_eq!(gain(ctx, voices), 0.3);

        let mut len = 0;
        assert_eq!(ponko_ctx_audio_mixer_state(ctx, null_mut(), 0, &mut len), PONKO_OK);
        let mut text = vec![0 as c_char; len + 1];
        assert_eq!(
            ponko_ctx_audio_mixer_state(ctx, text.as_mut_ptr(), len + 1, &mut len),
            PONKO_OK
        );
        let saved = std::ffi::CStr::from_ptr(text.as_ptr()).to_owned();
        assert!(saved.to_str().unwrap().contains("\"voices\""));
        assert_eq!(ponko_ctx_set_audio_bus_volume(ctx, voices, 1.0), PONKO_OK);
        assert_eq!(ponko_ctx_restore_audio_mixer_state(ctx, saved.as_ptr()), PONKO_OK);
        assert_eq!(gain(ctx, voices), 0.3);
        assert_eq!(
            ponko_ctx_restore_audio_mixer_state(ctx, c"{not json".as_ptr()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PonkoErrorKind::PONKO_ERROR_JSON);
    }
}

struct Run {
    path: CString,
    ticks: u32,
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let run = &*user.cast::<Run>();
        tree(ctx);
        effects_and_ducking(ctx);
        files_and_state(ctx, run.path.as_ptr());
        snapshots(ctx);
    }
}

unsafe extern "C" fn tick(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let run = &mut *user.cast::<Run>();
        run.ticks += 1;
        // One 1/60 s frame into a one-second fade.
        if run.ticks == 1 {
            assert!((weight(ctx, c"underwater") - 1.0 / 60.0).abs() < 1e-5);
        }
    }
}

#[test]
fn mixer_tree_effects_ducking_snapshots_and_state_work_from_c() {
    let file = std::env::temp_dir().join(format!("ponko-ffi-mixer-{}.json", std::process::id()));
    std::fs::write(&file, r#"{"buses": [{"name": "voices", "volume": 0.6}]}"#).unwrap();
    let mut run = Run {
        path: CString::new(file.to_str().unwrap()).unwrap(),
        ticks: 0,
    };
    unsafe {
        let g = ponko_game_new(c"mixer".as_ptr(), 1, 1);
        assert_eq!(
            ponko_game_sim_system(g, c"tick".as_ptr(), 1, Some(tick), (&raw mut run).cast()),
            PONKO_OK
        );
        let mut world = null_mut();
        let user = (&raw mut run).cast();
        assert_eq!(ponko_game_run_headless(g, 1, Some(setup), user, &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    assert_eq!(run.ticks, 1);
    let _ = std::fs::remove_file(&file);
}
