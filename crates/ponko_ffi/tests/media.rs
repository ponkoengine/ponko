//! Shapes, UI widgets, fonts, sound, screenshots and turns through the C API.
use ponko_ffi::*;
use std::ffi::{CStr, c_void};
use std::ptr::{null, null_mut};

use PonkoStatus::*;

/// Runs `system` as a sim system for `ticks` headless ticks.
fn run(ticks: u64, system: PonkoSystemFn, user: *mut c_void) {
    unsafe {
        let game = ponko_game_new(c"media".as_ptr(), 1, 1);
        assert_eq!(ponko_game_sim_system(game, c"s".as_ptr(), 1, system, user), PONKO_OK);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, ticks, None, null_mut(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
}
fn last_error() -> String {
    unsafe { CStr::from_ptr(ponko_last_error()) }
        .to_string_lossy()
        .into_owned()
}
const WHITE: PonkoColor = PonkoColor {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

#[derive(Default)]
struct Drawn {
    label: PonkoVec2,
    measured: PonkoVec2,
    measured_ex: PonkoVec2,
    clicked: bool,
    over_ui: bool,
    style: Option<PonkoUiStyle>,
}

unsafe extern "C" fn draw_everything(ctx: *mut PonkoCtx, user: *mut c_void) {
    let d = unsafe { &mut *user.cast::<Drawn>() };
    let rect = PonkoRect {
        min: PonkoVec2 { x: 0.0, y: 0.0 },
        max: PonkoVec2 { x: 100.0, y: 40.0 },
    };
    let v = PonkoVec2 { x: 5.0, y: 5.0 };
    unsafe {
        assert_eq!(ponko_ctx_draw_rect(ctx, rect, WHITE), PONKO_OK);
        assert_eq!(ponko_ctx_draw_line(ctx, v, rect.max, 2.0, WHITE), PONKO_OK);
        assert_eq!(ponko_ctx_draw_circle(ctx, v, 3.0, WHITE), PONKO_OK);
        assert_eq!(ponko_ctx_ui_panel(ctx, rect), PONKO_OK);
        assert_eq!(
            ponko_ctx_ui_label(ctx, v, c"Crabs: 12".as_ptr(), &mut d.label),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_label(ctx, v, c"no size wanted".as_ptr(), null_mut()),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_ui_button(ctx, rect, c"Go".as_ptr(), &mut d.clicked), PONKO_OK);
        assert_eq!(ponko_ctx_mouse_over_ui(ctx, &mut d.over_ui), PONKO_OK);
        assert_eq!(
            ponko_ctx_measure_text(ctx, c"Crabs: 12".as_ptr(), 16.0, &mut d.measured),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_measure_text_ex(
                ctx,
                c"Crabs: 12".as_ptr(),
                PonkoFont::default(),
                16.0,
                &mut d.measured_ex
            ),
            PONKO_OK
        );
        let mut params = ponko_text_params_default();
        params.size = 32.0;
        assert_eq!(ponko_ctx_draw_text_ex(ctx, c"big".as_ptr(), v, &params), PONKO_OK);
        assert_eq!(ponko_ctx_save_screenshot(ctx, c"never-written.png".as_ptr()), PONKO_OK);

        let mut style = ponko_ui_style_default();
        style.text_size = 24.0;
        style.accent = WHITE;
        assert_eq!(ponko_ctx_set_ui_style(ctx, &style), PONKO_OK);
        let mut read = ponko_ui_style_default();
        assert_eq!(ponko_ctx_ui_style(ctx, &mut read), PONKO_OK);
        d.style = Some(read);

        assert_eq!(ponko_ctx_set_ui_style(ctx, null()), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_ui_button(ctx, rect, c"Go".as_ptr(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_draw_text_ex(ctx, c"x".as_ptr(), v, null()),
            PONKO_INVALID_ARGUMENT
        );
    }
}

#[test]
fn shapes_text_and_widgets_work_headless() {
    let mut d = Drawn::default();
    run(1, Some(draw_everything), (&mut d as *mut Drawn).cast());
    assert!(d.label.x > 0.0 && d.label.y > 0.0, "{:?}", d.label);
    // The UI label uses the style's text size, 16 by default, like measure_text.
    assert_eq!(d.label, d.measured);
    assert_eq!(d.measured, d.measured_ex);
    assert!(!d.clicked, "nobody clicked in a headless run");
    // The headless mouse rests at (0, 0), inside the panel just drawn.
    assert!(d.over_ui);
    let style = d.style.unwrap();
    assert_eq!(style.text_size, 24.0);
    assert_eq!(style.accent, WHITE);
}

unsafe extern "C" fn unknown_font(ctx: *mut PonkoCtx, user: *mut c_void) {
    let mut size = PonkoVec2::default();
    let status = unsafe { ponko_ctx_measure_text_ex(ctx, c"x".as_ptr(), PonkoFont { id: 99 }, 16.0, &mut size) };
    unsafe { *user.cast::<Option<PonkoStatus>>() = Some(status) };
}

#[test]
fn made_up_fonts_fail_instead_of_crashing() {
    let mut status: Option<PonkoStatus> = None;
    run(1, Some(unknown_font), (&mut status as *mut Option<PonkoStatus>).cast());
    assert_ne!(status, Some(PONKO_OK));
    assert!(status.is_some());
}

unsafe extern "C" fn missing_files(ctx: *mut PonkoCtx, user: *mut c_void) {
    let errors = unsafe { &mut *user.cast::<Vec<String>>() };
    unsafe {
        let mut font = PonkoFont::default();
        assert_ne!(
            ponko_ctx_load_font(ctx, c"no-such-font.ttf".as_ptr(), &mut font),
            PONKO_OK
        );
        errors.push(last_error());
        let mut sound = PonkoSound { id: 0, seconds: 0.0 };
        assert_ne!(
            ponko_ctx_load_sound(ctx, c"no-such-sound.ogg".as_ptr(), &mut sound),
            PONKO_OK
        );
        errors.push(last_error());
    }
}

#[test]
#[cfg_attr(miri, ignore)] // the asset folder lookup reads the executable path
fn missing_fonts_and_sounds_fail_with_the_engine_message() {
    let mut errors: Vec<String> = Vec::new();
    run(1, Some(missing_files), (&mut errors as *mut Vec<String>).cast());
    assert_eq!(errors.len(), 2);
    assert!(errors[0].contains("no-such-font.ttf"), "{}", errors[0]);
    assert!(errors[1].contains("no-such-sound.ogg"), "{}", errors[1]);
}

#[derive(Default)]
struct Sounds {
    beep: Option<PonkoSound>,
    playing: Vec<PonkoPlaying>,
    errors: Vec<PonkoStatus>,
}

unsafe extern "C" fn make_sounds(ctx: *mut PonkoCtx, user: *mut c_void) {
    let s = unsafe { &mut *user.cast::<Sounds>() };
    let samples: Vec<f32> = (0..22050).map(|i| (i as f32 * 0.06).sin() * 0.3).collect();
    unsafe {
        let mut beep = PonkoSound { id: 0, seconds: 0.0 };
        assert_eq!(
            ponko_ctx_create_sound(ctx, 44100, samples.as_ptr(), samples.len(), &mut beep),
            PONKO_OK
        );
        s.beep = Some(beep);
        let mut p = PonkoPlaying { id: 7 };
        assert_eq!(ponko_ctx_play_sound(ctx, beep, null(), &mut p), PONKO_OK);
        s.playing.push(p);
        let mut params = ponko_sound_params_default();
        params.looping = true;
        params.volume = 0.5;
        assert_eq!(ponko_ctx_play_sound(ctx, beep, &params, &mut p), PONKO_OK);
        s.playing.push(p);
        assert_eq!(ponko_ctx_play_sound(ctx, beep, null(), null_mut()), PONKO_OK);
        assert_eq!(ponko_ctx_stop_sound(ctx, p), PONKO_OK);

        let mut out = beep;
        s.errors.push(ponko_ctx_create_sound(
            ctx,
            0,
            samples.as_ptr(),
            samples.len(),
            &mut out,
        ));
        s.errors.push(ponko_ctx_create_sound(ctx, 44100, null(), 4, &mut out));
        s.errors
            .push(ponko_ctx_create_sound(ctx, 44100, samples.as_ptr(), 4, null_mut()));
        // An empty sound is fine.
        assert_eq!(ponko_ctx_create_sound(ctx, 44100, null(), 0, &mut out), PONKO_OK);
        assert_eq!(out.seconds, 0.0);
    }
}

#[test]
fn sounds_are_made_from_samples_and_silent_headless() {
    let mut s = Sounds::default();
    run(1, Some(make_sounds), (&mut s as *mut Sounds).cast());
    let beep = s.beep.unwrap();
    assert!((beep.seconds - 0.5).abs() < 1e-3, "{}", beep.seconds);
    // Headless runs play nothing, but still hand each sound its own handle.
    let ids: Vec<u64> = s.playing.iter().map(|p| p.id).collect();
    assert!(
        ids.iter().all(|id| *id != 0) && ids.windows(2).all(|w| w[0] < w[1]),
        "{ids:?}"
    );
    assert_eq!(s.errors, [PONKO_INVALID_ARGUMENT; 3]);
}

#[derive(Default)]
struct TurnLog {
    seen: Vec<PonkoTurns>,
    zero: Option<PonkoStatus>,
}

unsafe extern "C" fn log_turns(ctx: *mut PonkoCtx, user: *mut c_void) {
    let log = unsafe { &mut *user.cast::<TurnLog>() };
    let mut t = PonkoTurns {
        every: 0,
        now: 0,
        dt: 0.0,
    };
    unsafe {
        assert_eq!(ponko_ctx_turns(ctx, 4, &mut t), PONKO_OK);
        log.zero = Some(ponko_ctx_turns(ctx, 0, &mut t));
    }
    log.seen.push(t);
}

#[test]
fn turns_cycle_through_the_round_with_its_whole_dt() {
    let mut log = TurnLog::default();
    run(8, Some(log_turns), (&mut log as *mut TurnLog).cast());
    assert_eq!(
        log.seen.iter().map(|t| t.now).collect::<Vec<_>>(),
        [0, 1, 2, 3, 0, 1, 2, 3]
    );
    assert!(
        log.seen
            .iter()
            .all(|t| t.every == 4 && (t.dt - 4.0 / 60.0).abs() < 1e-6)
    );
    assert_eq!(log.zero, Some(PONKO_INVALID_ARGUMENT));
}

#[derive(Default)]
struct HotReload {
    path: std::ffi::CString,
    file: std::path::PathBuf,
    texture: Option<PonkoTexture>,
    /// (tick, reloaded texture ids)
    seen: Vec<(u64, Vec<u32>)>,
}

unsafe extern "C" fn edit_and_watch(ctx: *mut PonkoCtx, user: *mut c_void) {
    let h = unsafe { &mut *user.cast::<HotReload>() };
    unsafe {
        let tick = ponko_ctx_sim_tick(ctx);
        if h.texture.is_none() {
            let mut t = std::mem::zeroed();
            assert_eq!(ponko_ctx_load_texture(ctx, h.path.as_ptr(), &mut t), PONKO_OK);
            h.texture = Some(t);
        }
        let (mut list, mut n) = (null(), 0);
        assert_eq!(ponko_ctx_reloaded_textures(ctx, &mut list, &mut n), PONKO_OK);
        if n > 0 {
            let ids = std::slice::from_raw_parts(list, n).iter().map(|t| t.id).collect();
            h.seen.push((tick, ids));
        }
        if tick == 2 {
            // Save the same image again: a new modification time is a change.
            let bytes = std::fs::read(&h.file).unwrap();
            // Windows file times move in ~16 ms steps: without the wait, a
            // same-length edit can carry the first write's time and length.
            std::thread::sleep(std::time::Duration::from_millis(50));
            std::fs::write(&h.file, bytes).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
    }
}

#[test]
fn edited_textures_are_reported_after_reloading() {
    let dir = std::env::temp_dir().join(format!("ponko-ffi-hot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("crab.png");
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/crab.png"), &file).unwrap();
    let mut h = HotReload {
        path: std::ffi::CString::new(file.to_str().unwrap()).unwrap(),
        file,
        ..Default::default()
    };
    unsafe {
        let game = ponko_game_new(c"hot".as_ptr(), 1, 1);
        assert_eq!(ponko_game_hot_reload(game, true), PONKO_OK);
        let user = (&mut h as *mut HotReload).cast();
        assert_eq!(
            ponko_game_sim_system(game, c"s".as_ptr(), 1, Some(edit_and_watch), user),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(game, 5, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    let id = h.texture.unwrap().id;
    assert_eq!(h.seen, vec![(3, vec![id])]);
    let (mut list, mut n) = (null(), 0);
    assert_eq!(
        unsafe { ponko_ctx_reloaded_textures(null(), &mut list, &mut n) },
        PONKO_INVALID_ARGUMENT
    );
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Rules {
    speed: f32,
    waves: u32,
}

#[derive(Default)]
struct DataTest {
    path: std::ffi::CString,
    rules: Rules,
    statuses: Vec<PonkoStatus>,
    errors: Vec<String>,
}

unsafe extern "C" fn load_rules(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let t = &mut *user.cast::<DataTest>();
        if ponko_ctx_sim_tick(ctx) > 0 {
            return;
        }
        let w = ponko_ctx_world(ctx);
        let mut id = 0;
        assert_eq!(
            // Namespaced, as C++ names its types.
            ponko_component_register(w, c"game::Rules".as_ptr(), 8, 4, &mut id),
            PONKO_OK
        );
        let mut e = 0;
        let v = PonkoValue {
            component: id,
            data: (&raw const t.rules).cast(),
            size: 8,
        };
        assert_eq!(ponko_spawn(w, &v, 1, &mut e), PONKO_OK);
        // Not reflected yet: refused with a reason.
        t.statuses.push(ponko_ctx_load_json(ctx, t.path.as_ptr(), e, id));
        t.errors.push(last_error());
        let fields = [
            PonkoField {
                name: c"speed".as_ptr(),
                kind: PonkoFieldType::PONKO_FIELD_F32 as u32,
                offset: 0,
            },
            PonkoField {
                name: c"waves".as_ptr(),
                kind: PonkoFieldType::PONKO_FIELD_U32 as u32,
                offset: 4,
            },
        ];
        assert_eq!(ponko_component_reflect(w, id, fields.as_ptr(), 2), PONKO_OK);
        t.statuses.push(ponko_ctx_load_json(ctx, t.path.as_ptr(), e, id));
        t.statuses
            .push(ponko_ctx_load_json(ctx, c"no-such-rules.json".as_ptr(), e, id));
        t.errors.push(last_error());
        let mut out = Rules::default();
        assert_eq!(ponko_get(w, e, id, (&raw mut out).cast(), 8), PONKO_OK);
        t.rules = out;
    }
}

#[test]
fn json_files_load_into_reflected_c_components() {
    let dir = std::env::temp_dir().join(format!("ponko-ffi-data-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("rules.json");
    std::fs::write(&file, r#"{"speed": 2.5, "waves": 7}"#).unwrap();
    let mut t = DataTest {
        path: std::ffi::CString::new(file.to_str().unwrap()).unwrap(),
        ..Default::default()
    };
    run(1, Some(load_rules), (&raw mut t).cast());
    assert_eq!(t.statuses, [PONKO_INVALID_ARGUMENT, PONKO_OK, PONKO_INVALID_ARGUMENT]);
    assert!(t.errors[0].contains("Rules"), "{}", t.errors[0]);
    assert!(t.errors[1].contains("not found in assets/"), "{}", t.errors[1]);
    assert_eq!((t.rules.speed, t.rules.waves), (2.5, 7));
}

unsafe extern "C" fn load_rules_resource(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let t = &mut *user.cast::<DataTest>();
        if ponko_ctx_sim_tick(ctx) > 0 {
            return;
        }
        let w = ponko_ctx_world(ctx);
        // Not described yet: refused with a reason.
        t.statuses.push(ponko_ctx_load_resource_json(
            ctx,
            t.path.as_ptr(),
            c"game::Rules".as_ptr(),
        ));
        t.errors.push(last_error());
        let fields = [
            PonkoField {
                name: c"speed".as_ptr(),
                kind: PonkoFieldType::PONKO_FIELD_F32 as u32,
                offset: 0,
            },
            PonkoField {
                name: c"waves".as_ptr(),
                kind: PonkoFieldType::PONKO_FIELD_U32 as u32,
                offset: 4,
            },
        ];
        let status = ponko_resource_reflect(w, c"game::Rules".as_ptr(), 8, 4, fields.as_ptr(), 2);
        assert_eq!(status, PONKO_OK);
        // Added by the load, since the world has no Rules yet.
        t.statuses.push(ponko_ctx_load_resource_json(
            ctx,
            t.path.as_ptr(),
            c"game::Rules".as_ptr(),
        ));
        let mut out = Rules::default();
        assert_eq!(
            ponko_resource_get(w, c"game::Rules".as_ptr(), (&raw mut out).cast(), 8),
            PONKO_OK
        );
        t.rules = out;
    }
}

#[test]
fn json_files_load_into_described_c_resources() {
    let dir = std::env::temp_dir().join(format!("ponko-ffi-resource-data-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("rules.json");
    std::fs::write(&file, r#"{"speed": 1.5, "waves": 4}"#).unwrap();
    let mut t = DataTest {
        path: std::ffi::CString::new(file.to_str().unwrap()).unwrap(),
        ..Default::default()
    };
    run(1, Some(load_rules_resource), (&raw mut t).cast());
    assert_eq!(t.statuses, [PONKO_INVALID_ARGUMENT, PONKO_OK]);
    assert!(t.errors[0].contains("game::Rules"), "{}", t.errors[0]);
    assert_eq!((t.rules.speed, t.rules.waves), (1.5, 4));
    std::fs::remove_dir_all(dir).ok();
}

#[derive(Default)]
struct Reload {
    path: std::ffi::CString,
    file: std::path::PathBuf,
    id: u32,
    entity: u64,
    seen: Vec<(u64, bool, u32)>,
}

unsafe extern "C" fn reload_rules(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let t = &mut *user.cast::<Reload>();
        let w = ponko_ctx_world(ctx);
        let tick = ponko_ctx_sim_tick(ctx);
        if tick == 0 {
            assert_eq!(
                ponko_component_register(w, c"Waves".as_ptr(), 4, 4, &mut t.id),
                PONKO_OK
            );
            let f = [PonkoField {
                name: c"waves".as_ptr(),
                kind: PonkoFieldType::PONKO_FIELD_U32 as u32,
                offset: 0,
            }];
            assert_eq!(ponko_component_reflect(w, t.id, f.as_ptr(), 1), PONKO_OK);
            let zero = 0u32;
            let v = PonkoValue {
                component: t.id,
                data: (&raw const zero).cast(),
                size: 4,
            };
            assert_eq!(ponko_spawn(w, &v, 1, &mut t.entity), PONKO_OK);
            assert_eq!(ponko_ctx_load_json(ctx, t.path.as_ptr(), t.entity, t.id), PONKO_OK);
        }
        let mut reloaded = true;
        assert_eq!(ponko_ctx_was_reloaded(ctx, t.path.as_ptr(), &mut reloaded), PONKO_OK);
        let mut waves = 0u32;
        assert_eq!(ponko_get(w, t.entity, t.id, (&raw mut waves).cast(), 4), PONKO_OK);
        t.seen.push((tick, reloaded, waves));
        if tick == 1 {
            std::thread::sleep(std::time::Duration::from_millis(50)); // see above
            std::fs::write(&t.file, r#"{"waves": 9}"#).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
    }
}

#[test]
fn edited_json_files_update_c_components_while_running() {
    let dir = std::env::temp_dir().join(format!("ponko-ffi-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("waves.json");
    std::fs::write(&file, r#"{"waves": 3}"#).unwrap();
    let mut t = Reload {
        path: std::ffi::CString::new(file.to_str().unwrap()).unwrap(),
        file,
        ..Default::default()
    };
    unsafe {
        let game = ponko_game_new(c"reload".as_ptr(), 1, 1);
        assert_eq!(ponko_game_hot_reload(game, true), PONKO_OK);
        assert_eq!(
            ponko_game_sim_system(game, c"s".as_ptr(), 1, Some(reload_rules), (&raw mut t).cast()),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(game, 4, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    assert_eq!(t.seen, [(0, false, 3), (1, false, 3), (2, true, 9), (3, false, 9)]);
}
