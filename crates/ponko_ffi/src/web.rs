//! C and C++ games in the browser (`web/build-cpp.sh`, the web guide's "C
//! and C++"): the game's code, compiled with the WASI SDK's clang, is linked
//! into `examples/web_main.rs`, which calls its `main` through [`run_main`].
//!
//! In a browser `ponko_game_run` can't block: winit starts the event loop
//! and leaves `run` by throwing a JavaScript exception, abandoning the
//! frames it passes, whose memory the event loop goes on using (the game's
//! state lives in one of them). From C and C++ that doesn't work as it does
//! for Rust:
//!
//! - an exception runs the destructors of the C++ frames it passes
//!   (WebAssembly exception handling catches it for their cleanups), so
//!   `ponko::game` would free its systems and `main` its locals while the
//!   game still uses them;
//! - with exception handling in the module, wasm-bindgen's wrappers of
//!   calls from JavaScript set the stack pointer back when an exception
//!   leaves them, freeing the abandoned frames.
//!
//! So `main` and the run are called directly, as table functions (no
//! wasm-bindgen wrapper), from JavaScript helpers. The helper around the run
//! catches winit's exception and notes how deep the stack is; the run then
//! leaves the C and C++ frames with a trap, which runs nothing, and the
//! helper around `main`, which has no WebAssembly frame below it, puts the
//! stack pointer back to that depth before anything else runs. The game's
//! frames, `main`'s locals among them, keep their memory while it plays, the
//! way they would on the desktop, where `run` blocks.

use std::cell::Cell;
use std::ffi::c_int;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[wasm_bindgen(inline_js = r#"
let left;
export function ponko_run_until_started(run, exports) {
  try {
    run();
    return false;
  } catch (e) {
    // Synchronously, before its event loop, nothing in the run throws
    // through wasm-bindgen (a WebAssembly.Exception) but winit leaving it.
    // A trap or another error is the run failing.
    if (!(e instanceof WebAssembly.Exception)) throw e;
    left = { exports, stack: exports.__stack_pointer.value };
    return true;
  }
}
export function ponko_run_main(main, after) {
  queueMicrotask(() => {
    let error;
    try { main(); } catch (e) { error = e; }
    if (left) {
      left.exports.__stack_pointer.value = left.stack;
      if (error instanceof WebAssembly.RuntimeError) return;
    }
    after(error);
  });
}
"#)]
extern "C" {
    /// Calls `run`; true if winit left it with its event loop started (the
    /// stack's depth then is kept for [`ponko_run_main`]).
    fn ponko_run_until_started(run: &js_sys::Function, exports: &JsValue) -> bool;
    /// Calls `main` from a microtask, then `after` with what it threw, if
    /// anything (nothing when it left a started game).
    fn ponko_run_main(main: &js_sys::Function, after: &JsValue);
}

thread_local! {
    /// What the table functions run.
    static JOB: Cell<Option<*mut dyn FnMut()>> = const { Cell::new(None) };
    /// The program's `main`, and what it returned.
    static MAIN: Cell<Option<unsafe extern "C" fn() -> c_int>> = const { Cell::new(None) };
    static EXIT_CODE: Cell<Option<c_int>> = const { Cell::new(None) };
}

/// `f` as a JavaScript function that calls it directly. A function's
/// address is its index in the module's function table.
fn table_function(f: extern "C" fn()) -> js_sys::Function {
    let table: js_sys::WebAssembly::Table = wasm_bindgen::function_table().unchecked_into();
    match table.get(f as *const () as usize as u32) {
        Ok(function) => function.unchecked_into(),
        Err(e) => wasm_bindgen::throw_val(e),
    }
}

extern "C" fn call_job() {
    if let Some(job) = JOB.with(Cell::take) {
        // SAFETY: `run_event_loop` stored it just before this call, and the
        // closure it points to lives in that function's frame, which is
        // still running.
        unsafe { (*job)() }
    }
}

/// `ponko_game_run` in a browser: returns `run`'s result if the game failed
/// before its event loop started, and otherwise never returns (see the module
/// documentation).
pub(crate) fn run_event_loop(
    run: impl FnOnce() -> Result<(), ponko::prelude::Error>,
) -> Result<(), ponko::prelude::Error> {
    let mut run = Some(run);
    let mut result = None;
    let mut job = || result = run.take().map(|run| run());
    let job: *mut (dyn FnMut() + '_) = &mut job;
    // SAFETY: only the lifetime is changed: `call_job` runs it during the
    // call below, while `job` is alive.
    let job = unsafe { std::mem::transmute::<*mut (dyn FnMut() + '_), *mut (dyn FnMut() + 'static)>(job) };
    let call = table_function(call_job);
    let exports = wasm_bindgen::exports();
    JOB.with(|slot| slot.set(Some(job)));
    if ponko_run_until_started(&call, &exports) {
        core::arch::wasm32::unreachable()
    }
    result.unwrap_or(Ok(()))
}

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
    let after = Closure::once_into_js(|error: JsValue| {
        if !error.is_undefined() {
            wasm_bindgen::throw_val(error);
        }
        if let Some(code) = EXIT_CODE.with(Cell::get).filter(|&code| code != 0) {
            wasm_bindgen::throw_str(&format!("main returned {code}"));
        }
    });
    ponko_run_main(&table_function(call_main), &after);
}
