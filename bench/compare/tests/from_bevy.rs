//! The Bevy half of every side-by-side snippet in the guide's "Coming from
//! Bevy" chapter (`book/src/from-bevy.md`), compiled against the Bevy this
//! crate pins (0.20.1), so the chapter can't quote Bevy code that does not
//! build. The chapter includes the regions between the `ANCHOR` lines.
//!
//! `cargo check --tests` in `bench/compare` checks it. Only
//! `score_counts_up_headless` is a test, the chapter's headless example.
#![allow(dead_code)]

use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, observe};

#[derive(Component)]
struct Velocity(Vec2);
#[derive(Component)]
struct Health(f32);
#[derive(Component)]
struct Player;

fn app() {
    // ANCHOR: app
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, movement)
        .run();
    // ANCHOR_END: app
}

// ANCHOR: sprite
fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(Camera2d);
    commands.spawn((
        Sprite::from_image(asset_server.load("crab.png")),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
}
// ANCHOR_END: sprite

// ANCHOR: movement
fn movement(time: Res<Time>, mut query: Query<(&Velocity, &mut Transform)>) {
    for (velocity, mut transform) in &mut query {
        transform.translation += velocity.0.extend(0.0) * time.delta_secs();
    }
}
// ANCHOR_END: movement

// ANCHOR: input
fn steer(keys: Res<ButtonInput<KeyCode>>, mut player: Single<&mut Velocity, With<Player>>) {
    player.0.x = 0.0;
    if keys.pressed(KeyCode::ArrowRight) {
        player.0.x += 200.0;
    }
    if keys.pressed(KeyCode::ArrowLeft) {
        player.0.x -= 200.0;
    }
    if keys.just_pressed(KeyCode::Space) {
        info!("jump");
    }
}
// ANCHOR_END: input

// ANCHOR: resource
#[derive(Resource)]
struct Score(u32);

fn add_point(mut score: ResMut<Score>) {
    score.0 += 1;
}

fn score_plugin(app: &mut App) {
    app.insert_resource(Score(0)).add_systems(Update, add_point);
}
// ANCHOR_END: resource

// ANCHOR: despawn
fn die(mut commands: Commands, query: Query<(Entity, &Health)>) {
    for (entity, health) in &query {
        if health.0 <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
// ANCHOR_END: despawn

// ANCHOR: messages
#[derive(Message)]
struct Died {
    who: Entity,
}

fn deaths(query: Query<(Entity, &Health)>, mut died: MessageWriter<Died>) {
    for (entity, health) in &query {
        if health.0 <= 0.0 {
            died.write(Died { who: entity });
        }
    }
}

fn funerals(mut commands: Commands, mut died: MessageReader<Died>) {
    for message in died.read() {
        commands.entity(message.who).despawn();
    }
}

fn messages_plugin(app: &mut App) {
    app.add_message::<Died>()
        .add_systems(Update, (deaths, funerals).chain());
}
// ANCHOR_END: messages

// ANCHOR: states
#[derive(States, Default, Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Screen {
    #[default]
    Title,
    Playing,
}

fn start(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<Screen>>) {
    if keys.just_pressed(KeyCode::Enter) {
        next.set(Screen::Playing);
    }
}

fn spawn_level(mut commands: Commands) {
    commands.spawn((Player, Velocity(Vec2::ZERO), Transform::default()));
}

fn states_plugin(app: &mut App) {
    app.init_state::<Screen>()
        .add_systems(OnEnter(Screen::Playing), spawn_level)
        .add_systems(Update, start.run_if(in_state(Screen::Title)))
        .add_systems(Update, movement.run_if(in_state(Screen::Playing)));
}
// ANCHOR_END: states

// ANCHOR: fixed
fn fixed_plugin(app: &mut App) {
    app.insert_resource(Time::<Fixed>::from_hz(120.0))
        .add_systems(FixedUpdate, movement);
}
// ANCHOR_END: fixed

// ANCHOR: timer
#[derive(Resource)]
struct SpawnTimer(Timer);

fn spawn_on_a_timer(time: Res<Time>, mut timer: ResMut<SpawnTimer>, mut commands: Commands) {
    if timer.0.tick(time.delta()).just_finished() {
        commands.spawn((Health(10.0), Transform::default()));
    }
}

fn timer_plugin(app: &mut App) {
    app.insert_resource(SpawnTimer(Timer::from_seconds(2.0, TimerMode::Repeating)))
        .add_systems(Update, spawn_on_a_timer);
}
// ANCHOR_END: timer

// ANCHOR: hierarchy
fn spawn_body(mut commands: Commands) {
    let body = commands.spawn((Player, Transform::from_xyz(10.0, 0.0, 0.0))).id();
    // A child at (2, 0) in the body's space.
    commands.spawn((Transform::from_xyz(2.0, 0.0, 0.0), ChildOf(body)));
}

fn clear_body(mut commands: Commands, body: Single<Entity, With<Player>>) {
    commands.entity(*body).despawn(); // and its children
}
// ANCHOR_END: hierarchy

// ANCHOR: text
#[derive(Component)]
struct ScoreText;

fn spawn_hud(mut commands: Commands) {
    commands.spawn((Text::new("Score: 0"), ScoreText));
}

fn update_hud(score: Res<Score>, mut text: Single<&mut Text, With<ScoreText>>) {
    text.0 = format!("Score: {}", score.0);
}
// ANCHOR_END: text

// ANCHOR: sound
fn play_sounds(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(AudioPlayer::new(asset_server.load("hit.ogg")));
    commands.spawn((
        AudioPlayer::new(asset_server.load("theme.ogg")),
        PlaybackSettings::LOOP,
    ));
}
// ANCHOR_END: sound

// ANCHOR: button
// use bevy::ui_widgets::{observe, Activate, Button};
fn spawn_menu(mut commands: Commands) {
    commands.spawn((
        Node {
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            Button,
            Node {
                width: px(150),
                height: px(65),
                ..default()
            },
            BackgroundColor(Color::srgb(0.15, 0.15, 0.15)),
            children![Text::new("Play")],
            observe(|_: On<Activate>, mut next: ResMut<NextState<Screen>>| {
                next.set(Screen::Playing);
            }),
        )],
    ));
}
// ANCHOR_END: button

// ANCHOR: headless
#[test]
fn score_counts_up_headless() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(score_plugin);
    app.update();
    app.update();
    assert_eq!(app.world().resource::<Score>().0, 2);
}
// ANCHOR_END: headless
