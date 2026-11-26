//! The game: rules, input mapping and drawing. Shared with `tests/coin_dash.rs`.
//!
//! Three layers, each a [`Plugin`] and testable on its own:
//!
//! 1. [`PlayerInput`]: `read_input` (frame system) turns keys and gamepad
//!    into an [`Intent`].
//! 2. [`Rules`]: `rules` (sim system) is the whole game: it reads only the
//!    `Intent` and the world, so a test can replace step 1 with a script and
//!    get the same result on every run.
//! 3. [`Drawing`]: entities with a `Sprite` are drawn by the engine's
//!    built-in `draw_sprites`; `hud` adds the text. Neither changes the game.
//!
//! [`SettingsScreen`] is a fourth plugin beside them, and [`CoinDash`] adds
//! all four.
//!
//! The game moves through five [`Screen`]s, the engine's game states
//! (`ponko::state`): `Title` → `Playing` ⇄ `Paused` (pushed over it),
//! `Playing` → `Result` → `Playing` again (or back to `Title`), and
//! `Title` ⇄ `Settings`. Only `rules` asks for a new screen; entering
//! `Playing` starts a round and entering `Title` clears the field. The best time and
//! most coins are kept in a small [`Progress`] file, and the player's choices
//! (UI scale, volume, rebound controls, language) in a [`settings::Settings`]
//! file; both survive a missing or damaged file.
//!
//! The arena is fitted into the window's safe area (letterboxed), and the
//! text is placed in it, every frame: a phone upright or turned, with a
//! notch or a home bar, shows the whole arena and every line (see `fit2d`).
//! On a touch screen the hints name taps and the on-screen buttons.
//!
//! Every text the player reads comes from the string tables in
//! `assets/coin_dash/lang` (English and Arabic, see [`LANGUAGES`]), through
//! the engine's `ponko::locale`: the settings screen's labels hold keys and
//! follow the language by themselves, and `hud` reads [`Strings`] as it draws.

// The test file includes this module but needs only part of it.
#![allow(dead_code)]

// Next to this file, whichever crate includes it (`main.rs` or `tests/coin_dash.rs`).
#[path = "settings.rs"]
pub mod settings;
#[path = "smoke.rs"]
pub mod smoke;
// Shared with Ember Hop: the play area and the text for any window.
#[path = "../common/fit2d.rs"]
pub mod fit2d;

#[allow(unused_imports)]
use ponko::d2::{Sprite, Transform};
use ponko::locale::{Arg, Strings};
use ponko::prelude::*;
use ponko::state::State;
use ponko::storage::Storage;
use ponko::ui::{UiTheme, UiTransition};
use serde::{Deserialize, Serialize};
use settings::Settings;

/// Size of the playfield, in world units (the window starts the same size;
/// another window shows it whole, as large as fits).
pub const ARENA: Vec2 = vec2(800.0, 600.0);
/// Coins on the field at the start of a round.
pub const COINS: usize = 8;
/// Seconds to collect them all.
pub const ROUND_SECONDS: f32 = 20.0;
/// Player speed, units per second.
pub const SPEED: f32 = 240.0;
pub const PLAYER_RADIUS: f32 = 14.0;
pub const COIN_RADIUS: f32 = 10.0;

/// The player. Position is the entity's [`Transform`].
#[derive(Component)]
pub struct Player;
/// A coin to collect.
#[derive(Component)]
pub struct Coin;

/// The pictures, loaded once at startup from `assets/coin_dash`: the player,
/// the coin and the grass field. Pixel art, drawn at twice its size.
#[derive(Clone, Copy)]
pub struct Art {
    player: Texture,
    pub coin: Texture,
    field: Texture,
}

/// The coin's file, relative to `assets/`. The packaged game finds it next to
/// the executable, wherever it was unpacked.
pub const COIN_ASSET: &str = "coin_dash/coin.png";

/// The sound effects and the music, loaded once at startup.
#[derive(Clone, Copy)]
pub struct Sounds {
    pickup: Sound,
    win: Sound,
    lose: Sound,
    select: Sound,
}

/// The music while it plays: its volume follows the settings.
#[derive(Clone, Copy)]
pub struct Music(Playing);

/// The music's share of the volume setting.
const MUSIC_VOLUME: f32 = 0.4;

/// Which screen the game is on: the game's state, so tests and tools can
/// read it ([`screen`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Screen {
    /// The menu shown at startup: best results, "press Enter".
    Title,
    /// UI scale, volume and controls, drawn by the retained UI in `settings`.
    Settings,
    /// A round is running.
    Playing,
    /// A round is frozen, pushed over `Playing`. Nothing moves and the clock stops.
    Paused,
    /// The round ended; waiting for "play again" or "back to title".
    Result(Outcome),
}

/// How a round ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Won,
    Lost,
}

/// Round bookkeeping. One resource, so tests and tools can read it directly.
#[derive(Debug, Clone)]
pub struct Round {
    pub collected: usize,
    pub time_left: f32,
    /// 0 before the first round, then 1 for the first round, +1 for every new one.
    pub number: u32,
    /// The finished round beat a saved best (set when it ends).
    pub new_best: bool,
}

/// What the player asked for since the last simulation tick. The sim reads
/// this, never the keyboard, so a test can write it directly.
#[derive(Debug, Default, Clone, Copy)]
pub struct Intent {
    /// Movement, each axis in `-1..=1`; y is positive down.
    pub direction: Vec2,
    // The buttons are latched until the sim takes them, so a tap between two
    // ticks (a frame with no tick) is not lost.
    /// Start, resume, play again.
    pub confirm: bool,
    /// Start a new round right now.
    pub restart: bool,
    /// Pause or resume; on the title screen, quit.
    pub pause: bool,
    /// Back to the title screen; on the title screen, quit.
    pub back: bool,
    /// Open the settings from the title screen.
    pub settings: bool,
    /// Leave the settings (its Back button or the Escape key).
    pub close_settings: bool,
}

// ANCHOR: controls
actions! {
    /// The action names from [`bindings`], resolved once at startup. Queries by
    /// `ActionId` are an array index, and a name that `bindings` doesn't define
    /// is logged once when resolved instead of silently reading false forever.
    pub struct Act {
        movement = "move",
        confirm = "confirm",
        restart = "restart",
        pause = "pause",
        back = "back",
        settings = "settings",
    }
}
// ANCHOR_END: controls

/// Random seed for coin layouts: the same seed and the same inputs always give
/// the same game.
pub const SEED: u64 = 2024;

/// The string tables, English (the reference) first. Arabic needs the
/// engine's `text-shaping` feature to be shown, see `settings::languages`.
pub const LANGUAGES: [&str; 2] = ["coin_dash/lang/en.json", "coin_dash/lang/ar.json"];

/// The game's string tables, loaded by [`SettingsScreen`]'s setup.
pub fn strings(world: &World) -> &Strings {
    world.resource::<Strings>()
}

/// The name of the game's folder in the player's data folder, where
/// `ctx.storage` keeps [`PROGRESS_FILE`] and the settings.
pub const APP_ID: &str = "coin-dash";

/// The game without a window, ready for `run` or `run_headless`: the
/// [`CoinDash`] plugin with progress and settings in the player's data
/// folder (`PONKO_DATA_DIR` moves it, handy for tests and smoke runs).
pub fn build() -> Game {
    Game::new("Coin Dash", ARENA.x as u32, ARENA.y as u32)
        .seed(SEED)
        .app_id(APP_ID)
        .plugin(CoinDash)
}

// ANCHOR: plugins
/// The whole game: its layers, each a plugin of its own. Startup is the
/// plugins' `setup`, in this order. Progress and settings are kept in
/// `ctx.storage`; a game without an app id (or a data folder) just doesn't
/// keep them.
pub struct CoinDash;

impl Plugin for CoinDash {
    fn build(&self, game: Game) -> Game {
        game.plugin(PlayerInput)
            .plugin(SettingsScreen)
            .plugin(Rules)
            .plugin(Drawing)
    }
}

/// Layer 1: keys and gamepad to an [`Intent`], once per rendered frame.
pub struct PlayerInput;

impl Plugin for PlayerInput {
    fn build(&self, game: Game) -> Game {
        game.add(Phase::Frame, read_input)
    }
    fn setup(&self, ctx: &mut Ctx) {
        // The default controls; the ids stay valid when the settings (or a
        // rebinding) replace them.
        ctx.input.set_actions(bindings());
        ctx.world.insert_resource(Act::resolve(ctx));
        ctx.world.insert_resource(Intent::default());
        ctx.world.insert_resource(Hints::default());
        ctx.input.set_touch_controls(touch_controls());
    }
}

/// Which hints the screens show: taps and the on-screen buttons while the
/// on-screen controls are up, else keys. Kept by `read_input`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Hints {
    pub touch: bool,
}

// ANCHOR: touch
/// The on-screen controls, shown on touch screens only: a stick to move,
/// pause in the corner while a round is on, settings on the title. A tap
/// anywhere else confirms (see [`settings::bindings_for`]).
pub fn touch_controls() -> ponko::ui::touch::TouchControls {
    ponko::ui::touch::TouchControls::new()
        .stick("move")
        .corner_button("pause", "II")
        .corner_button("settings", "Set")
}
// ANCHOR_END: touch

/// The player's choices (UI scale, volume, controls): loaded at startup,
/// changed on the settings screen.
pub struct SettingsScreen;

impl Plugin for SettingsScreen {
    fn build(&self, game: Game) -> Game {
        game.add(Phase::Frame, settings::settings_ui)
    }
    fn setup(&self, ctx: &mut Ctx) {
        for table in LANGUAGES {
            ctx.assets.load_strings(&mut ctx.world, table);
        }
        let (loaded, load) = Settings::load(&mut ctx.storage);
        ctx.input.set_actions(settings::bindings_for(&loaded.controls));
        ctx.ui.set_scale(loaded.ui_scale);
        // The settings screen's controls ease between their states.
        ctx.ui.set_theme(UiTheme {
            transition: UiTransition::new(0.12),
            press_scale: 0.97,
            ..*ctx.ui.theme()
        });
        settings::apply_language(&mut ctx.world, &loaded);
        ctx.world.insert_resource(loaded);
        ctx.world.insert_resource(settings::SettingsFile {
            load,
            save_failed: false,
        });
    }
}

/// Layer 2: the rules, on the fixed tick, the screens and the resources
/// they own. The game starts on the title screen with the progress loaded
/// from `ctx.storage`.
pub struct Rules;

impl Plugin for Rules {
    fn build(&self, game: Game) -> Game {
        game.state(Screen::Title)
            .on_enter(Screen::Title, |ctx: &mut Ctx| ctx.world.clear())
            .on_enter(Screen::Playing, new_round)
            .add(Phase::AfterPhysics, rules)
    }
    fn setup(&self, ctx: &mut Ctx) {
        ctx.world.insert_resource(Round {
            collected: 0,
            time_left: ROUND_SECONDS,
            number: 0,
            new_best: false,
        });
        let (progress, load) = Progress::load(&mut ctx.storage);
        ctx.world.insert_resource(progress);
        ctx.world.insert_resource(SaveFile {
            load,
            save_failed: false,
        });
    }
}

/// Layer 3: the pictures, the text and the sound effects. The sprites
/// themselves are drawn by the engine's built-in sprite system.
pub struct Drawing;

impl Plugin for Drawing {
    fn build(&self, game: Game) -> Game {
        game.add(Phase::Frame, hud)
    }
    fn setup(&self, ctx: &mut Ctx) {
        ctx.world.insert_resource(MenuFade::default());
        ctx.gfx.clear(Color::hex(0x14181f));
        let mut picture = |path: &str| {
            let texture = ctx.gfx.load_texture(path);
            ctx.gfx.set_texture_filter(texture, TextureFilter::Nearest);
            texture
        };
        let art = Art {
            player: picture("coin_dash/player.png"),
            coin: picture(COIN_ASSET),
            field: picture("coin_dash/field.png"),
        };
        ctx.world.insert_resource(art);
        let mut sound = |name: &str| ctx.audio.load(&format!("coin_dash/sounds/{name}.ogg"));
        let sounds = Sounds {
            pickup: sound("pickup"),
            win: sound("win"),
            lose: sound("lose"),
            select: sound("select"),
        };
        ctx.world.insert_resource(sounds);
        // Music is long: streamed from the file rather than decoded whole.
        let music = ctx.audio.load_stream("coin_dash/music/dash.ogg");
        let params = SoundParams {
            volume: ctx.world.resource::<Settings>().volume * MUSIC_VOLUME,
            looping: true,
            ..Default::default()
        };
        let playing = ctx.audio.play_ex(music, params);
        ctx.world.insert_resource(Music(playing));
    }
}
// ANCHOR_END: plugins

/// Plays one of the sound effects at the saved volume. Silent without audio.
fn sfx(ctx: &mut Ctx, pick: fn(&Sounds) -> Sound) {
    let sound = pick(ctx.world.resource::<Sounds>());
    let volume = ctx.world.resource::<Settings>().volume;
    if volume > 0.0 {
        ctx.audio.play_ex(
            sound,
            SoundParams {
                volume,
                ..Default::default()
            },
        );
    }
}

/// Keyboard and gamepad, bound to the same names: the default controls, see
/// [`settings::bindings_for`] for the ones a player rebound.
pub fn bindings() -> ActionMap {
    settings::bindings_for(&settings::Controls::default())
}

// ANCHOR: read_input
/// Frame system: named actions to [`Intent`]. Runs once per rendered frame.
pub fn read_input(ctx: &mut Ctx) {
    let on = screen(&ctx.world);
    let act = *ctx.world.resource::<Act>();
    ctx.input
        .set_touch_control_shown(act.pause, matches!(on, Screen::Playing | Screen::Paused));
    ctx.input.set_touch_control_shown(act.settings, on == Screen::Title);
    let direction = ctx.input.action_axis2(act.movement);
    let confirm = ctx.input.action_pressed(act.confirm);
    let restart = ctx.input.action_pressed(act.restart);
    let pause = ctx.input.action_pressed(act.pause);
    let back = ctx.input.action_pressed(act.back);
    let open_settings = ctx.input.action_pressed(act.settings);
    ctx.world.resource_mut::<Hints>().touch = ctx.input.touch_controls_shown();
    let intent = ctx.world.resource_mut::<Intent>();
    intent.direction = direction;
    intent.confirm |= confirm;
    intent.restart |= restart;
    intent.pause |= pause;
    intent.back |= back;
    intent.settings |= open_settings;
}
// ANCHOR_END: read_input

/// The screen the game is on.
pub fn screen(world: &World) -> Screen {
    *world.resource::<State<Screen>>().get()
}

/// Entering `Playing` (from the title, the result screen, or a restart):
/// clears the field, then places the player in the middle and the coins at
/// random spots for the next round. The spots come from the context's
/// seeded random numbers, so a given seed and round number always give one layout.
pub fn new_round(ctx: &mut Ctx) {
    let number = ctx.world.resource::<Round>().number + 1;
    ctx.world.clear();
    let art = *ctx.world.resource::<Art>();
    let mut rng = ctx.random().stream(u64::from(number));
    // The field: 400 x 300 pixels, doubled like every picture, under the rest.
    let field = Transform::at(ARENA / 2.0).scaled(Vec2::splat(2.0));
    ctx.world.spawn((field, Sprite::new(art.field).with_z(-1.0)));
    ctx.world
        .spawn((Transform::at(ARENA / 2.0), Sprite::new(art.player), Player));
    for _ in 0..COINS {
        let at = vec2(rng.range(40.0, ARENA.x - 40.0), rng.range(40.0, ARENA.y - 40.0));
        ctx.world.spawn((Transform::at(at), Sprite::new(art.coin), Coin));
    }
    ctx.world.insert_resource(Round {
        collected: 0,
        time_left: ROUND_SECONDS,
        number,
        new_best: false,
    });
}

/// Sim system: the whole game, one fixed step at a time. What a button does
/// depends on the screen (a new screen shows from the next tick on):
///
/// | Screen | confirm | restart | pause | back | settings |
/// |---|---|---|---|---|---|
/// | Title | play | | quit | quit | open settings |
/// | Playing | | new round | pause | | |
/// | Paused | resume | new round | resume | title | |
/// | Result | play again | play again | title | title | |
/// | Settings | | | | | |
///
/// The settings screen is the retained UI's: only `close_settings` (its Back
/// button or Escape) leaves it, see `settings::settings_ui`.
pub fn rules(ctx: &mut Ctx) {
    let Intent {
        direction,
        confirm,
        restart,
        pause,
        back,
        settings,
        close_settings,
    } = std::mem::take(ctx.world.resource_mut::<Intent>());
    // `take` cleared the latches; keep the held direction for the next tick.
    ctx.world.resource_mut::<Intent>().direction = direction;

    match *ctx.state::<Screen>() {
        Screen::Title => {
            if confirm {
                sfx(ctx, |s| s.select);
                ctx.set_state(Screen::Playing);
            } else if settings {
                sfx(ctx, |s| s.select);
                ctx.set_state(Screen::Settings);
            } else if pause || back {
                ctx.quit();
            }
        }
        Screen::Settings => {
            if close_settings {
                settings::leave_settings(ctx);
            }
        }
        Screen::Playing if pause => ctx.push_state(Screen::Paused),
        Screen::Playing if restart => ctx.set_state(Screen::Playing),
        Screen::Playing => play(ctx, direction),
        Screen::Paused if back => ctx.set_state(Screen::Title),
        Screen::Paused if restart => ctx.set_state(Screen::Playing),
        Screen::Paused if pause || confirm => ctx.pop_state::<Screen>(),
        Screen::Result(_) if confirm || restart => ctx.set_state(Screen::Playing),
        Screen::Result(_) if pause || back => ctx.set_state(Screen::Title),
        Screen::Paused | Screen::Result(_) => {}
    }
}

/// One tick of a running round: move, pick up coins, run the clock, end the
/// round when it is won or the time is up.
fn play(ctx: &mut Ctx, direction: Vec2) {
    // Move, clamped to the arena. A diagonal is not faster than a straight line.
    let step = direction.clamp_length_max(1.0) * SPEED * ctx.time.dt();
    let mut player = Vec2::ZERO;
    for (t, _) in ctx.world.query::<(&mut Transform, &Player)>().iter() {
        t.position = (t.position + step).clamp(Vec2::ZERO, ARENA);
        player = t.position;
    }

    // Pick up the coins the player touches.
    let reach = PLAYER_RADIUS + COIN_RADIUS;
    let mut touched = Vec::new(); // empty Vecs don't allocate
    for (e, t, _) in ctx.world.query::<(Entity, &Transform, &Coin)>().iter() {
        if t.position.distance(player) <= reach {
            touched.push(e);
        }
    }
    for e in &touched {
        ctx.world.despawn(*e);
    }
    if !touched.is_empty() {
        sfx(ctx, |s| s.pickup);
    }

    let dt = ctx.time.dt();
    let round = ctx.world.resource_mut::<Round>();
    round.collected += touched.len();
    round.time_left = (round.time_left - dt).max(0.0);
    if round.collected >= COINS {
        finish_round(ctx, Outcome::Won);
    } else if round.time_left <= 0.0 {
        finish_round(ctx, Outcome::Lost);
    }
}

/// Ends the round: records the result in the saved progress (and the file)
/// and shows the result screen.
fn finish_round(ctx: &mut Ctx, outcome: Outcome) {
    let (collected, elapsed) = {
        let round = ctx.world.resource::<Round>();
        (round.collected, ROUND_SECONDS - round.time_left)
    };
    let new_best = ctx.world.resource_mut::<Progress>().record(outcome, collected, elapsed);
    let progress = ctx.world.resource::<Progress>().clone();
    let failed = ctx.storage.is_available() && progress.save(&mut ctx.storage).is_err();
    ctx.world.resource_mut::<SaveFile>().save_failed = failed;
    ctx.world.resource_mut::<Round>().new_best = new_best;
    match outcome {
        Outcome::Won => sfx(ctx, |s| s.win),
        Outcome::Lost => sfx(ctx, |s| s.lose),
    }
    ctx.set_state(Screen::Result(outcome));
}

// ---- Saved progress ---------------------------------------------------------

/// Layout version of the save file. A file with another version is treated as
/// unreadable (and kept as a `.bad` copy), never half-understood.
pub const SAVE_VERSION: u32 = 1;

/// The progress file's name in `ctx.storage`.
pub const PROGRESS_FILE: &str = "progress.json";

/// What survives between runs: the best results over all rounds.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    pub version: u32,
    /// Fastest win, in seconds into the round.
    pub best_time: Option<f32>,
    /// Most coins in one round, won or not (the high score).
    pub best_coins: usize,
    pub rounds_played: u32,
    pub rounds_won: u32,
}

/// How the saved progress was found at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadStatus {
    /// There was no file (first run), or saving is off (no app id).
    Fresh,
    /// The file was read.
    Loaded,
    /// The file was damaged or from another version: the game started from
    /// zero and kept the old file next to it as `<name>.bad`.
    Recovered,
}

/// What happened to the progress file. A resource.
#[derive(Debug, Clone)]
pub struct SaveFile {
    pub load: LoadStatus,
    /// The last attempt to write failed (a read-only disk, say). The game goes on.
    pub save_failed: bool,
}

/// Reads the JSON file `name` from `storage` into a `T`. A missing file (or no
/// storage) is a first run (`Fresh`, the default value); a file that can't be
/// read, doesn't parse or fails `valid` is recovered from (`Recovered`, the
/// default value), and its bytes are copied to `<name>.bad` for inspection.
/// Shared by the progress and the settings files.
fn load_json<T: Default + serde::de::DeserializeOwned>(
    storage: &mut Storage,
    name: &str,
    valid: fn(&T) -> bool,
) -> (T, LoadStatus) {
    if !storage.is_available() {
        return (T::default(), LoadStatus::Fresh);
    }
    let bytes = match storage.read(name) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return (T::default(), LoadStatus::Fresh),
        Err(_) => return (T::default(), LoadStatus::Recovered),
    };
    match serde_json::from_slice::<T>(&bytes) {
        Ok(value) if valid(&value) => (value, LoadStatus::Loaded),
        _ => {
            let _ = storage.write(&format!("{name}.bad"), &bytes); // best effort
            (T::default(), LoadStatus::Recovered)
        }
    }
}

impl Progress {
    /// Reads [`PROGRESS_FILE`]. A missing file is a first run; a file that
    /// can't be read, doesn't parse or holds impossible numbers is recovered
    /// from: zeroed progress, and the old bytes copied to `progress.json.bad`
    /// for inspection.
    pub fn load(storage: &mut Storage) -> (Progress, LoadStatus) {
        load_json(storage, PROGRESS_FILE, Progress::is_valid)
    }

    /// Writes [`PROGRESS_FILE`]. `ctx.storage` writes safely: a crash in the
    /// middle leaves the old file, not half a new one.
    pub fn save(&self, storage: &mut Storage) -> Result<(), Error> {
        storage.write_json(
            PROGRESS_FILE,
            &Progress {
                version: SAVE_VERSION,
                ..self.clone()
            },
        )
    }

    /// Whether the numbers could have come from this game.
    fn is_valid(&self) -> bool {
        self.version == SAVE_VERSION
            && self.best_coins <= COINS
            && self.rounds_won <= self.rounds_played
            && self
                .best_time
                .is_none_or(|t| t.is_finite() && t > 0.0 && t <= ROUND_SECONDS)
    }

    /// Counts a finished round; true when it set a new best time or high score.
    pub fn record(&mut self, outcome: Outcome, collected: usize, elapsed: f32) -> bool {
        self.version = SAVE_VERSION;
        self.rounds_played += 1;
        let mut new_best = collected > self.best_coins;
        self.best_coins = self.best_coins.max(collected);
        if outcome == Outcome::Won {
            self.rounds_won += 1;
            // A win can't take longer than the round or no time at all.
            let elapsed = elapsed.clamp(f32::EPSILON, ROUND_SECONDS);
            if self.best_time.is_none_or(|best| elapsed < best) {
                self.best_time = Some(elapsed);
                new_best = true;
            }
        }
        new_best
    }
}

// ---- Drawing ----------------------------------------------------------------

/// One line of text on a screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub size: f32,
    pub color: Color,
}

fn line(text: impl Into<String>, size: f32, color: Color) -> Line {
    Line {
        text: text.into(),
        size,
        color,
    }
}

/// The text of the current screen, top to bottom, in the current language.
/// Pure, so a test can check what the player would read without a window.
pub fn screen_lines(world: &World) -> Vec<Line> {
    let gold = Color::hex(0xffbe0b);
    let green = Color::hex(0x8ac926);
    let red = Color::hex(0xff595e);
    let progress = world.resource::<Progress>();
    let save = world.resource::<SaveFile>();
    let settings = world.resource::<Settings>();
    let tr = strings(world);
    let key = |control| Arg::from(settings.controls.key_name(control));
    let touch = world.get_resource::<Hints>().is_some_and(|h| h.touch);
    use settings::Control as C;
    let best = if progress.rounds_played == 0 {
        tr.get("title.no_rounds").into_owned()
    } else {
        let time = progress.best_time.map_or(tr.get("progress.none").into_owned(), |t| {
            tr.format("progress.seconds", &[("seconds", Arg::from(t))])
        });
        tr.format(
            "progress.best",
            &[
                ("time", Arg::from(time)),
                ("coins", Arg::from(progress.best_coins)),
                ("total", Arg::from(COINS)),
                ("won", Arg::from(progress.rounds_won)),
                ("played", Arg::from(progress.rounds_played)),
            ],
        )
    };
    let mut lines = match screen(world) {
        // The settings screen is the retained UI's; a round shows only the HUD.
        Screen::Playing | Screen::Settings => return Vec::new(),
        Screen::Title => {
            let goal = [
                ("count", Arg::from(COINS)),
                ("seconds", Arg::from(ROUND_SECONDS as i64)),
            ];
            let mut lines = vec![
                line(tr.get("title.name"), 56.0, gold),
                line(tr.format("title.goal", &goal), 20.0, Color::WHITE),
                line(best, 20.0, Color::WHITE),
            ];
            if save.load == LoadStatus::Recovered {
                lines.push(line(tr.get("title.progress_reset"), 20.0, red));
            }
            if world.resource::<settings::SettingsFile>().load == LoadStatus::Recovered {
                lines.push(line(tr.get("title.settings_reset"), 20.0, red));
            }
            // A browser tab can't be closed by the game, so the web build has
            // no quit; nor has a phone, whose system closes apps.
            let keys = if touch {
                "title.touch"
            } else if cfg!(target_arch = "wasm32") {
                "title.keys"
            } else {
                "title.keys_quit"
            };
            let args = [
                ("confirm", key(C::Confirm)),
                ("settings", key(C::Settings)),
                ("pause", key(C::Pause)),
            ];
            lines.push(line(tr.format(keys, &args), 20.0, Color::WHITE));
            lines
        }
        Screen::Paused => vec![
            line(tr.get("paused.title"), 48.0, Color::WHITE),
            line(
                if touch {
                    tr.get("paused.touch").into_owned()
                } else {
                    tr.format(
                        "paused.keys",
                        &[
                            ("pause", key(C::Pause)),
                            ("restart", key(C::Restart)),
                            ("back", key(C::Back)),
                        ],
                    )
                },
                20.0,
                Color::WHITE,
            ),
        ],
        Screen::Result(outcome) => {
            let round = world.resource::<Round>();
            let mut lines = match outcome {
                Outcome::Won => vec![
                    line(tr.get("result.won"), 48.0, green),
                    line(
                        tr.format(
                            "result.time",
                            &[("seconds", Arg::from(ROUND_SECONDS - round.time_left))],
                        ),
                        24.0,
                        Color::WHITE,
                    ),
                ],
                Outcome::Lost => vec![
                    line(tr.get("result.lost"), 48.0, red),
                    line(
                        tr.format(
                            "result.coins",
                            &[("count", Arg::from(round.collected)), ("total", Arg::from(COINS))],
                        ),
                        24.0,
                        Color::WHITE,
                    ),
                ],
            };
            if round.new_best {
                lines.push(line(tr.get("result.new_best"), 24.0, gold));
            }
            lines.push(line(best, 20.0, Color::WHITE));
            let keys = if touch {
                tr.get("result.touch").into_owned()
            } else {
                tr.format("result.keys", &[("confirm", key(C::Confirm)), ("back", key(C::Back))])
            };
            lines.push(line(keys, 20.0, Color::WHITE));
            lines
        }
    };
    if save.save_failed {
        lines.push(line(tr.get("save.failed"), 20.0, red));
    }
    lines
}

/// How the lines of a screen come in: they fade in and rise into place, on
/// the UI clock, so they come in the same way on a paused round.
pub const MENU_IN: UiTransition = UiTransition::new(0.25);
/// How far the lines rise as they come in, at UI scale 1.
const MENU_RISE: f32 = 12.0;

/// The lines of which screen are shown, since when on the UI clock
/// ([`Ui::clock`](ponko::ui::Ui::clock)), and how far they have come in.
#[derive(Debug, Clone, Copy, Default)]
pub struct MenuFade {
    pub screen: Option<Screen>,
    pub since: f64,
    /// 0 when the screen has just changed, 1 once its lines are in.
    pub shown: f32,
}

/// How far the current screen's lines have come in, 0 to 1.
fn menu_fade(ctx: &mut Ctx) -> f32 {
    let now = ctx.ui.clock();
    let reduced = ctx.ui.reduced_motion();
    let current = screen(&ctx.world);
    let fade = ctx.world.resource_mut::<MenuFade>();
    if fade.screen != Some(current) {
        fade.screen = Some(current);
        fade.since = now;
    }
    fade.shown = if reduced {
        1.0
    } else {
        MENU_IN.progress((now - fade.since) as f32)
    };
    fade.shown
}

/// The arena, in world units.
pub fn arena() -> Rect {
    Rect::new(0.0, 0.0, ARENA.x, ARENA.y)
}

/// Frame system: the camera shows the whole arena in the window's safe
/// area, then the text goes on top of the sprites.
pub fn hud(ctx: &mut Ctx) {
    let room = ctx.window.safe_area();
    fit2d::fit_camera(&mut ctx.camera, arena(), room);
    let font = strings(&ctx.world).font().unwrap_or(Font::DEFAULT);
    for p in hud_text(ctx) {
        let params = TextParams::default()
            .with_font(font)
            .with_size(p.size)
            .with_color(p.color);
        ctx.gfx.draw_text_ex(&p.text, p.at, params);
    }
}

/// The text `hud` draws this frame, where it draws it, at the player's UI
/// scale, with the current language's font (the built-in one for
/// English): the status line in the top corner of the safe area (clear of
/// the on-screen pause button), the screen's lines centred in it, each
/// made smaller only where it wouldn't fit.
pub fn hud_text(ctx: &mut Ctx) -> Vec<fit2d::Placed> {
    let shown = menu_fade(ctx);
    let scale = ctx.world.resource::<Settings>().ui_scale;
    let font = strings(&ctx.world).font().unwrap_or(Font::DEFAULT);
    let room = ctx.window.safe_area();
    let mut placed = Vec::new();
    if ctx.world.resource::<State<Screen>>().in_stack(&Screen::Playing) {
        let round = ctx.world.resource::<Round>().clone();
        let status = strings(&ctx.world).format(
            "hud.status",
            &[
                ("round", Arg::from(round.number)),
                ("count", Arg::from(round.collected)),
                ("total", Arg::from(COINS)),
                ("seconds", Arg::from(round.time_left)),
            ],
        );
        // The on-screen pause button sits in the top right corner (its rect
        // is in window pixels).
        let mut right = room.max.x;
        if ctx.world.resource::<Hints>().touch
            && let Some(button) = ctx.ui.find("touch.pause").and_then(|id| ctx.ui.rect(id))
        {
            right = right.min(button.min.x - 8.0 * scale);
        }
        let margin = 12.0 * scale;
        let size = 20.0 * scale;
        let width = ctx.gfx.measure_text_ex(&status, font, size).x;
        let fitted = fit2d::fit_size(size, width, right - room.min.x - 2.0 * margin);
        let width = width * fitted / size;
        // Right to left, the status line starts at the right edge.
        let x = if strings(&ctx.world).is_rtl() {
            right - margin - width
        } else {
            room.min.x + margin
        };
        placed.push(fit2d::Placed {
            text: status,
            at: vec2(x, room.min.y + 8.0 * scale),
            size: fitted,
            width,
            color: Color::WHITE,
        });
    }
    // The block of lines, centred in the safe area.
    let lines: Vec<_> = screen_lines(&ctx.world)
        .into_iter()
        .map(|l| {
            let size = l.size * scale;
            let width = ctx.gfx.measure_text_ex(&l.text, font, size).x;
            (l.text, size, width, l.color.with_alpha(l.color.a * shown))
        })
        .collect();
    let height: f32 = lines.iter().map(|l| l.1 * 1.5).sum();
    let top = room.min.y + (room.size().y - height) / 2.0 + MENU_RISE * scale * (1.0 - shown);
    placed.extend(fit2d::stack_lines(lines, 1.5, room, 16.0 * scale, Some(top)));
    placed
}
