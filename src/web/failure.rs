//! Failures that stop a browser game once it runs (or while setup runs):
//! a panic, the WebGL2 context or the WebGPU device lost during play, a
//! worker thread that stopped. The startup result has its own callback ([`super::on_startup`]);
//! these come after it, at most once per page.
//!
//! The page learns about a failure through `game.js`'s `onPonkoFailure`
//! export, the game through [`on_failure`], whose answer is the text the
//! page shows.

use std::cell::RefCell;
use std::sync::Mutex;

use js_sys::Function;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;

/// What stopped the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FailureKind {
    /// Rust code panicked: the game's own or the engine's (a GPU validation
    /// error, for instance). A panic aborts a web build.
    Panic,
    /// The browser took the WebGL2 context away while the game was running
    /// (the GPU process crashed or was reset). The game can't draw again
    /// without a reload.
    ContextLost,
    /// The browser lost the WebGPU device while the game was running (the
    /// GPU process crashed or was reset, or the device was destroyed). The
    /// game can't draw again without a reload. The message carries the
    /// browser's reason, when it gives one.
    DeviceLost,
    /// A worker thread of a build with threads stopped (it panicked or its
    /// script failed), so the game stops too.
    ThreadStopped,
}

/// A failure that stopped the browser game: what kind it is, its message
/// (a panic's message, without the location) and, for a panic, where in
/// the source it happened.
#[derive(Clone, Debug)]
pub struct Failure {
    kind: FailureKind,
    message: String,
    location: Option<String>,
}

impl Failure {
    pub(crate) fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            location: None,
        }
    }

    /// What stopped the game.
    pub fn kind(&self) -> FailureKind {
        self.kind
    }

    /// What went wrong, for people: a panic's message, or the engine's.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// For a panic, the source position it came from (`src/main.rs:12:5`).
    pub fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

type GameCallback = Box<dyn Fn(&Failure) -> Option<String>>;
type PageCallback = Box<dyn FnOnce(&Failure, Option<&str>)>;

#[derive(Default)]
struct Failures {
    /// The failure, once there is one, and the game's text for it.
    failed: Option<(Failure, Option<String>)>,
    /// `on_failure`'s callbacks, in the order they were registered.
    game: Vec<GameCallback>,
    /// `onPonkoFailure`'s callbacks, waiting for a failure.
    page: Vec<PageCallback>,
}

thread_local! {
    static FAILURES: RefCell<Failures> = RefCell::default();
}

/// The last panic on a worker thread: the worker can't reach the page's
/// callbacks, so the page's thread reads it when it stops the game.
static WORKER_PANIC: Mutex<Option<String>> = Mutex::new(None);

/// Registers a callback for a failure that stops the browser game: a
/// panic (in setup or later), the WebGL2 context or the WebGPU device lost
/// while playing, or a worker thread that stopped. Return the text the page should show
/// instead of its own (`Some`), or `None` to keep the page's.
///
/// It runs once, on the page's thread, right before the game stops: for
/// a panic, inside the panic hook, so it must not panic itself (a second
/// panic aborts before the page is told) and should do little. Every
/// registered callback runs, in order; the last `Some` is the text. The
/// page reads it as `error.text` in its `onPonkoFailure` callback; the
/// generated page shows it in place of “The game stopped: …”.
///
/// ```no_run
/// # use ponko::prelude::*;
/// ponko::web::on_failure(|failure| {
///     Some(format!("Sorry, the game crashed ({failure}). Reload the page to play again."))
/// });
/// Game::new("My Game", 800, 600).run(|_ctx| {});
/// ```
pub fn on_failure(callback: impl Fn(&Failure) -> Option<String> + 'static) {
    let _ = FAILURES.try_with(|failures| {
        if let Ok(mut failures) = failures.try_borrow_mut() {
            failures.game.push(Box::new(callback));
        }
    });
}

/// `game.js`'s `onPonkoFailure(callback)`. The callback runs at most once,
/// with an `Error` whose `kind` is a [`FailureKind`] name (`"Panic"`,
/// `"ContextLost"`, `"DeviceLost"`, `"ThreadStopped"`), `message` the failure's message,
/// `location` a panic's source position (else `null`) and `text` the
/// game's own text from [`on_failure`] (else `null`). A callback registered
/// after the failure runs immediately.
#[wasm_bindgen(js_name = onPonkoFailure)]
#[doc(hidden)]
pub fn on_ponko_failure(callback: Function) {
    let callback: PageCallback = Box::new(move |failure, text| {
        if let Err(exception) = callback.call1(&JsValue::UNDEFINED, &to_js(failure, text)) {
            web_sys::console::error_1(&exception);
        }
    });
    // After a failure the module may have trapped while one of these was
    // borrowed: never wait on (or panic over) a borrow here.
    let mut callback = Some(callback);
    let failed = FAILURES
        .try_with(|failures| {
            let mut failures = failures.try_borrow_mut().ok()?;
            match &failures.failed {
                Some(failed) => Some(failed.clone()),
                None => {
                    failures.page.extend(callback.take());
                    None
                }
            }
        })
        .ok()
        .flatten();
    if let (Some(callback), Some((failure, text))) = (callback, failed) {
        callback(&failure, text.as_deref());
    }
}

fn to_js(failure: &Failure, text: Option<&str>) -> JsValue {
    let error = js_sys::Error::new(failure.message());
    let set = |name: &str, value: JsValue| {
        let _ = js_sys::Reflect::set(error.as_ref(), &JsValue::from_str(name), &value);
    };
    set("kind", JsValue::from_str(&format!("{:?}", failure.kind())));
    set("location", failure.location().map_or(JsValue::NULL, JsValue::from_str));
    set("text", text.map_or(JsValue::NULL, JsValue::from_str));
    error.into()
}

/// Reports `failure` to the game and the page, once: later failures (the
/// trap that follows a panic, a second panic) only reach the console.
/// On a worker thread, where neither callback lives, it does nothing.
pub(crate) fn report(failure: Failure) {
    // The failure is kept before the game's callbacks run: if one of them
    // panics, a page that subscribes later still learns what happened.
    let game = FAILURES.try_with(|failures| {
        let mut failures = failures.try_borrow_mut().ok()?;
        if failures.failed.is_some() {
            return None;
        }
        failures.failed = Some((failure.clone(), None));
        Some(std::mem::take(&mut failures.game))
    });
    let Some(game) = game.ok().flatten() else {
        return;
    };
    // A crash report (`ponko::crash`), when the game has them on: a panic
    // wrote its own in the panic hook already.
    if failure.kind != FailureKind::Panic {
        let _ = crate::crash::report(&failure);
    }
    let text = game.iter().fold(None, |text, callback| callback(&failure).or(text));
    let page = FAILURES
        .try_with(|failures| {
            let mut failures = failures.try_borrow_mut().ok()?;
            failures.failed = Some((failure.clone(), text.clone()));
            Some(std::mem::take(&mut failures.page))
        })
        .ok()
        .flatten()
        .unwrap_or_default();
    for callback in page {
        callback(&failure, text.as_deref());
    }
}

/// Sends panic messages to the browser console (they're lost otherwise)
/// and reports the panic to the game and the page. On a worker thread the
/// message is kept for the page's thread, which stops the game.
pub(crate) fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&JsValue::from_str(&info.to_string()));
        let message = info
            .payload_as_str()
            .unwrap_or("Box<dyn Any> (a panic without a message)")
            .to_string();
        if web_sys::window().is_none() {
            // A worker: it can't reach the page's callbacks.
            if let Ok(mut last) = WORKER_PANIC.try_lock() {
                *last = Some(message);
            }
            return;
        }
        report(Failure {
            kind: FailureKind::Panic,
            message,
            location: info.location().map(ToString::to_string),
        });
    }));
}

/// The failure for a worker thread that stopped: `why`, and the worker's
/// panic message when it panicked.
#[cfg_attr(not(target_feature = "atomics"), allow(dead_code))]
pub(crate) fn thread_stopped(why: &str) -> Failure {
    // The page's thread never waits for a lock a worker holds.
    let panic = WORKER_PANIC.try_lock().ok().and_then(|mut last| last.take());
    let message = match panic {
        Some(panic) => format!("{why} (it panicked: {panic})"),
        None => why.to_string(),
    };
    Failure::new(FailureKind::ThreadStopped, message)
}

/// The failure for the WebGL2 context lost while the game runs.
pub(crate) fn context_lost() -> Failure {
    Failure::new(
        FailureKind::ContextLost,
        "The WebGL2 context was lost while the game was running. Reload the page to try again.",
    )
}

/// The failure for the WebGPU device lost while the game runs.
/// `browser_message` is the browser's reason (`GPUDeviceLostInfo.message`),
/// which may be empty.
pub(crate) fn device_lost(browser_message: &str) -> Failure {
    Failure::new(FailureKind::DeviceLost, device_lost_message(browser_message))
}

/// What the page shows for a lost WebGPU device: the engine's sentence,
/// with the browser's reason in it when there is one.
fn device_lost_message(browser_message: &str) -> String {
    let reason = browser_message.trim().trim_end_matches('.').trim_end();
    if reason.is_empty() {
        "The WebGPU device was lost while the game was running. Reload the page to try again.".to_string()
    } else {
        format!("The WebGPU device was lost while the game was running: {reason}. Reload the page to try again.")
    }
}
