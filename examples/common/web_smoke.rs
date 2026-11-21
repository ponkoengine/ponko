//! What every browser-smoke fixture shares (`web/smoke/`, `web/smoke/fixtures.json`):
//! the failure cases the runner's self-tests ask for, and the state a fixture
//! publishes for the runner to read.
//!
//! Include it from an example with
//! `#[path = "common/web_smoke.rs"] mod smoke;` and call [`Smoke::frame`] once
//! per frame. A fixture publishes two things on the page:
//!
//! - `<html data-smoke="...">`, every frame: `{"fixture":"web_smoke","ticks":120,
//!   "frames":118,"clicks":1,"size":[800,600], ...}` plus whatever the fixture
//!   adds (`"asset":[32,32]`, `"screen":"Playing"`).
//! - `<html data-smoke-gpu="...">`, once: what the engine itself says about the
//!   GPU it opened (`Graphics::gpu_capabilities`, as JSON). The runner compares its
//!   backend with the canvas's context, so a WebGL2 page can't pass as WebGPU.
//!
//! The failure cases come from the page's query string: `?smoke_case=panic`
//! panics during setup, `?smoke_case=stall` pauses the simulation after a few
//! ticks, `?smoke_case=runtime-panic` panics in a frame after a few ticks,
//! and `?smoke_case=failure-text` does too, with the game's own failure
//! text for the page (`ponko::web::on_failure`). On the desktop the case
//! comes from the `SMOKE_CASE` environment variable and the state is
//! printed once a second.

use ponko::prelude::*;

use serde_json::{Map, Value, json};

/// Ticks a `stall` run simulates before pausing. Keep it well below the
/// runner's `MIN_PROGRESS_TICKS` (`web/smoke/lib.mjs`), so the stall is caught
/// however early the runner first sees the state.
pub const STALL_AFTER: u64 = 20;

/// One fixture's counters and its failure case.
pub struct Smoke {
    fixture: &'static str,
    case: String,
    frames: u64,
    clicks: u32,
    gpu_published: bool,
}

impl Smoke {
    /// Starts a fixture's run; call it first in the setup callback. Panics
    /// right away when the case is `panic`.
    pub fn new(fixture: &'static str) -> Self {
        let case = smoke_case();
        if case == "panic" {
            panic!("{fixture}: panicking during setup, as ?smoke_case=panic asked");
        }
        #[cfg(target_arch = "wasm32")]
        if case == "failure-text" {
            ponko::web::on_failure(move |failure| Some(format!("{fixture} stopped, in its own words: {failure}")));
        }
        Self {
            fixture,
            case,
            frames: 0,
            clicks: 0,
            gpu_published: false,
        }
    }

    /// Left clicks seen so far.
    #[allow(dead_code)] // fixtures that don't react to clicks don't call it
    pub fn clicks(&self) -> u32 {
        self.clicks
    }

    /// Counts the frame and its click, applies a `stall` case, and publishes
    /// the state. `extra` is a JSON object whose fields join the state.
    /// Call it once per frame, from a frame system.
    pub fn frame(&mut self, ctx: &mut Ctx, ticks: u64, extra: Value) {
        self.frames += 1;
        if self.case == "stall" && ticks >= STALL_AFTER && !ctx.time.is_paused() {
            ctx.time.pause();
        }
        if (self.case == "runtime-panic" || self.case == "failure-text") && ticks >= STALL_AFTER {
            panic!("{}: panicking while running, as ?smoke_case={} asked", self.fixture, self.case);
        }
        if ctx.input.mouse_pressed(MouseButton::Left) {
            self.clicks += 1;
        }
        let size = ctx.window_size();
        let mut state = Map::new();
        state.insert("fixture".into(), json!(self.fixture));
        state.insert("ticks".into(), json!(ticks));
        state.insert("frames".into(), json!(self.frames));
        state.insert("clicks".into(), json!(self.clicks));
        state.insert("size".into(), json!([size.x, size.y]));
        if let Value::Object(fields) = extra {
            state.extend(fields);
        }
        if !self.gpu_published {
            // The adapter is known once the first frame runs; None in
            // headless runs, so try again next frame there.
            if let Some(gpu) = ctx.gfx.gpu_capabilities() {
                publish("data-smoke-gpu", &serde_json::to_string(gpu).unwrap_or_default());
                self.gpu_published = true;
            }
        }
        let state = Value::Object(state).to_string();
        publish("data-smoke", &state);
        if self.frames.is_multiple_of(60) {
            print_state(&state);
        }
    }
}

/// The page's `smoke_case` query parameter (web), or `""`.
#[cfg(target_arch = "wasm32")]
fn smoke_case() -> String {
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    search
        .trim_start_matches('?')
        .split('&')
        .find_map(|pair| pair.strip_prefix("smoke_case="))
        .unwrap_or("")
        .to_string()
}

#[cfg(not(target_arch = "wasm32"))]
fn smoke_case() -> String {
    std::env::var("SMOKE_CASE").unwrap_or_default()
}

/// Sets an attribute on `<html>` for the browser runner.
#[cfg(target_arch = "wasm32")]
fn publish(attribute: &str, value: &str) {
    if let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = root.set_attribute(attribute, value);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish(_attribute: &str, _value: &str) {}

#[cfg(target_arch = "wasm32")]
fn print_state(_state: &str) {}

#[cfg(not(target_arch = "wasm32"))]
fn print_state(state: &str) {
    println!("{state}");
}
