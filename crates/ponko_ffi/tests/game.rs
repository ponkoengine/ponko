//! The C game API: systems, the context, commands, parallel queries, spatial queries.
use ponko_ffi::*;
use std::ffi::{CStr, c_void};
use std::ptr::{null, null_mut};

use PonkoStatus::*;

/// Runs `game` headless for `ticks` and returns the world.
unsafe fn run(game: *mut PonkoGame, ticks: u64, setup: PonkoSystemFn, user: *mut c_void) -> *mut PonkoWorld {
    let mut world = null_mut();
    assert_eq!(
        unsafe { ponko_game_run_headless(game, ticks, setup, user, &mut world) },
        PONKO_OK
    );
    world
}
fn game() -> *mut PonkoGame {
    unsafe { ponko_game_new(c"test".as_ptr(), 1, 1) }
}
fn len(world: *mut PonkoWorld) -> usize {
    let mut n = 0;
    assert_eq!(unsafe { ponko_world_len(world, &mut n) }, PONKO_OK);
    n
}
fn user<T>(t: &mut T) -> *mut c_void {
    (t as *mut T).cast()
}

#[derive(Default)]
struct Log {
    setup: u32,
    ticks: Vec<(u64, f32)>,
    every_3: Vec<u64>,
    stop_at: Option<u64>,
}

unsafe extern "C" fn log_setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        (*user.cast::<Log>()).setup += 1;
        assert_eq!(ponko_world_free(ponko_ctx_world(ctx)), PONKO_BUSY);
    }
}
unsafe extern "C" fn log_tick(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let log = &mut *user.cast::<Log>();
        let tick = ponko_ctx_sim_tick(ctx);
        log.ticks.push((tick, ponko_ctx_dt(ctx)));
        if log.stop_at == Some(tick) {
            assert_eq!(ponko_ctx_quit(ctx), PONKO_OK);
        }
    }
}
unsafe extern "C" fn log_every_3(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe { (*user.cast::<Log>()).every_3.push(ponko_ctx_sim_tick(ctx)) }
}

#[test]
fn sim_systems_run_on_the_fixed_tick_and_quit_stops_early() {
    let mut log = Log {
        stop_at: Some(4),
        ..Default::default()
    };
    let log_ptr = user(&mut log);
    unsafe {
        let g = game();
        assert_eq!(ponko_game_fixed_hz(g, 120), PONKO_OK);
        assert_eq!(
            ponko_game_sim_system(g, c"log".as_ptr(), 1, Some(log_tick), log_ptr),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_sim_system(g, null(), 3, Some(log_every_3), log_ptr),
            PONKO_OK
        );
        let world = run(g, 100, Some(log_setup), log_ptr);
        ponko_world_free(world);
    }
    assert_eq!(log.setup, 1);
    assert_eq!(log.ticks.iter().map(|t| t.0).collect::<Vec<_>>(), [0, 1, 2, 3, 4]);
    assert!(log.ticks.iter().all(|t| (t.1 - 1.0 / 120.0).abs() < 1e-7));
    assert_eq!(log.every_3.len(), 2, "{:?}", log.every_3);
}

#[test]
fn bad_settings_are_errors_and_keep_the_game_usable() {
    unsafe {
        let g = game();
        assert_eq!(ponko_game_fixed_hz(g, 0), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_game_spatial_grid(g, -1.0), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_game_sim_system(g, null(), 0, Some(log_tick), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_game_sim_system(g, null(), 1, None, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_game_remove_system(g, c"nope".as_ptr()), PONKO_NOT_FOUND);
        assert_eq!(
            ponko_game_remove_system(g, c"ponko::ecs::draw_sprites".as_ptr()),
            PONKO_OK
        );
        assert_eq!(ponko_game_fixed_hz(g, 30), PONKO_OK);
        assert_eq!(ponko_game_inspector(g, true), PONKO_OK);
        assert_eq!(ponko_game_inspector(g, false), PONKO_OK);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, 1, None, null_mut(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        // The game was consumed by the failed run, so only a fresh one runs.
        let g = game();
        assert_eq!(ponko_game_run_headless(g, 1, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
}

// ---- system ids, setup callbacks and plugins -----------------------------------

/// What ran, in order: a letter per callback.
#[derive(Default)]
struct Order(String);

macro_rules! mark {
    ($name:ident, $letter:literal) => {
        unsafe extern "C" fn $name(_: *mut PonkoCtx, user: *mut c_void) {
            unsafe { (*user.cast::<Order>()).0.push($letter) }
        }
    };
}
mark!(mark_a, 'a');
mark!(mark_b, 'b');
mark!(mark_c, 'c');
mark!(mark_s, 's');
mark!(mark_t, 't');
mark!(mark_r, 'r');

#[test]
fn systems_are_removed_by_the_id_adding_them_returned() {
    let mut order = Order::default();
    let user = user(&mut order);
    unsafe {
        let g = game();
        let (mut a, mut b, mut c) = (0, 0, 0);
        let after = PonkoPhase::PONKO_PHASE_AFTER_PHYSICS as u32;
        // The same function twice: only ids tell the two apart.
        assert_eq!(
            ponko_game_add_system_id(g, after, c"same".as_ptr(), 1, Some(mark_a), user, &mut a),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_add_system_id(g, after, c"same".as_ptr(), 1, Some(mark_b), user, &mut b),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_add_system_id(g, after, null(), 1, Some(mark_c), user, &mut c),
            PONKO_OK
        );
        assert!(a != 0 && b != 0 && c != 0 && a != b && b != c);
        assert_eq!(ponko_game_remove(g, a), PONKO_OK);
        // Removing again, or an id that names nothing, is not an error.
        assert_eq!(ponko_game_remove(g, a), PONKO_OK);
        assert_eq!(ponko_game_remove(g, PONKO_SYSTEM_STEP_NAVMESH3D), PONKO_OK);
        ponko_world_free(run(g, 2, None, null_mut()));
    }
    assert_eq!(order.0, "bcbc");
}

#[test]
fn built_in_systems_are_removed_by_their_constants() {
    unsafe {
        let g = game();
        for id in [
            PONKO_SYSTEM_ANIMATE_SPRITES,
            PONKO_SYSTEM_DRAW_TILEMAPS,
            PONKO_SYSTEM_DRAW_SPRITES,
            PONKO_SYSTEM_DRAW_PARTICLES,
            PONKO_SYSTEM_UPDATE_ANIMATORS,
            PONKO_SYSTEM_UPDATE_PARTICLES,
            PONKO_SYSTEM_UPDATE_AUDIO3D,
            PONKO_SYSTEM_UPDATE_SKELETONS2D,
        ] {
            assert_eq!(ponko_game_remove(g, id), PONKO_OK);
        }
        // Gone: the name no longer finds the sprite drawing.
        assert_eq!(
            ponko_game_remove_system(g, c"ponko::ecs::draw_sprites".as_ptr()),
            PONKO_NOT_FOUND
        );
        assert_eq!(ponko_game_remove(null_mut(), 1), PONKO_INVALID_ARGUMENT);
        ponko_game_free(g);
    }
    // The constants are Rust's `Systems` ids.
    use ponko::prelude::Systems;
    let pairs = [
        (PONKO_SYSTEM_PHYSICS, Systems::PHYSICS),
        (PONKO_SYSTEM_PHYSICS3D, Systems::PHYSICS3D),
        (PONKO_SYSTEM_INTERPOLATE_PHYSICS3D, Systems::INTERPOLATE_PHYSICS3D),
        (PONKO_SYSTEM_REBUILD_SPATIAL_GRID, Systems::REBUILD_SPATIAL_GRID),
        (PONKO_SYSTEM_ANIMATE_SPRITES, Systems::ANIMATE_SPRITES),
        (PONKO_SYSTEM_DRAW_TILEMAPS, Systems::DRAW_TILEMAPS),
        (PONKO_SYSTEM_DRAW_SPRITES, Systems::DRAW_SPRITES),
        (PONKO_SYSTEM_FOLLOW_ATTACHMENTS, Systems::FOLLOW_ATTACHMENTS),
        (PONKO_SYSTEM_STEP_NAVMESH3D, Systems::STEP_NAVMESH3D),
        (PONKO_SYSTEM_UPDATE_ANIMATORS, Systems::UPDATE_ANIMATORS),
        (PONKO_SYSTEM_UPDATE_PARTICLES, Systems::UPDATE_PARTICLES),
        (PONKO_SYSTEM_UPDATE_AUDIO3D, Systems::UPDATE_AUDIO3D),
        (PONKO_SYSTEM_DRAW_PARTICLES, Systems::DRAW_PARTICLES),
        (PONKO_SYSTEM_UPDATE_SKELETONS2D, Systems::UPDATE_SKELETONS2D),
    ];
    for (c, rust) in pairs {
        assert_eq!(c, rust.to_bits());
    }
}

#[test]
fn adding_with_an_id_rejects_what_adding_rejects() {
    unsafe {
        let g = game();
        let mut id = 9;
        let frame = PonkoPhase::PONKO_PHASE_FRAME as u32;
        assert_eq!(
            ponko_game_add_system_id(g, frame, null(), 2, Some(mark_a), null_mut(), &mut id),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(id, 0);
        assert_eq!(
            ponko_game_add_system_id(g, 99, null(), 1, Some(mark_a), null_mut(), &mut id),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_game_add_system_id(g, frame, null(), 1, None, null_mut(), &mut id),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_game_add_system_id(g, frame, null(), 1, Some(mark_a), null_mut(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        ponko_game_free(g);
    }
}

#[test]
fn setup_callbacks_run_once_in_order_before_the_run_setup() {
    let mut order = Order::default();
    let user = user(&mut order);
    unsafe {
        let g = game();
        assert_eq!(ponko_game_setup(g, Some(mark_s), user), PONKO_OK);
        assert_eq!(ponko_game_sim_system(g, null(), 1, Some(mark_a), user), PONKO_OK);
        assert_eq!(ponko_game_setup(g, Some(mark_t), user), PONKO_OK);
        assert_eq!(ponko_game_setup(g, None, user), PONKO_INVALID_ARGUMENT);
        ponko_world_free(run(g, 2, Some(mark_r), user));
    }
    assert_eq!(order.0, "straa");

    // With zero ticks the setups still run.
    let mut order = Order::default();
    let user = self::user(&mut order);
    unsafe {
        let g = game();
        assert_eq!(ponko_game_setup(g, Some(mark_s), user), PONKO_OK);
        ponko_world_free(run(g, 0, None, null_mut()));
    }
    assert_eq!(order.0, "s");
}

/// A plugin's build step: one system per phase it cares about.
unsafe extern "C" fn build_area(game: *mut PonkoGame, user: *mut c_void) {
    unsafe {
        let before = PonkoPhase::PONKO_PHASE_BEFORE_PHYSICS as u32;
        assert_eq!(
            ponko_game_add_system(game, before, null(), 1, Some(mark_b), user),
            PONKO_OK
        );
    }
}
unsafe extern "C" fn build_score(game: *mut PonkoGame, user: *mut c_void) {
    unsafe {
        assert_eq!(ponko_game_sim_system(game, null(), 1, Some(mark_a), user), PONKO_OK);
    }
}

#[test]
fn plugins_build_at_once_and_set_up_in_the_order_they_were_added() {
    let mut order = Order::default();
    let user = user(&mut order);
    unsafe {
        let g = game();
        // `score` is added first, but its system is in a later phase.
        assert_eq!(ponko_game_plugin(g, Some(build_score), Some(mark_s), user), PONKO_OK);
        assert_eq!(ponko_game_setup(g, Some(mark_c), user), PONKO_OK);
        assert_eq!(ponko_game_plugin(g, Some(build_area), Some(mark_t), user), PONKO_OK);
        // Setup only, and build only.
        assert_eq!(ponko_game_plugin(g, None, Some(mark_r), user), PONKO_OK);
        assert_eq!(ponko_game_plugin(g, Some(build_score), None, user), PONKO_OK);
        assert_eq!(ponko_game_plugin(g, None, None, user), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_game_plugin(null_mut(), Some(build_area), None, user),
            PONKO_INVALID_ARGUMENT
        );
        ponko_world_free(run(g, 1, None, null_mut()));
    }
    assert_eq!(order.0, "sctrbaa");
}

// ---- commands and busy rules ---------------------------------------------------

struct Ids {
    health: u32,
    seen: Vec<PonkoStatus>,
}

unsafe extern "C" fn spawn_hurt(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let ids = &mut *user.cast::<Ids>();
        let w = ponko_ctx_world(ctx);
        assert_eq!(
            ponko_component_register(w, c"Health".as_ptr(), 4, 4, &mut ids.health),
            PONKO_OK
        );
        for hp in [0i32, 5, 0, 7] {
            let v = PonkoValue {
                component: ids.health,
                data: (&hp as *const i32).cast(),
                size: 4,
            };
            assert_eq!(ponko_spawn(w, &v, 1, &mut 0), PONKO_OK);
        }
    }
}
struct Reaper {
    ctx: *mut PonkoCtx,
    ids: *mut Ids,
}
unsafe extern "C" fn reap_chunk(chunk: *const PonkoChunk, user: *mut c_void) {
    unsafe {
        let r = &*user.cast::<Reaper>();
        let ids = &mut *r.ids;
        let c = &*chunk;
        let hp = (*c.columns).cast::<i32>();
        for i in 0..c.count {
            if *hp.add(i) <= 0 {
                ids.seen
                    .push(ponko_ctx_despawn_later(r.ctx, ponko_entity_bits(*c.entities.add(i))));
                let one = 1i32;
                let v = PonkoValue {
                    component: ids.health,
                    data: (&one as *const i32).cast(),
                    size: 4,
                };
                ids.seen.push(ponko_ctx_spawn_later(r.ctx, &v, 1));
            }
        }
        // The ctx (not just the world) is busy while its world is borrowed.
        ids.seen.push(ponko_ctx_quit(r.ctx));
        ids.seen.push(ponko_ctx_pause(r.ctx));
        let mut down = false;
        ids.seen.push(ponko_ctx_key_down(r.ctx, 0, &mut down));
    }
}
unsafe extern "C" fn reap(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let ids = user.cast::<Ids>();
        let health = (*ids).health;
        let mut r = Reaper { ctx, ids };
        let w = ponko_ctx_world(ctx);
        assert_eq!(
            ponko_query(w, &health, 1, Some(reap_chunk), (&mut r as *mut Reaper).cast()),
            PONKO_OK
        );
        // Commands apply after the system, not now.
        let mut n = 0;
        ponko_world_len(w, &mut n);
        assert_eq!(n, 4);
    }
}

#[test]
fn commands_work_inside_queries_and_apply_after_the_system() {
    let mut ids = Ids {
        health: 0,
        seen: vec![],
    };
    let ids_ptr = user(&mut ids);
    unsafe {
        let g = game();
        ponko_game_sim_system(g, c"reap".as_ptr(), 1, Some(reap), ids_ptr);
        let world = run(g, 1, Some(spawn_hurt), ids_ptr);
        assert_eq!(len(world), 4);
        // Two zero-health entities were replaced by two with 1 hp.
        let mut total = 0;
        unsafe extern "C" fn sum(chunk: *const PonkoChunk, user: *mut c_void) {
            unsafe {
                let c = &*chunk;
                for i in 0..c.count {
                    *user.cast::<i32>() += *(*c.columns).cast::<i32>().add(i);
                }
            }
        }
        ponko_query(world, &ids.health, 1, Some(sum), user(&mut total));
        assert_eq!(total, 5 + 7 + 1 + 1);
        ponko_world_free(world);
    }
    let ok = ids.seen.iter().filter(|s| **s == PONKO_OK).count();
    let busy = ids.seen.iter().filter(|s| **s == PONKO_BUSY).count();
    assert_eq!((ok, busy), (4, 3), "{:?}", ids.seen);
}

#[test]
fn bad_spawn_later_values_are_skipped_when_applied() {
    unsafe extern "C" fn bad(ctx: *mut PonkoCtx, _: *mut c_void) {
        unsafe {
            let x = 1u8;
            let v = PonkoValue {
                component: 999,
                data: (&x as *const u8).cast(),
                size: 1,
            };
            assert_eq!(ponko_ctx_spawn_later(ctx, &v, 1), PONKO_OK);
            assert_eq!(ponko_ctx_spawn_later(ctx, null(), 1), PONKO_INVALID_ARGUMENT);
            assert_eq!(ponko_ctx_spawn_later(ctx, null(), 0), PONKO_OK);
        }
    }
    unsafe {
        let g = game();
        ponko_game_sim_system(g, null(), 1, Some(bad), null_mut());
        let world = run(g, 1, None, null_mut());
        assert_eq!(len(world), 1, "only the empty spawn applies");
        ponko_world_free(world);
    }
}

// ---- parallel queries -------------------------------------------------------

struct Par {
    ctx: *mut PonkoCtx,
    rejected: std::sync::atomic::AtomicUsize,
    threads: std::sync::Mutex<std::collections::HashSet<std::thread::ThreadId>>,
}
unsafe extern "C" fn double(chunk: *const PonkoChunk, user: *mut c_void) {
    unsafe {
        let p = &*user.cast::<Par>();
        let c = &*chunk;
        let v = (*c.columns).cast::<u32>();
        for i in 0..c.count {
            *v.add(i) *= 2;
        }
        if ponko_ctx_despawn_later(p.ctx, 0) == PONKO_BUSY {
            p.rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        p.threads.lock().unwrap().insert(std::thread::current().id());
    }
}
unsafe extern "C" fn par_system(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        let w = ponko_ctx_world(ctx);
        let mut id = 0;
        ponko_component_register(w, c"N".as_ptr(), 4, 4, &mut id);
        let p = Par {
            ctx,
            rejected: 0.into(),
            threads: Default::default(),
        };
        assert_eq!(
            ponko_query_par(w, &id, 1, Some(double), (&p as *const Par).cast_mut().cast()),
            PONKO_OK
        );
        assert!(p.rejected.load(std::sync::atomic::Ordering::Relaxed) > 0);
        // Commands are back once the parallel query is done.
        assert_eq!(ponko_ctx_spawn_later(ctx, null(), 0), PONKO_OK);
    }
}
unsafe extern "C" fn spawn_numbers(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        let w = ponko_ctx_world(ctx);
        let mut id = 0;
        ponko_component_register(w, c"N".as_ptr(), 4, 4, &mut id);
        let n = if cfg!(miri) { 100u32 } else { 100_000 };
        for i in 0..n {
            let v = PonkoValue {
                component: id,
                data: (&i as *const u32).cast(),
                size: 4,
            };
            ponko_spawn(w, &v, 1, &mut 0);
        }
    }
}

#[test]
#[cfg_attr(miri, ignore = "rayon needs tree borrows: see AGENTS.md (run with MIRIFLAGS)")]
fn parallel_queries_visit_every_row_once_and_turn_commands_off() {
    unsafe {
        let g = game();
        ponko_game_sim_system(g, null(), 1, Some(par_system), null_mut());
        let world = run(g, 1, Some(spawn_numbers), null_mut());
        let mut id = 0;
        ponko_component_register(world, c"N".as_ptr(), 4, 4, &mut id);
        let mut sum = 0u64;
        unsafe extern "C" fn add(chunk: *const PonkoChunk, user: *mut c_void) {
            unsafe {
                let c = &*chunk;
                for i in 0..c.count {
                    *user.cast::<u64>() += *(*c.columns).cast::<u32>().add(i) as u64;
                }
            }
        }
        ponko_query(world, &id, 1, Some(add), user(&mut sum));
        let n = if cfg!(miri) { 100u64 } else { 100_000 };
        assert_eq!(sum, n * (n - 1)); // 2 * (0 + 1 + ... + n-1)
        ponko_world_free(world);
    }
}

/// Values seen at each query row number (`first + i`).
struct Rows(Vec<std::sync::atomic::AtomicU32>);
unsafe extern "C" fn record_rows(chunk: *const PonkoChunk, user: *mut c_void) {
    unsafe {
        let (c, rows) = (&*chunk, &*user.cast::<Rows>());
        for i in 0..c.count {
            let v = *(*c.columns).cast::<u32>().add(i);
            rows.0[c.first + i].store(v + 1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

#[test]
#[cfg_attr(miri, ignore = "rayon needs tree borrows: see AGENTS.md (run with MIRIFLAGS)")]
fn chunks_number_their_rows_the_same_way_in_both_queries() {
    unsafe {
        let world = ponko_world_new();
        let (mut n, mut m) = (0, 0);
        ponko_component_register(world, c"N".as_ptr(), 4, 4, &mut n);
        ponko_component_register(world, c"M".as_ptr(), 4, 4, &mut m);
        let total = 30_000u32;
        for i in 0..total {
            let v = PonkoValue {
                component: n,
                data: (&i as *const u32).cast(),
                size: 4,
            };
            let extra = PonkoValue {
                component: m,
                data: (&i as *const u32).cast(),
                size: 4,
            };
            // Two archetypes, interleaved.
            let values = [v, extra];
            ponko_spawn(world, values.as_ptr(), if i % 3 == 0 { 2 } else { 1 }, &mut 0);
        }
        let fresh = || Rows((0..total).map(|_| 0.into()).collect());
        let (seq, par) = (fresh(), fresh());
        let ptr = |r: &Rows| (r as *const Rows).cast_mut().cast();
        assert_eq!(ponko_query(world, &n, 1, Some(record_rows), ptr(&seq)), PONKO_OK);
        assert_eq!(ponko_query_par(world, &n, 1, Some(record_rows), ptr(&par)), PONKO_OK);
        let load = |r: &Rows| {
            r.0.iter()
                .map(|a| a.load(std::sync::atomic::Ordering::Relaxed))
                .collect::<Vec<_>>()
        };
        let (seq, par) = (load(&seq), load(&par));
        assert!(seq.iter().all(|&v| v != 0), "every row number in 0..count is used");
        assert_eq!(seq, par);
        ponko_world_free(world);
    }
}

// ---- built-in components, spatial queries, random ---------------------------

unsafe extern "C" fn spawn_grid(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        let w = ponko_ctx_world(ctx);
        let (mut t, mut s) = (0, 0);
        assert_eq!(ponko_transform_component(w, &mut t), PONKO_OK);
        assert_eq!(ponko_sprite_component(w, &mut s), PONKO_OK);
        let mut tex = PonkoTexture {
            id: 0,
            width: 0,
            height: 0,
        };
        let px = [255u8; 16];
        assert_eq!(ponko_ctx_create_texture(ctx, 2, 2, px.as_ptr(), 16, &mut tex), PONKO_OK);
        assert_eq!(
            ponko_ctx_create_texture(ctx, 2, 2, px.as_ptr(), 15, &mut tex),
            PONKO_INVALID_ARGUMENT
        );
        for x in 0..10 {
            let transform = PonkoTransform2D {
                position: PonkoVec2 {
                    x: x as f32 * 10.0,
                    y: 0.0,
                },
                ..Default::default()
            };
            let sprite = PonkoSprite {
                texture: tex,
                z: x as f32,
                ..Default::default()
            };
            let values = [
                PonkoValue {
                    component: t,
                    data: (&transform as *const PonkoTransform2D).cast(),
                    size: size_of::<PonkoTransform2D>(),
                },
                PonkoValue {
                    component: s,
                    data: (&sprite as *const PonkoSprite).cast(),
                    size: size_of::<PonkoSprite>(),
                },
            ];
            assert_eq!(ponko_spawn(w, values.as_ptr(), 2, &mut 0), PONKO_OK);
        }
    }
}
unsafe extern "C" fn neighbors(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let mut hits = [PonkoGridHit {
            entity: PonkoEntity {
                index: 0,
                generation: 0,
            },
            position: Default::default(),
        }; 2];
        let mut n = 0;
        let center = PonkoVec2 { x: 45.0, y: 0.0 };
        assert_eq!(
            ponko_ctx_spatial_radius(ctx, center, 16.0, hits.as_mut_ptr(), 2, &mut n),
            PONKO_OK
        );
        // 30, 40, 50, 60 are within 16 of 45: four hits, two written.
        *user.cast::<(usize, f32, f32)>() = (n, hits[0].position.x.min(hits[1].position.x), 0.0);
        let mut near = hits[0];
        assert_eq!(
            ponko_ctx_spatial_nearest(ctx, PonkoVec2 { x: 1.0, y: 0.0 }, 100.0, u64::MAX, &mut near),
            PONKO_OK
        );
        (*user.cast::<(usize, f32, f32)>()).2 = near.position.x;
        assert_eq!(
            ponko_ctx_spatial_radius(ctx, center, 16.0, null_mut(), 0, &mut n),
            PONKO_OK
        );
        assert_eq!(n, 4);
    }
}

#[test]
fn builtin_components_round_trip_and_feed_the_spatial_grid() {
    let mut found = (0usize, 0.0f32, -1.0f32);
    unsafe {
        let g = game();
        ponko_game_spatial_grid(g, 16.0);
        ponko_game_sim_system(g, null(), 1, Some(neighbors), user(&mut found));
        let world = run(g, 1, Some(spawn_grid), null_mut());
        let (mut t, mut s) = (0, 0);
        ponko_transform_component(world, &mut t);
        ponko_sprite_component(world, &mut s);
        // Sprites can be read and written, but not used as query columns.
        let e = 9u64; // tenth entity, generation 0
        let mut sprite = PonkoSprite::default();
        assert_eq!(
            ponko_get(
                world,
                e,
                s,
                (&mut sprite as *mut PonkoSprite).cast(),
                size_of::<PonkoSprite>()
            ),
            PONKO_OK
        );
        assert_eq!((sprite.z, sprite.texture.width), (9.0, 2));
        sprite.has_region = true;
        sprite.region.max = PonkoVec2 { x: 1.0, y: 1.0 };
        assert_eq!(
            ponko_set(
                world,
                e,
                s,
                (&sprite as *const PonkoSprite).cast(),
                size_of::<PonkoSprite>()
            ),
            PONKO_OK
        );
        let mut back = PonkoSprite::default();
        ponko_get(
            world,
            e,
            s,
            (&mut back as *mut PonkoSprite).cast(),
            size_of::<PonkoSprite>(),
        );
        assert_eq!(back, sprite);
        unsafe extern "C" fn never(_: *const PonkoChunk, _: *mut c_void) {}
        assert_eq!(
            ponko_query(world, &s, 1, Some(never), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        let mut transform = PonkoTransform2D::default();
        assert_eq!(
            ponko_get(
                world,
                e,
                t,
                (&mut transform as *mut PonkoTransform2D).cast(),
                size_of::<PonkoTransform2D>()
            ),
            PONKO_OK
        );
        assert_eq!(transform.position.x, 90.0);
        assert_eq!(ponko_world_sort_spatial(world, 16.0), PONKO_OK);
        ponko_world_free(world);
    }
    assert_eq!(found.0, 4);
    assert!(found.1 >= 30.0);
    assert_eq!(found.2, 0.0);
}

unsafe extern "C" fn record_bits(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe { (*user.cast::<Vec<u64>>()).push(ponko_ctx_random_bits(ctx)) }
}

#[test]
fn random_bits_repeat_with_the_seed_and_change_every_tick() {
    let mut runs = vec![];
    for seed in [1, 1, 2] {
        let mut bits: Vec<u64> = vec![];
        unsafe {
            let g = game();
            ponko_game_seed(g, seed);
            ponko_game_sim_system(g, null(), 1, Some(record_bits), user(&mut bits));
            ponko_world_free(run(g, 3, None, null_mut()));
        }
        runs.push(bits);
    }
    assert_eq!(runs[0], runs[1]);
    assert_ne!(runs[0], runs[2]);
    assert_ne!(runs[0][0], runs[0][1]);
}

#[test]
#[cfg_attr(miri, ignore = "asset loading needs the real file system")]
fn missing_textures_fail_with_the_engine_message() {
    unsafe extern "C" fn load(ctx: *mut PonkoCtx, user: *mut c_void) {
        unsafe {
            let mut t = PonkoTexture {
                id: 0,
                width: 0,
                height: 0,
            };
            *user.cast::<PonkoStatus>() = ponko_ctx_load_texture(ctx, c"no/such/file.png".as_ptr(), &mut t);
        }
    }
    let mut status = PONKO_OK;
    unsafe {
        ponko_world_free(run(game(), 0, Some(load), user(&mut status)));
    }
    assert_eq!(status, PONKO_PANIC);
    let message = unsafe { CStr::from_ptr(ponko_last_error()) }.to_str().unwrap();
    assert!(message.contains("no/such/file.png"), "{message}");
}

#[test]
fn owned_grids_answer_radius_queries_with_user_data() {
    unsafe {
        let g = ponko_grid_new(10.0);
        assert!(ponko_grid_new(0.0).is_null());
        let items: Vec<_> = (0..50)
            .map(|i| PonkoGridItem {
                entity: PonkoEntity {
                    index: i,
                    generation: 0,
                },
                position: PonkoVec2 { x: i as f32, y: 0.0 },
                data: i * 2,
            })
            .collect();
        assert_eq!(ponko_grid_rebuild(g, items.as_ptr(), items.len()), PONKO_OK);
        assert_eq!(ponko_grid_len(g), 50);
        let mut out = vec![items[0]; 64];
        let mut n = 0;
        assert_eq!(
            ponko_grid_radius(
                g,
                PonkoVec2 { x: 25.0, y: 0.0 },
                2.5,
                out.as_mut_ptr(),
                out.len(),
                &mut n
            ),
            PONKO_OK
        );
        let mut data: Vec<_> = out[..n].iter().map(|i| i.data).collect();
        data.sort();
        assert_eq!(data, [46, 48, 50, 52, 54]);
        ponko_grid_free(g);
    }
}

/// Status codes and values from every gamepad call, in a headless run (no pads).
unsafe extern "C" fn check_gamepads(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let seen = &mut *user.cast::<Vec<(PonkoStatus, f32)>>();
        let mut count = 99;
        seen.push((ponko_ctx_gamepad_count(ctx, &mut count), count as f32));
        let mut pad = 0;
        seen.push((ponko_ctx_gamepad_at(ctx, 0, &mut pad), 0.0));
        let mut down = true;
        let south = PonkoGamepadButton::PONKO_GAMEPAD_SOUTH as u32;
        seen.push((
            ponko_ctx_gamepad_down(ctx, 0, south, &mut down),
            f32::from(u8::from(down)),
        ));
        seen.push((
            ponko_ctx_gamepad_pressed(ctx, 1, south, &mut down),
            f32::from(u8::from(down)),
        ));
        seen.push((
            ponko_ctx_gamepad_released(ctx, 7, south, &mut down),
            f32::from(u8::from(down)),
        ));
        let mut value = 5.0;
        let stick_y = PonkoGamepadAxis::PONKO_GAMEPAD_LEFT_STICK_Y as u32;
        seen.push((ponko_ctx_gamepad_axis(ctx, 0, stick_y, &mut value), value));
        // Out of range slot, button and axis.
        seen.push((ponko_ctx_gamepad_down(ctx, 8, south, &mut down), 0.0));
        seen.push((ponko_ctx_gamepad_down(ctx, 0, u32::MAX, &mut down), 0.0));
        seen.push((ponko_ctx_gamepad_axis(ctx, 0, 6, &mut value), 0.0));
    }
}

#[test]
fn gamepad_calls_read_empty_slots_and_reject_bad_arguments() {
    let mut seen: Vec<(PonkoStatus, f32)> = vec![];
    unsafe {
        let g = game();
        ponko_game_sim_system(g, null(), 1, Some(check_gamepads), user(&mut seen));
        ponko_world_free(run(g, 1, None, null_mut()));
    }
    assert_eq!(
        seen,
        [
            (PONKO_OK, 0.0),               // no pads in a headless run
            (PONKO_INVALID_ARGUMENT, 0.0), // so none at index 0
            (PONKO_OK, 0.0),               // empty slots read as up
            (PONKO_OK, 0.0),
            (PONKO_OK, 0.0),
            (PONKO_OK, 0.0), // and centered
            (PONKO_INVALID_ARGUMENT, 0.0),
            (PONKO_INVALID_ARGUMENT, 0.0),
            (PONKO_INVALID_ARGUMENT, 0.0),
        ]
    );
}

unsafe extern "C" fn log_phase(_: *mut PonkoCtx, user: *mut c_void) {
    // `user` points at (log, this system's label).
    unsafe {
        let (log, label) = &mut *user.cast::<(*mut Vec<u32>, u32)>();
        (**log).push(*label);
    }
}

#[test]
fn add_system_runs_fixed_phases_in_phase_order() {
    use PonkoPhase::*;
    let mut log = Vec::new();
    let log_ptr: *mut Vec<u32> = &mut log;
    let mut users: Vec<(*mut Vec<u32>, u32)> = [
        PONKO_PHASE_SIM_LAST,
        PONKO_PHASE_AFTER_PHYSICS,
        PONKO_PHASE_FRAME,
        PONKO_PHASE_BEFORE_PHYSICS,
        PONKO_PHASE_INPUT,
    ]
    .iter()
    .map(|&p| (log_ptr, p as u32))
    .collect();
    unsafe {
        let g = game();
        // A plain sim system (AFTER_PHYSICS) added first still runs after BEFORE_PHYSICS.
        let mut sim = (log_ptr, 99);
        assert_eq!(
            ponko_game_sim_system(g, c"sim".as_ptr(), 1, Some(log_phase), user(&mut sim)),
            PONKO_OK
        );
        for u in &mut users {
            let phase = u.1;
            assert_eq!(
                ponko_game_add_system(g, phase, c"phase".as_ptr(), 1, Some(log_phase), user(u)),
                PONKO_OK
            );
        }
        let world = run(g, 1, None, null_mut());
        ponko_world_free(world);
    }
    let p = |p: PonkoPhase| p as u32;
    assert_eq!(
        log,
        [
            p(PONKO_PHASE_INPUT),
            p(PONKO_PHASE_BEFORE_PHYSICS),
            99,
            p(PONKO_PHASE_AFTER_PHYSICS),
            p(PONKO_PHASE_SIM_LAST),
        ]
    );
}

#[test]
fn add_system_rejects_bad_phases_and_intervals() {
    use PonkoPhase::*;
    unsafe {
        let g = game();
        let add =
            |phase: u32, every: u32, f: PonkoSystemFn| ponko_game_add_system(g, phase, null(), every, f, null_mut());
        assert_eq!(add(9, 1, Some(log_tick)), PONKO_INVALID_ARGUMENT);
        assert_eq!(add(PONKO_PHASE_INPUT as u32, 0, Some(log_tick)), PONKO_INVALID_ARGUMENT);
        assert_eq!(add(PONKO_PHASE_FRAME as u32, 2, Some(log_tick)), PONKO_INVALID_ARGUMENT);
        assert_eq!(add(PONKO_PHASE_INPUT as u32, 1, None), PONKO_INVALID_ARGUMENT);
        assert_eq!(add(PONKO_PHASE_SIM_LAST as u32, 3, Some(noop)), PONKO_OK);
        assert_eq!(add(PONKO_PHASE_DRAW as u32, 1, Some(noop)), PONKO_OK);
        ponko_world_free(run(g, 1, None, null_mut()));
    }
}
unsafe extern "C" fn noop(_: *mut PonkoCtx, _: *mut c_void) {}
