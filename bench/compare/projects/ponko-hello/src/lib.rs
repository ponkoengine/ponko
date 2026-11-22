//! Hello world: an empty window.

use ponko::prelude::*;

/// The game: a window and nothing in it.
pub fn game() -> Game {
    Game::new("hello", 800, 600)
}
