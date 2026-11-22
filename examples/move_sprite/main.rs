//! One sprite sliding left and right: Bevy 0.20.1's `move_sprite` example,
//! ported. The guide's "Coming from Bevy" chapter walks through the port.
//!
//!   `cargo run --example move_sprite`
//!
//! The rules are in `game.rs`, played headlessly by `tests/move_sprite.rs`.
mod game;

// ANCHOR: main
fn main() {
    game::game().run();
}
// ANCHOR_END: main
