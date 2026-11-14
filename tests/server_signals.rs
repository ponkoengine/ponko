//! `Game::run_server` and stop signals. One test in its own program: the
//! handlers are process-wide, and a signal raised while none is installed
//! would end the test binary.

#![cfg(any(unix, windows))]

use std::time::Duration;

use ponko::prelude::*;
use ponko::server::{LogFormat, ServerConfig, ServerStatus, StopReason};

fn raise(signal: libc::c_int) {
    // SAFETY: raising a signal the server has a handler for.
    assert_eq!(unsafe { libc::raise(signal) }, 0);
}

/// The handler a signal has now (Unix).
#[cfg(unix)]
fn current_handler(signal: libc::c_int) -> libc::sighandler_t {
    // SAFETY: queries only (null new action).
    unsafe {
        let mut old: libc::sigaction = std::mem::zeroed();
        assert_eq!(libc::sigaction(signal, std::ptr::null(), &mut old), 0);
        old.sa_sigaction
    }
}

#[test]
fn stop_signals_start_the_grace_and_a_second_one_forces_the_stop() {
    #[cfg(unix)]
    let before = (current_handler(libc::SIGINT), current_handler(libc::SIGTERM));

    // SIGTERM: the game sees `Terminate`, wraps up in the next tick, quits.
    let world = Game::new("terminated", 1, 1)
        .fixed_hz(200)
        .sim_system(|ctx: &mut Ctx| {
            let status = ctx.world.resource::<ServerStatus>();
            if status.stopping == Some(StopReason::Terminate) {
                ctx.quit();
            } else if status.tick == 3 {
                raise(libc::SIGTERM);
            }
        })
        .run_server(
            ServerConfig::new()
                .log_format(LogFormat::Off)
                .shutdown_grace(Duration::from_secs(10))
                .max_ticks(2000),
            |_| {},
        );
    let status = world.resource::<ServerStatus>();
    assert_eq!(status.stopping, Some(StopReason::Terminate));
    assert_eq!(status.tick, 5, "ticks 0..=3, then one with the stop seen");

    // SIGINT ignored by the game: a second SIGINT ends the grace at once.
    let world = Game::new("interrupted", 1, 1)
        .fixed_hz(200)
        .sim_system(|ctx: &mut Ctx| {
            let status = ctx.world.resource::<ServerStatus>();
            if status.tick == 3 || status.tick == 6 {
                raise(libc::SIGINT);
            }
        })
        .run_server(
            ServerConfig::new()
                .log_format(LogFormat::Off)
                .shutdown_grace(Duration::from_secs(10))
                .max_ticks(2000),
            |_| {},
        );
    let status = world.resource::<ServerStatus>();
    assert_eq!(status.stopping, Some(StopReason::Interrupt));
    assert_eq!(status.tick, 7, "the second signal stopped it before tick 7");

    // The previous handlers are back once the servers have stopped.
    #[cfg(unix)]
    assert_eq!(before, (current_handler(libc::SIGINT), current_handler(libc::SIGTERM)));
}
