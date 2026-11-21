//! Crash reports (`ponko::crash`): a panic or a fatal error writes a report
//! into the game's data folder, the game's hook gets it, and nothing in it
//! names the player.
//!
//! The panic hook is one per process: the tests take turns.

use std::io::{Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ponko::crash::{self, CrashKind, CrashReport, CrashReports};
use ponko::prelude::*;
use ponko::storage::Storage;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn take_turn() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A fresh, empty folder for one test.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ponko-crash-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Runs a game whose first tick panics with `message`; the panic stops here.
fn crash_with(game: Game, message: String) {
    let result = catch_unwind(AssertUnwindSafe(|| {
        game.sim_system(move |_ctx: &mut Ctx| panic!("{message}"))
            .run_headless(1, |_ctx| {});
    }));
    assert!(result.is_err(), "the game panicked");
}

fn reports_in(dir: &Path) -> Vec<String> {
    crash::saved_reports(&Storage::in_folder(dir))
}

fn read(dir: &Path, name: &str) -> String {
    String::from_utf8(Storage::in_folder(dir).read(name).unwrap()).unwrap()
}

#[test]
fn a_panic_writes_a_report_into_the_games_data_folder() {
    let _turn = take_turn();
    let dir = temp_dir("panic");
    let game = Game::new("Space Crab", 1, 1)
        .data_dir(&dir)
        .crash_reports(CrashReports::on().game_version("1.2.3"));
    crash_with(game, "the crab fell off the map".into());

    let names = reports_in(&dir);
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].starts_with("crash-reports/") && names[0].ends_with(".txt"));
    let text = read(&dir, &names[0]);
    for line in [
        "Space Crab crash report",
        "kind: panic",
        "message: the crab fell off the map",
        "location: tests/crash_reports.rs:",
        "game version: 1.2.3",
        &format!("engine version: {}", env!("CARGO_PKG_VERSION")),
        &format!("os: {}", std::env::consts::OS),
        "gpu: not opened",
        "settings:",
        "backtrace:",
    ] {
        assert!(text.contains(line), "the report has {line:?}:\n{text}");
    }
    let backtrace = text.split("backtrace:\n").nth(1).unwrap();
    assert!(backtrace.lines().count() > 3, "a backtrace with frames:\n{text}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_report_names_no_user_and_paths_are_relative_to_the_games_folders() {
    let _turn = take_turn();
    let dir = temp_dir("private");
    let game = Game::new("t", 1, 1).data_dir(&dir).crash_reports(true);
    let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap();
    let exe_dir = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    let message = format!(
        "could not read {} nor {} nor {}",
        dir.join("saves").join("slot1.json").display(),
        exe_dir.join("assets.pack").display(),
        Path::new(&home).join("Desktop").join("mod.png").display(),
    );
    crash_with(game, message);

    let text = read(&dir, &reports_in(&dir)[0]);
    let sep = std::path::MAIN_SEPARATOR;
    assert!(
        text.contains(&format!(
            "could not read <data>{sep}saves{sep}slot1.json nor <game>{sep}assets.pack nor ~{sep}Desktop{sep}mod.png"
        )),
        "{text}"
    );
    assert!(
        !text.contains(&home),
        "the home folder stays out of the report:\n{text}"
    );
    let user = Path::new(&home).file_name().unwrap().to_string_lossy().into_owned();
    if user.len() > 3 {
        assert!(
            !text.contains(&user),
            "the user's name stays out of the report:\n{text}"
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_games_hook_gets_the_report_after_it_was_saved() {
    let _turn = take_turn();
    let dir = temp_dir("hook");
    let seen: Arc<Mutex<Vec<CrashReport>>> = Arc::default();
    let into = seen.clone();
    let game = Game::new("t", 1, 1)
        .data_dir(&dir)
        .crash_reports(CrashReports::on().on_crash(move |report| {
            // The file is there when the hook runs.
            assert!(report.path().is_some_and(Path::is_file));
            into.lock().unwrap().push(report.clone());
        }));
    crash_with(game, "boom".into());

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let report = &seen[0];
    assert_eq!(report.kind(), CrashKind::Panic);
    assert_eq!(report.message(), "boom");
    assert_eq!(report.file(), Some(reports_in(&dir)[0].as_str()));
    assert_eq!(report.path(), Some(dir.join(report.file().unwrap()).as_path()));
    assert_eq!(read(&dir, report.file().unwrap()), report.text());
    let json = serde_json::to_value(report).unwrap();
    assert_eq!(json["message"], "boom");
    assert_eq!(json["kind"], "Panic");
    assert!(json.get("path").is_none(), "the absolute path stays out of the JSON");
    let _ = std::fs::remove_dir_all(dir);
}

/// What the local server got: the request line and the body.
fn serve_one_post(listener: std::net::TcpListener) -> std::thread::JoinHandle<(String, String)> {
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = stream.read(&mut buf).unwrap();
            request.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&request);
            if let Some(end) = text.find("\r\n\r\n") {
                let length: usize = text[..end]
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                if request.len() >= end + 4 + length || n == 0 {
                    stream.write_all(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap();
                    let text = String::from_utf8(request).unwrap();
                    let (head, body) = text.split_once("\r\n\r\n").unwrap();
                    return (head.lines().next().unwrap().to_string(), body.to_string());
                }
            }
        }
    })
}

#[test]
fn the_hook_can_send_the_report_to_a_server_the_game_names() {
    let _turn = take_turn();
    let dir = temp_dir("send");
    // The game's own server: here a local one, in this process.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap();
    let server = serve_one_post(listener);
    let send = move |report: &CrashReport| {
        let body = report.text();
        let mut stream = std::net::TcpStream::connect(endpoint).unwrap();
        let head = format!("POST /crash HTTP/1.1\r\nHost: {endpoint}\r\nContent-Type: text/plain\r\n");
        write!(stream, "{head}Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
    };
    let game = Game::new("t", 1, 1)
        .data_dir(&dir)
        .crash_reports(CrashReports::on().on_crash(send));
    crash_with(game, "sent".into());

    let (request, body) = server.join().unwrap();
    assert_eq!(request, "POST /crash HTTP/1.1");
    assert_eq!(body, read(&dir, &reports_in(&dir)[0]));
    assert!(body.contains("message: sent"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn headless_runs_write_no_reports_unless_asked() {
    let _turn = take_turn();
    let dir = temp_dir("default");
    crash_with(Game::new("t", 1, 1).data_dir(&dir), "not reported".into());
    assert!(reports_in(&dir).is_empty());
    crash_with(
        Game::new("t", 1, 1).data_dir(&dir).crash_reports(false),
        "not reported".into(),
    );
    assert!(reports_in(&dir).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn reports_end_with_the_run() {
    let _turn = take_turn();
    let dir = temp_dir("ended");
    Game::new("t", 1, 1)
        .data_dir(&dir)
        .crash_reports(true)
        .run_headless(1, |_ctx| {});
    let later = catch_unwind(|| panic!("after the game"));
    assert!(later.is_err());
    assert!(crash::report("after the game").is_none());
    assert!(reports_in(&dir).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn only_the_first_panic_of_a_run_gets_a_report() {
    let _turn = take_turn();
    let dir = temp_dir("first");
    let game = Game::new("t", 1, 1)
        .data_dir(&dir)
        .crash_reports(true)
        .sim_system(|_ctx: &mut Ctx| {
            // A panic the game catches still gets a report...
            let _ = catch_unwind(|| panic!("the first"));
        });
    // ...and the ones after it in the run don't.
    crash_with(game, "the second".into());
    let names = reports_in(&dir);
    assert_eq!(names.len(), 1);
    assert!(read(&dir, &names[0]).contains("message: the first"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn only_the_newest_reports_are_kept() {
    let _turn = take_turn();
    let dir = temp_dir("kept");
    let mut storage = Storage::in_folder(&dir);
    for day in 1..=crash::KEPT + 3 {
        storage
            .write(&format!("crash-reports/2001-01-{day:02}_00-00-00.txt"), b"old")
            .unwrap();
    }
    storage.write("saves/slot1.json", b"{}").unwrap();
    crash_with(Game::new("t", 1, 1).data_dir(&dir).crash_reports(true), "new".into());
    let names = reports_in(&dir);
    assert_eq!(names.len(), crash::KEPT);
    assert!(
        read(&dir, &names[0]).contains("message: new"),
        "the new one is the newest"
    );
    assert_eq!(
        names.last().unwrap(),
        &format!("crash-reports/2001-01-{:02}_00-00-00.txt", 5),
        "the oldest went"
    );
    assert!(storage.exists("saves/slot1.json"), "other files stay");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_game_without_storage_still_gets_its_hook() {
    let _turn = take_turn();
    let seen: Arc<Mutex<Option<CrashReport>>> = Arc::default();
    let into = seen.clone();
    let game = Game::new("t", 1, 1).crash_reports(CrashReports::on().on_crash(move |report| {
        *into.lock().unwrap() = Some(report.clone());
    }));
    crash_with(game, "nowhere to save".into());
    let report = seen.lock().unwrap().take().expect("the hook ran");
    assert_eq!(report.file(), None);
    assert_eq!(report.path(), None);
    assert_eq!(report.message(), "nowhere to save");
}

#[test]
fn a_fatal_error_writes_a_report() {
    let _turn = take_turn();
    let dir = temp_dir("error");
    let failed = Game::new("t", 1, 1)
        .data_dir(&dir)
        .crash_reports(true)
        .try_setup(|_ctx| {
            Err(ponko::error::Error::new(
                ponko::error::ErrorKind::NotFound,
                "level1.json is missing",
            ))
        })
        .try_run_headless(1);
    assert!(failed.is_err());
    let names = reports_in(&dir);
    assert_eq!(names.len(), 1);
    let text = read(&dir, &names[0]);
    assert!(text.contains("kind: error"), "{text}");
    assert!(text.contains("level1.json is missing"), "{text}");
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(any(unix, windows))]
#[test]
fn the_last_lines_the_game_printed_are_in_the_report() {
    let _turn = take_turn();
    let dir = temp_dir("log");
    let game = Game::new("t", 1, 1).data_dir(&dir).crash_reports(true);
    let result = catch_unwind(AssertUnwindSafe(|| {
        game.run_headless(1, |_ctx: &mut Ctx| -> () {
            // Straight to the process's standard error (the test harness
            // keeps what `eprintln!` prints for itself).
            let mut err = std::io::stderr();
            for i in 0..crash::LOG_LINES + 5 {
                writeln!(err, "loading level part {i}").unwrap();
            }
            panic!("after the log");
        });
    }));
    assert!(result.is_err());
    let text = read(&dir, &reports_in(&dir)[0]);
    let log = text
        .split("log lines:\n")
        .nth(1)
        .unwrap_or_else(|| panic!("a log:\n{text}"));
    assert!(
        log.contains(&format!("loading level part {}", crash::LOG_LINES + 4)),
        "{text}"
    );
    assert!(!log.contains("loading level part 4\n"), "only the last lines:\n{text}");
    let _ = std::fs::remove_dir_all(dir);
}
