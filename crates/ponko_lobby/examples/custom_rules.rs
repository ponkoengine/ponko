//! A lobby server of a game's own: the reference server's library with a
//! matchmaking rule by skill rating and a host migration rule, on UDP and
//! WebSocket. The guide's "Lobbies, matchmaking and relays" chapter shows it.
//!
//! ```sh
//! cargo run -p ponko_lobby --example custom_rules
//! ```

use std::time::Duration;

// ANCHOR: matchmaker
use ponko::net::lobby::{MemberId, RoomInfo};
use ponko_lobby::{Matchmaker, Waiting};

/// Puts players together whose ratings are within `window`, which widens
/// by 50 for every 5 seconds the oldest of them has waited.
struct CloseRatings;

impl Matchmaker for CloseRatings {
    fn form(&mut self, _mode: &str, size: u16, waiting: &[Waiting], now: Duration, groups: &mut Vec<Vec<usize>>) {
        let size = usize::from(size);
        let mut used = vec![false; waiting.len()];
        for first in 0..waiting.len() {
            if used[first] {
                continue;
            }
            let waited = now.saturating_sub(waiting[first].since).as_secs() as i32;
            let window = 100 + 50 * (waited / 5);
            let group: Vec<usize> = (first..waiting.len())
                .filter(|&i| !used[i] && (waiting[i].rating - waiting[first].rating).abs() <= window)
                .take(size)
                .collect();
            if group.len() == size {
                group.iter().for_each(|&i| used[i] = true);
                groups.push(group);
            }
        }
    }
}

/// The member who joined last hosts next: in this game the newest player
/// has the freshest copy of the world.
fn newest_member(room: &RoomInfo) -> Option<MemberId> {
    room.members.last().copied()
}
// ANCHOR_END: matchmaker

fn main() -> std::io::Result<()> {
    // ANCHOR: server
    use ponko::net::NetMessages;
    use ponko_lobby::{LobbyLimits, LobbyServer};

    let mut limits = LobbyLimits::default();
    limits.max_room_size = 8;
    let mut server = LobbyServer::new(limits)
        .matchmaker(CloseRatings)
        .on_host_left(newest_member);
    server.listen(NetMessages::host("0.0.0.0:7700", 1024)?.accept_web("0.0.0.0:7701")?);
    loop {
        server.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    // ANCHOR_END: server
}
