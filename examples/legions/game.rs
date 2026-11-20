//! The game: map, armies, rules, orders, replays, the world hash, and how it
//! looks. Shared with `tests/legions.rs` (headless runs and replays) and the
//! offscreen GPU test `src/render/legions_render_test.rs`.
//!
//! Three layers, each a [`Plugin`]:
//!
//! 1. [`Legions`]: the rules, all of them simulation systems on the fixed
//!    tick. They read only the world and the [`Orders`] given at a tick, so
//!    the same seed, army size and orders give the same battle, bit for bit,
//!    whatever the thread count, frame rate or machine.
//! 2. [`Controls`]: a frame system that turns the mouse and keys into
//!    [`Order`]s. It writes nothing else.
//! 3. [`View`]: the sprite animation, the ground, the HUD. None of it changes
//!    the game, so a windowed run and a headless one are the same battle.
//!
//! What the engine does for it: flow fields on a [`NavGrid`] (one per army,
//! shared by 50,000 units), spatial grids for targeting ([`SpatialGrid`]),
//! queries on all cores (`par_for_each`), seeded per-entity randomness
//! (`ctx.random().of(e)`), sprites from a [`SpriteSheet`] drawn as instanced
//! batches, reflection (the `ponko-mcp` tools can read [`Score`] or any unit),
//! and the fixed simulation tick.

// The tests include this module but need only part of it.
#![allow(dead_code)]

use ponko::flow::{FlowField, NavGrid};
use ponko::prelude::*;
use ponko::sheet::SpriteSheet;
use ponko::spatial::SpatialGrid;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---- tuning ---------------------------------------------------------------------

/// Units in a battle unless told otherwise: two armies of 50,000.
pub const DEFAULT_UNITS: usize = 100_000;
/// The map unless told otherwise.
pub const SEED: u64 = 1;
pub const APP_ID: &str = "dev.ponko.legions";
pub const WINDOW: Vec2 = vec2(1280.0, 720.0);

/// World units per nav cell.
pub const CELL: f32 = 4.0;
/// Map size (in cells) for [`UNITS_PER_MAP`] units; bigger armies get a
/// bigger map, so the crowd density stays the same.
pub const MAP: UVec2 = uvec2(400, 225);
pub const UNITS_PER_MAP: f32 = 20_000.0;
/// The cost of a mud cell: units cross it this many times slower.
pub const MUD: u8 = 5;

/// Soldiers charge enemies this close.
pub const SIGHT: f32 = 24.0;
/// Friends closer than this push each other apart.
pub const PERSONAL_SPACE: f32 = 3.5;
/// World units an arrow flies per tick.
pub const ARROW_SPEED: f32 = 3.0;
/// An arrow hits the enemy nearest to where it lands, if one is this close.
pub const ARROW_SPLASH: f32 = 3.0;
/// Ticks a fallen unit stays on the field.
pub const CORPSE_TICKS: u16 = 240;
/// A round lasts at most three minutes; then the bigger army wins.
pub const ROUND_TICKS: u64 = 3 * 60 * 60;
/// Ticks between the end of a round and the next.
pub const RESTART_TICKS: u16 = 300;
/// Every 4th unit is an archer.
pub const ARCHER_EVERY: usize = 4;
/// Space between units in the starting formation.
pub const SPACING: f32 = 3.2;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Team {
    Red,
    Blue,
}

impl Team {
    pub const BOTH: [Team; 2] = [Team::Red, Team::Blue];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn enemy(self) -> Team {
        match self {
            Team::Red => Team::Blue,
            Team::Blue => Team::Red,
        }
    }
    /// The team's colour as sRGB bytes, for the sprite sheet.
    pub fn rgba8(self) -> [u8; 4] {
        match self {
            Team::Red => [0xFF, 0x6B, 0x5B, 0xFF],
            Team::Blue => [0x5B, 0xB8, 0xFF, 0xFF],
        }
    }
    pub fn color(self) -> Color {
        match self {
            Team::Red => Color::hex(0xFF6B5B),
            Team::Blue => Color::hex(0x5BB8FF),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Melee: charges what it sees, hits what it reaches.
    Soldier,
    /// Ranged: stops when an enemy is in range and shoots arrows.
    Archer,
}

/// How a kind of unit fights.
pub struct Stats {
    /// World units per second.
    pub speed: f32,
    pub hp: f32,
    /// Per hit (soldier) or per arrow (archer).
    pub damage: f32,
    /// Ticks between attacks.
    pub cooldown: u16,
    /// How far it hits: melee reach, or bow range.
    pub reach: f32,
}

impl Kind {
    pub fn stats(self) -> &'static Stats {
        match self {
            Kind::Soldier => &Stats {
                speed: 32.0,
                hp: 12.0,
                damage: 3.0,
                cooldown: 36,
                reach: 5.0,
            },
            Kind::Archer => &Stats {
                speed: 32.0,
                hp: 8.0,
                damage: 6.0,
                cooldown: 100,
                reach: 80.0,
            },
        }
    }
}

// ---- components and resources ------------------------------------------------------

/// A unit. Reflected, so tools can read and change it.
#[derive(Serialize, Deserialize, Component)]
pub struct Unit {
    pub team: Team,
    pub kind: Kind,
    pub hp: f32,
    pub vel: Vec2,
    /// Ticks until it may attack again.
    pub cooldown: u16,
    /// Ticks since it last attacked (for the animation), up to 255.
    pub swing: u8,
    /// An enemy was within reach (or range) at the last look.
    pub engaged: bool,
    /// Away from crowding friends; refreshed every other tick.
    #[serde(skip)]
    pub push: Vec2,
    /// What it does this tick: decided in parallel, applied in order.
    #[serde(skip)]
    pub act: Act,
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub enum Act {
    #[default]
    None,
    /// A melee hit on this enemy.
    Hit(Entity),
    /// An arrow at this point.
    Shoot(Vec2),
}

/// An arrow in flight: it flies straight from `from` to `to` in `flight`
/// ticks and hits whoever stands nearest to `to` when it lands.
#[derive(Serialize, Deserialize, Component)]
pub struct Arrow {
    pub team: Team,
    pub from: Vec2,
    pub to: Vec2,
    pub tick: u16,
    pub flight: u16,
    /// Who it hit, found on the tick it lands.
    #[serde(skip)]
    pub hit: Option<Entity>,
}

/// A fallen unit, gone after [`CORPSE_TICKS`].
#[derive(Serialize, Deserialize, Component)]
pub struct Corpse {
    pub team: Team,
    pub kind: Kind,
    pub ttl: u16,
}

/// What an army is doing.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stance {
    /// March on the enemy army.
    Attack,
    /// March to the rally point.
    Rally,
    /// Stay put; fight what comes.
    Hold,
}

/// The map, one flow field per army, and where each army is headed.
pub struct Battle {
    pub nav: NavGrid,
    pub fields: [FlowField; 2],
    pub goal: [Vec2; 2],
    pub stance: [Stance; 2],
    /// Where each army starts: the middle of its formation.
    pub start: [Vec2; 2],
    /// Units per army.
    pub army: usize,
    /// How long the last flow field took (for the HUD only: not game state).
    pub field_ms: f32,
}

impl Battle {
    /// The world size of the map.
    pub fn world_size(&self) -> Vec2 {
        self.nav.size().as_vec2() * CELL
    }

    fn set_goal(&mut self, team: Team, at: Vec2) {
        let start = ponko::perf::Instant::now();
        self.goal[team.index()] = at;
        self.fields[team.index()].compute(&self.nav, &[at]);
        self.field_ms = start.elapsed().as_secs_f32() * 1000.0;
    }
}

/// Who's winning. Reflected, so tools can read it (`ponko-mcp`'s
/// `get_resource`, `watch`). Pairs are `[red, blue]`.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Score {
    pub alive: [usize; 2],
    pub fallen: [u32; 2],
    pub arrows: usize,
    /// Arrows that hit someone, this round.
    pub arrow_hits: u32,
    /// Where each army is, on average.
    pub center: [Vec2; 2],
    pub round: u32,
    pub round_tick: u64,
    pub rounds_won: [u32; 2],
    /// Set when the round is over: `Some(None)` is a draw.
    pub outcome: Option<Option<Team>>,
    /// Ticks until the next round, once this one is over.
    pub restart_in: u16,
}

/// What a new unit, arrow or corpse looks like: sprites when there is
/// something to draw them, nothing in a plain headless run.
#[derive(Clone, Copy)]
pub struct Look(pub Option<Art>);

/// The pictures: every unit, arrow and corpse is a frame of one sheet, so
/// the whole army is a single instanced batch.
#[derive(Clone, Copy)]
pub struct Art {
    pub sheet: SpriteSheet,
}

// ---- orders, recording and replay -----------------------------------------------------

/// What the player can tell the red army. Orders are the game's only input:
/// a replay is the list of orders with the tick each one took effect.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Order {
    /// March to this point of the map.
    Rally { x: f32, y: f32 },
    /// March on the enemy army.
    Attack,
    /// Stay put.
    Hold,
    /// Start a new round.
    Restart,
}

/// An order and the tick it took effect on.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct Stamped {
    pub tick: u64,
    pub order: Order,
}

/// Orders given since the last tick (by [`Controls`], or by a test); the
/// first simulation system of the next tick applies them.
#[derive(Default)]
pub struct Orders {
    pub pending: Vec<Order>,
}

/// Orders of a replay, applied at their ticks.
pub struct Feed {
    pub orders: Vec<Stamped>,
    pub next: usize,
}

/// Records every applied order; [`record_checkpoint`] and the controls
/// (on Escape) write the replay file.
pub struct Recorder {
    pub path: PathBuf,
    pub units: usize,
    pub seed: u64,
    pub orders: Vec<Stamped>,
    /// Write a checkpoint every this many ticks (0: only on quit).
    pub every: u64,
}

/// The replay file's `format`.
pub const REPLAY_FORMAT: &str = "ponko-legions-replay";
pub const REPLAY_VERSION: u32 = 1;

/// A recorded battle: everything needed to play it again, and the world
/// hash it must end with.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Replay {
    pub format: String,
    pub version: u32,
    pub seed: u64,
    pub units: usize,
    /// Ticks to run.
    pub ticks: u64,
    pub orders: Vec<Stamped>,
    /// [`world_hash`] after the last tick, as 16 hex digits.
    pub hash: String,
}

impl Replay {
    /// Reads a replay file.
    ///
    /// # Errors
    /// If the file can't be read, isn't a replay, or is of another version.
    pub fn load(path: &Path) -> Result<Replay, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let replay: Replay = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if replay.format != REPLAY_FORMAT || replay.version != REPLAY_VERSION {
            return Err(format!(
                "{}: not a {REPLAY_FORMAT} file of version {REPLAY_VERSION}",
                path.display()
            ));
        }
        Ok(replay)
    }

    /// Writes it as pretty JSON.
    ///
    /// # Errors
    /// If the file can't be written.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The expected hash as a number.
    pub fn expected_hash(&self) -> Option<u64> {
        u64::from_str_radix(&self.hash, 16).ok()
    }
}

/// Formats a world hash the way replay files keep it.
pub fn hash_hex(hash: u64) -> String {
    format!("{hash:016x}")
}

// ---- the world hash ---------------------------------------------------------------------

/// FNV-1a, 64 bits: simple, and the same on every platform.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
    }
    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.bytes(&v.to_bits().to_le_bytes());
    }
    fn vec2(&mut self, v: Vec2) {
        self.f32(v.x);
        self.f32(v.y);
    }
}

// ANCHOR: hash
/// A hash of the whole game state, bit for bit: every unit, arrow and
/// corpse (by entity, so the order they sit in memory doesn't matter) and
/// the score, the goals and the stances. Sprites, the camera and timings
/// are left out: they don't change the game.
///
/// Two runs with the same seed, army size and orders end with the same
/// hash on any thread count and any platform; a replay file keeps it.
pub fn world_hash(world: &World) -> u64 {
    let mut rows: Vec<(u64, u64)> = Vec::new();
    world
        .query_ref::<(Entity, &d2::Transform, &Unit)>()
        .for_each(|(e, t, u)| {
            let mut h = Fnv::new();
            h.bytes(&[1, u.team as u8, u.kind as u8, u.swing, u8::from(u.engaged)]);
            h.vec2(t.position);
            h.f32(u.hp);
            h.vec2(u.vel);
            h.u64(u64::from(u.cooldown));
            rows.push((e.to_bits(), h.0));
        });
    world
        .query_ref::<(Entity, &d2::Transform, &Arrow)>()
        .for_each(|(e, t, a)| {
            let mut h = Fnv::new();
            h.bytes(&[2, a.team as u8]);
            h.vec2(t.position);
            h.vec2(a.from);
            h.vec2(a.to);
            h.u64(u64::from(a.tick) << 16 | u64::from(a.flight));
            rows.push((e.to_bits(), h.0));
        });
    world
        .query_ref::<(Entity, &d2::Transform, &Corpse)>()
        .for_each(|(e, t, c)| {
            let mut h = Fnv::new();
            h.bytes(&[3, c.team as u8, c.kind as u8]);
            h.vec2(t.position);
            h.u64(u64::from(c.ttl));
            rows.push((e.to_bits(), h.0));
        });
    rows.sort_unstable();
    let mut h = Fnv::new();
    for (e, row) in rows {
        h.u64(e);
        h.u64(row);
    }
    if let Some(s) = world.get_resource::<Score>() {
        for team in 0..2 {
            h.u64(s.alive[team] as u64);
            h.u64(u64::from(s.fallen[team]));
            h.u64(u64::from(s.rounds_won[team]));
            h.vec2(s.center[team]);
        }
        h.u64(s.arrows as u64);
        h.u64(u64::from(s.arrow_hits));
        h.u64(u64::from(s.round));
        h.u64(s.round_tick);
        h.u64(match s.outcome {
            None => 0,
            Some(None) => 1,
            Some(Some(team)) => 2 + team as u64,
        });
        h.u64(u64::from(s.restart_in));
    }
    if let Some(b) = world.get_resource::<Battle>() {
        for team in 0..2 {
            h.vec2(b.goal[team]);
            h.u64(b.stance[team] as u64);
        }
    }
    h.0
}
// ANCHOR_END: hash

// ---- building the game -----------------------------------------------------------------

/// The game in a window: rules, controls and view.
pub fn build(units: usize, seed: u64) -> Game {
    Game::new("Legions", WINDOW.x as u32, WINDOW.y as u32)
        .seed(seed)
        .app_id(APP_ID)
        .plugin(Legions { units, art: true })
        .plugin(Controls)
        .plugin(View)
}

/// The rules alone, for headless runs and replays.
pub fn headless(units: usize, seed: u64) -> Game {
    Game::new("legions", 1, 1)
        .seed(seed)
        .plugin(Legions { units, art: false })
}

/// `game` playing `replay`'s orders at their ticks.
pub fn with_replay(game: Game, replay: &Replay) -> Game {
    game.plugin(ReplayOrders(replay.orders.clone()))
}

/// `game` recording its orders into a replay at `path`, with a checkpoint
/// every `every` ticks (0: only when the player quits with Escape).
pub fn with_recorder(game: Game, path: PathBuf, units: usize, seed: u64, every: u64) -> Game {
    game.plugin(Recording {
        path,
        units,
        seed,
        every,
    })
}

/// Feeds a replay's orders: see [`with_replay`].
pub struct ReplayOrders(pub Vec<Stamped>);

impl Plugin for ReplayOrders {
    fn build(&self, game: Game) -> Game {
        game
    }
    fn setup(&self, ctx: &mut Ctx) {
        ctx.world.insert_resource(Feed {
            orders: self.0.clone(),
            next: 0,
        });
    }
}

/// Records the orders: see [`with_recorder`].
pub struct Recording {
    pub path: PathBuf,
    pub units: usize,
    pub seed: u64,
    pub every: u64,
}

impl Plugin for Recording {
    fn build(&self, game: Game) -> Game {
        game.add(Phase::SimLast, record_checkpoint)
    }
    fn setup(&self, ctx: &mut Ctx) {
        ctx.world.insert_resource(Recorder {
            path: self.path.clone(),
            units: self.units,
            seed: self.seed,
            orders: Vec::new(),
            every: self.every,
        });
    }
}

/// Plays `replay` headless; returns the world and its hash.
pub fn play_replay(replay: &Replay) -> (World, u64) {
    let world = with_replay(headless(replay.units, replay.seed), replay).run_headless(replay.ticks, |_| {});
    let hash = world_hash(&world);
    (world, hash)
}

/// The rules: map, armies, fighting, arrows, deaths, rounds and orders.
pub struct Legions {
    /// Units in both armies together.
    pub units: usize,
    /// Give units sprites (needed to see them; a headless run doesn't).
    pub art: bool,
}

impl Plugin for Legions {
    fn build(&self, game: Game) -> Game {
        game.add(Phase::Input, take_orders)
            .add(Phase::AfterPhysics, aim.every(30))
            .add(Phase::AfterPhysics, fight().named("fight"))
            .add(Phase::AfterPhysics, resolve().named("resolve"))
            .add(Phase::AfterPhysics, keep_neighbors_close.every(60))
    }

    fn setup(&self, ctx: &mut Ctx) {
        let art = self.art.then(|| make_art(ctx));
        setup(ctx, self.units, art);
    }
}

// ---- the map ----------------------------------------------------------------------------

/// The map for `units` units, and the middle of each army's formation.
///
/// Rocks and lakes (walls) and mud in blobs, a border, and a clear field
/// where each army starts. The same seed makes the same map.
pub fn make_map(units: usize, seed: u64) -> (NavGrid, [Rect; 2]) {
    let scale = (units as f32 / UNITS_PER_MAP).max(1.0).sqrt();
    let size = (MAP.as_vec2() * scale).as_uvec2();
    let mut nav = NavGrid::new(size.x, size.y, CELL);
    let mut rng = Rng::new(seed);
    let area = scale * scale;
    let mut blob = |nav: &mut NavGrid, cost: u8, radius: f32| {
        let c = vec2(rng.range(0.0, size.x as f32), rng.range(0.0, size.y as f32));
        let r = radius * rng.range(0.6, 1.4);
        for y in (c.y - r).max(0.0) as u32..((c.y + r) as u32).min(size.y) {
            for x in (c.x - r).max(0.0) as u32..((c.x + r) as u32).min(size.x) {
                if vec2(x as f32, y as f32).distance_squared(c) < r * r {
                    nav.set_cost(x, y, cost);
                }
            }
        }
    };
    for _ in 0..(14.0 * area) as usize {
        blob(&mut nav, MUD, 18.0);
    }
    for _ in 0..(50.0 * area) as usize {
        blob(&mut nav, NavGrid::WALL, 6.0);
    }
    for _ in 0..(4.0 * area) as usize {
        blob(&mut nav, NavGrid::WALL, 14.0);
    }
    let world = size.as_vec2() * CELL;
    let zones = formations(units, world);
    for zone in zones {
        let (min, max) = ((zone.min / CELL).as_uvec2(), (zone.max / CELL).as_uvec2() + 2);
        for y in min.y.saturating_sub(2)..max.y.min(size.y) {
            for x in min.x.saturating_sub(2)..max.x.min(size.x) {
                nav.set_cost(x, y, 1);
            }
        }
    }
    for x in 0..size.x {
        nav.set_cost(x, 0, NavGrid::WALL);
        nav.set_cost(x, size.y - 1, NavGrid::WALL);
    }
    for y in 0..size.y {
        nav.set_cost(0, y, NavGrid::WALL);
        nav.set_cost(size.x - 1, y, NavGrid::WALL);
    }
    // If the blobs cut the armies off from each other, dig a road between them.
    let (a, b) = (zones[0].center(), zones[1].center());
    let mut flow = FlowField::new();
    flow.compute(&nav, &[b]);
    if flow.distance_at(a).is_none() {
        let y = (a.y / CELL) as u32;
        for x in 1..size.x - 1 {
            for y in y - 2..=y + 2 {
                let c = nav.cost(x, y).unwrap_or(1);
                nav.set_cost(x, y, c.clamp(1, MUD));
            }
        }
    }
    (nav, zones)
}

/// Where the armies start: two blocks facing each other across the middle
/// of a `world`-sized map, a few seconds' march apart.
pub fn formations(units: usize, world: Vec2) -> [Rect; 2] {
    let army = (units / 2).max(1) as f32;
    let height = world.y * 0.7;
    let depth = (army * SPACING * SPACING / height).max(SPACING) * 1.15;
    let gap = 320.0_f32.min(world.x * 0.3);
    let top = (world.y - height) / 2.0;
    let mid = world.x / 2.0;
    [
        Rect::new(mid - gap / 2.0 - depth, top, depth, height),
        Rect::new(mid + gap / 2.0, top, depth, height),
    ]
}

// ---- setup and rounds ----------------------------------------------------------------------

/// Builds the map and the first round. `art` gives units sprites.
pub fn setup(ctx: &mut Ctx, units: usize, art: Option<Art>) {
    let seed = ctx.random().stream(0).u64();
    let (nav, zones) = make_map(units, seed);
    ctx.world.reflect::<Unit>();
    ctx.world.reflect::<Arrow>();
    ctx.world.reflect::<Corpse>();
    ctx.world.reflect_resource::<Score>();
    let start = [zones[0].center(), zones[1].center()];
    ctx.world.insert_resource(Battle {
        nav,
        fields: [FlowField::new(), FlowField::new()],
        goal: [start[1], start[0]],
        stance: [Stance::Attack; 2],
        start,
        army: units / 2,
        field_ms: 0.0,
    });
    ctx.world.insert_resource(Score::default());
    ctx.world.insert_resource(Orders::default());
    ctx.world.insert_resource(Look(art));
    start_round(ctx);
}

/// Both armies at full strength in their formations, soldiers in front and
/// archers behind; the field cleared of the last round.
pub fn start_round(ctx: &mut Ctx) {
    let mut gone: Vec<Entity> = Vec::new();
    ctx.world.query::<(Entity, &Unit)>().for_each(|(e, _)| gone.push(e));
    ctx.world.query::<(Entity, &Arrow)>().for_each(|(e, _)| gone.push(e));
    ctx.world.query::<(Entity, &Corpse)>().for_each(|(e, _)| gone.push(e));
    for e in gone {
        ctx.world.despawn(e);
    }
    let round = {
        let s = ctx.world.resource_mut::<Score>();
        let won = s.rounds_won;
        *s = Score {
            round: s.round + 1,
            rounds_won: won,
            ..Score::default()
        };
        s.round
    };
    let mut rng = ctx.random().stream(u64::from(round));
    let art = ctx.world.resource::<Look>().0;
    let battle = ctx.world.resource_mut::<Battle>();
    let world = battle.world_size();
    let army = battle.army;
    let zones = formations(army * 2, world);
    let mut units = Vec::with_capacity(army * 2);
    for team in Team::BOTH {
        let zone = zones[team.index()];
        // Columns from the front line back: soldiers first, archers behind.
        let rows = ((zone.size().y / SPACING) as usize).max(1);
        let forward = if team == Team::Red { 1.0 } else { -1.0 };
        let front = if team == Team::Red { zone.max.x } else { zone.min.x };
        let mut placed = 0;
        let mut slot = 0;
        while placed < army {
            let (col, row) = (slot / rows, slot % rows);
            slot += 1;
            let jitter = vec2(rng.range(-0.8, 0.8), rng.range(-0.8, 0.8));
            let at = vec2(
                front - forward * (col as f32 + 0.5) * SPACING,
                zone.min.y + (row as f32 + 0.5) * SPACING,
            ) + jitter;
            if slot > army * 4 || battle.nav.walkable(at) {
                let kind = if placed >= army - army / ARCHER_EVERY {
                    Kind::Archer
                } else {
                    Kind::Soldier
                };
                units.push((team, kind, at));
                placed += 1;
            }
        }
    }
    battle.stance = [Stance::Attack; 2];
    let start = [zones[0].center(), zones[1].center()];
    battle.start = start;
    battle.set_goal(Team::Red, start[1]);
    battle.set_goal(Team::Blue, start[0]);
    match art {
        Some(art) => {
            ctx.world.spawn_batch(
                units
                    .into_iter()
                    .map(|(team, kind, at)| (unit_transform(at), new_unit(team, kind), unit_sprite(art, team, kind))),
            );
        }
        None => {
            ctx.world.spawn_batch(
                units
                    .into_iter()
                    .map(|(team, kind, at)| (unit_transform(at), new_unit(team, kind))),
            );
        }
    }
    let s = ctx.world.resource_mut::<Score>();
    s.alive = [army; 2];
    s.center = start;
}

fn unit_transform(at: Vec2) -> d2::Transform {
    d2::Transform {
        scale: Vec2::splat(0.75),
        ..d2::Transform::at(at)
    }
}

fn new_unit(team: Team, kind: Kind) -> Unit {
    Unit {
        team,
        kind,
        hp: kind.stats().hp,
        vel: Vec2::ZERO,
        cooldown: 0,
        swing: u8::MAX,
        engaged: false,
        push: Vec2::ZERO,
        act: Act::None,
    }
}

// ---- systems ---------------------------------------------------------------------------------

// ANCHOR: take_orders
/// First simulation system of every tick: applies the orders given since
/// the last tick and the replay's orders for this tick, and records them.
pub fn take_orders(ctx: &mut Ctx) {
    let tick = ctx.time.sim_tick();
    let mut orders = std::mem::take(&mut ctx.world.resource_mut::<Orders>().pending);
    if let Some(feed) = ctx.world.get_resource_mut::<Feed>() {
        while let Some(s) = feed.orders.get(feed.next).filter(|s| s.tick <= tick) {
            orders.push(s.order);
            feed.next += 1;
        }
    }
    for &order in &orders {
        if let Some(rec) = ctx.world.get_resource_mut::<Recorder>() {
            rec.orders.push(Stamped { tick, order });
        }
        apply(ctx, Team::Red, order);
    }
    orders.clear();
    // Hand the (empty) buffer back, so giving orders doesn't allocate.
    ctx.world.resource_mut::<Orders>().pending = orders;
}
// ANCHOR_END: take_orders

/// What an order does to `team`.
pub fn apply(ctx: &mut Ctx, team: Team, order: Order) {
    if order == Order::Restart {
        start_round(ctx);
        return;
    }
    let center = ctx.world.resource::<Score>().center;
    let b = ctx.world.resource_mut::<Battle>();
    match order {
        Order::Rally { x, y } => {
            let at = vec2(x, y);
            if b.nav.walkable(at) {
                b.stance[team.index()] = Stance::Rally;
                b.set_goal(team, at);
            }
        }
        Order::Attack => {
            b.stance[team.index()] = Stance::Attack;
            let at = center[team.enemy().index()];
            if b.nav.walkable(at) {
                b.set_goal(team, at);
            }
        }
        Order::Hold => b.stance[team.index()] = Stance::Hold,
        Order::Restart => {}
    }
}

/// Twice a second: an attacking army heads for the enemy unit nearest to
/// the enemy army's middle (so its goal is always a reachable spot).
/// Blue always attacks.
pub fn aim(ctx: &mut Ctx) {
    let center = ctx.world.resource::<Score>().center;
    let stance = ctx.world.resource::<Battle>().stance;
    let mut best = [(f32::INFINITY, None::<Vec2>); 2];
    ctx.world.query::<(&d2::Transform, &Unit)>().for_each(|(t, u)| {
        // Each army looks for the enemy nearest the enemy's middle.
        let hunter = u.team.enemy().index();
        let d = t.position.distance_squared(center[u.team.index()]);
        if d < best[hunter].0 {
            best[hunter] = (d, Some(t.position));
        }
    });
    let b = ctx.world.resource_mut::<Battle>();
    for team in Team::BOTH {
        let i = team.index();
        if stance[i] != Stance::Attack {
            continue;
        }
        if let Some(at) = best[i].1 {
            // Recompute only when the target moved more than a few cells.
            if at.distance_squared(b.goal[i]) > (3.0 * CELL) * (3.0 * CELL) {
                b.set_goal(team, at);
            }
        }
    }
}

/// Ticks an arrow takes from `from` to `to`.
pub fn flight_ticks(from: Vec2, to: Vec2) -> u16 {
    ((from.distance(to) / ARROW_SPEED) as u16).max(6)
}

/// A random offset in the square from -1 to 1. Plain arithmetic (no sine or
/// cosine, which differ between platforms' math libraries), so every
/// platform gets the same bits.
fn jitter(rng: &mut Rng) -> Vec2 {
    vec2(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0))
}

/// Every tick, on all cores: each unit finds its target and moves; then
/// each arrow flies and, landing, finds whom it hit.
///
/// Soldiers look for enemies every tick while fighting and on their turn
/// (every 4th tick) while marching. Archers look when ready to shoot, and
/// on their turn (every 8th tick). Crowding is refreshed every other tick.
pub fn fight() -> impl FnMut(&mut Ctx) {
    // Per army, rebuilt every tick: a fine grid (friends crowding a unit,
    // enemies in its reach: only the closest few matter) and a coarse one
    // (enemies seen from afar).
    let mut near = [
        SpatialGrid::new(PERSONAL_SPACE * 2.0),
        SpatialGrid::new(PERSONAL_SPACE * 2.0),
    ];
    // The enemy grids carry each unit's velocity, so archers can lead
    // their targets.
    let mut far = [SpatialGrid::with_data(SIGHT), SpatialGrid::with_data(SIGHT)];
    let mut positions: Vec<(Entity, Vec2, Team, Vec2)> = Vec::new();
    move |ctx: &mut Ctx| {
        let (dt, random) = (ctx.time.dt(), ctx.random());
        let (soldier_turn, archer_turn) = (ctx.time.turns(4), ctx.time.turns(8));
        let crowding = ctx.time.sim_tick().is_multiple_of(2);
        if ctx.world.resource::<Score>().outcome.is_some() {
            return;
        }
        ctx.world
            .query::<(Entity, &d2::Transform, &Unit)>()
            .par_collect_into(&mut positions, |(e, t, u)| (e, t.position, u.team, u.vel));
        let [near_red, near_blue] = &mut near;
        let [far_red, far_blue] = &mut far;
        let all = &positions;
        let of = |team: Team| all.iter().filter(move |p| p.2 == team).map(|&(e, at, _, _)| (e, at));
        let moving = |team: Team| {
            all.iter()
                .filter(move |p| p.2 == team)
                .map(|&(e, at, _, vel)| (e, at, vel))
        };
        rayon::scope(|s| {
            s.spawn(|_| near_red.rebuild_from(of(Team::Red)));
            s.spawn(|_| near_blue.rebuild_from(of(Team::Blue)));
            s.spawn(|_| far_red.rebuild_with_data(moving(Team::Red)));
            s.spawn(|_| far_blue.rebuild_with_data(moving(Team::Blue)));
        });
        let battle = ctx.world.remove_resource::<Battle>().expect("the battle is set up");
        let (near, far, b) = (&near, &far, &battle);
        ctx.world
            .query::<(Entity, &mut d2::Transform, &mut Unit)>()
            .par_for_each(|(e, t, unit)| {
                let mut rng = random.of(e);
                let stats = unit.kind.stats();
                let me = t.position;
                let team = unit.team.index();
                unit.act = Act::None;
                unit.cooldown = unit.cooldown.saturating_sub(1);
                unit.swing = unit.swing.saturating_add(1);
                if crowding {
                    let mut push = Vec2::ZERO;
                    near[team].for_each_in_radius(me, PERSONAL_SPACE, |other| {
                        if other.entity != e {
                            let away = me - other.position;
                            push += away / (away.length_squared() + 0.01);
                        }
                    });
                    unit.push = push.clamp_length_max(1.0);
                }
                let enemies = &far[unit.team.enemy().index()];
                let mut halt = false;
                let mut charge = None;
                match unit.kind {
                    Kind::Soldier => {
                        if unit.engaged || soldier_turn.includes(e) {
                            // In reach, on the fine grid; else in sight, on the coarse one.
                            let seen = near[unit.team.enemy().index()]
                                .nearest(me, stats.reach, |_| true)
                                .map(|g| (g.entity, g.position))
                                .or_else(|| enemies.nearest(me, SIGHT, |_| true).map(|g| (g.entity, g.position)));
                            unit.engaged = seen.is_some();
                            if let Some((enemy, at)) = seen {
                                if at.distance_squared(me) < stats.reach * stats.reach {
                                    halt = true;
                                    if unit.cooldown == 0 {
                                        unit.cooldown = stats.cooldown + rng.below(8) as u16;
                                        unit.swing = 0;
                                        unit.act = Act::Hit(enemy);
                                    }
                                } else {
                                    charge = Some(at);
                                }
                            }
                        }
                    }
                    Kind::Archer => {
                        let ready = unit.cooldown == 0;
                        if (ready && unit.engaged) || archer_turn.includes(e) {
                            let seen = enemies.nearest(me, stats.reach, |_| true);
                            unit.engaged = seen.is_some();
                            if let (Some(enemy), true) = (seen, ready) {
                                unit.cooldown = stats.cooldown + rng.below(20) as u16;
                                unit.swing = 0;
                                // Aim where the target will be when the arrow lands.
                                let flight = flight_ticks(me, enemy.position);
                                let lead = enemy.data * (f32::from(flight) * dt);
                                unit.act = Act::Shoot(enemy.position + lead + jitter(&mut rng) * 1.5);
                            }
                        }
                        halt = unit.engaged;
                    }
                }
                let mut want = if halt {
                    Vec2::ZERO
                } else if let Some(at) = charge {
                    (at - me).normalize_or_zero() * stats.speed
                } else if b.stance[team] == Stance::Hold {
                    Vec2::ZERO
                } else {
                    b.fields[team].direction_at(me) * stats.speed
                };
                let slow = b.nav.cell_at(me).and_then(|c| b.nav.cost(c.x, c.y)).unwrap_or(1).max(1) as f32;
                want = want / slow + unit.push * stats.speed;
                unit.vel = unit.vel.lerp(want, 0.2) + jitter(&mut rng) * 1.5;
                // Slide along walls instead of walking into them.
                let next = me + unit.vel * dt;
                if b.nav.walkable(next) {
                    t.position = next;
                } else if b.nav.walkable(vec2(next.x, me.y)) {
                    t.position.x = next.x;
                } else if b.nav.walkable(vec2(me.x, next.y)) {
                    t.position.y = next.y;
                }
            });
        ctx.world
            .query::<(&mut d2::Transform, &mut Arrow)>()
            .par_for_each(|(t, arrow)| {
                arrow.tick += 1;
                let along = f32::from(arrow.tick) / f32::from(arrow.flight);
                t.position = arrow.from + (arrow.to - arrow.from) * along;
                if arrow.tick >= arrow.flight {
                    arrow.hit = far[arrow.team.enemy().index()]
                        .nearest(arrow.to, ARROW_SPLASH, |_| true)
                        .map(|g| g.entity);
                }
            });
        ctx.world.insert_resource(battle);
    }
}

/// Applies this tick's hits in order (so the result doesn't depend on
/// thread timing), looses the arrows, removes the dead and the landed
/// arrows, and ends and restarts rounds.
pub fn resolve() -> impl FnMut(&mut Ctx) {
    let mut hits: Vec<(Entity, f32)> = Vec::new();
    let mut shots: Vec<(Team, Vec2, Vec2)> = Vec::new();
    move |ctx: &mut Ctx| {
        hits.clear();
        shots.clear();
        ctx.world
            .query::<(&d2::Transform, &Unit)>()
            .for_each(|(t, u)| match u.act {
                Act::Hit(target) => hits.push((target, u.kind.stats().damage)),
                Act::Shoot(at) => shots.push((u.team, t.position, at)),
                Act::None => {}
            });
        let arrow_damage = Kind::Archer.stats().damage;
        let cmds = &mut ctx.commands;
        let mut flying = 0;
        let mut arrow_hits = 0;
        ctx.world.query::<(Entity, &Arrow)>().for_each(|(e, a)| {
            if a.tick < a.flight {
                flying += 1;
            } else {
                if let Some(target) = a.hit {
                    hits.push((target, arrow_damage));
                    arrow_hits += 1;
                }
                cmds.despawn(e);
            }
        });
        for &(target, damage) in &hits {
            if let Some(u) = ctx.world.get_mut::<Unit>(target) {
                u.hp -= damage;
            }
        }
        let art = ctx.world.resource::<Look>().0;
        let mut alive = [0usize; 2];
        let mut sum = [Vec2::ZERO; 2];
        let mut fallen = [0u32; 2];
        let cmds = &mut ctx.commands;
        ctx.world
            .query::<(Entity, &d2::Transform, &Unit)>()
            .for_each(|(e, t, u)| {
                let team = u.team.index();
                if u.hp <= 0.0 {
                    cmds.despawn(e);
                    fallen[team] += 1;
                    let corpse = Corpse {
                        team: u.team,
                        kind: u.kind,
                        ttl: CORPSE_TICKS,
                    };
                    let at = unit_transform(t.position);
                    match art {
                        Some(art) => cmds.spawn((at, corpse, corpse_sprite(art, u.team))),
                        None => cmds.spawn((at, corpse)),
                    };
                } else {
                    alive[team] += 1;
                    sum[team] += t.position;
                }
            });
        ctx.world.query::<(Entity, &mut Corpse)>().for_each(|(e, c)| {
            c.ttl -= 1;
            if c.ttl == 0 {
                cmds.despawn(e);
            }
        });
        for &(team, from, to) in &shots {
            let flight = flight_ticks(from, to);
            let arrow = Arrow {
                team,
                from,
                to,
                tick: 0,
                flight,
                hit: None,
            };
            // The angle is only for the picture: nothing in the game reads it.
            let at = d2::Transform {
                rotation: (to - from).to_angle(),
                ..d2::Transform::at(from)
            };
            match art {
                Some(art) => cmds.spawn((at, arrow, arrow_sprite(art))),
                None => cmds.spawn((at, arrow)),
            };
        }
        let s = ctx.world.resource_mut::<Score>();
        s.alive = alive;
        s.arrows = flying + shots.len();
        s.arrow_hits += arrow_hits;
        for team in 0..2 {
            s.fallen[team] += fallen[team];
            if alive[team] > 0 {
                s.center[team] = sum[team] / alive[team] as f32;
            }
        }
        s.round_tick += 1;
        match s.outcome {
            None if alive[0] == 0 || alive[1] == 0 || s.round_tick >= ROUND_TICKS => {
                let winner = match alive[0].cmp(&alive[1]) {
                    std::cmp::Ordering::Greater => Some(Team::Red),
                    std::cmp::Ordering::Less => Some(Team::Blue),
                    std::cmp::Ordering::Equal => None,
                };
                if let Some(team) = winner {
                    s.rounds_won[team.index()] += 1;
                }
                s.outcome = Some(winner);
                s.restart_in = RESTART_TICKS;
            }
            Some(_) => {
                s.restart_in = s.restart_in.saturating_sub(1);
                if s.restart_in == 0 {
                    start_round(ctx);
                }
            }
            None => {}
        }
    }
}

/// Once a second, everything is reordered in memory by where it stands, so
/// neighbors on the map are neighbors in memory: grid rebuilds, flow field
/// lookups and searches then mostly hit the cache.
pub fn keep_neighbors_close(ctx: &mut Ctx) {
    ctx.world
        .sort_by::<d2::Transform>(|t| ponko::spatial::spatial_key(t.position, 64.0));
}

/// Last simulation system: writes the replay every `every` ticks, so a
/// crash or a closed window loses at most that much.
pub fn record_checkpoint(ctx: &mut Ctx) {
    let ticks = ctx.time.sim_tick() + 1;
    let every = ctx.world.get_resource::<Recorder>().map_or(0, |r| r.every);
    if every > 0 && ticks.is_multiple_of(every) {
        write_recording(ctx, ticks);
    }
}

/// Writes the recorder's replay: its orders, `ticks` ticks, and the hash of
/// the world now (which must be the world after `ticks` ticks).
pub fn write_recording(ctx: &mut Ctx, ticks: u64) {
    let hash = world_hash(&ctx.world);
    let Some(rec) = ctx.world.get_resource::<Recorder>() else {
        return;
    };
    let replay = Replay {
        format: REPLAY_FORMAT.into(),
        version: REPLAY_VERSION,
        seed: rec.seed,
        units: rec.units,
        ticks,
        orders: rec.orders.clone(),
        hash: hash_hex(hash),
    };
    if let Err(e) = replay.save(&rec.path) {
        eprintln!("legions: could not write the replay: {e}");
    }
}

// ---- controls ---------------------------------------------------------------------------------

/// Mouse and keys to orders: a frame system.
pub struct Controls;

impl Plugin for Controls {
    fn build(&self, game: Game) -> Game {
        game.add(Phase::Frame, controls)
    }
}

/// Left click: rally there. A: attack. H: hold. R: a new round. Space:
/// pause. 1-3: speed. Escape: quit (writing the recording, if any).
pub fn controls(ctx: &mut Ctx) {
    let mut order = None;
    if ctx.input.mouse_pressed(MouseButton::Left) && !ctx.ui.wants_pointer() {
        let at = ctx.mouse_world();
        order = Some(Order::Rally { x: at.x, y: at.y });
    } else if ctx.input.key_pressed(Key::A) {
        order = Some(Order::Attack);
    } else if ctx.input.key_pressed(Key::H) {
        order = Some(Order::Hold);
    } else if ctx.input.key_pressed(Key::R) {
        order = Some(Order::Restart);
    }
    // While a replay plays, its orders are the only ones.
    if let (Some(order), false) = (order, ctx.world.get_resource::<Feed>().is_some()) {
        ctx.world.resource_mut::<Orders>().pending.push(order);
    }
    if ctx.input.key_pressed(Key::Space) {
        if ctx.time.is_paused() {
            ctx.time.resume();
        } else {
            ctx.time.pause();
        }
    }
    for (key, speed) in [(Key::Num1, 1.0), (Key::Num2, 2.0), (Key::Num3, 4.0)] {
        if ctx.input.key_pressed(key) {
            ctx.time.set_speed(speed);
        }
    }
    if ctx.input.key_pressed(Key::Escape) {
        // Every tick so far has run: the world is the one after them.
        let ticks = ctx.time.sim_tick();
        write_recording(ctx, ticks);
        ctx.quit();
    }
}

// ---- the view ---------------------------------------------------------------------------------

/// Frame size of the sprite sheet, in pixels.
pub const FRAME: u32 = 8;
/// Columns of the sheet: four walking frames, standing, two of attacking.
pub const COLUMNS: u32 = 8;
/// Rows: red soldier, red archer, blue soldier, blue archer, then the arrow
/// and the fallen.
pub const ROWS: u32 = 5;
const STAND: u32 = 4;
const WIND_UP: u32 = 5;
const STRIKE: u32 = 6;

fn row(team: Team, kind: Kind) -> u32 {
    team.index() as u32 * 2 + kind as u32
}

fn unit_sprite(art: Art, team: Team, kind: Kind) -> Sprite {
    art.sheet.sprite(row(team, kind) * COLUMNS + STAND)
}

fn arrow_sprite(art: Art) -> Sprite {
    let mut sprite = art.sheet.sprite(4 * COLUMNS);
    sprite.z = 2.0;
    sprite
}

fn corpse_sprite(art: Art, team: Team) -> Sprite {
    let mut sprite = art.sheet.sprite(4 * COLUMNS + 1 + team.index() as u32);
    sprite.z = -1.0;
    sprite
}

/// The sprite sheet's pixels (`COLUMNS * FRAME` by `ROWS * FRAME`, RGBA):
/// little figures drawn by code, so the game ships no picture files.
pub fn sheet_pixels() -> (u32, u32, Vec<u8>) {
    let (w, h) = (COLUMNS * FRAME, ROWS * FRAME);
    let mut px = vec![0u8; (w * h * 4) as usize];
    let mut put = |x: i32, y: i32, c: [u8; 4]| {
        if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
            px[((y as u32 * w + x as u32) * 4) as usize..][..4].copy_from_slice(&c);
        }
    };
    let dark = [24, 20, 28, 255];
    let skin = [240, 214, 180, 255];
    let wood = [150, 104, 56, 255];
    let steel = [220, 226, 236, 255];
    for team in Team::BOTH {
        let body = team.rgba8();
        for kind in [Kind::Soldier, Kind::Archer] {
            let oy = (row(team, kind) * FRAME) as i32;
            for col in 0..7 {
                let ox = (col * FRAME) as i32;
                // Legs: apart and together in turn while walking.
                let stride = match col {
                    0 => 1,
                    2 => -1,
                    _ => 0,
                };
                put(ox + 3 - stride.max(0), oy + 7, dark);
                put(ox + 4 + (-stride).max(0), oy + 7, dark);
                // Body and head.
                for y in 3..7 {
                    for x in 2..6 {
                        put(ox + x, oy + y, body);
                    }
                }
                put(ox + 3, oy + 1, skin);
                put(ox + 4, oy + 1, skin);
                put(ox + 3, oy + 2, skin);
                put(ox + 4, oy + 2, skin);
                match kind {
                    Kind::Soldier => {
                        // A sword: raised to wind up, forward to strike.
                        let blade: &[(i32, i32)] = match col {
                            WIND_UP_COL => &[(6, 2), (6, 1), (6, 0)],
                            STRIKE_COL => &[(6, 4), (7, 4), (7, 3)],
                            _ => &[(6, 3), (6, 2), (6, 1)],
                        };
                        for &(x, y) in blade {
                            put(ox + x, oy + y, steel);
                        }
                    }
                    Kind::Archer => {
                        // A bow: drawn back to wind up.
                        let bow: &[(i32, i32)] = match col {
                            WIND_UP_COL => &[(7, 2), (6, 3), (6, 4), (7, 5)],
                            _ => &[(6, 2), (7, 3), (7, 4), (6, 5)],
                        };
                        for &(x, y) in bow {
                            put(ox + x, oy + y, wood);
                        }
                    }
                }
            }
        }
    }
    // The arrow: a shaft pointing right.
    let oy = (4 * FRAME) as i32;
    for x in 1..7 {
        put(x, oy + 4, wood);
    }
    put(7, oy + 4, steel);
    put(0, oy + 3, [240, 240, 240, 255]);
    put(0, oy + 5, [240, 240, 240, 255]);
    // The fallen: a body lying down, in each team's colour.
    for team in Team::BOTH {
        let ox = ((1 + team.index() as u32) * FRAME) as i32;
        let mut body = team.rgba8();
        for c in &mut body[..3] {
            *c /= 2;
        }
        for x in 1..6 {
            put(ox + x, oy + 4, body);
            put(ox + x, oy + 5, body);
        }
        put(ox + 6, oy + 4, [150, 130, 110, 255]);
    }
    (w, h, px)
}

const WIND_UP_COL: u32 = WIND_UP;
const STRIKE_COL: u32 = STRIKE;

/// Creates the sprite sheet.
pub fn make_art(ctx: &mut Ctx) -> Art {
    let (w, h, px) = sheet_pixels();
    let texture = ctx.gfx.create_texture(w, h, &px);
    ctx.gfx.set_texture_filter(texture, TextureFilter::Nearest);
    Art {
        sheet: SpriteSheet::new(texture, FRAME, FRAME),
    }
}

/// The ground's pixels, one per nav cell (`nav.size()`, RGBA): grass, mud
/// and rock.
pub fn ground_pixels(nav: &NavGrid) -> Vec<u8> {
    let size = nav.size();
    let mut pixels = vec![0u8; (size.x * size.y * 4) as usize];
    for y in 0..size.y {
        for x in 0..size.x {
            // A little per-cell noise so the ground doesn't look flat.
            let n = ((x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) % 9) as u8;
            let px = match nav.cost(x, y).unwrap_or(NavGrid::WALL) {
                NavGrid::WALL => [52 + n, 56 + n, 66 + n, 255],
                c if c > 1 => [74 + n, 56 + n, 34, 255],
                _ => [30 + n, 46 + n, 32, 255],
            };
            pixels[((y * size.x + x) * 4) as usize..][..4].copy_from_slice(&px);
        }
    }
    pixels
}

/// The ground texture, made once at startup.
#[derive(Clone, Copy)]
pub struct Ground(pub Texture);

/// The animation, the ground, the rally point and the HUD.
pub struct View;

impl Plugin for View {
    fn build(&self, game: Game) -> Game {
        game.add(Phase::Late, animate)
            .add(Phase::Draw, draw_ground)
            .add(Phase::Draw, hud)
            .system(ponko::camera::camera_controls)
    }

    fn setup(&self, ctx: &mut Ctx) {
        let (size, pixels, world) = {
            let b = ctx.world.resource::<Battle>();
            (b.nav.size(), ground_pixels(&b.nav), b.world_size())
        };
        let tex = ctx.gfx.create_texture(size.x, size.y, &pixels);
        ctx.gfx.set_texture_filter(tex, TextureFilter::Nearest);
        ctx.world.insert_resource(Ground(tex));
        ctx.camera.zoom = (ctx.camera.viewport() / world).min_element();
        ctx.camera.look_at(world / 2.0);
    }
}

/// Which frame of its row a unit shows: striking or winding up just after
/// an attack, walking while it moves (each unit at its own step), else
/// standing. A pure function of the unit and the tick.
pub fn unit_frame(unit: &Unit, entity: Entity, tick: u64) -> u32 {
    match unit.swing {
        0..6 => STRIKE,
        6..14 => WIND_UP,
        _ if unit.vel.length_squared() > 16.0 => ((tick / 6 + u64::from(entity.index())) % 4) as u32,
        _ => STAND,
    }
}

/// Frame system, on all cores: every unit's sprite shows its frame; the
/// fallen fade out.
pub fn animate(ctx: &mut Ctx) {
    let Some(art) = ctx.world.resource::<Look>().0 else {
        return;
    };
    let tick = ctx.time.sim_tick();
    ctx.world
        .query::<(Entity, &Unit, &mut Sprite)>()
        .par_for_each(|(e, u, sprite)| {
            let frame = row(u.team, u.kind) * COLUMNS + unit_frame(u, e, tick);
            sprite.region = Some(art.sheet.frame(frame));
        });
    ctx.world.query::<(&Corpse, &mut Sprite)>().par_for_each(|(c, sprite)| {
        sprite.color = Color::WHITE.with_alpha(f32::from(c.ttl) / f32::from(CORPSE_TICKS));
    });
}

/// Draw system: the ground under everything and the red army's goal.
pub fn draw_ground(ctx: &mut Ctx) {
    let Some(&Ground(tex)) = ctx.world.get_resource::<Ground>() else {
        return;
    };
    ctx.gfx.clear(Color::BLACK);
    let (world, goal, stance) = {
        let b = ctx.world.resource::<Battle>();
        (b.world_size(), b.goal[0], b.stance[0])
    };
    ctx.gfx.draw_ex(
        tex,
        world / 2.0,
        DrawParams::default().with_scale(Vec2::splat(CELL)).with_z(-10.0),
    );
    if stance == Stance::Rally {
        ctx.gfx.draw_circle(goal, 6.0, Color::rgba(1.0, 0.9, 0.3, 0.9));
    }
}

/// Draw system: the score, the timings and the keys.
pub fn hud(ctx: &mut Ctx) {
    let s = ctx.world.resource::<Score>().clone();
    let (field_ms, stance) = {
        let b = ctx.world.resource::<Battle>();
        (b.field_ms, b.stance[0])
    };
    let sim_ms: f32 = ctx
        .perf
        .system_timings()
        .iter()
        .filter(|t| t.sim)
        .map(|t| t.avg_ms)
        .sum();
    let speed = if ctx.time.is_paused() {
        "paused".to_string()
    } else {
        format!("{}x", ctx.time.speed())
    };
    let recording = if ctx.world.get_resource::<Recorder>().is_some() {
        "  REC"
    } else if ctx.world.get_resource::<Feed>().is_some() {
        "  REPLAY"
    } else {
        ""
    };
    let panel = Rect::new(12.0, 12.0, 330.0, 170.0);
    ctx.immediate_ui().panel(panel);
    let at = panel.min + ctx.ui.immediate.style.padding;
    let text = format!(
        "round {}  red {} vs blue {}  (won {}-{})\nfallen {} / {}, arrows in flight {} ({} hit)\nred: {stance:?}{recording}\nsim {sim_ms:.2} ms/tick, {:.0} FPS, {speed}\nfield {field_ms:.2} ms\nclick: rally  A: attack  H: hold\nR: new round  1-3: speed  Esc: quit",
        s.round,
        s.alive[0],
        s.alive[1],
        s.rounds_won[0],
        s.rounds_won[1],
        s.fallen[0],
        s.fallen[1],
        s.arrows,
        s.arrow_hits,
        ctx.time.fps()
    );
    ctx.immediate_ui().label(at, &text);
    if let Some(outcome) = s.outcome {
        let (msg, color) = match outcome {
            Some(Team::Red) => ("Red wins! A new round begins soon", Team::Red.color()),
            Some(Team::Blue) => ("Blue wins. A new round begins soon", Team::Blue.color()),
            None => ("A draw. A new round begins soon", Color::WHITE),
        };
        let center = ctx.window_size() / 2.0;
        let size = ctx.gfx.measure_text(msg, 32.0);
        ctx.gfx.draw_text(msg, center - size / 2.0, 32.0, color);
    }
}
