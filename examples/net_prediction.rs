//! Client prediction and reconciliation over a bad link: 150 ms round trip
//! and every 10th packet lost. The client moves its walker on the tick of
//! the input; the server, which alone knows about a wind that pushes the
//! walker across a strip of the map, corrects it, and the corrections slide
//! in instead of jumping.
//! Prints what the player would see. Run with
//! `cargo run --example net_prediction --no-default-features`.
use std::time::Duration;

use ponko::net::{InputQueue, Lane, MessageEvent, NetMessages, PeerId, Prediction, Reconciled, Smoothing};
use ponko::prelude::*;
use serde::{Deserialize, Serialize};

const DT: f32 = 1.0 / 60.0;
/// Half the round trip, in seconds.
const ONE_WAY: f64 = 0.075;

// ANCHOR: step
/// What a tick changes: the state both sides simulate.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Walker {
    pos: Vec2,
    vel: Vec2,
}

/// One tick's input.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Stick(Vec2);

/// The step for one input: the client predicts with it and the server
/// simulates with it, so they agree unless something only the server knows
/// gets involved.
fn walk(w: &mut Walker, stick: &Stick) {
    w.vel += (stick.0 * 5.0 - w.vel) * 0.2;
    w.pos += w.vel * DT;
}
// ANCHOR_END: step

/// The game's packets; the engine only moves the bytes.
#[derive(Serialize, Deserialize)]
enum Packet {
    Inputs(Vec<(u16, Stick)>),
    State { ack: u16, walker: Walker },
}

/// Holds packets back to simulate the link's delay (the loopback itself
/// drops and reorders them).
#[derive(Default)]
struct Delay(Vec<(f64, Vec<u8>)>);
impl Delay {
    fn release(&mut self, now: f64) -> Vec<Vec<u8>> {
        let (due, held): (Vec<_>, Vec<_>) = self.0.drain(..).partition(|(at, _)| *at <= now);
        self.0 = held;
        due.into_iter().map(|(_, bytes)| bytes).collect()
    }
}

pub fn main() {
    let mut nets = NetMessages::loopback_lossy(2, 10);
    let mut guest = None;
    while guest.is_none() {
        for net in &mut nets {
            net.poll_elapsed(Duration::from_secs_f32(DT)).unwrap();
        }
        guest = nets[0].events().find_map(|e| match e {
            MessageEvent::Connected(peer) => Some(peer),
            _ => None,
        });
    }
    let guest = guest.unwrap();
    let (mut up, mut down) = (Delay::default(), Delay::default());
    let start = Walker {
        pos: Vec2::ZERO,
        vel: Vec2::ZERO,
    };

    // ANCHOR: setup
    // Client: its own walker, predicted; what it draws, smoothed.
    let mut prediction = Prediction::<Walker, Stick>::new(start);
    let mut smoothing = Smoothing::<Vec2>::new();
    // Server: the client's inputs, in order, and the real walker.
    let mut inputs = InputQueue::<Stick>::new();
    let mut server = start;
    // ANCHOR_END: setup

    let (mut biggest_correction, mut biggest_slide, mut last_shown) = (0.0f32, 0.0f32, start.pos);
    let mut response = None;
    for tick in 0..480u32 {
        let now = f64::from(tick) * f64::from(DT);
        let stick = Stick(match tick {
            30..150 => vec2(1.0, 0.0),
            150..270 => vec2(0.0, 1.0),
            _ => Vec2::ZERO,
        });

        // ANCHOR: client_tick
        // Client, every fixed tick: move at once, then send every input the
        // server hasn't confirmed (repeats make up for lost packets).
        let before = prediction.state().pos;
        if tick < 420 {
            prediction.predict(stick, walk); // (then a second for the last packets to land)
        }
        let recent: Vec<_> = prediction.unacked().rev().take(16).map(|(seq, s)| (seq, *s)).collect();
        let packet = serde_json::to_vec(&Packet::Inputs(recent)).unwrap();
        up.0.push((now + ONE_WAY, packet)); // to the server, through the slow link
        // ANCHOR_END: client_tick
        if response.is_none() && stick.0 != Vec2::ZERO {
            response = Some(prediction.state().pos != before);
        }
        let stepped = prediction.state().pos - before;
        for bytes in up.release(now) {
            nets[1].send(PeerId::HOST, Lane::Unreliable, &bytes).unwrap();
        }
        for bytes in down.release(now) {
            nets[0].send(guest, Lane::Unreliable, &bytes).unwrap();
        }
        for net in &mut nets {
            net.poll_elapsed(Duration::from_secs_f32(DT)).unwrap();
        }

        // ANCHOR: server_tick
        // Server, every fixed tick: queue what arrived (repeats and late
        // ones are ignored), run one step per input, answer with the state
        // and the number of the last input in it.
        while let Some(event) = nets[0].next_event() {
            if let Some(Packet::Inputs(received)) = event.decode() {
                for (seq, stick) in received {
                    inputs.push(seq, stick);
                }
            }
        }
        while let Some((_, stick)) = inputs.pop() {
            walk(&mut server, &stick);
            if (1.0..4.0).contains(&server.pos.y) {
                server.pos.x -= 2.0 * DT; // a wind the client doesn't know about
            }
        }
        if let Some(ack) = inputs.ack() {
            let state = serde_json::to_vec(&Packet::State { ack, walker: server }).unwrap();
            down.0.push((now + ONE_WAY, state)); // to the client, through the slow link
        }
        // ANCHOR_END: server_tick

        // ANCHOR: reconcile
        // Client, when a state arrives: compare with the prediction for that
        // input; if they differ, replay the newer inputs on top of it, and
        // let the drawn walker slide over instead of jumping.
        while let Some(event) = nets[1].next_event() {
            if let Some(Packet::State { ack, walker }) = event.decode()
                && let Reconciled::Corrected { before, .. } = prediction.reconcile(ack, walker, walk)
            {
                smoothing.correct(before.pos, prediction.state().pos);
                biggest_correction = biggest_correction.max(before.pos.distance(prediction.state().pos));
            }
        }
        // Every frame: draw the predicted walker, moved by what is left of
        // the corrections.
        smoothing.update(DT);
        let shown = smoothing.apply(prediction.state().pos);
        // ANCHOR_END: reconcile

        biggest_slide = biggest_slide.max((shown - last_shown - stepped).length());
        last_shown = shown;
    }

    let stats = prediction.stats();
    println!("150 ms round trip, every 10th packet lost");
    println!("moves on the tick of the input: {}", response.unwrap());
    println!(
        "{} ticks predicted, {} confirmed by the server, {} corrected ({} inputs replayed)",
        stats.predicted, stats.confirmed, stats.corrections, stats.replayed
    );
    println!(
        "server: {} inputs applied, {} never arrived",
        inputs.stats().applied,
        inputs.stats().skipped
    );
    println!("biggest correction: {biggest_correction:.3} units; biggest slide in one frame: {biggest_slide:.4}");
    println!("client at {:?}, server at {:?}", prediction.state().pos, server.pos);
    assert!(response.unwrap());
    assert!(stats.corrections > 0 && biggest_slide < biggest_correction);
    assert_eq!(prediction.state(), &server, "the client ends where the server is");
}
