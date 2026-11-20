//! A lobby server and clients in memory, on a simulated clock.
#![allow(dead_code)]

use std::time::Duration;

use ponko::net::NetMessages;
use ponko::net::lobby::{LobbyClient, LobbyEvent, RoomInfo};
use ponko_lobby::{AddressKey, LobbyLimits, LobbyServer};

pub const STEP: Duration = Duration::from_millis(10);

pub struct Lobby {
    pub server: LobbyServer,
    pub clients: Vec<LobbyClient>,
    pub events: Vec<Vec<LobbyEvent>>,
}

/// Limits per connection: every in-memory client has the same IP.
pub fn limits() -> LobbyLimits {
    let mut limits = LobbyLimits::default();
    limits.address_key = AddressKey::Connection;
    limits
}

impl Lobby {
    pub fn new(clients: usize, limits: LobbyLimits) -> Self {
        Self::with(clients, LobbyServer::new(limits).seed(42))
    }

    pub fn with(clients: usize, mut server: LobbyServer) -> Self {
        let mut endpoints = NetMessages::loopback(clients + 1).into_iter();
        server.listen(endpoints.next().unwrap());
        let mut lobby = Self {
            server,
            clients: endpoints.map(LobbyClient::new).collect(),
            events: vec![Vec::new(); clients],
        };
        lobby.run_until(|l| l.clients.iter().all(LobbyClient::is_connected));
        lobby.events.iter_mut().for_each(Vec::clear);
        lobby
    }

    /// One step of the clock for the server and every client; their events
    /// are added to `events`.
    pub fn step(&mut self) {
        self.server.poll_elapsed(STEP);
        for (client, events) in self.clients.iter_mut().zip(&mut self.events) {
            client.poll_elapsed(STEP).unwrap();
            while let Some(event) = client.next_event() {
                events.push(event);
            }
        }
    }

    pub fn steps(&mut self, n: usize) {
        (0..n).for_each(|_| self.step());
    }

    pub fn run_until(&mut self, mut done: impl FnMut(&Self) -> bool) {
        for _ in 0..1000 {
            self.step();
            if done(self) {
                return;
            }
        }
        panic!("not done after 1000 steps (10 simulated seconds)");
    }

    /// Takes client `i`'s events so far.
    pub fn take(&mut self, i: usize) -> Vec<LobbyEvent> {
        std::mem::take(&mut self.events[i])
    }

    /// Client `i` creates a room of `size`; returns it once it is in.
    pub fn create(&mut self, i: usize, size: u16) -> RoomInfo {
        self.clients[i].create_room(size).unwrap();
        self.run_until(|l| l.clients[i].room().is_some());
        self.take(i);
        self.clients[i].room().unwrap().clone()
    }

    /// Clients `who` join `code`, and everyone's events are read.
    pub fn join(&mut self, who: &[usize], code: &str) {
        for &i in who {
            self.clients[i].join_room(code).unwrap();
        }
        self.run_until(|l| who.iter().all(|&i| l.clients[i].room().is_some()));
        self.steps(5);
    }
}
