//! Legions, the showcase battle (`examples/legions`), played headless:
//! the same seed and orders give the same world hash on one thread and on
//! many, with sprites or without, and through a recorded replay; the
//! committed replay ends with the hash it recorded; orders steer the red
//! army; arrows fly and kill; rounds end and start again. No window, no GPU.
//!
//! The tests drive the game's own plugins (`examples/legions/game.rs`).

#[path = "../examples/legions/game.rs"]
mod game;

use game::*;
use ponko::actions::InputEvent;
use ponko::prelude::*;
use ponko::scenario::InputScenario;
use std::path::PathBuf;

/// Small armies, so a debug test build runs them in a moment.
const UNITS: usize = 2_000;

/// The replay committed next to the game.
fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/legions/replays/skirmish.json")
}

fn hash_of(game: Game, ticks: u64) -> (World, u64) {
    let world = game.run_headless(ticks, |_| {});
    let hash = world_hash(&world);
    (world, hash)
}

/// `f` on a thread pool of `threads` threads.
fn on_threads<R: Send>(threads: usize, f: impl FnOnce() -> R + Send) -> R {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("a thread pool")
        .install(f)
}

fn temp_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ponko-legions-{name}-{}.json", std::process::id()))
}

#[test]
fn the_same_seed_gives_the_same_battle_and_the_armies_really_fight() {
    let (a, ha) = hash_of(headless(UNITS, 3), 600);
    let (_, hb) = hash_of(headless(UNITS, 3), 600);
    assert_eq!(hash_hex(ha), hash_hex(hb));
    let s = a.resource::<Score>();
    // Both armies lost units to swords and arrows, and arrows were in the air.
    assert!(s.fallen[0] > 50 && s.fallen[1] > 50, "{s:?}");
    assert!(
        s.alive[0] + s.alive[1] + s.fallen[0] as usize + s.fallen[1] as usize == UNITS,
        "{s:?}"
    );
}

#[test]
fn a_different_seed_gives_a_different_battle() {
    assert_ne!(hash_of(headless(UNITS, 3), 120).1, hash_of(headless(UNITS, 4), 120).1);
}

#[test]
fn the_hash_is_the_same_on_one_thread_and_on_many() {
    let one = on_threads(1, || hash_of(headless(UNITS, 5), 500).1);
    let four = on_threads(4, || hash_of(headless(UNITS, 5), 500).1);
    let all = hash_of(headless(UNITS, 5), 500).1;
    assert_eq!(hash_hex(one), hash_hex(four));
    assert_eq!(hash_hex(one), hash_hex(all));
}

#[test]
fn sprites_do_not_change_the_battle() {
    let plain = hash_of(headless(UNITS, 6), 400).1;
    let with_art = Game::new("legions", 1, 1).seed(6).plugin(Legions {
        units: UNITS,
        art: true,
    });
    let (mut world, drawn) = hash_of(with_art, 400);
    assert_eq!(hash_hex(plain), hash_hex(drawn));
    // The drawn run really had sprites: one per unit, arrow and corpse.
    let sprites = world.query::<&Sprite>().count();
    let things = world.query::<&Unit>().count() + world.query::<&Arrow>().count() + world.query::<&Corpse>().count();
    assert_eq!(sprites, things);
    assert!(world.query::<&Corpse>().count() > 0);
}

#[test]
fn arrows_fly_land_and_kill() {
    // One archer and one soldier, both holding, too far apart for the
    // soldier to charge: only arrows can settle it.
    let mut world = headless(UNITS, 7).run_headless(400, |ctx| {
        let mut all = Vec::new();
        ctx.world.query::<(Entity, &Unit)>().for_each(|(e, _)| all.push(e));
        for e in all {
            ctx.world.despawn(e);
        }
        let at = ctx.world.resource::<Battle>().start[0];
        let unit = |team, kind: Kind| Unit {
            team,
            kind,
            hp: kind.stats().hp,
            vel: Vec2::ZERO,
            cooldown: 0,
            swing: u8::MAX,
            engaged: false,
            push: Vec2::ZERO,
            act: Act::None,
        };
        ctx.world.spawn((d2::Transform::at(at), unit(Team::Red, Kind::Archer)));
        ctx.world
            .spawn((d2::Transform::at(at + vec2(60.0, 0.0)), unit(Team::Blue, Kind::Soldier)));
        ctx.world.resource_mut::<Battle>().stance = [Stance::Hold; 2];
    });
    let s = world.resource::<Score>().clone();
    assert_eq!(s.fallen, [0, 1], "{s:?}");
    // A soldier takes two arrows.
    assert!(s.arrow_hits >= 2, "{s:?}");
    // The archer stayed where it was, and won: the round is over.
    assert_eq!(world.query::<&Unit>().count(), 1);
    assert_eq!(s.outcome, Some(Some(Team::Red)));
}

#[test]
fn in_a_full_battle_arrows_hit_and_both_armies_lose_units() {
    let world = headless(UNITS, 7).run_headless(500, |_| {});
    let s = world.resource::<Score>();
    assert!(s.arrow_hits > 50, "{s:?}");
    assert!(s.fallen[0] > 50 && s.fallen[1] > 50, "{s:?}");
}

#[test]
fn hold_keeps_the_red_army_in_place_and_rally_moves_it() {
    let center_after = |orders: Vec<Stamped>| {
        let replay = Replay {
            format: REPLAY_FORMAT.into(),
            version: REPLAY_VERSION,
            seed: 8,
            units: UNITS,
            ticks: 120,
            orders,
            hash: String::new(),
        };
        let (world, _) = play_replay(&replay);
        let s = world.resource::<Score>().clone();
        let b = world.resource::<Battle>();
        (s.center[0], b.start[0], b.stance[0])
    };
    let (held, start, stance) = center_after(vec![Stamped {
        tick: 0,
        order: Order::Hold,
    }]);
    assert_eq!(stance, Stance::Hold);
    // Only crowding spreads it a little; it doesn't march.
    assert!((held.x - start.x).abs() < 6.0, "held at {held} from {start}");
    // The default is to attack: the army walks toward blue.
    let (attacking, _, stance) = center_after(Vec::new());
    assert_eq!(stance, Stance::Attack);
    assert!(attacking.x > start.x + 20.0, "attacking at {attacking} from {start}");
    // Rallying behind the start: it walks back.
    let back = (40..150)
        .rev()
        .map(|dx| start - vec2(dx as f32, 0.0))
        .find(|&p| nav_walkable(8, p))
        .expect("open ground behind red");
    let (rallied, _, stance) = center_after(vec![Stamped {
        tick: 10,
        order: Order::Rally { x: back.x, y: back.y },
    }]);
    assert_eq!(stance, Stance::Rally);
    assert!(rallied.x < start.x - 20.0, "rallied to {rallied} from {start}");
}

/// Whether `at` is open ground on the map of `seed`.
fn nav_walkable(seed: u64, at: Vec2) -> bool {
    headless(UNITS, seed)
        .run_headless(0, |_| {})
        .resource::<Battle>()
        .nav
        .walkable(at)
}

#[test]
fn a_round_ends_and_the_next_begins() {
    let world = headless(200, 9).run_headless(4000, |_| {});
    let s = world.resource::<Score>();
    assert!(s.round >= 2, "still round {}: {s:?}", s.round);
    assert!(s.rounds_won[0] + s.rounds_won[1] >= 1, "{s:?}");
}

// ANCHOR: record
/// The windowed game (rules, controls and view) on headless frames, with
/// the mouse and keys scripted: what a player would do, recorded.
fn record(path: &std::path::Path, units: usize, seed: u64, ticks: u64) -> u64 {
    // The window is 1280 x 720 and the camera shows the whole map: a click
    // a little left of the middle rallies red there.
    let script = InputScenario::new()
        .at(60, InputEvent::Key(Key::H, true))
        .at(61, InputEvent::Key(Key::H, false))
        .at(150, InputEvent::MouseMove(vec2(560.0, 300.0)))
        .at(151, InputEvent::Mouse(MouseButton::Left, true))
        .at(152, InputEvent::Mouse(MouseButton::Left, false))
        .at(300, InputEvent::Key(Key::A, true))
        .at(301, InputEvent::Key(Key::A, false));
    let game = with_recorder(build(units, seed), path.to_path_buf(), units, seed, ticks);
    let (_, hash) = hash_of(game.headless_frames(true).input_scenario(script), ticks);
    hash
}
// ANCHOR_END: record

#[test]
fn recorded_input_replays_to_the_same_world_hash() {
    let path = temp_file("record");
    let recorded = record(&path, UNITS, 10, 480);
    let replay = Replay::load(&path).expect("the recorder wrote the replay");
    let _ = std::fs::remove_file(&path);
    // The script's three orders, at the tick after the frame that saw them.
    let orders: Vec<Order> = replay.orders.iter().map(|s| s.order).collect();
    assert!(
        matches!(orders.as_slice(), [Order::Hold, Order::Rally { .. }, Order::Attack]),
        "{orders:?}"
    );
    assert_eq!(replay.orders[0].tick, 61);
    assert_eq!((replay.ticks, replay.units, replay.seed), (480, UNITS, 10));
    assert_eq!(replay.hash, hash_hex(recorded));
    // Played again by the rules alone, with no window, sprites or controls.
    let (_, replayed) = play_replay(&replay);
    assert_eq!(hash_hex(replayed), replay.hash);
    // And a replay without its orders is a different battle.
    let mut no_orders = replay.clone();
    no_orders.orders.clear();
    assert_ne!(hash_hex(play_replay(&no_orders).1), replay.hash);
}

#[test]
fn quitting_with_escape_writes_a_replay_of_every_tick_so_far() {
    let path = temp_file("escape");
    let script = InputScenario::new()
        .at(40, InputEvent::Key(Key::H, true))
        .at(41, InputEvent::Key(Key::H, false))
        .at(250, InputEvent::Key(Key::Escape, true));
    // No checkpoints: only Escape writes the file, and it ends the run.
    let game = with_recorder(build(UNITS, 11), path.clone(), UNITS, 11, 0);
    let (_, quit_at) = hash_of(game.headless_frames(true).input_scenario(script), 1000);
    let replay = Replay::load(&path).expect("Escape wrote the replay");
    let _ = std::fs::remove_file(&path);
    assert_eq!(replay.ticks, 251, "ticks 0 to 250 ran before the frame that saw Escape");
    assert_eq!(
        replay.orders,
        vec![Stamped {
            tick: 41,
            order: Order::Hold
        }]
    );
    assert_eq!(replay.hash, hash_hex(quit_at));
    assert_eq!(hash_hex(play_replay(&replay).1), replay.hash);
}

#[test]
fn watching_a_replay_ignores_the_mouse_and_keys() {
    let replay = Replay::load(&fixture()).expect("the committed replay");
    // A player pressing keys while the replay plays changes nothing.
    let script = InputScenario::new()
        .at(100, InputEvent::Key(Key::H, true))
        .at(101, InputEvent::Key(Key::H, false))
        .at(200, InputEvent::Key(Key::R, true));
    let game = with_replay(build(replay.units, replay.seed), &replay);
    let (_, watched) = hash_of(game.headless_frames(true).input_scenario(script), replay.ticks);
    assert_eq!(hash_hex(watched), replay.hash);
}

#[test]
fn the_committed_replay_ends_with_its_hash_on_one_thread_and_on_many() {
    let replay = Replay::load(&fixture()).expect("the committed replay");
    assert!(replay.orders.len() >= 3, "it has orders to replay");
    let expected = replay.expected_hash().expect("a hex hash");
    assert_eq!(hash_hex(on_threads(1, || play_replay(&replay).1)), hash_hex(expected));
    assert_eq!(hash_hex(on_threads(3, || play_replay(&replay).1)), hash_hex(expected));
    assert_eq!(hash_hex(play_replay(&replay).1), hash_hex(expected));
}

#[test]
fn a_file_that_is_not_a_replay_is_refused() {
    let path = temp_file("bad");
    std::fs::write(
        &path,
        r#"{"format":"something-else","version":1,"seed":1,"units":2,"ticks":1,"orders":[],"hash":""}"#,
    )
    .unwrap();
    let err = Replay::load(&path).unwrap_err();
    let _ = std::fs::remove_file(&path);
    assert!(err.contains("not a ponko-legions-replay file"), "{err}");
}

/// Writes `examples/legions/replays/skirmish.json` again, through the same
/// recorder a player's game uses. Only when the rules change on purpose:
/// `cargo test --test legions -- --ignored write_the_committed_replay`.
#[test]
#[ignore = "rewrites the committed replay"]
fn write_the_committed_replay() {
    let hash = record(&fixture(), UNITS, SEED, 600);
    println!("wrote {} with hash {}", fixture().display(), hash_hex(hash));
}
