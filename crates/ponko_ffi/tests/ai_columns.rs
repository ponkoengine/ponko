//! World brains inside C query loops: `Brain` and `Blackboard` are query
//! columns, read and written row by row with the `ponko_ai_brain_*` and
//! `ponko_ai_blackboard_*` calls (in `ponko_query` and `ponko_query_par`),
//! and the blackboard getters for entities and points.
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

fn bits(e: &PonkoEntity) -> u64 {
    u64::from(e.index) | (u64::from(e.generation) << 32)
}

/// The ids of the brain and blackboard columns.
fn columns(world: *mut PonkoWorld) -> [u32; 2] {
    let (mut brain, mut board) = (0, 0);
    unsafe {
        assert_eq!(ponko_ai_brain_component(world, &mut brain), PONKO_OK);
        assert_eq!(ponko_ai_blackboard_component(world, &mut board), PONKO_OK);
    }
    [brain, board]
}

/// Guards chase what they see; the chase ends when the game says so.
const GUARD: &str = r#"{"root": {"selector": [
    {"sequence": [{"check": "alert"}, {"do": "chase", "speed": 4.5}]},
    {"do": "idle"}
]}}"#;

/// What the loops saw.
#[derive(Default)]
struct Log {
    guards: Vec<u64>,
    /// Per tick, per row: entity, first action, started, speed.
    rows: Vec<Vec<(u64, u64, bool, f32)>>,
    /// What the act loop read back from the blackboards at tick 3.
    read: Vec<(u64, u64, PonkoVec2, PonkoVec3, f32, bool)>,
    /// Statuses of calls that must fail, and of ctx calls inside the loop.
    refused: Vec<PonkoStatus>,
}

/// Senses inside a loop: each guard's blackboard row is written where the
/// query visits it, no entity list collected first.
unsafe extern "C" fn senses(ctx: *mut PonkoCtx, user: *mut c_void) {
    struct Visit {
        tick: u64,
        alert: u64,
        target: u64,
        home: u64,
        at: u64,
        energy: u64,
    }
    unsafe extern "C" fn row(chunk: *const PonkoChunk, user: *mut c_void) {
        // SAFETY: the visit outlives the query; a live chunk.
        unsafe {
            let v = &*user.cast::<Visit>();
            let chunk_ref = &*chunk;
            let entities = std::slice::from_raw_parts(chunk_ref.entities, chunk_ref.count);
            for (i, e) in entities.iter().enumerate() {
                let board = ponko_chunk_ai_blackboard(chunk, 0, i);
                assert!(!board.is_null());
                // Guards see the intruder from tick 2 on.
                assert_eq!(ponko_ai_blackboard_set_flag(board, v.alert, v.tick >= 2), PONKO_OK);
                assert_eq!(ponko_ai_blackboard_set_entity(board, v.target, bits(e)), PONKO_OK);
                let p = PonkoVec2 {
                    x: e.index as f32,
                    y: 2.0,
                };
                assert_eq!(ponko_ai_blackboard_set_vec2(board, v.home, p), PONKO_OK);
                let q = PonkoVec3 {
                    x: 1.0,
                    y: e.index as f32,
                    z: 3.0,
                };
                assert_eq!(ponko_ai_blackboard_set_vec3(board, v.at, q), PONKO_OK);
                assert_eq!(ponko_ai_blackboard_set_num(board, v.energy, 0.25), PONKO_OK);
            }
        }
    }
    let visit = Visit {
        tick: unsafe { ponko_ctx_sim_tick(ctx) },
        alert: key("alert"),
        target: key("target"),
        home: key("home"),
        at: key("at"),
        energy: key("energy"),
    };
    let world = unsafe { ponko_ctx_world(ctx) };
    let [_, board] = columns(world);
    let ids = [board];
    let status = unsafe { ponko_query(world, ids.as_ptr(), 1, Some(row), (&raw const visit).cast_mut().cast()) };
    assert_eq!(status, PONKO_OK, "{}", last_error());
    let _ = user;
}

unsafe extern "C" fn think(ctx: *mut PonkoCtx, _: *mut c_void) {
    // SAFETY: a live ctx.
    assert_eq!(unsafe { ponko_ctx_ai_think(ctx) }, PONKO_OK, "{}", last_error());
}

/// Acting inside a loop: each row's brain says what to do, the blackboard
/// what was seen; the first guard's chase is over at tick 4.
unsafe extern "C" fn act(ctx: *mut PonkoCtx, user: *mut c_void) {
    struct Visit<'a> {
        log: &'a mut Log,
        ctx: *mut PonkoCtx,
        tick: u64,
        row: Vec<(u64, u64, bool, f32)>,
    }
    unsafe extern "C" fn rows(chunk: *const PonkoChunk, user: *mut c_void) {
        // SAFETY: the visit outlives the query; a live chunk.
        unsafe {
            let v = &mut *user.cast::<Visit>();
            let c = &*chunk;
            let entities = std::slice::from_raw_parts(c.entities, c.count);
            for (i, e) in entities.iter().enumerate() {
                // Columns follow the query: brain, then blackboard.
                let brain = ponko_chunk_ai_brain(chunk, 0, i);
                let board = ponko_chunk_ai_blackboard(chunk, 1, i);
                let (mut action, mut started, mut speed) = (u64::MAX, false, 0.0);
                assert_eq!(ponko_ai_brain_action(brain, 0, &mut action), PONKO_OK);
                if action == key("chase") {
                    assert_eq!(ponko_ai_brain_started(brain, 0, &mut started), PONKO_OK);
                    assert_eq!(ponko_ai_brain_param_num(brain, 0, key("speed"), &mut speed), PONKO_OK);
                }
                let mut second = u64::MAX;
                assert_eq!(ponko_ai_brain_action(brain, 1, &mut second), PONKO_OK);
                assert_eq!(second, 0, "one action at a time");
                v.row.push((bits(e), action, started, speed));
                if v.tick == 3 {
                    let (mut target, mut home, mut at) = (0, PonkoVec2::default(), PonkoVec3::default());
                    let (mut energy, mut alert) = (0.0, false);
                    assert_eq!(ponko_ai_blackboard_entity(board, key("target"), &mut target), PONKO_OK);
                    assert_eq!(ponko_ai_blackboard_vec2(board, key("home"), &mut home), PONKO_OK);
                    assert_eq!(ponko_ai_blackboard_vec3(board, key("at"), &mut at), PONKO_OK);
                    assert_eq!(ponko_ai_blackboard_num(board, key("energy"), &mut energy), PONKO_OK);
                    assert_eq!(ponko_ai_blackboard_flag(board, key("alert"), &mut alert), PONKO_OK);
                    v.log.read.push((bits(e), target, home, at, energy, alert));
                    // Wrong kinds and unknown keys are not found.
                    v.log
                        .refused
                        .push(ponko_ai_blackboard_vec2(board, key("at"), &mut home));
                    v.log
                        .refused
                        .push(ponko_ai_blackboard_entity(board, key("energy"), &mut target));
                    v.log.refused.push(ponko_ai_blackboard_vec3(board, 12_345_678, &mut at));
                    // Out of range rows and columns are null.
                    assert!(ponko_chunk_ai_brain(chunk, 2, i).is_null());
                    assert!(ponko_chunk_ai_brain(chunk, 0, c.count).is_null());
                    // The ctx calls by entity are still off in a loop.
                    v.log.refused.push(ponko_ctx_ai_action(v.ctx, bits(e), 0, &mut action));
                }
                if v.tick == 4 && bits(e) == v.log.guards[0] {
                    assert_eq!(ponko_ai_brain_finish(brain, 0, PONKO_AI_SUCCESS), PONKO_OK);
                    v.log.refused.push(ponko_ai_brain_finish(brain, 0, PONKO_AI_RUNNING));
                }
            }
        }
    }
    // SAFETY: the log outlives the run; a live ctx.
    let log = unsafe { &mut *user.cast::<Log>() };
    let mut visit = Visit {
        log,
        ctx,
        tick: unsafe { ponko_ctx_sim_tick(ctx) },
        row: Vec::new(),
    };
    let world = unsafe { ponko_ctx_world(ctx) };
    let ids = columns(world);
    let status = unsafe { ponko_query(world, ids.as_ptr(), 2, Some(rows), (&raw mut visit).cast()) };
    assert_eq!(status, PONKO_OK, "{}", last_error());
    let mut row = std::mem::take(&mut visit.row);
    row.sort_by_key(|r| r.0);
    visit.log.rows.push(row);
}

fn add_guards(ctx: *mut PonkoCtx, log: &mut Log, count: usize, period: u32) {
    unsafe {
        let json = CString::new(GUARD).unwrap();
        let tree = ponko_behavior_tree_new(json.as_ptr());
        assert!(!tree.is_null(), "{}", last_error());
        assert_eq!(ponko_ctx_ai_add_tree(ctx, c"guard".as_ptr(), tree), PONKO_OK);
        ponko_behavior_tree_free(tree);
        for _ in 0..count {
            let g = spawn(ctx);
            assert_eq!(ponko_ctx_ai_add_brain(ctx, g, c"guard".as_ptr(), period), PONKO_OK);
            log.guards.push(g);
        }
        // An entity without a brain: the queries never visit it.
        spawn(ctx);
    }
}

#[test]
fn world_brains_are_read_and_written_inside_query_callbacks() {
    let mut log = Log::default();
    let user: *mut c_void = (&raw mut log).cast();
    let game = new_game();
    unsafe {
        for (name, f) in [
            (c"senses", senses as unsafe extern "C" fn(*mut PonkoCtx, *mut c_void)),
            (c"think", think),
            (c"act", act),
        ] {
            assert_eq!(ponko_game_sim_system(game, name.as_ptr(), 1, Some(f), user), PONKO_OK);
        }
    }
    let world = run_game(game, 7, |ctx| {
        // SAFETY: the log outlives the run.
        add_guards(ctx, unsafe { &mut *user.cast::<Log>() }, 3, 1);
    });
    unsafe { ponko_world_free(world) };
    let (chase, idle) = (key("chase"), key("idle"));
    let guards = log.guards.clone();
    // Every tick visits the three guards, not the entity without a brain.
    assert_eq!(log.rows.len(), 7);
    for row in &log.rows {
        assert_eq!(row.iter().map(|r| r.0).collect::<Vec<_>>(), guards);
    }
    // Not alert before tick 2: idle.
    for tick in 0..2 {
        assert!(log.rows[tick].iter().all(|r| r.1 == idle), "tick {tick}");
    }
    // Alert from tick 2 on: they chase at 4.5, started at tick 2 only.
    assert!(log.rows[2].iter().all(|r| *r == (r.0, chase, true, 4.5)));
    assert!(log.rows[3].iter().all(|r| *r == (r.0, chase, false, 4.5)));
    // The first guard's chase was reported done at tick 4: at tick 5's
    // think the tree succeeds (no action), and at tick 6 it starts over and
    // chases again (still alert), started anew. The others chase on.
    assert_eq!(log.rows[5][0], (guards[0], 0, false, 0.0));
    assert_eq!(log.rows[6][0], (guards[0], chase, true, 4.5));
    assert_eq!(log.rows[5][1], (guards[1], chase, false, 4.5));
    assert_eq!(log.rows[6][2], (guards[2], chase, false, 4.5));
    // What the senses loop wrote, read back in the act loop.
    assert_eq!(log.read.len(), 3);
    for (e, target, home, at, energy, alert) in &log.read {
        let index = (*e & 0xffff_ffff) as f32;
        assert_eq!(target, e);
        assert_eq!((home.x, home.y), (index, 2.0));
        assert_eq!((at.x, at.y, at.z), (1.0, index, 3.0));
        assert_eq!(*energy, 0.25);
        assert!(*alert);
    }
    assert_eq!(
        log.refused,
        [
            PONKO_NOT_FOUND,
            PONKO_NOT_FOUND,
            PONKO_NOT_FOUND,
            PONKO_BUSY,
            PONKO_NOT_FOUND,
            PONKO_NOT_FOUND,
            PONKO_NOT_FOUND,
            PONKO_BUSY,
            PONKO_NOT_FOUND,
            PONKO_NOT_FOUND,
            PONKO_NOT_FOUND,
            PONKO_BUSY,
            PONKO_INVALID_ARGUMENT,
        ]
    );
}

/// Senses on all cores: every row's blackboard gets its own number.
unsafe extern "C" fn senses_par(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe extern "C" fn rows(chunk: *const PonkoChunk, user: *mut c_void) {
        // SAFETY: the key outlives the query; a live chunk of this thread.
        unsafe {
            let energy = *user.cast::<u64>();
            let c = &*chunk;
            for i in 0..c.count {
                let e = &*c.entities.add(i);
                let board = ponko_chunk_ai_blackboard(chunk, 1, i);
                let brain = ponko_chunk_ai_brain(chunk, 0, i);
                let mut action = u64::MAX;
                // Reads of the brain are fine from any thread, too.
                assert_eq!(ponko_ai_brain_action(brain, 0, &mut action), PONKO_OK);
                assert_eq!(ponko_ai_blackboard_set_num(board, energy, e.index as f32), PONKO_OK);
            }
        }
    }
    let energy = key("energy");
    let world = unsafe { ponko_ctx_world(ctx) };
    let ids = columns(world);
    let status = unsafe {
        ponko_query_par(
            world,
            ids.as_ptr(),
            2,
            Some(rows),
            (&raw const energy).cast_mut().cast(),
        )
    };
    assert_eq!(status, PONKO_OK, "{}", last_error());
}

/// Reads back what `senses_par` wrote, through the ctx calls by entity.
unsafe extern "C" fn check_par(ctx: *mut PonkoCtx, user: *mut c_void) {
    // SAFETY: the log outlives the run; a live ctx.
    let log = unsafe { &mut *user.cast::<Log>() };
    let energy = key("energy");
    for &g in &log.guards {
        let mut v = -1.0;
        let status = unsafe { ponko_ctx_ai_num(ctx, g, energy, &mut v) };
        log.read
            .push((g, 0, PonkoVec2::default(), PonkoVec3::default(), v, status == PONKO_OK));
    }
}

#[test]
fn blackboard_rows_are_written_from_query_par_threads() {
    let mut log = Log::default();
    let user: *mut c_void = (&raw mut log).cast();
    let game = new_game();
    unsafe {
        assert_eq!(
            ponko_game_sim_system(game, c"senses".as_ptr(), 1, Some(senses_par), user),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_sim_system(game, c"check".as_ptr(), 1, Some(check_par), user),
            PONKO_OK
        );
    }
    // Enough rows that the query splits them over several threads.
    let world = run_game(game, 1, |ctx| {
        // SAFETY: the log outlives the run.
        add_guards(ctx, unsafe { &mut *user.cast::<Log>() }, 5000, 1);
    });
    unsafe { ponko_world_free(world) };
    assert_eq!(log.read.len(), 5000);
    for (g, _, _, _, energy, found) in &log.read {
        assert!(*found);
        assert_eq!(*energy, (g & 0xffff_ffff) as f32, "each row its own number");
    }
}

#[test]
fn brain_columns_are_only_for_queries() {
    let world = run_game(new_game(), 0, |ctx| unsafe {
        let w = ponko_ctx_world(ctx);
        let [brain, board] = columns(w);
        assert_ne!(brain, board);
        // The same ids every time.
        assert_eq!(columns(w), [brain, board]);
        let g = spawn(ctx);
        assert_eq!(ponko_ctx_ai_add_brain(ctx, g, c"anything".as_ptr(), 1), PONKO_OK);
        // No C layout: get, set and remove refuse them.
        let mut bytes = [0u8; 512];
        assert_eq!(
            ponko_get(w, g, brain, bytes.as_mut_ptr().cast(), bytes.len()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_set(w, g, board, bytes.as_ptr().cast(), bytes.len()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_remove(w, g, brain), PONKO_INVALID_ARGUMENT);
        // Null rows are refused by every row call.
        let mut k = 0;
        assert_eq!(
            ponko_ai_brain_action(std::ptr::null(), 0, &mut k),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ai_blackboard_set_num(std::ptr::null_mut(), key("x"), 1.0),
            PONKO_INVALID_ARGUMENT
        );
    });
    unsafe { ponko_world_free(world) };
}

#[test]
fn blackboard_entities_and_points_read_back_from_world_and_native_brains() {
    let (target, home, at) = (key("target"), key("home"), key("at"));
    let world = run_game(new_game(), 0, |ctx| unsafe {
        let g = spawn(ctx);
        let mut e = 0;
        let (mut p2, mut p3) = (PonkoVec2::default(), PonkoVec3::default());
        // No blackboard yet.
        assert_eq!(ponko_ctx_ai_entity(ctx, g, target, &mut e), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_ai_add_brain(ctx, g, c"anything".as_ptr(), 1), PONKO_OK);
        assert_eq!(ponko_ctx_ai_set_entity(ctx, g, target, g), PONKO_OK);
        assert_eq!(
            ponko_ctx_ai_set_vec2(ctx, g, home, PonkoVec2 { x: 1.0, y: 2.0 }),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ai_set_vec3(ctx, g, at, PonkoVec3 { x: 1.0, y: 2.0, z: 3.0 }),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ai_entity(ctx, g, target, &mut e),
            PONKO_OK,
            "{}",
            last_error()
        );
        assert_eq!(ponko_ctx_ai_vec2(ctx, g, home, &mut p2), PONKO_OK);
        assert_eq!(ponko_ctx_ai_vec3(ctx, g, at, &mut p3), PONKO_OK);
        assert_eq!(e, g);
        assert_eq!((p2.x, p2.y), (1.0, 2.0));
        assert_eq!((p3.x, p3.y, p3.z), (1.0, 2.0, 3.0));
        // Another kind of value at the key, or no such key: not found.
        assert_eq!(ponko_ctx_ai_vec2(ctx, g, at, &mut p2), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_ai_vec3(ctx, g, home, &mut p3), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_ai_entity(ctx, g, home, &mut e), PONKO_NOT_FOUND);
        assert!(last_error().contains("no entity"), "{}", last_error());
        assert_eq!(ponko_ctx_ai_entity(ctx, g, 12_345_678, &mut e), PONKO_NOT_FOUND);
        assert_eq!(
            ponko_ctx_ai_vec2(ctx, g, home, std::ptr::null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    });
    unsafe { ponko_world_free(world) };

    // A native brain keeps its blackboard itself.
    unsafe {
        let brain = ponko_brain_new(1);
        let (mut e, mut p2, mut p3) = (0, PonkoVec2::default(), PonkoVec3::default());
        assert_eq!(ponko_brain_entity(brain, target, &mut e), PONKO_NOT_FOUND);
        assert_eq!(ponko_brain_set_entity(brain, target, 42), PONKO_OK);
        assert_eq!(
            ponko_brain_set_vec2(brain, home, PonkoVec2 { x: -1.0, y: 0.5 }),
            PONKO_OK
        );
        assert_eq!(
            ponko_brain_set_vec3(brain, at, PonkoVec3 { x: 4.0, y: 5.0, z: 6.0 }),
            PONKO_OK
        );
        assert_eq!(ponko_brain_entity(brain, target, &mut e), PONKO_OK);
        assert_eq!(ponko_brain_vec2(brain, home, &mut p2), PONKO_OK);
        assert_eq!(ponko_brain_vec3(brain, at, &mut p3), PONKO_OK);
        assert_eq!(e, 42);
        assert_eq!((p2.x, p2.y), (-1.0, 0.5));
        assert_eq!((p3.x, p3.y, p3.z), (4.0, 5.0, 6.0));
        assert_eq!(ponko_brain_vec3(brain, home, &mut p3), PONKO_NOT_FOUND);
        assert_eq!(ponko_brain_vec2(brain, target, &mut p2), PONKO_NOT_FOUND);
        assert_eq!(
            ponko_brain_entity(std::ptr::null(), target, &mut e),
            PONKO_INVALID_ARGUMENT
        );
        ponko_brain_free(brain);
    }
}
