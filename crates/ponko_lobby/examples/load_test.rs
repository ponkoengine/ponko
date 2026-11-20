//! The lobby server under load, headless, in one process:
//!
//! ```sh
//! cargo run --release -p ponko_lobby --example load_test            # 1000 rooms
//! cargo run --release -p ponko_lobby --example load_test -- 200 4   # 200 rooms of 4
//! ```
//!
//! 1000 rooms of 3 made by code plus 100 by matchmaking (3300 clients over
//! in-memory endpoints, through the real protocol and reliability layer),
//! 20 relay messages of 64 bytes from every member to its room, then every
//! host leaves and the rooms migrate. It prints the rates and the memory.

use ponko_lobby::load::{LoadConfig, run};

/// Resident memory at most so far in bytes, and CPU seconds used (user and
/// system), on Unix; `None` elsewhere.
fn usage() -> Option<(u64, f64)> {
    #[cfg(unix)]
    {
        // SAFETY: getrusage fills the zeroed struct it is given.
        let usage = unsafe {
            let mut usage: libc::rusage = std::mem::zeroed();
            if libc::getrusage(libc::RUSAGE_SELF, &mut usage) != 0 {
                return None;
            }
            usage
        };
        let max = u64::try_from(usage.ru_maxrss).ok()?;
        // Bytes on macOS, kilobytes on Linux and the BSDs.
        let rss = if cfg!(target_os = "macos") { max } else { max * 1024 };
        let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
        Some((rss, secs(usage.ru_utime) + secs(usage.ru_stime)))
    }
    #[cfg(not(unix))]
    None
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut config = LoadConfig::default();
    if let Some(rooms) = args.next() {
        config.rooms = rooms.parse().expect("rooms: a number");
    }
    if let Some(size) = args.next() {
        config.room_size = size.parse().expect("room size: a number");
    }
    let before = usage();
    let report = match run(&config) {
        Ok(report) => report,
        Err(e) => {
            eprintln!("load test failed: {e}");
            std::process::exit(1);
        }
    };
    let mib = |b: u64| b as f64 / (1024.0 * 1024.0);
    println!(
        "{} rooms of {} by code + {} by matchmaking, {} clients",
        config.rooms, config.room_size, config.matched_rooms, report.clients
    );
    println!("connect          {:>8.2} s", report.connect.as_secs_f64());
    println!(
        "fill rooms       {:>8.2} s   {:>9.0} rooms/s",
        report.fill_rooms.as_secs_f64(),
        report.rooms_per_second
    );
    println!("matchmaking      {:>8.2} s", report.matchmaking.as_secs_f64());
    println!(
        "relay            {:>8.2} s   {:>9.0} msgs/s  {:.2} MiB/s payload  ({} of {} delivered)",
        report.relay.as_secs_f64(),
        report.relay_messages_per_second,
        report.relay_mib_per_second,
        report.relay_delivered,
        report.relay_expected
    );
    println!("host migrations  {:>8}", report.migrations_seen);
    println!(
        "total            {:>8.2} s wall, {:.1} s simulated",
        report.total.as_secs_f64(),
        report.simulated.as_secs_f64()
    );
    match (before, usage()) {
        (Some((rss_before, cpu_before)), Some((rss, cpu))) => {
            println!(
                "cpu              {:>8.2} s   (user + system; wall above is longer on a busy machine)",
                cpu - cpu_before
            );
            println!(
                "peak memory      {:>8.1} MiB (process: server and every client; {:.1} MiB at start)",
                mib(rss),
                mib(rss_before)
            );
        }
        _ => println!("cpu, memory      not available on this platform"),
    }
    let s = report.server;
    println!(
        "server: {} rooms made, {} matched, {} migrations, {} requests, {} refused, {} relay dropped",
        s.rooms_created, s.matches_made, s.host_migrations, s.requests, s.refused, s.relay_dropped
    );
}
