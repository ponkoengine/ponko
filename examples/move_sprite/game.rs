//! The rules of `move_sprite`, without drawing: shared by the example and its
//! headless test (`tests/move_sprite.rs`).
//!
//! A port of Bevy 0.20.1's `examples/2d/move_sprite.rs`, the worked example
//! of the guide's "Coming from Bevy" chapter (`book/src/from-bevy.md`): one
//! sprite slides right at 150 units a second, turns back past x = 200, and
//! slides left until it passes x = -200.
use ponko::d2::Transform;
use ponko::prelude::*;

// ANCHOR: port
/// Which way the sprite is moving.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
}

/// How fast the sprite moves, in world units a second.
pub const SPEED: f32 = 150.0;
/// The sprite turns back once it is farther than this from the middle.
pub const TURN_AT: f32 = 200.0;

/// Bevy's `setup`. There is no camera entity: `ctx.camera` always exists, and
/// looking at (0, 0) puts the origin in the middle of the window, as Bevy's
/// `Camera2d` does.
pub fn setup(ctx: &mut Ctx) {
    ctx.camera.look_at(Vec2::ZERO);
    let icon = ctx.gfx.load_texture("branding/ponko-face-256.png");
    ctx.world
        .spawn((Transform::at(Vec2::ZERO), Sprite::new(icon), Direction::Right));
}

/// Bevy's `sprite_movement`, as a simulation system: the same speed at any
/// frame rate, and it runs in headless tests.
pub fn sprite_movement(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    ctx.world
        .query::<(&mut Direction, &mut Transform)>()
        .for_each(|(logo, transform)| {
            match *logo {
                Direction::Right => transform.position.x += SPEED * dt,
                Direction::Left => transform.position.x -= SPEED * dt,
            }

            if transform.position.x > TURN_AT {
                *logo = Direction::Left;
            } else if transform.position.x < -TURN_AT {
                *logo = Direction::Right;
            }
        });
}

/// The game, ready to `run()` in a window or `run_headless(ticks)` in a test.
pub fn game() -> ConfiguredGame {
    Game::new("Ponko: move_sprite", 800, 600)
        .sim_system(sprite_movement)
        .setup(setup)
}
// ANCHOR_END: port
