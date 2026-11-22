//! Rollback from C (`ponko_ctx_rollback_*`) and world snapshots
//! (`ponko_world_*_snapshot`): two C games on two threads trade packets
//! that arrive late and out of order and both end in the world an offline
//! run with the same inputs has; and the calls' error paths.
mod common;

use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use common::{new_game, run_ctx, run_game};
use ponko_ffi::*;

use PonkoStatus::*;

const DELAY: u32 = 2;
const ACTIVE: u64 = 60;
const FRAMES: u64 = 120;

/// One player's input: 2 bytes.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
#[repr(C)]
struct Pad {
    dx: i8,
    fire: u8,
}

/// The C component every player has.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
#[repr(C)]
struct Racer {
    x: f32,
    v: f32,
    shots: u32,
}

fn pad(p: u32, t: u64) -> Pad {
    if t >= ACTIVE {
        return Pad::default();
    }
    let h = (t / 3 + 1).wrapping_mul(2_654_435_761) + u64::from(p) * 40_503;
    Pad {
        dx: ((h >> 7) & 3) as i8 - 1,
        fire: ((h >> 11) & 1) as u8,
    }
}

/// Both peers meet here once a frame.
#[derive(Default)]
struct Wire {
    gate: Mutex<(u32, u64)>,
    turn: Condvar,
    /// `(to, due frame, sequence, bytes)`.
    packets: Mutex<Vec<(u32, u64, u64, Vec<u8>)>>,
}

impl Wire {
    fn meet(&self) {
        let mut s = self.gate.lock().unwrap();
        let generation = s.1;
        s.0 += 1;
        if s.0 == 2 {
            *s = (0, generation + 1);
            self.turn.notify_all();
            return;
        }
        while s.1 == generation {
            let (next, timeout) = self.turn.wait_timeout(s, Duration::from_secs(30)).unwrap();
            s = next;
            assert!(!timeout.timed_out() || s.1 != generation, "the other peer stopped");
        }
    }
}

/// One game's state, handed to its C systems.
struct Game {
    me: u32,
    online: bool,
    component: u32,
    racers: [u64; 2],
    wire: Option<Arc<Wire>>,
    frame: u64,
    seq: u64,
    noise: u64,
    /// The world at the start of each tick (offline) or of the last frame.
    seen: Vec<(u64, [Racer; 2])>,
    stats: PonkoRollbackStats,
    desync: bool,
    resimulated_ticks: u64,
}

unsafe fn racers(ctx: *mut PonkoCtx, g: &Game) -> [Racer; 2] {
    let world = unsafe { ponko_ctx_world(ctx) };
    g.racers.map(|e| {
        let mut r = Racer::default();
        let status = unsafe { ponko_get(world, e, g.component, (&raw mut r).cast(), size_of::<Racer>()) };
        assert_eq!(status, PONKO_OK);
        r
    })
}

unsafe extern "C" fn race(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let g = &mut *user.cast::<Game>();
        let t = ponko_ctx_sim_tick(ctx);
        let mut pads = [Pad::default(); 2];
        for p in 0..2 {
            pads[p as usize] = if g.online {
                let mut pad = Pad::default();
                assert_eq!(ponko_ctx_rollback_input(ctx, p, (&raw mut pad).cast(), 2), PONKO_OK);
                pad
            } else {
                t.checked_sub(u64::from(DELAY)).map_or(Pad::default(), |from| pad(p, from))
            };
        }
        if g.online {
            let mut again = false;
            assert_eq!(ponko_ctx_rollback_resimulating(ctx, &mut again), PONKO_OK);
            g.resimulated_ticks += u64::from(again);
        }
        let world = ponko_ctx_world(ctx);
        let mut now = racers(ctx, g);
        for (r, pad) in now.iter_mut().zip(pads) {
            r.v = r.v * 0.9 + f32::from(pad.dx);
            r.x += r.v / 60.0;
            if pad.fire != 0 && t.is_multiple_of(5) {
                r.shots += 1;
            }
        }
        for (e, r) in g.racers.iter().zip(&now) {
            assert_eq!(ponko_set(world, *e, g.component, (&raw const *r).cast(), size_of::<Racer>()), PONKO_OK);
        }
    }
}

unsafe extern "C" fn record(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let g = &mut *user.cast::<Game>();
        let now = (ponko_ctx_sim_tick(ctx), racers(ctx, g));
        g.seen.push(now);
    }
}

unsafe extern "C" fn network(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let g = &mut *user.cast::<Game>();
        let t = ponko_ctx_sim_tick(ctx);
        g.seen.clear();
        g.seen.push((t, racers(ctx, g)));
        let input = pad(g.me, t);
        assert_eq!(ponko_ctx_rollback_set_input(ctx, (&raw const input).cast(), 2), PONKO_OK);
        let (mut data, mut size) = (null(), 0);
        assert_eq!(ponko_ctx_rollback_packet(ctx, &mut data, &mut size), PONKO_OK);
        let bytes = std::slice::from_raw_parts(data, size).to_vec();
        g.noise = g.noise.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        g.seq += 1;
        let wire = g.wire.clone().expect("a peer has a wire");
        // 1 to 5 frames late: later packets often overtake earlier ones.
        wire.packets
            .lock()
            .unwrap()
            .push((1 - g.me, g.frame + 1 + (g.noise >> 33) % 5, g.seq, bytes));
        wire.meet();
        g.frame += 1;
        let mut due = Vec::new();
        {
            let mut packets = wire.packets.lock().unwrap();
            let mut i = 0;
            while i < packets.len() {
                if packets[i].0 == g.me && packets[i].1 <= g.frame {
                    due.push(packets.swap_remove(i));
                } else {
                    i += 1;
                }
            }
        }
        due.sort_by_key(|p| std::cmp::Reverse(p.2));
        for p in &due {
            let mut accepted = false;
            assert_eq!(ponko_ctx_rollback_receive(ctx, p.3.as_ptr().cast(), p.3.len(), &mut accepted), PONKO_OK);
            assert!(accepted);
        }
        assert_eq!(ponko_ctx_rollback_stats(ctx, &mut g.stats), PONKO_OK);
        let (mut frame, mut player) = (0, 0);
        assert_eq!(ponko_ctx_rollback_desync(ctx, &mut g.desync, &mut frame, &mut player), PONKO_OK);
    }
}

fn play(me: u32, wire: Option<Arc<Wire>>) -> Box<Game> {
    let online = wire.is_some();
    let mut g = Box::new(Game {
        me,
        online,
        component: 0,
        racers: [0; 2],
        wire,
        frame: 0,
        seq: 0,
        noise: 17 + u64::from(me),
        seen: Vec::new(),
        stats: PonkoRollbackStats::default(),
        desync: true,
        resimulated_ticks: 0,
    });
    let user = (&raw mut *g).cast::<c_void>();
    let game = new_game();
    unsafe {
        assert_eq!(ponko_game_headless_frames(game, true), PONKO_OK);
        assert_eq!(ponko_game_sim_system(game, c"race".as_ptr(), 1, Some(race), user), PONKO_OK);
        let frame_system = if online { network } else { record };
        assert_eq!(ponko_game_system(game, c"net".as_ptr(), Some(frame_system), user), PONKO_OK);
    }
    let ticks = if online { FRAMES } else { FRAMES + 1 };
    let world = run_game(game, ticks, |ctx| unsafe {
        let g = &mut *user.cast::<Game>();
        let w = ponko_ctx_world(ctx);
        assert_eq!(
            ponko_component_register(w, c"racer".as_ptr(), size_of::<Racer>(), 4, &mut g.component),
            PONKO_OK
        );
        for (i, e) in g.racers.iter_mut().enumerate() {
            let r = Racer {
                x: i as f32 * 10.0,
                ..Racer::default()
            };
            let value = PonkoValue {
                component: g.component,
                data: (&raw const r).cast(),
                size: size_of::<Racer>(),
            };
            assert_eq!(ponko_spawn(w, &value, 1, e), PONKO_OK);
        }
        if g.online {
            let mut config = ponko_rollback_config_default(2, g.me);
            config.input_delay = DELAY;
            config.checksum_every = 1;
            assert_eq!(ponko_ctx_rollback_start(ctx, &config), PONKO_OK);
            let input = pad(g.me, 0);
            assert_eq!(ponko_ctx_rollback_set_input(ctx, (&raw const input).cast(), 2), PONKO_OK);
            let (mut frame, mut has, mut confirmed) = (9, true, 9);
            assert_eq!(ponko_ctx_rollback_frames(ctx, &mut frame, &mut has, &mut confirmed), PONKO_OK);
            assert_eq!(frame, 0, "no tick ran yet");
        }
    });
    unsafe { ponko_world_free(world) };
    g
}

#[test]
fn two_c_peers_over_a_late_reordering_link_reach_the_offline_world() {
    let offline = play(0, None);
    let wire = Arc::new(Wire::default());
    let other = {
        let wire = Arc::clone(&wire);
        std::thread::spawn(move || play(1, Some(wire)))
    };
    let first = play(0, Some(wire));
    let second = other.join().expect("peer thread");
    for (i, peer) in [&first, &second].into_iter().enumerate() {
        assert!(!peer.desync, "peer {i}: the world hashes agree");
        assert!(peer.stats.rollbacks > 0, "peer {i} rolled back");
        assert_eq!(peer.resimulated_ticks, peer.stats.resimulated, "peer {i}: ticks run again");
        let (tick, last) = peer.seen[0];
        let reference = offline.seen.iter().find(|s| s.0 == tick).expect("the offline run reached that tick");
        assert_eq!(last, reference.1, "peer {i}: the offline world at tick {tick}");
    }
    assert_eq!(first.seen, second.seen, "both peers end in the same world");
}

#[test]
fn rollback_calls_refuse_bad_arguments_and_say_when_no_session_runs() {
    run_ctx(|ctx| unsafe {
        let mut bad = ponko_rollback_config_default(1, 0);
        assert_eq!(ponko_ctx_rollback_start(ctx, &bad), PONKO_INVALID_ARGUMENT);
        bad = ponko_rollback_config_default(2, 2);
        assert_eq!(ponko_ctx_rollback_start(ctx, &bad), PONKO_INVALID_ARGUMENT);
        bad = ponko_rollback_config_default(2, 0);
        bad.max_prediction = 0;
        assert_eq!(ponko_ctx_rollback_start(ctx, &bad), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_rollback_start(ctx, null()), PONKO_INVALID_ARGUMENT);

        let byte = 1u8;
        assert_eq!(ponko_ctx_rollback_set_input(ctx, (&raw const byte).cast(), 1), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_rollback_stop(ctx), PONKO_NOT_FOUND);
        let mut out = [0u8; 4];
        assert_eq!(ponko_ctx_rollback_input(ctx, 0, out.as_mut_ptr().cast(), 4), PONKO_NOT_FOUND);

        let config = ponko_rollback_config_default(2, 0);
        assert_eq!(ponko_ctx_rollback_start(ctx, &config), PONKO_OK);
        let big = [0u8; PONKO_ROLLBACK_INPUT_MAX + 1];
        assert_eq!(ponko_ctx_rollback_set_input(ctx, big.as_ptr().cast(), big.len()), PONKO_INVALID_ARGUMENT);
        let mut accepted = true;
        assert_eq!(ponko_ctx_rollback_receive(ctx, b"junk".as_ptr().cast(), 4, &mut accepted), PONKO_OK);
        assert!(!accepted, "not a rollback packet");
        let mut stats = PonkoRollbackStats::default();
        assert_eq!(ponko_ctx_rollback_stats(ctx, &mut stats), PONKO_OK);
        assert_eq!(stats.rejected, 1);
        assert_eq!(ponko_ctx_rollback_stop(ctx), PONKO_OK);
        assert_eq!(ponko_ctx_rollback_stats(ctx, &mut stats), PONKO_NOT_FOUND);
    });
}

#[test]
fn snapshots_restore_c_components_and_entity_ids() {
    unsafe {
        let w = ponko_world_new();
        let mut id = 0;
        assert_eq!(ponko_component_register(w, c"racer".as_ptr(), size_of::<Racer>(), 4, &mut id), PONKO_OK);
        let spawn = |x: f32| {
            let r = Racer { x, ..Racer::default() };
            let value = PonkoValue {
                component: id,
                data: (&raw const r).cast(),
                size: size_of::<Racer>(),
            };
            let mut e = 0;
            assert_eq!(ponko_spawn(w, &value, 1, &mut e), PONKO_OK);
            e
        };
        let a = spawn(1.0);
        let s = ponko_world_snapshot_new();
        assert_eq!(ponko_world_save_snapshot(w, s), PONKO_OK);
        let mut n = 0;
        assert_eq!(ponko_world_snapshot_len(s, &mut n), PONKO_OK);
        assert_eq!(n, 1);
        let moved = Racer { x: 5.0, ..Racer::default() };
        assert_eq!(ponko_set(w, a, id, (&raw const moved).cast(), size_of::<Racer>()), PONKO_OK);
        let b = spawn(2.0);
        assert_eq!(ponko_world_restore_snapshot(w, s), PONKO_OK);
        let mut r = Racer::default();
        assert_eq!(ponko_get(w, a, id, (&raw mut r).cast(), size_of::<Racer>()), PONKO_OK);
        assert_eq!(r.x, 1.0);
        let mut alive = true;
        assert_eq!(ponko_is_alive(w, b, &mut alive), PONKO_OK);
        assert!(!alive);
        assert_eq!(spawn(2.0), b, "the next spawn gets the id it got after the save");

        let other = ponko_world_new();
        assert_eq!(ponko_world_restore_snapshot(other, s), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_world_save_snapshot(w, null_mut()), PONKO_INVALID_ARGUMENT);
        ponko_world_snapshot_free(s);
        ponko_world_snapshot_free(null_mut());
        ponko_world_free(other);
        ponko_world_free(w);
    }
}
