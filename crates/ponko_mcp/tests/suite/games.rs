//! The games the tasks start from. They follow the 2D project template
//! (`ponko new --template 2d`): the rules are sim systems, numbers to tune
//! live in JSON files under `assets/` that hot reload, and every gameplay
//! type is reflected so the MCP tools can read and change it.
//!
//! Each game is small on purpose: a task is about finding and fixing one
//! thing with the tools, not about reading a large code base.

use super::tasks::GameAndSetup;
use ponko::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ------------------------------------------------------------------ arena

/// The playing field, in pixels.
pub const ARENA: Vec2 = Vec2::new(800.0, 600.0);
/// Where the player starts.
pub const START: Vec2 = Vec2::new(100.0, 300.0);
/// How close the player has to come to collect a coin.
pub const PICKUP_RADIUS: f32 = 24.0;

/// Numbers to tune without recompiling: `assets/settings.json`.
#[derive(Serialize, Deserialize)]
pub struct Settings {
    /// How fast the player moves, in pixels per second.
    pub speed: f32,
}

/// Where the player wants to go this tick; the keyboard sets it.
#[derive(Default, Serialize, Deserialize)]
pub struct Intent(pub Vec2);

/// The entity the player controls.
#[derive(Component, Serialize, Deserialize)]
pub struct Player;

/// A coin; touching it adds `value` to the score.
#[derive(Component, Serialize, Deserialize)]
pub struct Coin {
    /// Points it is worth.
    pub value: u32,
}

/// The score.
#[derive(Default, Serialize, Deserialize)]
pub struct Score {
    /// Points so far.
    pub points: u32,
}

/// The finish line, and the tick the player first crossed it.
#[derive(Serialize, Deserialize)]
pub struct Flag {
    /// The line's x.
    pub x: f32,
    /// The first tick the player was at or past it; `null` until then.
    pub reached_tick: Option<u64>,
}

/// The arena: a player that the arrow keys move, coins, a score and a flag.
fn arena(game: Game) -> Game {
    game.headless_frames(true)
        .system(read_input)
        .sim_system(movement)
        .sim_system(collect)
        .sim_system(finish_line)
}

fn arena_setup(ctx: &mut Ctx) {
    ctx.assets.load_resource::<Settings>(&mut ctx.world, "settings.json");
    ctx.world.insert_resource(Intent::default());
    ctx.world.insert_resource(Score::default());
    ctx.world.insert_resource(Flag {
        x: 700.0,
        reached_tick: None,
    });
    ctx.world.reflect::<Player>();
    ctx.world.reflect::<Coin>();
    ctx.world.reflect::<d2::Transform>();
    ctx.world.reflect_resource::<Settings>();
    ctx.world.reflect_resource::<Intent>();
    ctx.world.reflect_resource::<Score>();
    ctx.world.reflect_resource::<Flag>();
    ctx.world.spawn((Player, d2::Transform::at(START)));
}

/// Frame system: arrow keys (or WASD) set the intent.
fn read_input(ctx: &mut Ctx) {
    let held = |ctx: &Ctx, a: Key, b: Key| ctx.input.key_down(a) || ctx.input.key_down(b);
    let mut dir = Vec2::ZERO;
    if held(ctx, Key::Left, Key::A) {
        dir.x -= 1.0;
    }
    if held(ctx, Key::Right, Key::D) {
        dir.x += 1.0;
    }
    if held(ctx, Key::Up, Key::W) {
        dir.y -= 1.0;
    }
    if held(ctx, Key::Down, Key::S) {
        dir.y += 1.0;
    }
    ctx.world.resource_mut::<Intent>().0 = dir;
}

/// Sim system: moves the player by the intent, inside the arena.
fn movement(ctx: &mut Ctx) {
    let step =
        ctx.world.resource::<Intent>().0.clamp_length_max(1.0) * ctx.world.resource::<Settings>().speed * ctx.time.dt();
    ctx.world
        .query::<(&mut d2::Transform, &Player)>()
        .for_each(|(t, _)| t.position = (t.position + step).clamp(Vec2::ZERO, ARENA));
}

/// Sim system: coins the player touches go, and add to the score.
fn collect(ctx: &mut Ctx) {
    let mut players = Vec::new();
    ctx.world
        .query::<(&d2::Transform, &Player)>()
        .for_each(|(t, _)| players.push(t.position));
    let mut taken = Vec::new();
    ctx.world
        .query::<(Entity, &d2::Transform, &Coin)>()
        .for_each(|(e, t, c)| {
            if players.iter().any(|p| p.distance(t.position) <= PICKUP_RADIUS) {
                taken.push((e, c.value));
            }
        });
    for (e, value) in taken {
        ctx.world.despawn(e);
        ctx.world.resource_mut::<Score>().points += value;
    }
}

/// Sim system: notes the first tick the player reaches the flag.
fn finish_line(ctx: &mut Ctx) {
    let tick = ctx.time.sim_tick();
    let x = ctx.world.resource::<Flag>().x;
    let mut past = false;
    ctx.world
        .query::<(&d2::Transform, &Player)>()
        .for_each(|(t, _)| past |= t.position.x >= x);
    let flag = ctx.world.resource_mut::<Flag>();
    if past && flag.reached_tick.is_none() {
        flag.reached_tick = Some(tick);
    }
}

/// `tune-speed` and `coin-level`.
pub fn arena_game() -> GameAndSetup {
    (arena(Game::new("arena", ARENA.x as u32, ARENA.y as u32)), arena_setup)
}

// ------------------------------------------------------------------ enemies

/// One line of `assets/enemies.json`: an enemy the wave spawns.
#[derive(Clone, Serialize, Deserialize)]
pub struct EnemyKind {
    /// Its name.
    pub name: String,
    /// Health it starts with.
    pub health: f32,
    /// How much of the hazard's damage it ignores: 0 none, 0.5 half.
    pub armor: f32,
}

/// The enemies a wave is made of, from `assets/enemies.json`.
#[derive(Clone, Serialize, Deserialize)]
pub struct Roster {
    /// One entry per enemy.
    pub enemies: Vec<EnemyKind>,
}

/// An enemy.
#[derive(Component, Serialize, Deserialize)]
pub struct Enemy {
    /// Its name, from the roster.
    pub name: String,
    /// Damage it ignores (see [`EnemyKind::armor`]).
    pub armor: f32,
}

/// Health.
#[derive(Component, Serialize, Deserialize)]
pub struct Health {
    /// Left.
    pub current: f32,
}

/// Set `wave` to true to replace every enemy with a fresh wave from the
/// roster (the game does it at the start, and between rounds).
#[derive(Serialize, Deserialize)]
pub struct Waves {
    /// A new wave is wanted.
    pub wave: bool,
    /// Waves spawned so far.
    pub spawned: u32,
}

/// Sent when an enemy dies.
#[derive(Serialize)]
pub struct Died {
    /// Who.
    pub name: String,
    /// When.
    pub tick: u64,
}

/// Damage per second that the hazard deals to every enemy.
pub const HAZARD_DPS: f32 = 30.0;

fn enemies_setup(ctx: &mut Ctx) {
    ctx.assets.load_resource::<Roster>(&mut ctx.world, "enemies.json");
    ctx.world.insert_resource(Waves { wave: true, spawned: 0 });
    ctx.world.reflect::<Enemy>();
    ctx.world.reflect::<Health>();
    ctx.world.reflect_resource::<Roster>();
    ctx.world.reflect_resource::<Waves>();
    ctx.events.reflect::<Died>();
    spawn_wave(ctx);
}

/// Sim system: a new wave when one is asked for.
fn waves(ctx: &mut Ctx) {
    if ctx.world.resource::<Waves>().wave {
        spawn_wave(ctx);
    }
}

fn spawn_wave(ctx: &mut Ctx) {
    let mut old = Vec::new();
    ctx.world.query::<(Entity, &Enemy)>().for_each(|(e, _)| old.push(e));
    for e in old {
        ctx.world.despawn(e);
    }
    let roster = ctx.world.resource::<Roster>().clone();
    for (i, kind) in roster.enemies.iter().enumerate() {
        ctx.world.spawn((
            Enemy {
                name: kind.name.clone(),
                armor: kind.armor,
            },
            Health { current: kind.health },
            d2::Transform::at(vec2(100.0 + 100.0 * i as f32, 300.0)),
        ));
    }
    let waves = ctx.world.resource_mut::<Waves>();
    waves.wave = false;
    waves.spawned += 1;
}

/// Sim system: the hazard hurts every enemy, less through armor.
fn hazard(ctx: &mut Ctx) {
    let damage = HAZARD_DPS * ctx.time.dt();
    ctx.world
        .query::<(&mut Health, &Enemy)>()
        .for_each(|(h, e)| h.current -= damage * (1.0 - e.armor));
}

/// Sim system: enemies at zero health die.
fn deaths(ctx: &mut Ctx) {
    let tick = ctx.time.sim_tick();
    let mut dead = Vec::new();
    ctx.world
        .query::<(Entity, &Health, &Enemy)>()
        .for_each(|(e, h, enemy)| {
            if h.current <= 0.0 {
                dead.push((e, enemy.name.clone()));
            }
        });
    for (e, name) in dead {
        ctx.world.despawn(e);
        ctx.events.send(Died { name, tick });
    }
}

/// `immortal-enemy`.
pub fn enemies_game() -> GameAndSetup {
    let game = Game::new("waves", ARENA.x as u32, ARENA.y as u32)
        .sim_system(waves)
        .sim_system(hazard)
        .sim_system(deaths);
    (game, enemies_setup)
}

// ------------------------------------------------------------------ crowd

/// How the crowding system searches for neighbours: `assets/tuning.json`.
#[derive(Serialize, Deserialize)]
pub struct Tuning {
    /// Two villagers closer than this are crowded.
    pub radius: f32,
    /// The side of the search grid's cells. The search looks at the
    /// villager's cell and the eight around it.
    pub cell_size: f32,
}

/// A villager.
#[derive(Component, Serialize, Deserialize)]
pub struct Villager;

/// What the crowding system found in its last tick.
#[derive(Default, Serialize, Deserialize)]
pub struct Crowding {
    /// Pairs of villagers closer than `Tuning.radius`.
    pub pairs: u64,
    /// Distance checks it made to find them: the work a tick costs.
    pub checks: u64,
}

/// Villagers in the crowd.
pub const VILLAGERS: usize = 1500;
/// The field they stand in, in pixels.
pub const FIELD: f32 = 2000.0;

/// The villagers' positions: fixed, from a small generator, so every run
/// (and the checker's own count) sees the same crowd.
pub fn villager_positions() -> Vec<Vec2> {
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 40) as f32 / (1u64 << 24) as f32
    };
    (0..VILLAGERS).map(|_| vec2(next() * FIELD, next() * FIELD)).collect()
}

fn crowd_setup(ctx: &mut Ctx) {
    ctx.assets.load_resource::<Tuning>(&mut ctx.world, "tuning.json");
    ctx.world.insert_resource(Crowding::default());
    ctx.world.reflect::<Villager>();
    ctx.world.reflect::<d2::Transform>();
    ctx.world.reflect_resource::<Tuning>();
    ctx.world.reflect_resource::<Crowding>();
    for p in villager_positions() {
        ctx.world.spawn((Villager, d2::Transform::at(p)));
    }
}

/// Sim system: counts crowded pairs with a grid of `Tuning.cell_size`.
fn crowding(ctx: &mut Ctx) {
    let (radius, cell) = {
        let t = ctx.world.resource::<Tuning>();
        (t.radius, t.cell_size.max(1.0))
    };
    let mut at = Vec::new();
    ctx.world
        .query::<(&d2::Transform, &Villager)>()
        .for_each(|(t, _)| at.push(t.position));
    let key = |p: Vec2| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, p) in at.iter().enumerate() {
        grid.entry(key(*p)).or_default().push(i);
    }
    let (mut pairs, mut checks) = (0, 0);
    for (i, p) in at.iter().enumerate() {
        let (cx, cy) = key(*p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &j in grid.get(&(cx + dx, cy + dy)).map_or(&[][..], Vec::as_slice) {
                    if j > i {
                        checks += 1;
                        if p.distance(at[j]) < radius {
                            pairs += 1;
                        }
                    }
                }
            }
        }
    }
    *ctx.world.resource_mut::<Crowding>() = Crowding { pairs, checks };
}

/// `slow-crowd`.
pub fn crowd_game() -> GameAndSetup {
    let game = Game::new("crowd", ARENA.x as u32, ARENA.y as u32).sim_system(crowding);
    (game, crowd_setup)
}

// ------------------------------------------------------------------ glow

/// The shader that makes the player glow.
pub const GLOW_SHADER: &str = "shaders/glow.wgsl";

fn glow_setup(ctx: &mut Ctx) {
    arena_setup(ctx);
    // With hot reload on, a shader that doesn't compile gets a stand-in and
    // stays watched until a saved fix compiles.
    let _glow = ctx.gfx.load_sprite_shader(GLOW_SHADER);
}

/// `broken-shader`.
pub fn glow_game() -> GameAndSetup {
    (arena(Game::new("glow", ARENA.x as u32, ARENA.y as u32)), glow_setup)
}

// ------------------------------------------------------------------ hud

/// How the HUD shows the score: `assets/hud.json`.
#[derive(Serialize, Deserialize)]
pub struct HudStyle {
    /// The score label's text; `{points}` becomes the score.
    pub score_text: String,
    /// Whether the score label is shown.
    pub show_score: bool,
}

/// The HUD's nodes.
#[derive(Clone, Copy)]
struct Hud {
    score: ponko::ui::UiId,
}

fn hud_setup(ctx: &mut Ctx) {
    arena_setup(ctx);
    ctx.assets.load_resource::<HudStyle>(&mut ctx.world, "hud.json");
    ctx.world.reflect_resource::<HudStyle>();
    let root = ctx.ui.root().name("hud").id();
    let score = ctx.ui.label(root, "").name("hud.score").id();
    ctx.ui.push_modeless(root);
    ctx.world.insert_resource(Hud { score });
}

/// Frame system: the score label follows the score and the HUD style.
fn hud(ctx: &mut Ctx) {
    let Hud { score } = *ctx.world.resource::<Hud>();
    let (text, show) = {
        let style = ctx.world.resource::<HudStyle>();
        let points = ctx.world.resource::<Score>().points;
        (
            style.score_text.replace("{points}", &points.to_string()),
            style.show_score,
        )
    };
    ctx.ui.set_text(score, text);
    let layout = ponko::ui::UiLayout::auto();
    ctx.ui.set_layout(score, if show { layout } else { layout.hidden() });
}

/// `hidden-score`.
pub fn hud_game() -> GameAndSetup {
    let game = arena(Game::new("hud", ARENA.x as u32, ARENA.y as u32)).system(hud);
    (game, hud_setup)
}

// ------------------------------------------------------------------ dash

/// The level the dash game loads at the start.
pub const DASH_LEVEL: &str = "levels/run.json";

/// `assets/dash.json`: the dash game's numbers.
#[derive(Serialize, Deserialize)]
pub struct DashSettings {
    /// How fast the player moves, in pixels per second.
    pub speed: f32,
    /// How close the player has to come to a coin to collect it, in pixels.
    pub pickup_radius: f32,
}

/// Spikes: touching them drops every point.
#[derive(Component, Serialize, Deserialize)]
pub struct Spikes;

/// How close the player has to come to spikes to be hurt.
pub const SPIKES_RADIUS: f32 = 20.0;

/// Sent when spikes take the player's points.
#[derive(Serialize)]
pub struct Hurt {
    /// Points lost.
    pub lost: u32,
    /// When.
    pub tick: u64,
}

/// Sent when a coin is collected.
#[derive(Serialize)]
pub struct Collected {
    /// Points it gave.
    pub value: u32,
    /// When.
    pub tick: u64,
}

fn dash_setup(ctx: &mut Ctx) {
    ctx.assets.load_resource::<DashSettings>(&mut ctx.world, "dash.json");
    ctx.world.insert_resource(Intent::default());
    ctx.world.insert_resource(Score::default());
    ctx.world.reflect::<Player>();
    ctx.world.reflect::<Coin>();
    ctx.world.reflect::<Spikes>();
    ctx.world.reflect::<d2::Transform>();
    ctx.world.reflect_resource::<DashSettings>();
    ctx.world.reflect_resource::<Score>();
    ctx.events.reflect::<Hurt>();
    ctx.events.reflect::<Collected>();
    ctx.load_scene(DASH_LEVEL).expect("the dash level loads");
}

/// Sim system: the player moves at the file's speed.
fn dash_movement(ctx: &mut Ctx) {
    let step = ctx.world.resource::<Intent>().0.clamp_length_max(1.0)
        * ctx.world.resource::<DashSettings>().speed
        * ctx.time.dt();
    ctx.world
        .query::<(&mut d2::Transform, &Player)>()
        .for_each(|(t, _)| t.position = (t.position + step).clamp(Vec2::ZERO, ARENA));
}

/// Sim system: coins within the pickup radius are collected; spikes take
/// every point.
fn dash_touch(ctx: &mut Ctx) {
    let tick = ctx.time.sim_tick();
    let radius = ctx.world.resource::<DashSettings>().pickup_radius;
    let mut players = Vec::new();
    ctx.world
        .query::<(&d2::Transform, &Player)>()
        .for_each(|(t, _)| players.push(t.position));
    let near = |at: Vec2, r: f32| players.iter().any(|p| p.distance(at) <= r);
    let mut taken = Vec::new();
    ctx.world
        .query::<(Entity, &d2::Transform, &Coin)>()
        .for_each(|(e, t, c)| {
            if near(t.position, radius) {
                taken.push((e, c.value));
            }
        });
    for (e, value) in taken {
        ctx.world.despawn(e);
        ctx.world.resource_mut::<Score>().points += value;
        ctx.events.send(Collected { value, tick });
    }
    let mut hurt = false;
    ctx.world
        .query::<(&d2::Transform, &Spikes)>()
        .for_each(|(t, _)| hurt |= near(t.position, SPIKES_RADIUS));
    let points = ctx.world.resource::<Score>().points;
    if hurt && points > 0 {
        ctx.world.resource_mut::<Score>().points = 0;
        ctx.events.send(Hurt { lost: points, tick });
    }
}

/// `two-causes`.
pub fn dash_game() -> GameAndSetup {
    let game = Game::new("dash", ARENA.x as u32, ARENA.y as u32)
        .headless_frames(true)
        .system(read_input)
        .sim_system(dash_movement)
        .sim_system(dash_touch);
    (game, dash_setup)
}

// ------------------------------------------------------------------ cannon

/// Where the cannon stands, on the ground.
pub const CANNON_AT: Vec2 = Vec2::new(40.0, 500.0);
/// Gravity, in pixels per second squared (down).
pub const GRAVITY: f32 = 400.0;
/// Air drag: the ball loses `DRAG * speed²` of speed per second. The goal
/// doesn't give it; the agent measures its effect.
pub const DRAG: f32 = 0.0011;

/// `assets/cannon.json`.
#[derive(Serialize, Deserialize)]
pub struct CannonSettings {
    /// The ball's speed when it leaves the barrel, in pixels per second.
    pub muzzle_speed: f32,
    /// The barrel's angle above the ground, in degrees.
    pub angle_degrees: f32,
}

/// Set `fire` to fire once (Space does too).
#[derive(Default, Serialize, Deserialize)]
pub struct Trigger {
    /// A shot is wanted.
    pub fire: bool,
}

/// Where a shot must land: between `x_min` and `x_max`.
#[derive(Serialize, Deserialize)]
pub struct Target {
    /// The near edge.
    pub x_min: f32,
    /// The far edge.
    pub x_max: f32,
}

/// A cannonball in flight.
#[derive(Component, Serialize, Deserialize)]
pub struct Ball {
    /// Its velocity, in pixels per second.
    pub velocity: Vec2,
}

/// Sent when a ball lands.
#[derive(Serialize)]
pub struct Landed {
    /// Where.
    pub x: f32,
    /// Whether that is on the target.
    pub on_target: bool,
    /// When.
    pub tick: u64,
}

fn cannon_setup(ctx: &mut Ctx) {
    ctx.assets
        .load_resource::<CannonSettings>(&mut ctx.world, "cannon.json");
    ctx.world.insert_resource(Trigger::default());
    ctx.world.insert_resource(Target {
        x_min: 600.0,
        x_max: 640.0,
    });
    ctx.world.reflect::<Ball>();
    ctx.world.reflect::<d2::Transform>();
    ctx.world.reflect_resource::<CannonSettings>();
    ctx.world.reflect_resource::<Trigger>();
    ctx.world.reflect_resource::<Target>();
    ctx.events.reflect::<Landed>();
}

/// Frame system: Space pulls the trigger.
fn cannon_input(ctx: &mut Ctx) {
    if ctx.input.key_pressed(Key::Space) {
        ctx.world.resource_mut::<Trigger>().fire = true;
    }
}

/// Sim system: fires a ball when the trigger is pulled and none is flying.
fn cannon_fire(ctx: &mut Ctx) {
    if !std::mem::take(&mut ctx.world.resource_mut::<Trigger>().fire) {
        return;
    }
    let mut flying = false;
    ctx.world.query::<&Ball>().for_each(|_| flying = true);
    if flying {
        return;
    }
    let (speed, angle) = {
        let s = ctx.world.resource::<CannonSettings>();
        (s.muzzle_speed, s.angle_degrees.to_radians())
    };
    let velocity = vec2(angle.cos(), -angle.sin()) * speed;
    ctx.world.spawn((Ball { velocity }, d2::Transform::at(CANNON_AT)));
}

/// Sim system: balls fly under gravity and drag, and land on the ground.
fn cannon_flight(ctx: &mut Ctx) {
    let (dt, tick) = (ctx.time.dt(), ctx.time.sim_tick());
    let mut landed = Vec::new();
    ctx.world
        .query::<(Entity, &mut Ball, &mut d2::Transform)>()
        .for_each(|(e, b, t)| {
            b.velocity.y += GRAVITY * dt;
            let speed = b.velocity.length();
            b.velocity -= b.velocity * (DRAG * speed * dt);
            t.position += b.velocity * dt;
            if t.position.y >= CANNON_AT.y && b.velocity.y > 0.0 {
                landed.push((e, t.position.x));
            }
        });
    let target = (
        ctx.world.resource::<Target>().x_min,
        ctx.world.resource::<Target>().x_max,
    );
    for (e, x) in landed {
        ctx.world.despawn(e);
        let on_target = (target.0..=target.1).contains(&x);
        ctx.events.send(Landed { x, on_target, tick });
    }
}

/// `cannon-range`.
pub fn cannon_game() -> GameAndSetup {
    let game = Game::new("cannon", ARENA.x as u32, ARENA.y as u32)
        .headless_frames(true)
        .system(cannon_input)
        .sim_system(cannon_fire)
        .sim_system(cannon_flight);
    (game, cannon_setup)
}

// ------------------------------------------------------------------ cave

/// The level the cave game loads at the start.
pub const CAVE_LEVEL: &str = "levels/cave.json";
/// Where the spawner puts new bats.
pub const BAT_SPAWN: Vec2 = Vec2::new(700.0, 300.0);

/// `assets/spawner.json`: how the cave adds bats while it plays.
#[derive(Serialize, Deserialize)]
pub struct Spawner {
    /// The prefab each new bat is spawned from.
    pub prefab: String,
    /// Seconds between two new bats.
    pub every_secs: f32,
    /// No new bat while this many are alive.
    pub max_bats: u32,
    /// The spawned bat's health is the prefab's times this (difficulty).
    pub health_scale: f32,
}

/// What the spawner does now: its clock, and the file's numbers as the
/// game uses them (copied from `assets/spawner.json` every tick).
#[derive(Default, Serialize, Deserialize)]
pub struct Spawning {
    /// Seconds since the spawner last added a bat.
    pub secs: f32,
    /// Seconds between two new bats.
    pub every_secs: f32,
    /// The spawned bat's health is the prefab's times this.
    pub health_scale: f32,
}

/// A bat.
#[derive(Component, Serialize, Deserialize)]
pub struct Bat;

fn cave_setup(ctx: &mut Ctx) {
    ctx.assets.load_resource::<Spawner>(&mut ctx.world, "spawner.json");
    ctx.world.insert_resource(Spawning::default());
    ctx.world.reflect::<Player>();
    ctx.world.reflect::<Bat>();
    ctx.world.reflect::<Health>();
    ctx.world.reflect::<d2::Transform>();
    ctx.world.reflect_resource::<Spawning>();
    ctx.load_scene(CAVE_LEVEL).expect("the cave level loads");
}

/// Sim system: a bat from the prefab every `every_secs`, up to `max_bats`.
fn spawn_bats(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    let (every, max, scale, prefab) = {
        let s = ctx.world.resource::<Spawner>();
        (s.every_secs, s.max_bats, s.health_scale, s.prefab.clone())
    };
    let spawning = ctx.world.resource_mut::<Spawning>();
    spawning.every_secs = every;
    spawning.health_scale = scale;
    spawning.secs += dt;
    if spawning.secs < every {
        return;
    }
    spawning.secs = 0.0;
    let mut bats = 0;
    ctx.world.query::<&Bat>().for_each(|_| bats += 1);
    if bats >= max {
        return;
    }
    match ctx.spawn_prefab(&prefab, BAT_SPAWN) {
        Ok(bat) => {
            if let Some(h) = ctx.world.get_mut::<Health>(bat) {
                h.current *= scale;
            }
        }
        Err(e) => eprintln!("spawner: {e}"),
    }
}

/// `tough-bats`.
pub fn cave_game() -> GameAndSetup {
    let game = Game::new("cave", ARENA.x as u32, ARENA.y as u32).sim_system(spawn_bats);
    (game, cave_setup)
}

// ------------------------------------------------------------------ defense

/// `assets/defense.json`: the pads the towers are built on. Not reflected:
/// a world save doesn't carry it, so loading one keeps the file's towers.
#[derive(Serialize, Deserialize)]
pub struct Defense {
    /// Pad names, one tower each.
    pub towers: Vec<String>,
}

/// A place a tower can stand.
#[derive(Component, Serialize, Deserialize)]
pub struct Pad {
    /// Its name, as `assets/defense.json` gives it.
    pub name: String,
}

/// A tower: shoots the walker in range that is furthest along the road.
#[derive(Component, Serialize, Deserialize)]
pub struct Tower {
    /// The pad it stands on.
    pub pad: String,
    /// Seconds until it can shoot again.
    pub reload: f32,
}

/// An enemy walking the road to the gate.
#[derive(Component, Serialize, Deserialize)]
pub struct Walker {
    /// Pixels per second.
    pub speed: f32,
    /// How far along the road it is, in pixels (negative: not on it yet).
    pub along: f32,
}

/// The gate at the road's end.
#[derive(Default, Serialize, Deserialize)]
pub struct Gate {
    /// Walkers that reached it.
    pub breaches: u32,
    /// Walkers the towers stopped.
    pub stopped: u32,
}

/// Sent when a walker reaches the gate.
#[derive(Serialize)]
pub struct Breached {
    /// When.
    pub tick: u64,
}

/// The towers the file last asked for (not reflected).
#[derive(Default)]
struct Built(Vec<String>);

/// The road, from the spawn to the gate.
pub const ROAD: &[Vec2] = &[
    Vec2::new(0.0, 100.0),
    Vec2::new(600.0, 100.0),
    Vec2::new(600.0, 300.0),
    Vec2::new(100.0, 300.0),
    Vec2::new(100.0, 500.0),
    Vec2::new(800.0, 500.0),
];

/// The pads: name and place.
pub const PADS: &[(&str, Vec2)] = &[
    ("north", Vec2::new(275.0, 51.0)),
    ("east", Vec2::new(570.0, 452.0)),
    ("bend", Vec2::new(178.0, 377.0)),
    ("cliff", Vec2::new(769.0, 342.0)),
    ("south", Vec2::new(270.0, 433.0)),
    ("corner", Vec2::new(65.0, 301.0)),
];

/// A tower's reach, in pixels.
pub const TOWER_RANGE: f32 = 130.0;
/// Seconds between a tower's shots.
pub const TOWER_RELOAD: f32 = 0.5;
/// Damage per shot.
pub const TOWER_DAMAGE: f32 = 10.0;

/// The wave: (speed, health, start delay in seconds) per walker.
pub const WAVE: &[(f32, f32, f32)] = &[
    (60.0, 80.0, 0.0),
    (60.0, 80.0, 1.0),
    (60.0, 80.0, 2.0),
    (140.0, 40.0, 6.0),
    (140.0, 40.0, 6.5),
    (140.0, 40.0, 7.0),
    (140.0, 40.0, 7.5),
    (50.0, 210.0, 3.0),
    (60.0, 80.0, 10.0),
    (60.0, 80.0, 11.0),
];

/// Where a walker `along` pixels down the road is.
pub fn road_at(along: f32) -> Vec2 {
    let mut left = along.max(0.0);
    for w in ROAD.windows(2) {
        let len = w[0].distance(w[1]);
        if left <= len {
            return w[0] + (w[1] - w[0]) * (left / len);
        }
        left -= len;
    }
    *ROAD.last().unwrap()
}

/// The road's length.
pub fn road_length() -> f32 {
    ROAD.windows(2).map(|w| w[0].distance(w[1])).sum()
}

fn defense_setup(ctx: &mut Ctx) {
    ctx.assets.load_resource::<Defense>(&mut ctx.world, "defense.json");
    ctx.world.insert_resource(Gate::default());
    ctx.world.insert_resource(Built::default());
    ctx.world.reflect::<Pad>();
    ctx.world.reflect::<Tower>();
    ctx.world.reflect::<Walker>();
    ctx.world.reflect::<Health>();
    ctx.world.reflect::<d2::Transform>();
    ctx.world.reflect_resource::<Gate>();
    ctx.events.reflect::<Breached>();
    for (name, at) in PADS {
        ctx.world.spawn((Pad { name: (*name).into() }, d2::Transform::at(*at)));
    }
    for &(speed, health, delay) in WAVE {
        ctx.world.spawn((
            Walker {
                speed,
                along: -delay * speed,
            },
            Health { current: health },
            d2::Transform::at(road_at(0.0)),
        ));
    }
}

/// Sim system: builds the file's towers when it changes, or when a world
/// load took them away.
fn build_towers(ctx: &mut Ctx) {
    let wanted = ctx.world.resource::<Defense>().towers.clone();
    let mut towers = Vec::new();
    ctx.world.query::<(Entity, &Tower)>().for_each(|(e, _)| towers.push(e));
    let changed = ctx.world.resource::<Built>().0 != wanted;
    if !changed && !(towers.is_empty() && !wanted.is_empty()) {
        return;
    }
    for e in towers {
        ctx.world.despawn(e);
    }
    let mut pads = Vec::new();
    ctx.world
        .query::<(&Pad, &d2::Transform)>()
        .for_each(|(p, t)| pads.push((p.name.clone(), t.position)));
    for name in &wanted {
        match pads.iter().find(|(n, _)| n == name) {
            Some((_, at)) => {
                ctx.world.spawn((
                    Tower {
                        pad: name.clone(),
                        reload: 0.0,
                    },
                    d2::Transform::at(*at),
                ));
            }
            None => eprintln!("defense.json: no pad `{name}`"),
        }
    }
    ctx.world.resource_mut::<Built>().0 = wanted;
}

/// Sim system: walkers walk; one that reaches the gate breaches it.
fn walk(ctx: &mut Ctx) {
    let (dt, tick, end) = (ctx.time.dt(), ctx.time.sim_tick(), road_length());
    let mut arrived = Vec::new();
    ctx.world
        .query::<(Entity, &mut Walker, &mut d2::Transform)>()
        .for_each(|(e, w, t)| {
            w.along += w.speed * dt;
            t.position = road_at(w.along);
            if w.along >= end {
                arrived.push(e);
            }
        });
    for e in arrived {
        ctx.world.despawn(e);
        ctx.world.resource_mut::<Gate>().breaches += 1;
        ctx.events.send(Breached { tick });
    }
}

/// Sim system: towers shoot the walker in range furthest along the road.
fn shoot(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    let mut walkers = Vec::new();
    ctx.world
        .query::<(Entity, &Walker, &d2::Transform)>()
        .for_each(|(e, w, t)| {
            if w.along >= 0.0 {
                walkers.push((e, w.along, t.position));
            }
        });
    let mut shots = Vec::new();
    ctx.world
        .query::<(&mut Tower, &d2::Transform)>()
        .for_each(|(tower, t)| {
            tower.reload = (tower.reload - dt).max(0.0);
            if tower.reload > 0.0 {
                return;
            }
            let target = walkers
                .iter()
                .filter(|w| w.2.distance(t.position) <= TOWER_RANGE)
                .max_by(|a, b| a.1.total_cmp(&b.1));
            if let Some(&(e, _, _)) = target {
                shots.push(e);
                tower.reload = TOWER_RELOAD;
            }
        });
    for e in shots {
        let dead = ctx.world.get_mut::<Health>(e).is_some_and(|h| {
            h.current -= TOWER_DAMAGE;
            h.current <= 0.0
        });
        if dead {
            ctx.world.despawn(e);
            ctx.world.resource_mut::<Gate>().stopped += 1;
        }
    }
}

/// `hold-the-gate`.
pub fn defense_game() -> GameAndSetup {
    let game = Game::new("defense", ARENA.x as u32, ARENA.y as u32)
        .sim_system(build_towers)
        .sim_system(walk)
        .sim_system(shoot);
    (game, defense_setup)
}
