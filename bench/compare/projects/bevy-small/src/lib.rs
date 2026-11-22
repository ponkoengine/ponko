//! The small game of the scoreboard: Ponko's 2D template
//! (`crates/ponko_cli/templates/2d`) written with Bevy. A square the arrow
//! keys (or WASD) move inside an 800 x 600 arena, a hint in text, Escape
//! quits. The template's settings come from `assets/settings.json` and
//! reload while it runs; Bevy has no JSON asset loader of its own, so the
//! speed is a resource with its value here.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The size of the playing field, in pixels.
pub const ARENA: Vec2 = Vec2::new(800.0, 600.0);
/// The size of the player's square, in pixels.
pub const PLAYER_SIZE: f32 = 40.0;

/// Numbers to tune.
#[derive(Resource, Serialize, Deserialize)]
pub struct Settings {
    /// How fast the player moves, in pixels per second.
    pub speed: f32,
}

/// Where the player wants to go this tick, -1 to 1 on each axis.
#[derive(Resource, Default, Serialize, Deserialize)]
pub struct Intent(pub Vec2);

/// The entity the player controls.
#[derive(Component, Serialize, Deserialize)]
pub struct Player;

/// The whole game.
pub fn app() -> App {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "bevy-small".into(),
            resolution: (ARENA.x as u32, ARENA.y as u32).into(),
            ..default()
        }),
        ..default()
    }))
    .insert_resource(Settings { speed: 300.0 })
    .init_resource::<Intent>()
    .add_systems(Startup, setup)
    .add_systems(Update, read_input)
    .add_systems(FixedUpdate, movement);
    app
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Player,
        Sprite::from_color(Color::srgb(0.95, 0.55, 0.2), Vec2::splat(PLAYER_SIZE)),
        Transform::default(),
    ));
    commands.spawn((
        Text::new("Arrow keys or WASD to move, Escape to quit"),
        TextFont {
            font_size: FontSize::Px(20.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            top: Val::Px(16.0),
            ..default()
        },
    ));
}

/// Moves the player by the intent, inside the arena.
pub fn movement(
    time: Res<Time>,
    intent: Res<Intent>,
    settings: Res<Settings>,
    mut players: Query<&mut Transform, With<Player>>,
) {
    let step = intent.0.clamp_length_max(1.0) * settings.speed * time.delta_secs();
    let half = (ARENA - Vec2::splat(PLAYER_SIZE)) / 2.0;
    for mut t in &mut players {
        let p = (t.translation.truncate() + step).clamp(-half, half);
        t.translation = p.extend(0.0);
    }
}

/// The arrow keys (or WASD) set the intent. Escape quits.
fn read_input(keys: Res<ButtonInput<KeyCode>>, mut intent: ResMut<Intent>, mut exit: MessageWriter<AppExit>) {
    let held = |a, b| keys.pressed(a) || keys.pressed(b);
    let mut dir = Vec2::ZERO;
    if held(KeyCode::ArrowLeft, KeyCode::KeyA) {
        dir.x -= 1.0;
    }
    if held(KeyCode::ArrowRight, KeyCode::KeyD) {
        dir.x += 1.0;
    }
    if held(KeyCode::ArrowUp, KeyCode::KeyW) {
        dir.y += 1.0;
    }
    if held(KeyCode::ArrowDown, KeyCode::KeyS) {
        dir.y -= 1.0;
    }
    intent.0 = dir;
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
