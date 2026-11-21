//! The `ponko-lobby` program: the smoke run `ponko package` does, and a
//! real run that serves a client and stops on `SIGTERM`.

#[cfg(unix)]
use std::io::{BufRead, BufReader};
use std::process::Command;
#[cfg(unix)]
use std::process::{Child, Stdio};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use ponko::net::lobby::LobbyClient;

const BIN: &str = env!("CARGO_BIN_EXE_ponko-lobby");

/// Kills and reaps the program if a test fails before it exits.
#[cfg(unix)]
struct Reaped(Child);

#[cfg(unix)]
impl Drop for Reaped {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn the_program_passes_the_package_smoke_run() {
    let out = Command::new(BIN)
        .env("PONKO_SMOKE", "5")
        .env("LOBBY_UDP", "127.0.0.1:0")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("ponko: smoke run passed"));
}

#[test]
fn the_program_refuses_settings_it_cannot_use() {
    let out = Command::new(BIN).env("LOBBY_HZ", "fast").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let log = String::from_utf8_lossy(&out.stdout);
    assert!(
        log.contains("lobby_start_failed") && log.contains("LOBBY_HZ=fast"),
        "{log}"
    );
    let help = Command::new(BIN).arg("--help").output().unwrap();
    assert!(String::from_utf8_lossy(&help.stdout).contains("LOBBY_UDP"));
}

#[cfg(unix)]
#[test]
fn the_program_serves_clients_and_stops_cleanly_on_sigterm() {
    let mut child = Reaped(
        Command::new(BIN)
            .env("LOBBY_UDP", "127.0.0.1:0")
            .env("LOBBY_STUN", "127.0.0.1:0")
            .env("LOBBY_REPORT_SECS", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let mut lines = BufReader::new(child.0.stdout.take().unwrap()).lines();
    // The first log lines say where it listens.
    let (udp, stun) = loop {
        let line = lines.next().expect("the server logs where it listens").unwrap();
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        if value["event"] == "lobby_listening" {
            break (
                value["udp"].as_str().unwrap().to_owned(),
                value["stun"].as_str().unwrap().to_owned(),
            );
        }
    };
    // Its STUN responder tells a socket its own address.
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    // The server is another process: these waits end when it answers, and
    // their limits are hang detectors only.
    socket.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    socket
        .send_to(&ponko::net::stun::binding_request([5; 12]), stun.as_str())
        .unwrap();
    let mut buf = [0u8; 128];
    let (n, _) = socket.recv_from(&mut buf).expect("a STUN answer");
    assert_eq!(
        ponko::net::stun::mapped_address(&buf[..n], [5; 12]),
        Some(socket.local_addr().unwrap())
    );
    let mut client = LobbyClient::join(udp.as_str()).unwrap();
    client.create_room(2).unwrap();
    let start = Instant::now();
    while client.room().is_none() {
        assert!(start.elapsed() < Duration::from_secs(30), "no room from the server");
        client.poll().unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(client.room().unwrap().code.len(), 6);

    // SAFETY: signals a child process this test started and still owns.
    assert_eq!(unsafe { libc::kill(child.0.id() as i32, libc::SIGTERM) }, 0);
    let mut left = false;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(30) {
        client.poll().unwrap();
        while let Some(event) = client.next_event() {
            left |= event == ponko::net::lobby::LobbyEvent::Left(ponko::net::lobby::LeaveReason::ServerStopping);
        }
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "{status:?}");
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(left, "the member heard that the server stops");
    let rest: Vec<String> = lines.map_while(Result::ok).collect();
    assert!(rest.iter().any(|l| l.contains("\"lobby_stopped\"")), "{rest:?}");
    assert!(rest.iter().any(|l| l.contains("\"server_stopped\"")), "{rest:?}");
}
