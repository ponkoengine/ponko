//! End to end: `ponko new --server` against this engine checkout, then the
//! server program as its own process, the example client joining it over
//! UDP, and `SIGTERM` stopping it cleanly, the way `docker stop` does. The
//! container itself is CI's "Server container" job.
//!
//! Ignored by default: it builds the engine for the template (minutes on a
//! cold machine). Run it with
//! `cargo test -p ponko_cli --test server -- --ignored`. The build output
//! goes to `CARGO_TARGET_DIR` if it is set, else to
//! `target/ponko-server-e2e`, deleted afterwards.

use std::io::{BufRead, BufReader};
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Deletes a folder when dropped, also when the test fails.
struct Remove(PathBuf);
impl Drop for Remove {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Kills the server if the test fails before it stopped.
struct Kill(std::process::Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// A UDP port nobody uses right now.
fn free_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[test]
#[ignore = "builds the engine for the server template (minutes)"]
fn the_server_template_accepts_its_client_and_stops_on_sigterm() {
    let root = workspace_root();
    let projects = std::env::temp_dir().join(format!("ponko-server-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&projects);
    std::fs::create_dir_all(&projects).unwrap();
    let _projects = Remove(projects.clone());
    let (target, _target) = match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => (PathBuf::from(dir), None),
        None => {
            let dir = root.join("target/ponko-server-e2e");
            (dir.clone(), Some(Remove(dir)))
        }
    };

    let made = Command::new(env!("CARGO_BIN_EXE_ponko"))
        .current_dir(&projects)
        .args(["new", "e2e-server", "--server", "--path"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    let dir = projects.join("e2e-server");

    // The project's own tests: a server and a client in one process.
    let tested = cargo()
        .current_dir(&dir)
        .args(["test", "--quiet"])
        .env("CARGO_TARGET_DIR", &target)
        .status()
        .unwrap();
    assert!(tested.success(), "cargo test in the generated server");
    let built = cargo()
        .current_dir(&dir)
        .args(["build", "--quiet", "--bins", "--examples"])
        .env("CARGO_TARGET_DIR", &target)
        .status()
        .unwrap();
    assert!(built.success(), "cargo build in the generated server");

    // `ponko package` with its smoke run (`PONKO_SMOKE`, which
    // `run_server` honours), as the Dockerfile's build stage runs it.
    let packaged = Command::new(env!("CARGO_BIN_EXE_ponko"))
        .current_dir(&dir)
        .args(["package", "--format", "folder", "--profile", "dev", "--out"])
        .arg(projects.join("package"))
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .unwrap();
    assert!(
        packaged.status.success(),
        "ponko package: {}{}",
        String::from_utf8_lossy(&packaged.stdout),
        String::from_utf8_lossy(&packaged.stderr)
    );

    let exe = |path: &str| {
        target
            .join("debug")
            .join(format!("{path}{}", std::env::consts::EXE_SUFFIX))
    };
    let addr = format!("127.0.0.1:{}", free_port());
    let mut server = Kill(
        Command::new(exe("e2e-server"))
            .current_dir(&dir)
            .env("SERVER_ADDR", &addr)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    // Every log line, as JSON, from a thread (the pipe must not fill up).
    let stdout = server.0.stdout.take().unwrap();
    let (lines_tx, lines) = std::sync::mpsc::channel::<serde_json::Value>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let value = serde_json::from_str(&line).unwrap_or_else(|e| panic!("not a JSON line ({e}): {line}"));
            if lines_tx.send(value).is_err() {
                break;
            }
        }
    });
    let mut seen = Vec::new();
    // The server is another process: each wait ends at its log line, and its
    // limit is only a hang detector.
    let mut wait_for = |event: &str, within: Duration| {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if let Ok(line) = lines.recv_timeout(Duration::from_millis(100)) {
                let found = line["event"] == event;
                seen.push(line);
                if found {
                    return seen.last().unwrap().clone();
                }
            }
        }
        panic!("no `{event}` log line within {within:?}; saw {seen:#?}");
    };
    let start = wait_for("server_start", Duration::from_secs(30));
    assert_eq!(start["hz"], 30);
    assert_eq!(start["listen"], addr.as_str());
    assert_eq!(
        start["control"],
        serde_json::Value::Null,
        "no control endpoint unless asked"
    );

    let client = Command::new(exe("examples/client"))
        .current_dir(&dir)
        .arg(&addr)
        .env("TICKS", "60")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&client.stdout);
    assert!(
        client.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&client.stderr)
    );
    assert!(out.contains("joined"), "{out}");
    let joined = wait_for("player_joined", Duration::from_secs(30));
    assert_eq!(joined["name"], "bot");
    wait_for("client_left", Duration::from_secs(30));

    #[cfg(unix)]
    {
        // SIGTERM, as `docker stop` sends it.
        let killed = Command::new("kill")
            .args(["-TERM", &server.0.id().to_string()])
            .status()
            .unwrap();
        assert!(killed.success());
        let stopping = wait_for("server_stopping", Duration::from_secs(30));
        assert_eq!(stopping["reason"], "terminate");
        let stopped = wait_for("server_stopped", Duration::from_secs(30));
        assert_eq!(stopped["reason"], "terminate");
        let status = server.0.wait().unwrap();
        assert!(status.success(), "a clean exit after SIGTERM: {status}");
        assert!(
            seen.iter().any(|l| l["event"] == "tick_report"),
            "a tick report at the stop at least"
        );
    }
}
