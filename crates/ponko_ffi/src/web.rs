//! C and C++ games in the browser (`web/build-cpp.sh`, the web guide's "C
//! and C++"): the game's code, compiled with the WASI SDK's clang, is linked
//! into `examples/web_main.rs`, which calls its `main` through [`run_main`].
//!
//! In a browser `ponko_game_run` can't block: the event loop starts, and
//! winit leaves `run` by throwing a JavaScript exception, which would also
//! leave the game's `main`. A thrown exception runs the destructors of the
//! C++ frames it passes (WebAssembly exception handling catches it for their
//! cleanups), so `ponko::game` would free its systems and `main` its locals
//! while the game still uses them. Here the exception is caught right where
//! it is thrown, and the C and C++ frames are left with a trap instead, which
//! runs nothing. `main` runs from a JavaScript task with no WebAssembly frame
//! below it, so the trap leaves every frame and the stack pointer stays
//! below `main`'s locals: they keep their memory while the game plays, the
//! way they would on the desktop, where `run` blocks, and the way a Rust
//! game's locals do in a browser.

use std::cell::Cell;
use std::ffi::c_int;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[wasm_bindgen(inline_js = r#"
export function ponko_run_main(main, after) {
  queueMicrotask(() => {
    let error;
    try { main(); } catch (e) { error = e; }
    after(error);
  });
}
"#)]
extern "C" {
    /// Calls `main` (a WebAssembly function, called directly, not through
    /// wasm-bindgen's wrappers, which would mark the module failed after a
    /// trap) from a microtask, then `after` with what it threw, if anything.
    fn ponko_run_main(main: &js_sys::Function, after: &JsValue);
}

thread_local! {
    /// Set just before the trap that leaves a running game's frames.
    static LEFT_RUNNING: Cell<bool> = const { Cell::new(false) };
    /// The program's `main`, and what it returned.
    static MAIN: Cell<Option<unsafe extern "C" fn() -> c_int>> = const { Cell::new(None) };
    static EXIT_CODE: Cell<Option<c_int>> = const { Cell::new(None) };
}

/// Runs `f` called from JavaScript through a wasm-bindgen closure, so a
/// JavaScript exception that leaves it stops here, as `Err`, instead of
/// passing the frames below.
fn call_catching(f: &mut dyn FnMut()) -> Result<(), JsValue> {
    // SAFETY: only the lifetime is changed. The closure made from `f` is
    // called by `call0` below and dropped before this returns; JavaScript
    // keeps no other reference to it.
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

/// Calls the program's `main` from [`run_main`]'s microtask.
extern "C" fn call_main() {
    if let Some(main) = MAIN.with(Cell::take) {
        // SAFETY: the program's entry point, called once, with no arguments.
        let code = unsafe { main() };
        EXIT_CODE.with(|exit| exit.set(Some(code)));
    }
}

/// Calls a C program's `main` (`examples/web_main.rs`) soon after this
/// returns, from a microtask. Once `main` has returned, or has started the
/// game (`ponko_game_run`), which then keeps running in the page, nothing
/// more happens; whatever else stopped it (a trap, an exception, a nonzero
/// exit code) is thrown from that task, for the page to show.
#[doc(hidden)]
pub fn run_main(main: unsafe extern "C" fn() -> c_int) {
    MAIN.with(|slot| slot.set(Some(main)));
    // A function's address is its index in the module's function table.
    let table: js_sys::WebAssembly::Table = wasm_bindgen::function_table().unchecked_into();
    let call = match table.get(call_main as *const () as usize as u32) {
        Ok(call) => call,
        Err(e) => wasm_bindgen::throw_val(e),
    };
    let after = Closure::once_into_js(|error: JsValue| {
        if !error.is_undefined() {
            if !LEFT_RUNNING.with(Cell::get) {
                wasm_bindgen::throw_val(error);
            }
        } else if let Some(code) = EXIT_CODE.with(Cell::get).filter(|&code| code != 0) {
            wasm_bindgen::throw_str(&format!("main returned {code}"));
        }
    });
    ponko_run_main(call.unchecked_ref(), &after);
}
