//! The threaded web build's warning self-test (`web/smoke/threads.mjs --case
//! warning`; the guide's "Threads on the web"). A function only a build with
//! threads compiles, never used: `web/build.sh --threads web_threads_warning`
//! with `RUSTFLAGS="-D warnings"` must fail on it. If that build passes, the
//! threaded build has lost `-D warnings` (or its atomics), and a warning in
//! the engine's threaded code would go unnoticed.
//!
//! Everywhere else it is an empty program, with no warning.

/// Unused on purpose: the warning the self-test expects.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
fn seen_only_by_a_threaded_build() {}

fn main() {}
