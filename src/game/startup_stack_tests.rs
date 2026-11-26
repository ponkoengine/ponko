//! Startup's stack. Windows gives a program's main thread 1 MB (Linux and
//! macOS 8 MB), and a game's startup runs there. These tests run startup
//! on a thread with half of that, so a change that makes startup's stack
//! frames grow fails here rather than in a player's game.
//!
//! They measure the build they run in: the test profile, the engine at
//! the workspace's opt-level. `PONKO_STARTUP_STACK_KB` gives the thread
//! another size, to find what startup takes (the smallest size that
//! passes).

#[cfg(all(windows, feature = "window", feature = "3d"))]
use std::sync::Arc;
#[cfg(all(windows, feature = "window", feature = "3d"))]
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;

/// The stack startup must fit in: half of Windows' main-thread stack.
const BUDGET_KB: usize = 512;

/// Runs `f` on a thread with [`BUDGET_KB`] of stack. Overflowing it ends
/// the test process with "has overflowed its stack".
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let kb = std::env::var("PONKO_STARTUP_STACK_KB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(BUDGET_KB);
    std::thread::Builder::new()
        .name(format!("startup on {kb} KB of stack"))
        .stack_size(kb * 1024)
        .spawn(f)
        .expect("a thread")
        .join()
        .expect("startup ran")
}

/// What a windowless game shares with a windowed one: the `Ctx`, setup
/// and frames (no window, GPU or event loop).
#[test]
fn headless_startup_fits_in_half_of_a_windows_main_thread_stack() {
    let world = on_small_stack(|| {
        Game::new("startup stack", 320, 240)
            .headless_frames(true)
            .setup(|ctx| ctx.world.insert_resource(7u32))
            .try_run_headless(3)
            .expect("ran")
    });
    assert_eq!(world.get_resource::<u32>(), Some(&7));
}

/// A game that makes a 3D mesh in setup, then counts its frames and stops
/// after three.
#[cfg(all(windows, feature = "window", feature = "3d"))]
fn counting(frames: &Arc<AtomicU32>) -> impl FnOnce(&mut Ctx) -> Box<dyn FnMut(&mut Ctx)> + Send + 'static {
    let frames = frames.clone();
    move |ctx: &mut Ctx| {
        // A 3D game makes the 3D pass in setup: its first mesh.
        let cube = ctx.gfx.create_plain_mesh(&crate::render::MeshData::cube(1.0));
        ctx.world
            .spawn((crate::three::Transform3D::default(), crate::three::Model::new(cube)));
        Box::new(move |ctx: &mut Ctx| {
            if frames.fetch_add(1, Ordering::Relaxed) + 1 >= 3 {
                ctx.quit();
            }
        })
    }
}

/// The whole windowed startup: event loop, window, GPU device, setup with
/// a 3D mesh, three frames. Windows lets a test run the event loop off the
/// main thread (`window::run_window`); other systems don't, and their main
/// thread has 8 MB. Opens a window, so it is opt-in:
/// `cargo test -p ponko --lib --features 3d startup_stack -- --ignored`
/// (alone: a process can make one event loop).
#[cfg(all(windows, feature = "window", feature = "3d"))]
#[test]
#[ignore = "opens a window"]
fn windowed_startup_fits_in_half_of_a_windows_main_thread_stack() {
    let frames = Arc::new(AtomicU32::new(0));
    let setup = counting(&frames);
    on_small_stack(move || {
        Game::new("startup stack", 320, 240)
            .try_run(setup)
            .map_err(|e| e.to_string())
    })
    .expect("the game ran (it needs a GPU)");
    assert!(frames.load(Ordering::Relaxed) >= 3, "it drew frames");
}
