//! Water: a lake with a planar reflection and shoreline foam, a river
//! carried by a flow map, and the sea's Gerstner waves, with crates
//! floating on all three and contact ripples where they splash. Under the
//! sea the underwater view takes over.
//!
//!   `cargo run --release --example water --features physics3d`
//!
//! WASD flies, Q and E go down and up, Shift is faster, the arrows look
//! around. 1, 2 and 3 pick the water quality (Low, Medium, High), R the
//! lake's reflection (sky, screen-space, planar), Space drops a crate.
//!
//! The scene and keys live in `game.rs`, which `tests/water.rs` also runs
//! without a window.

mod game;

fn main() {
    ponko::prelude::Game::new("water", 1280, 720)
        .physics3d(ponko::prelude::vec3(0.0, -9.81, 0.0))
        .plugin(game::WaterDemo)
        .configured()
        .run();
}
