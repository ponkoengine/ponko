//! World brains through the C API: trees in the world, `Brain` and
//! `Blackboard` components, `ai::think` and `ai::Signal` events sent from C.
mod common;

use common::{last_error, new_game, run_game};
use ponko_ffi::PonkoAiStatus::*;
use ponko_ffi::*;
use std::ffi::{CString, c_void};

use PonkoStatus::*;

fn key(name: &str) -> u64 {
    let name = CString::new(name).unwrap();
    let mut id = 0;
    assert_eq!(unsafe { ponko_ai_key(name.as_ptr(), &mut id) }, PONKO_OK);
    id
}

fn spawn(ctx: *mut PonkoCtx) -> u64 {
    let mut e = 0;
    // SAFETY: a live ctx in a setup callback; no components.
    assert_eq!(
        unsafe { ponko_spawn(ponko_ctx_world(ctx), std::ptr::null(), 0, &mut e) },
        PONKO_OK
    );
    e
}

/// Guards that sleep until they hear the alarm, then run (to where they
/// were told) and shout at once.
const GUARD: &str = r#"{"root": {"selector": [
    {"sequence": [
        {"wait_for": "alarm", "store": "by"},
        {"parallel": [{"do": "run", "speed": 3}, {"do": "shout"}]},
        {"do": "rest"}
    ]}
]}}"#;

/// What the C systems saw, tick by tick: each guard's actions.
#[derive(Default)]
struct Log {
    guards: Vec<u64>,
    intruder: u64,
    rows: Vec<Vec<(u64, u64)>>,
    started: Vec<(u64, bool, bool)>,
    speed: f32,
}

unsafe extern "C" fn senses(ctx: *mut PonkoCtx, user: *mut c_void) {
    // SAFETY: the log outlives the run; a live ctx.
    let log = unsafe { &mut *user.cast::<Log>() };
    let tick = unsafe { ponko_ctx_sim_tick(ctx) };
    unsafe {
        if tick == 5 {
            // To the first guard only, saying who.
            assert_eq!(
                ponko_ctx_ai_signal_entity(ctx, log.guards[0], key("alarm"), log.intruder),
                PONKO_OK,
                "{}",
                last_error()
            );
        }
        if tick == 9 {
            assert_eq!(ponko_ctx_ai_signal(ctx, PONKO_AI_EVERY_BRAIN, key("alarm")), PONKO_OK);
        }
    }
}

unsafe extern "C" fn think(ctx: *mut PonkoCtx, _: *mut c_void) {
    // SAFETY: a live ctx.
    assert_eq!(unsafe { ponko_ctx_ai_think(ctx) }, PONKO_OK, "{}", last_error());
}

unsafe extern "C" fn act(ctx: *mut PonkoCtx, user: *mut c_void) {
    // SAFETY: the log outlives the run; a live ctx.
    let log = unsafe { &mut *user.cast::<Log>() };
    let tick = unsafe { ponko_ctx_sim_tick(ctx) };
    let (run, shout) = (key("run"), key("shout"));
    let mut row = Vec::new();
    for &g in &log.guards {
        let (mut first, mut second) = (u64::MAX, u64::MAX);
        unsafe {
            assert_eq!(ponko_ctx_ai_action(ctx, g, 0, &mut first), PONKO_OK);
            assert_eq!(ponko_ctx_ai_action(ctx, g, 1, &mut second), PONKO_OK);
        }
        row.push((first, second));
        if first == run {
            let (mut r, mut s) = (false, false);
            unsafe {
                assert_eq!(ponko_ctx_ai_started(ctx, g, run, &mut r), PONKO_OK);
                let expect = if second == shout { PONKO_OK } else { PONKO_NOT_FOUND };
                assert_eq!(ponko_ctx_ai_started(ctx, g, shout, &mut s), expect);
                assert_eq!(
                    ponko_ctx_ai_param_num(ctx, g, 0, key("speed"), &mut log.speed),
                    PONKO_OK
                );
            }
            log.started.push((tick, r, s));
        }
    }
    // The first guard: the shout is over at tick 7; the run at tick 8.
    let g = log.guards[0];
    unsafe {
        if tick == 7 {
            assert_eq!(ponko_ctx_ai_finish(ctx, g, shout, PONKO_AI_SUCCESS), PONKO_OK);
        }
        if tick == 8 {
            assert_eq!(ponko_ctx_ai_finish(ctx, g, 0, PONKO_AI_SUCCESS), PONKO_OK);
            // Not doing it any more (the run is reported, the shout gone).
            assert_eq!(ponko_ctx_ai_finish(ctx, g, shout, PONKO_AI_SUCCESS), PONKO_NOT_FOUND);
        }
    }
    log.rows.push(row);
}

#[test]
fn c_games_send_signals_to_world_brains_and_act_on_several_actions() {
    let mut log = Log::default();
    let user: *mut c_void = (&raw mut log).cast();
    let game = new_game();
    unsafe {
        assert_eq!(
            ponko_game_sim_system(game, c"senses".as_ptr(), 1, Some(senses), user),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_sim_system(game, c"think".as_ptr(), 1, Some(think), user),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_sim_system(game, c"act".as_ptr(), 1, Some(act), user),
            PONKO_OK
        );
    }
    let world = run_game(game, 12, |ctx| unsafe {
        let json = CString::new(GUARD).unwrap();
        let tree = ponko_behavior_tree_new(json.as_ptr());
        assert!(!tree.is_null(), "{}", last_error());
        assert_eq!(ponko_ctx_ai_add_tree(ctx, c"guard".as_ptr(), tree), PONKO_OK);
        // The world keeps what it needs.
        ponko_behavior_tree_free(tree);
        let log = &mut *user.cast::<Log>();
        log.intruder = spawn(ctx);
        for _ in 0..2 {
            let g = spawn(ctx);
            // A slow thinker: only the signal wakes it in time.
            assert_eq!(ponko_ctx_ai_add_brain(ctx, g, c"guard".as_ptr(), 100), PONKO_OK);
            log.guards.push(g);
        }
    });
    unsafe { ponko_world_free(world) };
    let (run, shout, rest) = (key("run"), key("shout"), key("rest"));
    let none = (0, 0);
    // Both wait for the alarm; the first hears it at tick 5, the second at 9.
    for tick in 0..5 {
        assert_eq!(log.rows[tick], [none, none], "tick {tick}");
    }
    assert_eq!(log.rows[5], [(run, shout), none]);
    assert_eq!(log.rows[6], [(run, shout), none]);
    assert_eq!(log.rows[7], [(run, shout), none]);
    // The shout reported done at 7: the run carries on alone.
    assert_eq!(log.rows[8], [(run, 0), none]);
    // The run reported at 8: the parallel succeeded, the guard rests.
    assert_eq!(log.rows[9], [(rest, 0), (run, shout)]);
    assert_eq!(log.speed, 3.0);
    // Started at the think that chose them, not after.
    assert_eq!(log.started[0], (5, true, true));
    assert_eq!(log.started[1], (6, false, false));
    assert!(log.started.contains(&(9, true, true)));
}

#[test]
fn world_brains_read_trees_from_assets_and_blackboards_from_c() {
    let game = new_game();
    let world = run_game(game, 0, |ctx| unsafe {
        let source = ponko_memory_source_new(c"ai".as_ptr());
        let tree = br#"{"root": {"sequence": [{"check": "alert"}, {"do": "chase", "speed": 4.5}]}}"#;
        assert_eq!(
            ponko_memory_source_insert(source, c"ai/guard.json".as_ptr(), tree.as_ptr(), tree.len()),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_add_memory_source(ctx, source), PONKO_OK);
        ponko_memory_source_free(source);
        assert_eq!(
            ponko_ctx_ai_load_tree(ctx, c"ai/guard.json".as_ptr()),
            PONKO_OK,
            "{}",
            last_error()
        );
        assert_eq!(
            ponko_ctx_ai_load_tree(ctx, c"ai/missing.json".as_ptr()),
            PONKO_NOT_FOUND
        );

        let g = spawn(ctx);
        let (alert, chase, speed) = (key("alert"), key("chase"), key("speed"));
        // No brain yet: nothing to read or write.
        let mut k = 7;
        assert_eq!(ponko_ctx_ai_action(ctx, g, 0, &mut k), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_ai_set_flag(ctx, g, alert, true), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_ai_add_brain(ctx, g, c"ai/guard.json".as_ptr(), 1), PONKO_OK);
        assert_eq!(ponko_ctx_ai_add_brain(ctx, g, c"".as_ptr(), 1), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_ai_set_flag(ctx, g, alert, true), PONKO_OK);
        assert_eq!(ponko_ctx_ai_set_num(ctx, g, key("health"), 0.5), PONKO_OK);
        assert_eq!(ponko_ctx_ai_set_entity(ctx, g, key("home"), g), PONKO_OK);
        assert_eq!(
            ponko_ctx_ai_set_vec2(ctx, g, key("p2"), PonkoVec2 { x: 1.0, y: 2.0 }),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ai_set_vec3(ctx, g, key("p3"), PonkoVec3 { x: 1.0, y: 2.0, z: 3.0 }),
            PONKO_OK
        );
        let (mut flag, mut num) = (false, 0.0);
        assert_eq!(ponko_ctx_ai_flag(ctx, g, alert, &mut flag), PONKO_OK);
        assert_eq!(ponko_ctx_ai_num(ctx, g, key("health"), &mut num), PONKO_OK);
        assert!(flag);
        assert_eq!(num, 0.5);
        assert_eq!(ponko_ctx_ai_num(ctx, g, alert, &mut num), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_ai_flag(ctx, g, key("health"), &mut flag), PONKO_NOT_FOUND);

        // Think once by hand: the guard chases.
        assert_eq!(ponko_ctx_ai_think(ctx), PONKO_OK);
        assert_eq!(ponko_ctx_ai_action(ctx, g, 0, &mut k), PONKO_OK);
        assert_eq!(k, chase);
        assert_eq!(ponko_ctx_ai_action(ctx, g, 1, &mut k), PONKO_OK);
        assert_eq!(k, 0, "one action");
        let mut v = 0.0;
        assert_eq!(ponko_ctx_ai_param_num(ctx, g, chase, speed, &mut v), PONKO_OK);
        assert_eq!(v, 4.5);
        assert_eq!(ponko_ctx_ai_param_num(ctx, g, chase, alert, &mut v), PONKO_NOT_FOUND);
        assert_eq!(
            ponko_ctx_ai_param_num(ctx, g, key("flee"), speed, &mut v),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_ai_finish(ctx, g, chase, PONKO_AI_RUNNING),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_ai_finish(ctx, g, chase, PONKO_AI_FAILURE), PONKO_OK);
        // Bad entities and keys.
        assert_eq!(
            ponko_ctx_ai_add_brain(ctx, u64::MAX - 1, c"ai/guard.json".as_ptr(), 1),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_ai_signal(ctx, g, 12_345_678), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_ai_signal_num(ctx, g, alert, 1.0), PONKO_OK);
        assert_eq!(
            ponko_ctx_ai_signal_vec2(ctx, g, alert, PonkoVec2 { x: 0.0, y: 0.0 }),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ai_signal_vec3(ctx, g, alert, PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 }),
            PONKO_OK
        );
    });
    unsafe { ponko_world_free(world) };
}

#[test]
fn a_tree_that_runs_itself_is_refused_from_c_naming_the_cycle() {
    let world = run_game(new_game(), 0, |ctx| unsafe {
        let a = CString::new(r#"{"root": {"subtree": "b"}}"#).unwrap();
        let b = CString::new(r#"{"root": {"subtree": "a"}}"#).unwrap();
        let (a, b) = (ponko_behavior_tree_new(a.as_ptr()), ponko_behavior_tree_new(b.as_ptr()));
        assert_eq!(ponko_ctx_ai_add_tree(ctx, c"a".as_ptr(), a), PONKO_OK);
        assert_eq!(ponko_ctx_ai_add_tree(ctx, c"b".as_ptr(), b), PONKO_INVALID_ARGUMENT);
        assert!(last_error().contains("b -> a -> b"), "{}", last_error());
        assert_eq!(
            ponko_ctx_ai_add_tree(ctx, c"c".as_ptr(), std::ptr::null()),
            PONKO_INVALID_ARGUMENT
        );
        ponko_behavior_tree_free(a);
        ponko_behavior_tree_free(b);
    });
    unsafe { ponko_world_free(world) };
}
