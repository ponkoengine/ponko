//! Orb Arena's rules: everything but the window, so `tests/net_arena.rs`
//! plays it headless (a dedicated server and two bots in one test).
//!
//! Players run around a walled square collecting orbs. The server owns
//! the world: it spawns a player for every client that joins, moves the
//! orbs, decides who collected what and keeps the scores. Clients send
//! only their controls; their own player is predicted, everything else is
//! replicated. Events go both ways: the server announces every collection,
//! a client can wave and the server tells everyone.

use ponko::net::{
    InterestConfig, InterestTier, Interpolate, NetEvent, NetId, PeerId, Replica, Replicate, Replicated, Replication,
    ReplicationRole,
};
use ponko::prelude::*;
use serde::{Deserialize, Serialize};

/// Ticks per second, on every side.
pub const HZ: u32 = 60;
/// Half the arena's width: it spans -HALF..HALF on both axes.
pub const HALF: f32 = 300.0;
/// Orbs on the field at any time.
pub const ORBS: usize = 24;
/// How close a player must come to collect an orb.
pub const REACH: f32 = 14.0;

// ANCHOR: declare
/// A player: where it is and how fast it goes. Predicted on its owner's
/// machine, so all of its movement is in [`run`].
#[derive(Component, Replicate, Clone, Copy, Default, PartialEq, Debug)]
pub struct Player {
    #[replicate(min = -300.0, max = 300.0, step = 0.01)]
    pub at: Vec2,
    #[replicate(min = -8.0, max = 8.0, step = 0.01)]
    pub vel: Vec2,
}

/// A player's score.
#[derive(Component, Replicate, Clone, Copy, Default, PartialEq, Debug)]
pub struct Score(#[replicate(bits = 16)] pub u16);

/// An orb, drifting: shown between updates on clients.
#[derive(Component, Replicate, Clone, Copy, Default, PartialEq, Debug)]
pub struct Orb {
    #[replicate(min = -300.0, max = 300.0, step = 0.05)]
    pub at: Vec2,
}

/// What a player presses each tick.
#[derive(Replicate, Clone, Copy, Default, PartialEq, Debug)]
pub struct Controls {
    #[replicate(min = -1.0, max = 1.0, step = 0.01)]
    pub stick: Vec2,
    pub dash: bool,
}

/// Server to clients: a player collected an orb.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Collected {
    pub by: NetId,
    pub score: u16,
}

/// Client to server: "hello!".
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Wave;

/// Server to clients: someone waved.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Waved {
    pub by: NetId,
}

/// One input's worth of movement: the server simulates it, the owner
/// predicts it, and they agree to the bit.
pub fn run(p: &mut Player, c: &Controls) {
    let top = if c.dash { 7.5 } else { 4.0 };
    p.vel += (c.stick.clamp_length_max(1.0) * top - p.vel) * 0.25;
    if c.stick == Vec2::ZERO && p.vel.length_squared() < 0.01 {
        p.vel = Vec2::ZERO; // come to rest instead of creeping
    }
    p.at += p.vel;
    for axis in 0..2 {
        if p.at[axis].abs() > HALF - 8.0 {
            p.at[axis] = p.at[axis].clamp(-(HALF - 8.0), HALF - 8.0);
            p.vel[axis] = 0.0;
        }
    }
}

/// What replicates, the same on the server and every client.
pub fn replication() -> Replication {
    Replication::new()
        .input::<Controls>()
        .predicted::<Player, Controls>(run)
        .component::<Score>()
        .interpolated::<Orb>()
        .event::<Collected>()
        .event::<Wave>()
        .event::<Waved>()
        // 32 KB/s a client; near orbs every tick, far ones every 3rd.
        .budget(32_000)
        .interest(InterestConfig {
            tiers: vec![
                InterestTier {
                    radius: 220.0,
                    every: 1,
                },
                InterestTier {
                    radius: 900.0,
                    every: 3,
                },
            ],
            ..InterestConfig::default()
        })
        .position(|world, e| {
            let at = world
                .get::<Player>(e)
                .map(|p| p.at)
                .or_else(|| world.get::<Orb>(e).map(|o| o.at));
            at.map(|at| at.extend(0.0))
        })
}
// ANCHOR_END: declare

impl Interpolate for Orb {
    fn interpolate(a: &Self, b: &Self, span: f32, t: f32) -> Self {
        Self {
            at: Vec2::interpolate(&a.at, &b.at, span, t),
        }
    }
    fn extrapolate(prev: &Self, last: &Self, span: f32, ahead: f32) -> Self {
        Self {
            at: Vec2::extrapolate(&prev.at, &last.at, span, ahead),
        }
    }
}

/// The game, on either side: the server's rules run only where
/// replication says this is the server (a client's world holds only what
/// the server sends it).
pub fn build(title: &str) -> Game {
    Game::new(title, 800, 600)
        .fixed_hz(HZ)
        .replication(replication())
        .sim_system(spawn_orbs)
        .sim_system(join_and_leave)
        .sim_system(drift)
        .sim_system(collect)
        .sim_system(relay_waves)
}

/// Whether this game is the server (or offline).
pub fn is_server(ctx: &Ctx) -> bool {
    ctx.net.replication.role() != ReplicationRole::Client
}

/// The orbs' place in their circle: orb `n`'s spot on the field, moving
/// slowly with the ticks.
fn orb_at(n: u32, tick: u64) -> Vec2 {
    let seed = n as f32 * 2.399_963; // the golden angle: spread evenly
    let r = 40.0 + (n.wrapping_mul(37) % 230) as f32;
    let a = seed + tick as f32 * 0.004 * if n.is_multiple_of(2) { 1.0 } else { -1.0 };
    vec2(a.cos(), a.sin()) * r.min(HALF - 20.0)
}

/// Each orb remembers which spot it follows.
#[derive(Component, Clone, Copy)]
pub struct Spot(pub u32);

/// The next spot for a new orb.
#[derive(Default)]
pub struct NextSpot(pub u32);

/// Server: the orbs, on the first tick.
fn spawn_orbs(ctx: &mut Ctx) {
    if !is_server(ctx) || ctx.world.get_resource::<NextSpot>().is_some() {
        return;
    }
    ctx.world.insert_resource(NextSpot(ORBS as u32));
    for n in 0..ORBS as u32 {
        ctx.world.spawn((Orb { at: orb_at(n, 0) }, Spot(n), Replicated::new()));
    }
}

/// The host plays too (a listen server) when this resource is present.
pub struct HostPlays;

// ANCHOR: server
/// Server: a player for every client that joined (and the host, if it
/// plays); a client that left takes its player with it.
fn join_and_leave(ctx: &mut Ctx) {
    if !is_server(ctx) {
        return;
    }
    let mut want = ctx.net.replication.clients();
    if ctx.world.get_resource::<HostPlays>().is_some() {
        want.push(PeerId::HOST);
    }
    let mut have = Vec::new();
    ctx.world
        .query::<(Entity, &Player, &Replicated)>()
        .for_each(|(e, _, r)| {
            have.push((e, r.owner()));
        });
    for &(e, owner) in &have {
        if owner.is_none_or(|o| !want.contains(&o)) {
            ctx.commands.despawn(e);
        }
    }
    for peer in want {
        if !have.iter().any(|(_, o)| *o == Some(peer)) {
            let corner = (peer.to_bits() % 4) as f32 * std::f32::consts::FRAC_PI_2;
            let at = vec2(corner.cos(), corner.sin()) * (HALF - 40.0);
            ctx.commands.spawn((
                Player { at, vel: Vec2::ZERO },
                Score(0),
                // Players matter more than orbs when the budget is tight.
                Replicated::owned_by(peer).with_priority(4.0),
            ));
        }
    }
}

/// Server: orbs drift around their circles.
fn drift(ctx: &mut Ctx) {
    if !is_server(ctx) {
        return;
    }
    let tick = ctx.time.sim_tick();
    ctx.world
        .query::<(&mut Orb, &Spot)>()
        .for_each(|(orb, spot)| orb.at = orb_at(spot.0, tick));
}

/// Server: a player touching an orb collects it; a new one appears
/// elsewhere, and everyone hears about it.
fn collect(ctx: &mut Ctx) {
    if !is_server(ctx) {
        return;
    }
    let mut orbs = Vec::new();
    ctx.world
        .query::<(Entity, &Orb)>()
        .for_each(|(e, o)| orbs.push((e, o.at)));
    let mut taken = Vec::new();
    ctx.world
        .query::<(&Player, &mut Score, &Replicated)>()
        .for_each(|(p, score, r)| {
            for &(orb, at) in &orbs {
                if !taken.contains(&orb) && p.at.distance(at) < REACH {
                    taken.push(orb);
                    score.0 += 1;
                    if let Some(by) = r.net_id() {
                        ctx.net.replication.to_clients(&Collected { by, score: score.0 });
                    }
                }
            }
        });
    let tick = ctx.time.sim_tick();
    for orb in taken {
        ctx.world.despawn(orb);
        let n = {
            let next = ctx.world.resource_mut::<NextSpot>();
            next.0 += 1;
            next.0
        };
        ctx.world
            .spawn((Orb { at: orb_at(n, tick) }, Spot(n), Replicated::new()));
    }
}
// ANCHOR_END: server

/// Server: tells everyone who waved (by their player's id: a client that
/// waves before its player is replicated isn't heard).
fn relay_waves(ctx: &mut Ctx) {
    if !is_server(ctx) {
        return;
    }
    let waves: Vec<PeerId> = ctx.events.read::<NetEvent<Wave>>().iter().map(|w| w.from).collect();
    for from in waves {
        let mut by = None;
        ctx.world.query::<(&Player, &Replicated)>().for_each(|(_, r)| {
            if r.owner() == Some(from) {
                by = r.net_id();
            }
        });
        if let Some(by) = by {
            ctx.net.replication.to_clients(&Waved { by });
        }
    }
}

// ANCHOR: bot
/// A player that plays by itself: heads for the nearest orb, dashing when
/// it is far. On a client it steers its predicted player; on a listen
/// server, the host's.
pub fn bot(ctx: &mut Ctx) {
    let me = ctx.net.replication.me();
    let mut mine = None;
    let mut orbs = Vec::new();
    ctx.world
        .query::<(&Player, Option<&Replica>, Option<&Replicated>)>()
        .for_each(|(p, replica, original)| {
            let ours = replica.is_some_and(Replica::is_mine)
                || original.is_some_and(|r| r.owner().is_some() && r.owner() == me);
            if ours {
                mine = Some(p.at);
            }
        });
    ctx.world.query::<&Orb>().for_each(|o| orbs.push(o.at));
    let controls = match mine {
        Some(at) => {
            let target = orbs
                .iter()
                .copied()
                .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at)));
            match target {
                Some(t) => Controls {
                    stick: (t - at).normalize_or_zero(),
                    dash: t.distance(at) > 120.0,
                },
                None => Controls::default(),
            }
        }
        None => Controls::default(),
    };
    ctx.net.replication.set_input(controls);
}
// ANCHOR_END: bot

/// Every player, mine first, with its score: what a scoreboard shows.
pub fn scoreboard(ctx: &mut Ctx) -> Vec<(bool, u16)> {
    let me = ctx.net.replication.me();
    let mut rows = Vec::new();
    ctx.world
        .query::<(&Score, Option<&Replica>, Option<&Replicated>)>()
        .for_each(|(s, replica, original)| {
            let mine = replica.is_some_and(Replica::is_mine)
                || original.is_some_and(|r| r.owner().is_some() && r.owner() == me);
            rows.push((mine, s.0));
        });
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    rows
}
