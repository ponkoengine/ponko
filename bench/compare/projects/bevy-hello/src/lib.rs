//! Hello world: an empty window.

use bevy::prelude::*;

/// The app: a window and nothing in it.
pub fn app() -> App {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);
    app
}
