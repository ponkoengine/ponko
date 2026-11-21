//! Crash reports from C: the game's version and hook from a
//! `PonkoCrashReports`, the game's own fatal error (`ponko_crash_report`),
//! a panic, the saved reports listed newest first and read back, and the
//! descriptor's checks. One test: the panic hook is the process's.
mod common;
use common::last_error;
use ponko_ffi::*;
use std::ffi::{CStr, CString, c_void};
use std::ptr::null_mut;
use std::sync::Mutex;

use PonkoStatus::*;

/// One report as the hook saw it.
#[derive(Debug, Clone)]
struct Seen {
    kind: u32,
    message: String,
    game_version: Option<String>,
    engine_version: String,
    file: Option<String>,
    path: Option<String>,
    text: String,
    user: usize,
}

static SEEN: Mutex<Vec<Seen>> = Mutex::new(Vec::new());

unsafe fn string(p: *const std::ffi::c_char) -> Option<String> {
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

unsafe extern "C" fn hook(report: *const PonkoCrashReport, user: *mut c_void) {
    let r = unsafe { &*report };
    let seen = unsafe {
        Seen {
            kind: r.kind,
            message: string(r.message).unwrap_or_default(),
            game_version: string(r.game_version),
            engine_version: string(r.engine_version).unwrap_or_default(),
            file: string(r.file),
            path: string(r.path),
            text: string(r.text).unwrap_or_default(),
            user: user as usize,
        }
    };
    SEEN.lock().unwrap_or_else(|e| e.into_inner()).push(seen);
}

fn reports(mode: PonkoCrashReportsMode, version: Option<&std::ffi::CStr>) -> PonkoCrashReports {
    PonkoCrashReports {
        struct_size: size_of::<PonkoCrashReports>() as u32,
        version: PONKO_CRASH_REPORTS_VERSION,
        mode: mode as u32,
        game_version: version.map_or(std::ptr::null(), |v| v.as_ptr()),
        on_crash: Some(hook),
        user: 42usize as *mut c_void,
    }
}

unsafe fn saved(ctx: *mut PonkoCtx) -> Vec<String> {
    let mut len = 0;
    assert_eq!(
        unsafe { ponko_ctx_saved_crash_reports(ctx, null_mut(), 0, &mut len) },
        PONKO_OK
    );
    let mut buffer = vec![0u8; len + 1];
    assert_eq!(
        unsafe { ponko_ctx_saved_crash_reports(ctx, buffer.as_mut_ptr().cast(), buffer.len(), &mut len) },
        PONKO_OK
    );
    let text = String::from_utf8_lossy(&buffer[..len]).into_owned();
    text.lines().map(str::to_string).filter(|l| !l.is_empty()).collect()
}

unsafe fn read(ctx: *mut PonkoCtx, name: &str) -> String {
    let name = CString::new(name).unwrap();
    let mut len = 0;
    unsafe {
        assert_eq!(
            ponko_ctx_storage_read(ctx, name.as_ptr(), null_mut(), 0, &mut len),
            PONKO_OK
        );
        let mut bytes = vec![0u8; len];
        assert_eq!(
            ponko_ctx_storage_read(ctx, name.as_ptr(), bytes.as_mut_ptr(), len, &mut len),
            PONKO_OK
        );
        String::from_utf8(bytes).unwrap()
    }
}

fn game(dir: &CString, reports: Option<&PonkoCrashReports>) -> *mut PonkoGame {
    unsafe {
        let game = ponko_game_new(c"Space Crab".as_ptr(), 1, 1);
        assert_eq!(ponko_game_hot_reload(game, false), PONKO_OK);
        assert_eq!(ponko_game_data_dir(game, dir.as_ptr()), PONKO_OK);
        if let Some(r) = reports {
            assert_eq!(ponko_game_crash_reports(game, r), PONKO_OK, "{}", last_error());
        }
        game
    }
}

#[test]
fn crash_reports_from_c_name_the_version_run_the_hook_and_are_saved() {
    let dir = std::env::temp_dir().join(format!("ponko-ffi-crash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let c_dir = CString::new(dir.to_string_lossy().as_bytes()).unwrap();

    // On, with a version and a hook: the game's own error, then a panic.
    let on = reports(PonkoCrashReportsMode::PONKO_CRASH_REPORTS_ON, Some(c"1.4.2"));
    let world = common::run_game(game(&c_dir, Some(&on)), 0, |ctx| unsafe {
        assert_eq!(ponko_crash_report(c"the save file is damaged".as_ptr()), PONKO_OK);
        {
            let seen = SEEN.lock().unwrap();
            let r = seen.last().expect("the hook ran");
            assert_eq!(r.kind, PonkoCrashKind::PONKO_CRASH_ERROR as u32);
            assert_eq!(r.message, "the save file is damaged");
            assert_eq!(r.game_version.as_deref(), Some("1.4.2"));
            assert!(!r.engine_version.is_empty());
            assert_eq!(r.user, 42, "the hook gets the game's user pointer");
            assert!(r.text.contains("game version: 1.4.2"), "{}", r.text);
            let file = r.file.clone().expect("saved");
            assert!(file.starts_with("crash-reports/") && file.ends_with(".txt"), "{file}");
            assert!(std::path::Path::new(r.path.as_deref().unwrap()).exists());
            // Listed and read back as any stored file.
            assert_eq!(saved(ctx), vec![file.clone()]);
            assert_eq!(read(ctx, &file), r.text);
        }
        // A panic the game catches gets its report too.
        let caught = std::panic::catch_unwind(|| panic!("the crab fell off the map"));
        assert!(caught.is_err());
        let seen = SEEN.lock().unwrap();
        let r = seen.last().unwrap();
        assert_eq!(r.kind, PonkoCrashKind::PONKO_CRASH_PANIC as u32);
        assert_eq!(r.message, "the crab fell off the map");
    });
    unsafe { ponko_world_free(world) };
    assert_eq!(SEEN.lock().unwrap().len(), 2);

    // The next run lists both, newest first; reports off write nothing.
    let off = reports(PonkoCrashReportsMode::PONKO_CRASH_REPORTS_OFF, None);
    let world = common::run_game(game(&c_dir, Some(&off)), 0, |ctx| unsafe {
        let names = saved(ctx);
        assert!(names.len() <= 2 && !names.is_empty(), "{names:?}");
        let mut sorted = names.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(names, sorted, "newest first");
        assert_eq!(ponko_crash_report(c"not written".as_ptr()), PONKO_NOT_FOUND);
    });
    unsafe { ponko_world_free(world) };
    assert_eq!(SEEN.lock().unwrap().len(), 2, "no hook with reports off");

    // The build's default is off in a headless run.
    let world = common::run_game(game(&c_dir, None), 0, |_| unsafe {
        assert_eq!(ponko_crash_report(c"headless".as_ptr()), PONKO_NOT_FOUND);
    });
    unsafe { ponko_world_free(world) };

    // The descriptor's checks: a bad mode, version 0, null.
    unsafe {
        let g = ponko_game_new(c"bad".as_ptr(), 1, 1);
        let bad_mode = PonkoCrashReports {
            mode: 9,
            ..reports(PonkoCrashReportsMode::PONKO_CRASH_REPORTS_ON, None)
        };
        assert_eq!(ponko_game_crash_reports(g, &bad_mode), PONKO_INVALID_ARGUMENT);
        let v0 = PonkoCrashReports { version: 0, ..bad_mode };
        assert_eq!(ponko_game_crash_reports(g, &v0), PONKO_INVALID_ARGUMENT);
        assert!(last_error().contains("version is 0"), "{}", last_error());
        assert_eq!(ponko_game_crash_reports(g, std::ptr::null()), PONKO_INVALID_ARGUMENT);
        ponko_game_free(g);
    }
    let _ = std::fs::remove_dir_all(dir);
}
