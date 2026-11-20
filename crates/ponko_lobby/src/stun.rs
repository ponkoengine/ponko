//! A STUN responder: answers binding requests with the address they came
//! from, so WebRTC peers (the engine's feature `webrtc`) learn the public
//! address their NAT gives them from the lobby itself, and a game needs no
//! STUN server of its own.
//!
//! Binding requests only (RFC 8489 without authentication), the part
//! address discovery uses; no TURN. Answers are limited per IP address, so
//! the server can't be used to flood a third party with spoofed requests.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Instant;

use ponko::net::stun;

/// Requests answered per IP address and second, and the burst allowed.
const RATE: f64 = 20.0;
const BURST: f64 = 40.0;
/// Datagrams one poll reads at most.
const READS: usize = 1024;
/// Addresses remembered for the rate limit before idle ones are forgotten.
const MAX_TRACKED: usize = 65_536;

/// What a [`StunResponder`] has done since it started.
/// ```
/// assert_eq!(ponko_lobby::StunStats::default().answered, 0);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct StunStats {
    /// Binding requests answered.
    pub answered: u64,
    /// Requests over an address's rate limit, not answered.
    pub rate_limited: u64,
    /// Datagrams that weren't binding requests, ignored.
    pub ignored: u64,
}

/// A STUN binding responder on its own UDP socket (port 3478 is usual).
/// Poll it as often as the lobby; it owns no thread.
///
/// ```
/// let mut stun = ponko_lobby::StunResponder::bind("127.0.0.1:0").unwrap();
/// assert_eq!(stun.poll(), 0); // nothing waiting
/// ```
#[derive(Debug)]
pub struct StunResponder {
    socket: UdpSocket,
    started: Instant,
    buckets: HashMap<IpAddr, (f64, f64)>,
    stats: StunStats,
    response: Vec<u8>,
}

impl StunResponder {
    /// Binds a nonblocking UDP socket on `addr`.
    ///
    /// # Errors
    ///
    /// The error of binding or configuring the socket.
    ///
    /// ```
    /// let stun = ponko_lobby::StunResponder::bind("127.0.0.1:0").unwrap();
    /// ```
    pub fn bind(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            started: Instant::now(),
            buckets: HashMap::new(),
            stats: StunStats::default(),
            response: Vec::with_capacity(64),
        })
    }

    /// The socket's address.
    ///
    /// # Errors
    ///
    /// The socket's error.
    ///
    /// ```
    /// let stun = ponko_lobby::StunResponder::bind("127.0.0.1:0").unwrap();
    /// assert_ne!(stun.local_addr().unwrap().port(), 0);
    /// ```
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Answers the binding requests waiting (at most 1024); returns how
    /// many it answered.
    /// ```
    /// let mut stun = ponko_lobby::StunResponder::bind("127.0.0.1:0").unwrap();
    /// stun.poll();
    /// ```
    pub fn poll(&mut self) -> usize {
        let mut buf = [0u8; 1500];
        let mut answered = 0;
        for _ in 0..READS {
            let Ok((n, from)) = self.socket.recv_from(&mut buf) else {
                break; // WouldBlock, or an ICMP error from an earlier answer
            };
            if !stun::binding_response(&buf[..n], from, &mut self.response) {
                self.stats.ignored += 1;
                continue;
            }
            if !self.allow(from.ip()) {
                self.stats.rate_limited += 1;
                continue;
            }
            if self.socket.send_to(&self.response, from).is_ok() {
                self.stats.answered += 1;
                answered += 1;
            }
        }
        answered
    }

    /// Takes one answer from `ip`'s budget.
    fn allow(&mut self, ip: IpAddr) -> bool {
        let now = self.started.elapsed().as_secs_f64();
        if self.buckets.len() >= MAX_TRACKED {
            // Forget addresses whose budget has refilled.
            self.buckets.retain(|_, (_, at)| now - *at < BURST / RATE);
        }
        let (tokens, at) = self.buckets.entry(ip).or_insert((BURST, now));
        *tokens = (*tokens + (now - *at) * RATE).min(BURST);
        *at = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// What it has done so far.
    /// ```
    /// let stun = ponko_lobby::StunResponder::bind("127.0.0.1:0").unwrap();
    /// assert_eq!(stun.stats().answered, 0);
    /// ```
    pub fn stats(&self) -> StunStats {
        self.stats
    }
}
