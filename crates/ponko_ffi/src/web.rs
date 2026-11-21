//! C and C++ games in the browser (`web/build-cpp.sh`, the guide's "C and
//! C++ on the web"): the game's code, compiled with the WASI SDK's clang, is
//! linked into `examples/web_main.rs`, which calls its `main`.
//!
//! In a browser `ponko_game_run` can't block: the event loop starts, and
//! winit leaves `run` by throwing a JavaScript exception, which also leaves
//! the game's `main`. A thrown exception runs the destructors of the C++
//! frames it passes (WebAssembly exception handling catches it for their
//! cleanups), so `ponko::game` would free its systems and `main` its locals
//! while the game still uses them. Here the exception is caught right where
//! it is thrown and the C and C++ frames are left with a trap instead, which
//! runs nothing: they stay as they were, as if `run` were still blocking,
//! the way it does on the desktop. [`run_main`] stops that trap.

use std::cell::Cell;
use std::ffi::c_int;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

thread_local! {
    /// Set just before the trap that leaves a running game's frames.
    static LEFT_RUNNING: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` called from JavaScript, so a JavaScript exception or a trap that
/// leaves it stops here, as `Err`, instead of passing the frames below.
fn call_catching(f: &mut dyn FnMut()) -> Result<(), JsValue> {
    // SAFETY: only the lifetime is changed. The closure made from `f` is
    // called by `call0` below and dropped before this returns;
    // JavaScript keeps no other reference to it.
    let f: &'static mut dyn FnMut() = unsafe { std::mem::transmute::<&mut dyn FnMut(), &'static mut dyn FnMut()>(f) };
    let closure = Closure::<dyn FnMut()>::new(f);
    let function: &js_sys::Function = closure.as_ref().unchecked_ref();
    let result = function.call0(&JsValue::UNDEFINED);
    drop(closure);
    result.map(|_| ())
}

/// Whether `e` is winit's exception for leaving a started event loop.
fn is_event_loop_started(e: &JsValue) -> bool {
    e.dyn_ref::<js_sys::Error>()
        .is_some_and(|e| String::from(e.message()).starts_with("Using exceptions for control flow"))
}

/// `ponko_game_run` in a browser: returns `run`'s result if the game failed
/// before its event loop started, and otherwise never returns (see the module
/// documentation).
pub(crate) fn run_event_loop(
    run: impl FnOnce() -> Result<(), ponko::prelude::Error>,
) -> Result<(), ponko::prelude::Error> {
    let mut run = Some(run);
    let mut result = None;
    let outcome = call_catching(&mut || result = run.take().map(|run| run()));
    match (outcome, result) {
        // `run` returned: the game failed before its event loop started.
        (Ok(()), result) => result.unwrap_or(Ok(())),
        (Err(e), _) if is_event_loop_started(&e) => {
            LEFT_RUNNING.with(|left| left.set(true));
            core::arch::wasm32::unreachable()
        }
        (Err(e), _) => wasm_bindgen::throw_val(e),
    }
}

/// Calls a C program's `main` (`examples/web_main.rs`). Returns once `main`
/// has returned, or has started the game (`ponko_game_run`), which then keeps
/// running in the page; a trap or exception of anything else is rethrown.
#[doc(hidden)]
pub fn run_main(main: unsafe extern "C" fn() -> c_int) {
    let mut code = None;
    // SAFETY: `main` is the program's entry point, called once, with no arguments.
    let outcome = call_catching(&mut || code = Some(unsafe { main() }));
    match outcome {
        Ok(()) => {
            if let Some(code) = code.filter(|&code| code != 0) {
                wasm_bindgen::throw_str(&format!("main returned {code}"));
            }
        }
        Err(_) if LEFT_RUNNING.with(Cell::get) => {}
        Err(e) => wasm_bindgen::throw_val(e),
    }
}
