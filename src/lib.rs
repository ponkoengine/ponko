//! # Ponko
//!
//! A code-first 2D and 3D game engine with integrated simulation, C++
//! bindings, and tools for AI-assisted development.
//!
//! ```no_run
//! use ponko::prelude::*;
//!
//! fn main() {
//!     Game::new("My Game", 800, 600).run(|ctx| {
//!         let crab = ctx.gfx.load_texture("crab.png");
//!         let mut pos = vec2(400.0, 300.0);
//!
//!         move |ctx: &mut Ctx| {
//!             if ctx.input.key_down(Key::Right) { pos.x += 200.0 * ctx.time.dt(); }
//!             if ctx.input.key_down(Key::Left)  { pos.x -= 200.0 * ctx.time.dt(); }
//!             if ctx.input.key_pressed(Key::Escape) { ctx.quit(); }
//!             ctx.gfx.draw(crab, pos);
//!         }
//!     });
//! }
//! ```
//!
//! Start with `use ponko::prelude::*;`, then add dimension and service modules as needed.
//!
//! The [guide](https://ponko.dev/guide/) explains the engine chapter by
//! chapter, starting with [Getting started](https://ponko.dev/guide/getting-started.html);
//! this reference documents every item.
//!
//! ## Where things are
//!
//! - [`prelude`]: what a typical game needs, in one import.
//! - [`game`]: [`Game`](game::Game), the builder that sets up and runs a
//!   game, its systems and phases; `Ctx`, the per-frame context, is in the prelude.
//! - [`d2`] and `d3` (feature `3d`): 2D and 3D drawing, cameras, scenes and
//!   models; `terrain` (feature `3d`) and [`tilemap`] for worlds.
//! - The ECS: entities, components, queries and resources live on the
//!   game's world (`ctx.world`), with systems added on [`Game`](game::Game);
//!   [`events`] carries messages between systems, [`systems`] holds the
//!   engine's built-in systems and [`schedule`] explains which run together.
//! - [`math`]: vectors, quaternions and matrices (glam's types).
//! - [`input`] and [`actions`]: keys, mouse, gamepads and rebindable actions.
//! - [`ui`] and [`text`]: the retained UI, fonts and text layout.
//! - [`audio`]: sound effects and music.
//! - [`d2`] and `d3`: the types of each dimension, physics among them
//!   (features `physics` and `physics3d`); [`nav`] and [`flow`] for
//!   pathfinding and navigation meshes.
//! - [`assets`] and [`streaming`]: asset files, GPU asset loading with
//!   budgets, background jobs and streaming big worlds.
//! - [`net`]: networking: lockstep, messages and snapshot replication.
//! - [`perf`], [`diagnostics`] and [`render`]: frame profiling, ECS
//!   diagnostics and renderer statistics.
//! - [`reflect`] and [`scenario`]: components by name as JSON, and scripted
//!   input for gameplay tests, for tools and AI agents.
//!
//! ## Features
//!
//! - `window` (default): a window and GPU rendering (winit + wgpu). Without
//!   it the engine is simulation only: `Game::run_headless` works, drawing
//!   calls do nothing, `Game::run` panics. For simulation servers and CI,
//!   with a much smaller build.
//! - `audio` (default): sound through kira; without it sounds are silent.
//! - `inspector` (default): the in-game inspector overlay (egui), opened
//!   with the backquote key. On in debug builds; see `Game::inspector`.
//! - `gamepad` (default): gamepads through gilrs (the browser's own on the web); without it
//!   `Input::gamepads` is always empty. On Linux it needs libudev headers.
//! - `profiling`: per-system scopes for `puffin_viewer`. (Not needed for
//!   [`profile_scope!`], [`Perf::report`](perf::Perf::report) or [`CountingAllocator`](ponko::perf::CountingAllocator), which
//!   are always built in and cost one atomic load while profiling is off.)
//! - `hotpatch`: change system code while the game runs, with
//!   `dx serve --hot-patch --features hotpatch`.
//! - `3d`: meshes, materials, glTF models, a perspective camera, lights and
//!   shadows, drawn under the 2D layer (`ponko::d3`, `ctx.scene3d`).
//! - `navmesh`: navigation meshes for 3D (Recast baking and Detour path and
//!   crowd queries, in pure Rust). Turns on `3d`.
//! - `physics`: 2D physics with rapier (`Game::physics`).
//! - `physics3d`: 3D physics with rapier (`Game::physics3d`): bodies,
//!   colliders (heightfield terrain too), queries, a character controller and
//!   raycast-wheel vehicles. Turns on `3d`.
//! - `physics-determinism`: physics with bit-identical results on every
//!   platform (for lockstep and replays), a little slower. Turns on `physics`.
//! - `control-env`: a release build honors `PONKO_CONTROL=1` and opens the
//!   control endpoint for tools (debug builds always do). Off, a release build
//!   opens it only when the game calls `Game::control_endpoint(true)`.
//! - `accessibility`: screen readers read and operate the retained UI of the
//!   game's window (`ui::UiAccessAdapter`), through AccessKit: macOS
//!   (VoiceOver), Windows (UI Automation: Narrator, NVDA) and Linux (AT-SPI:
//!   Orca). On the web it builds and does nothing. Needs `window`.
//! - `steam`: lockstep and byte messages over Steam's relays and lobbies
//!   (`Net::steam_host`, `net::SteamClient`); the raw `steamworks` crate is
//!   `ponko::net::unstable_steamworks`, outside the versions.
//! - `bench-internals`: **unstable** benchmark drivers for the engine's own
//!   benchmark crate. Not part of the API; they change in any release.
//!
//! What a 0.x release may change, and what it keeps, is in
//! [STABILITY.md](https://github.com/ponkoengine/ponko/blob/main/STABILITY.md).

#![warn(missing_docs)]
// docs.rs: "Available on crate feature X only" on feature-gated items.
#![cfg_attr(docsrs, feature(doc_cfg))]
// Without a window, the code that only the window loop calls is unused.
#![cfg_attr(not(feature = "window"), allow(dead_code))]

// `#[derive(Component)]` names the trait `::ponko::prelude::Component`, in
// this crate too; tests also include example files as they are.
extern crate self as ponko;

pub mod anim;
pub mod d2;
#[cfg(feature = "3d")]
pub mod d3;
pub mod nav;
pub mod prelude;
#[cfg(feature = "3d")]
pub mod terrain;
#[cfg(feature = "3d")]
pub mod water;

pub mod access;
pub mod actions;
pub mod ai;
mod alloc_count;
pub mod aseprite;
mod asset_cache;
pub mod assets;
pub mod attach;
pub mod audio;
pub mod camera;
mod camera2d;
#[cfg(any(feature = "physics", feature = "physics3d"))]
pub mod collision;
pub mod color;
#[cfg(feature = "compressed-textures")]
pub(crate) mod compressed;
mod control;
pub mod crash;
mod ctx;
pub mod debug;
pub mod diagnostics;
mod draw_space;
mod ecs;
mod edit2d;
#[cfg(feature = "3d")]
mod edit3d;
mod editor;
pub mod error;
pub mod events;
pub mod flow;
mod frame_pool;
/// Forces the engine's passes with a size threshold to share their work out
/// or not, for threshold sweeps. Unstable (`bench-internals`).
#[doc(hidden)]
#[cfg(feature = "bench-internals")]
pub mod bench_sharing {
    pub use crate::frame_pool::{Sharing, set_sharing, shared_runs};

    /// The time a hierarchy level must project to on the calling thread to
    /// be shared out, in nanoseconds.
    pub fn hierarchy_budget_ns() -> u64 {
        crate::hierarchy::SERIAL_BUDGET_NS as u64
    }

    /// The fewest 2D skeletons whose drawing is shared out.
    pub fn skeletons_from() -> usize {
        crate::skeleton2d::draw::PARALLEL_MIN
    }

    /// Whether this build allocates from one heap per thread (a web build
    /// with threads and `--cfg ponko_web_thread_heaps`).
    pub fn per_thread_allocator() -> bool {
        cfg!(all(
            target_arch = "wasm32",
            target_feature = "atomics",
            ponko_web_thread_heaps
        ))
    }

    /// The fewest 3D characters whose moves are shared out.
    #[cfg(feature = "physics3d")]
    pub fn characters_from() -> usize {
        crate::physics3d::CHARACTERS_PARALLEL_FROM
    }
}
pub mod game;
// The built-in controllers on the desktop: gilrs.
#[cfg(all(feature = "gamepad", not(target_arch = "wasm32")))]
mod gamepad;
// The second desktop backend: SDL 3's gamepads (feature `gamepad-sdl`), and
// its tests with a scripted device layer in every test build.
#[cfg(any(test, all(feature = "gamepad-sdl", not(target_arch = "wasm32"))))]
mod gamepad_sdl;
// The calls into SDL for it.
#[cfg(all(feature = "gamepad-sdl", not(target_arch = "wasm32")))]
mod gamepad_sdl_sys;
// The built-in controllers of every web build (and its tests, natively): the
// browser's gamepads, read by the engine itself.
#[cfg(all(feature = "gamepad", any(test, target_arch = "wasm32")))]
mod gamepad_web;
pub mod gfx;
pub mod handle;
pub mod hierarchy;
// Rumble through the browser's `vibrationActuator`, for `gamepad_web`.
#[cfg(all(feature = "gamepad", any(test, target_arch = "wasm32")))]
mod web_rumble;

/// Raw handle ids, for the engine's own language bindings (`ponko_ffi`),
/// whose C handles are plain ids. **Not part of the API**: hidden, unstable,
/// and changed or removed in any release without notice. Games never need
/// it: handles come from the engine, and a made-up id names nothing, or
/// something else.
#[doc(hidden)]
pub mod __ffi {
    use crate::handle::raw::RawHandle;

    /// The id inside a handle of one id. Unstable (see the [module](self)).
    #[must_use]
    pub fn id<H: RawHandle>(handle: H) -> u32 {
        handle.raw_id()
    }

    /// A handle from an id from [`id`]. Sizes and lengths read 0; check it
    /// with `Ctx::is_live` before trusting it. Unstable (see the
    /// [module](self)).
    #[must_use]
    pub fn from_id<H: RawHandle>(id: u32) -> H {
        H::from_raw_id(id)
    }

    /// The first system named `name` (in phase order), for removal by
    /// name. Unstable (see the [module](self)).
    #[must_use]
    pub fn system_named(game: &crate::game::Game, name: &str) -> Option<crate::game::SystemId> {
        game.system_named(name)
    }

    /// The render target that `texture` (a C `PonkoTexture`) is. The caller
    /// checks it is one first (`Graphics::is_render_target`). Unstable.
    #[cfg(feature = "3d")]
    #[must_use]
    pub fn render_target(texture: crate::texture::Texture) -> crate::render::RenderTarget {
        crate::render::RenderTarget::from_texture(texture)
    }

    /// The slot bits of a mesh's, GPU buffer's or collision mesh's id: the
    /// bits above count how often the slot was given out. Unstable.
    #[cfg(feature = "3d")]
    pub const SLOT_MASK: u32 = crate::handle::INDEX_MASK;
}
mod hot_reload;
pub mod input;
pub mod inventory;
pub mod locale;
pub mod math;
pub mod mods;

#[cfg(feature = "inspector")]
mod inspector;
mod light2d;
mod logs;
#[cfg(feature = "navmesh")]
mod nav3d;
pub mod net;
/// Entry points for the fuzz targets in `fuzz/` that reach internals (built
/// with `--cfg fuzzing`, as `cargo fuzz` does). Not part of the API.
#[cfg(fuzzing)]
#[doc(hidden)]
pub mod __fuzz {
    pub use crate::assets::fuzz_texture as texture;
    #[cfg(feature = "audio")]
    pub use crate::audio::fuzz_sound as sound;
    #[cfg(all(feature = "audio", feature = "video"))]
    pub use crate::audio::fuzz_webm_audio as webm_audio;
    pub use crate::control::fuzz_requests as control_requests;
    pub use crate::net::fuzz::{lockstep_packets, message_packets, replication_packets};
}
pub mod particles;
mod particles2d;

mod jobs;
pub mod perf;
mod perf_report;
#[cfg(feature = "physics")]
mod physics;
#[cfg(feature = "physics3d")]
mod physics3d;
#[cfg(any(feature = "physics", feature = "physics3d"))]
mod physics_links;
mod profiler;
#[cfg(any(all(target_arch = "wasm32", feature = "window"), test))]
mod query;
pub mod random;
pub mod reflect;
pub mod render;
mod residency;
pub mod scenario;
mod scene;
pub mod schedule;
mod schema;
mod scope;
pub mod server;
pub mod sheet;
pub mod skeleton2d;
pub mod spatial;
mod sprite_material;
pub mod state;
pub mod storage;
/// Deprecated: [`Job`](crate::streaming::Job) and `CellStreamer` moved to
/// [`streaming`], next to the asset streaming they work with.
pub mod stream {
    /// Moved: [`ponko::streaming::Job`](crate::streaming::Job).
    #[deprecated(since = "0.2.0", note = "use `ponko::streaming::Job`")]
    pub type Job<T> = crate::streaming::Job<T>;
    /// Moved: `ponko::streaming::CellStreamer`.
    #[cfg(feature = "3d")]
    #[deprecated(since = "0.2.0", note = "use `ponko::streaming::CellStreamer`")]
    pub type CellStreamer = crate::streaming::CellStreamer;
}
pub mod streaming;
pub mod system_view;
pub mod systems;
pub mod text;
pub mod texture;
#[cfg(feature = "3d")]
mod three;
pub mod tilemap;
mod time;
pub mod ui;
#[cfg(feature = "video")]
pub mod video;
#[cfg(all(target_arch = "wasm32", feature = "window"))]
pub mod web;
// Worker threads for web builds compiled with atomics (the guide's
// "Threads on the web").
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
mod web_threads;
// One heap per thread for web builds with threads that ask for it with
// `--cfg ponko_web_thread_heaps` (see the module).
#[cfg(all(target_arch = "wasm32", target_feature = "atomics", ponko_web_thread_heaps))]
mod web_alloc;

/// The whole ECS crate ([`ponko_ecs`]), for advanced use: component ids,
/// read-only world views, the dynamic and C path ([`World::raw`](ponko_ecs::World::raw)), chunks
/// and archetypes. Games normally need only the ECS types in the prelude.
///
/// **Maturity: proven**, from the guide chapter `ecs.md`. Game: Coin Dash and Ember Hop, packaged
/// and played on Linux, macOS and Windows. Tests: `crates/ponko_ecs/tests`, and its unsafe code
/// under Miri. Platforms: Linux, macOS and Windows, headless (the workspace tests in the list of
/// checks). Graphics: not involved.
pub mod ecs_core {
    #[doc(no_inline)]
    pub use ponko_ecs::*;
}

/// Window control and headless backend seam.
pub mod window;
