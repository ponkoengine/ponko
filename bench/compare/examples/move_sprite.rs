// Bevy 0.20.1's `examples/2d/move_sprite.rs`, unchanged below the anchor:
// https://github.com/bevyengine/bevy/blob/v0.20.1/examples/2d/move_sprite.rs
// (Bevy is dual-licensed MIT or Apache-2.0.) The guide's "Coming from Bevy"
// chapter quotes it next to its port, `examples/move_sprite/` in the engine;
// `cargo check --examples` here keeps the quote compiling against the pinned
// Bevy. Never run by the scoreboard.
// ANCHOR: original
//! Renders a 2D scene containing a single, moving sprite.

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, sprite_movement)
        .run();
}

#[derive(Component)]
enum Direction {
    Left,
    Right,
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(Camera2d);

    commands.spawn((
        Sprite::from_image(asset_server.load("branding/icon.png")),
        Transform::from_xyz(0., 0., 0.),
        Direction::Right,
    ));
}

/// The sprite is animated by changing its translation depending on the time that has passed since
/// the last frame.
fn sprite_movement(time: Res<Time>, mut sprite_position: Query<(&mut Direction, &mut Transform)>) {
    for (mut logo, mut transform) in &mut sprite_position {
        match *logo {
            Direction::Right => transform.translation.x += 150. * time.delta_secs(),
            Direction::Left => transform.translation.x -= 150. * time.delta_secs(),
        }

        if transform.translation.x > 200. {
            *logo = Direction::Left;
        } else if transform.translation.x < -200. {
            *logo = Direction::Right;
        }
    }
}
// ANCHOR_END: original
