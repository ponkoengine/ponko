//! A TURN client (RFC 8656) over UDP, for the desktop WebRTC agent
//! (`rtc_native.rs`): when two peers can't reach each other at all (both
//! behind strict NATs), each can borrow an address on a TURN server, which
//! relays their packets.
//!
//! Sans-IO, like str0m: the agent hands it the server's datagrams and the
//! protocol time, and sends what it asks to. What it does:
//!
//! - **Allocate** a relayed address, with the server's long-term
//!   credentials: the first request is refused with a realm and a nonce,
//!   the second carries the user name and a MESSAGE-INTEGRITY (an
//!   HMAC-SHA1 under `MD5(user:realm:password)`). A stale nonce (438) is
//!   replaced and the request sent again.
//! - **Refresh** the allocation before its lifetime ends.
//! - **CreatePermission** for each peer IP it sends to, renewed every four
//!   minutes (permissions last five).
//! - Packets to peers go out as **Send** indications; packets from peers
//!   arrive as **Data** indications. No channels: 36 bytes of overhead a
//!   packet, one round trip less to set up.
//!
//! Requests are resent after 0.5, 1, 2 and 4 s, then given up. Over UDP
//! only: `turns:` (TLS) and `?transport=tcp` servers are for browsers.

use std::collections::{BTreeMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use super::stun::MAGIC_COOKIE;

const HEADER: usize = 20;

// Methods (RFC 8656, section 18).
const ALLOCATE: u16 = 0x003;
const REFRESH: u16 = 0x004;
const SEND: u16 = 0x006;
const DATA: u16 = 0x007;
const CREATE_PERMISSION: u16 = 0x008;

// Attributes (RFC 8489, section 18.3; RFC 8656, section 18).
const USERNAME: u16 = 0x0006;
const MESSAGE_INTEGRITY: u16 = 0x0008;
const ERROR_CODE: u16 = 0x0009;
const LIFETIME: u16 = 0x000D;
const XOR_PEER_ADDRESS: u16 = 0x0012;
const DATA_ATTR: u16 = 0x0013;
const REALM: u16 = 0x0014;
const NONCE: u16 = 0x0015;
const XOR_RELAYED_ADDRESS: u16 = 0x0016;
const REQUESTED_TRANSPORT: u16 = 0x0019;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;

/// REQUESTED-TRANSPORT's protocol number for UDP.
const UDP: u8 = 17;
/// What the server grants when a success says nothing (RFC 8656, 2.2).
const DEFAULT_LIFETIME: Duration = Duration::from_secs(600);
/// Permissions last five minutes; renew them after four.
const PERMISSION_RENEW: Duration = Duration::from_secs(240);
/// Tries per request (the first, then resends after 0.5, 1, 2 and 4 s).
const TRIES: u32 = 5;
/// Authentication answers in a row before the credentials count as wrong.
const AUTH_TRIES: u32 = 3;
/// The largest packet relayed: far above the message protocol's 1500 bytes.
const MAX_DATA: usize = 4096;

/// What a STUN message is (RFC 8489, section 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Class {
    Request,
    Indication,
    Success,
    Error,
}

impl Class {
    fn bits(self) -> u16 {
        match self {
            Self::Request => 0b00,
            Self::Indication => 0b01,
            Self::Success => 0b10,
            Self::Error => 0b11,
        }
    }
}

/// A method and class as the 14-bit message type, the class bits spread
/// between the method's.
fn message_type(method: u16, class: Class) -> u16 {
    let c = class.bits();
    (method & 0x000F) | ((method & 0x0070) << 1) | ((method & 0x0F80) << 2) | ((c & 1) << 4) | ((c & 2) << 7)
}

fn split_type(t: u16) -> (u16, Class) {
    let method = (t & 0x000F) | ((t >> 1) & 0x0070) | ((t >> 2) & 0x0F80);
    let class = match ((t >> 4) & 1) | ((t >> 7) & 2) {
        0b00 => Class::Request,
        0b01 => Class::Indication,
        0b10 => Class::Success,
        _ => Class::Error,
    };
    (method, class)
}

/// HMAC-SHA1 (RFC 2104) of `parts`, one after the other.
pub(super) fn hmac_sha1(key: &[u8], parts: &[&[u8]]) -> [u8; 20] {
    use sha1::{Digest, Sha1};
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..20].copy_from_slice(&Sha1::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha1::new();
    inner.update(block.map(|b| b ^ 0x36));
    for part in parts {
        inner.update(part);
    }
    let mut outer = Sha1::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// The long-term credential key: `MD5(username ":" realm ":" password)`
/// (RFC 8489, section 9.2.2; the strings as given, without SASLprep).
pub(super) fn long_term_key(username: &str, realm: &str, password: &str) -> [u8; 16] {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(username.as_bytes());
    h.update(b":");
    h.update(realm.as_bytes());
    h.update(b":");
    h.update(password.as_bytes());
    h.finalize().into()
}

/// XORs an address with the cookie (and for IPv6 the transaction id), as
/// the XOR-*-ADDRESS attributes carry it; the same both ways.
fn xor(addr: SocketAddr, tx: &[u8; 12]) -> SocketAddr {
    let port = addr.port() ^ (MAGIC_COOKIE >> 16) as u16;
    match addr.ip() {
        IpAddr::V4(v4) => SocketAddr::new(IpAddr::V4(Ipv4Addr::from(u32::from(v4) ^ MAGIC_COOKIE)), port),
        IpAddr::V6(v6) => {
            let mut mask = [0u8; 16];
            mask[..4].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
            mask[4..].copy_from_slice(tx);
            let bytes = v6.octets();
            let ip: [u8; 16] = std::array::from_fn(|i| bytes[i] ^ mask[i]);
            SocketAddr::new(IpAddr::V6(Ipv6Addr::from(ip)), port)
        }
    }
}

/// A STUN message being written.
pub(super) struct Writer {
    buf: Vec<u8>,
    tx: [u8; 12],
}

impl Writer {
    pub(super) fn new(method: u16, class: Class, tx: [u8; 12]) -> Self {
        let mut buf = Vec::with_capacity(128);
        buf.extend_from_slice(&message_type(method, class).to_be_bytes());
        buf.extend_from_slice(&[0, 0]);
        buf.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        buf.extend_from_slice(&tx);
        Self { buf, tx }
    }

    fn set_length(&mut self, len: usize) {
        let len = u16::try_from(len).unwrap_or(u16::MAX);
        self.buf[2..4].copy_from_slice(&len.to_be_bytes());
    }

    pub(super) fn attr(&mut self, kind: u16, value: &[u8]) -> &mut Self {
        let len = u16::try_from(value.len()).unwrap_or(u16::MAX);
        self.buf.extend_from_slice(&kind.to_be_bytes());
        self.buf.extend_from_slice(&len.to_be_bytes());
        self.buf.extend_from_slice(value);
        while !self.buf.len().is_multiple_of(4) {
            self.buf.push(0);
        }
        let body = self.buf.len() - HEADER;
        self.set_length(body);
        self
    }

    pub(super) fn xor_addr(&mut self, kind: u16, addr: SocketAddr) -> &mut Self {
        let x = xor(addr, &self.tx);
        let mut value = vec![0, if x.is_ipv4() { 1 } else { 2 }];
        value.extend_from_slice(&x.port().to_be_bytes());
        match x.ip() {
            IpAddr::V4(v4) => value.extend_from_slice(&v4.octets()),
            IpAddr::V6(v6) => value.extend_from_slice(&v6.octets()),
        }
        self.attr(kind, &value)
    }

    /// The message, with a MESSAGE-INTEGRITY under `key` last.
    pub(super) fn sign(mut self, key: &[u8]) -> Vec<u8> {
        let body = self.buf.len() - HEADER;
        self.set_length(body + 24);
        let mac = hmac_sha1(key, &[&self.buf]);
        self.buf.extend_from_slice(&MESSAGE_INTEGRITY.to_be_bytes());
        self.buf.extend_from_slice(&20u16.to_be_bytes());
        self.buf.extend_from_slice(&mac);
        self.buf
    }

    pub(super) fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// A STUN message read from a datagram.
pub(super) struct Message<'a> {
    pub(super) method: u16,
    pub(super) class: Class,
    pub(super) tx: [u8; 12],
    raw: &'a [u8],
    /// (type, offset of the attribute's header, value length).
    attrs: Vec<(u16, usize, usize)>,
}

/// Reads a STUN message: `None` for anything else, or a malformed one.
pub(super) fn parse(raw: &[u8]) -> Option<Message<'_>> {
    if raw.len() < HEADER || raw[0] & 0xC0 != 0 || raw[4..8] != MAGIC_COOKIE.to_be_bytes() {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([raw[2], raw[3]]));
    if len % 4 != 0 || HEADER + len != raw.len() {
        return None;
    }
    let (method, class) = split_type(u16::from_be_bytes([raw[0], raw[1]]));
    let tx: [u8; 12] = raw[8..20].try_into().ok()?;
    let mut attrs = Vec::new();
    let mut at = HEADER;
    while at + 4 <= raw.len() {
        let kind = u16::from_be_bytes([raw[at], raw[at + 1]]);
        let vlen = usize::from(u16::from_be_bytes([raw[at + 2], raw[at + 3]]));
        if at + 4 + vlen > raw.len() {
            return None;
        }
        attrs.push((kind, at, vlen));
        at += 4 + vlen.div_ceil(4) * 4;
    }
    Some(Message {
        method,
        class,
        tx,
        raw,
        attrs,
    })
}

impl Message<'_> {
    /// The first attribute of this type (attributes after a
    /// MESSAGE-INTEGRITY aren't covered by it, and are ignored).
    pub(super) fn get(&self, kind: u16) -> Option<&[u8]> {
        let mut found = None;
        for &(k, at, len) in &self.attrs {
            if k == kind && found.is_none() {
                found = Some(&self.raw[at + 4..at + 4 + len]);
            }
            if k == MESSAGE_INTEGRITY {
                break;
            }
        }
        found
    }

    pub(super) fn text(&self, kind: u16) -> Option<&str> {
        self.get(kind).and_then(|v| std::str::from_utf8(v).ok())
    }

    pub(super) fn xor_addr(&self, kind: u16) -> Option<SocketAddr> {
        let v = self.get(kind)?;
        let port = u16::from_be_bytes(v.get(2..4)?.try_into().ok()?);
        let ip = match (v.get(1)?, v.len()) {
            (1, 8) => IpAddr::V4(Ipv4Addr::from(<[u8; 4]>::try_from(&v[4..8]).ok()?)),
            (2, 20) => IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&v[4..20]).ok()?)),
            _ => return None,
        };
        Some(xor(SocketAddr::new(ip, port), &self.tx))
    }

    /// The error code of an error response (class and number: 401, 438).
    pub(super) fn error_code(&self) -> Option<u16> {
        let v = self.get(ERROR_CODE)?;
        let class = u16::from(*v.get(2)? & 0x07);
        Some(class * 100 + u16::from(*v.get(3)?))
    }

    pub(super) fn lifetime(&self) -> Option<Duration> {
        let v = self.get(LIFETIME)?;
        Some(Duration::from_secs(u64::from(u32::from_be_bytes(v.try_into().ok()?))))
    }

    /// Whether the message carries a MESSAGE-INTEGRITY that checks out
    /// under `key`.
    pub(super) fn verify(&self, key: &[u8]) -> bool {
        let Some(&(_, at, 20)) = self.attrs.iter().find(|a| a.0 == MESSAGE_INTEGRITY) else {
            return false;
        };
        let mut head = [0u8; HEADER];
        head.copy_from_slice(&self.raw[..HEADER]);
        let len = u16::try_from(at - HEADER + 24).unwrap_or(u16::MAX);
        head[2..4].copy_from_slice(&len.to_be_bytes());
        let mac = hmac_sha1(key, &[&head, &self.raw[HEADER..at]]);
        mac[..] == self.raw[at + 4..at + 24]
    }
}

/// What a request was for, to act on its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Purpose {
    Allocate,
    Refresh,
    Permission(IpAddr),
}

struct Pending {
    tx: [u8; 12],
    purpose: Purpose,
    bytes: Vec<u8>,
    sent: u32,
    next: Duration,
}

/// What the client tells the agent.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum TurnEvent {
    /// The server relays for us at `relayed`; it sees our packets come
    /// from `mapped` (a reflexive address, like STUN's).
    Allocated {
        relayed: SocketAddr,
        mapped: Option<SocketAddr>,
    },
    /// A peer's packet, relayed.
    Data { peer: SocketAddr, bytes: Vec<u8> },
    /// The allocation failed or ended: no relayed address any more.
    Failed(String),
}

/// One TURN server's allocation, as the client sees it.
pub(super) struct TurnClient {
    server: SocketAddr,
    username: String,
    password: String,
    realm: Option<String>,
    nonce: Option<String>,
    key: Option<[u8; 16]>,
    relayed: Option<SocketAddr>,
    /// When to refresh the allocation, and when it ends without one.
    refresh_at: Duration,
    expires: Duration,
    pending: Vec<Pending>,
    /// Peer IPs with a permission asked for, and when to renew it.
    permissions: BTreeMap<IpAddr, Duration>,
    /// Authentication answers in a row.
    auth_answers: u32,
    failed: bool,
    events: VecDeque<TurnEvent>,
    random: fn() -> u64,
}

impl TurnClient {
    /// A client for the server at `server`; its first request goes out on
    /// the first [`poll`](Self::poll).
    pub(super) fn new(server: SocketAddr, username: &str, password: &str) -> Self {
        Self::with_random(server, username, password, super::messages::random_u64)
    }

    fn with_random(server: SocketAddr, username: &str, password: &str, random: fn() -> u64) -> Self {
        let mut client = Self {
            server,
            username: username.to_owned(),
            password: password.to_owned(),
            realm: None,
            nonce: None,
            key: None,
            relayed: None,
            refresh_at: Duration::MAX,
            expires: Duration::MAX,
            pending: Vec::new(),
            permissions: BTreeMap::new(),
            auth_answers: 0,
            failed: false,
            events: VecDeque::new(),
            random,
        };
        client.request(Purpose::Allocate, Duration::ZERO);
        client
    }

    pub(super) fn server(&self) -> SocketAddr {
        self.server
    }

    pub(super) fn relayed(&self) -> Option<SocketAddr> {
        self.relayed
    }

    fn transaction(&self) -> [u8; 12] {
        let (a, b) = ((self.random)().to_le_bytes(), (self.random)().to_le_bytes());
        std::array::from_fn(|i| if i < 8 { a[i] } else { b[i - 8] })
    }

    /// Queues a request for `purpose`, signed once the server's realm is
    /// known; it goes out on the next poll.
    fn request(&mut self, purpose: Purpose, now: Duration) {
        self.pending.retain(|p| p.purpose != purpose);
        let tx = self.transaction();
        let method = match purpose {
            Purpose::Allocate => ALLOCATE,
            Purpose::Refresh => REFRESH,
            Purpose::Permission(_) => CREATE_PERMISSION,
        };
        let mut w = Writer::new(method, Class::Request, tx);
        match purpose {
            Purpose::Allocate => {
                w.attr(REQUESTED_TRANSPORT, &[UDP, 0, 0, 0]);
            }
            Purpose::Refresh => {
                let secs = u32::try_from(DEFAULT_LIFETIME.as_secs()).unwrap_or(600);
                w.attr(LIFETIME, &secs.to_be_bytes());
            }
            Purpose::Permission(ip) => {
                w.xor_addr(XOR_PEER_ADDRESS, SocketAddr::new(ip, 0));
            }
        }
        let bytes = match (&self.realm, &self.nonce, &self.key) {
            (Some(realm), Some(nonce), Some(key)) => {
                w.attr(USERNAME, self.username.as_bytes());
                w.attr(REALM, realm.as_bytes());
                w.attr(NONCE, nonce.as_bytes());
                w.sign(key)
            }
            _ => w.finish(),
        };
        self.pending.push(Pending {
            tx,
            purpose,
            bytes,
            sent: 0,
            next: now,
        });
    }

    fn fail(&mut self, why: String) {
        if !self.failed {
            self.failed = true;
            self.relayed = None;
            self.pending.clear();
            self.events.push_back(TurnEvent::Failed(why));
        }
    }

    /// Resends, refreshes and renews what is due at `now`; datagrams for
    /// the server go to `out`.
    pub(super) fn poll(&mut self, now: Duration, out: &mut Vec<Vec<u8>>) {
        if self.failed {
            return;
        }
        if self.relayed.is_some() && now >= self.expires {
            return self.fail("the TURN allocation expired".into());
        }
        if self.relayed.is_some() && now >= self.refresh_at {
            self.refresh_at = Duration::MAX;
            self.request(Purpose::Refresh, now);
        }
        let due: Vec<IpAddr> = self
            .permissions
            .iter()
            .filter(|&(_, &renew)| renew <= now)
            .map(|(&ip, _)| ip)
            .collect();
        for ip in due {
            self.permissions.insert(ip, now + PERMISSION_RENEW);
            self.request(Purpose::Permission(ip), now);
        }
        let mut gave_up = None;
        for p in &mut self.pending {
            if p.next > now {
                continue;
            }
            if p.sent >= TRIES {
                gave_up = Some(p.purpose);
                continue;
            }
            out.push(p.bytes.clone());
            p.next = now + Duration::from_millis(500 << p.sent);
            p.sent += 1;
        }
        match gave_up {
            Some(Purpose::Allocate | Purpose::Refresh) => self.fail("the TURN server didn't answer".into()),
            Some(Purpose::Permission(ip)) => {
                self.pending.retain(|p| p.purpose != Purpose::Permission(ip));
                self.permissions.remove(&ip);
            }
            None => {}
        }
    }

    /// Takes a datagram from the server; false if it isn't one of ours.
    pub(super) fn receive(&mut self, packet: &[u8], now: Duration) -> bool {
        let Some(m) = parse(packet) else { return false };
        if m.class == Class::Indication {
            if m.method == DATA
                && let (Some(peer), Some(bytes)) = (m.xor_addr(XOR_PEER_ADDRESS), m.get(DATA_ATTR))
                && self.relayed.is_some()
                && bytes.len() <= MAX_DATA
                && self.permissions.contains_key(&peer.ip().to_canonical())
            {
                self.events.push_back(TurnEvent::Data {
                    peer,
                    bytes: bytes.to_vec(),
                });
            }
            return true;
        }
        let Some(i) = self.pending.iter().position(|p| p.tx == m.tx) else {
            return true; // A late answer to a request already answered.
        };
        // A success answer to a signed request must be signed by the server
        // too (RFC 8489, section 9.2.4); a forged one is ignored. Error
        // answers may come unsigned (a 438 carries the new nonce), but a
        // signature that doesn't verify is never taken.
        let signed = m.get(MESSAGE_INTEGRITY).is_some();
        if self
            .key
            .is_some_and(|key| (signed && !m.verify(&key)) || (!signed && m.class == Class::Success))
        {
            return true;
        }
        let purpose = self.pending.remove(i).purpose;
        match m.class {
            Class::Success => {
                self.auth_answers = 0;
                self.succeeded(purpose, &m, now);
            }
            Class::Error => self.refused(purpose, &m, now),
            _ => {}
        }
        true
    }

    fn succeeded(&mut self, purpose: Purpose, m: &Message<'_>, now: Duration) {
        match purpose {
            Purpose::Allocate => {
                let Some(relayed) = m.xor_addr(XOR_RELAYED_ADDRESS) else {
                    return self.fail("the TURN server gave no relayed address".into());
                };
                self.relayed = Some(relayed);
                self.schedule_refresh(m.lifetime().unwrap_or(DEFAULT_LIFETIME), now);
                self.events.push_back(TurnEvent::Allocated {
                    relayed,
                    mapped: m.xor_addr(XOR_MAPPED_ADDRESS),
                });
            }
            Purpose::Refresh => self.schedule_refresh(m.lifetime().unwrap_or(DEFAULT_LIFETIME), now),
            Purpose::Permission(_) => {}
        }
    }

    fn schedule_refresh(&mut self, lifetime: Duration, now: Duration) {
        let early = lifetime.saturating_sub(Duration::from_secs(60)).max(lifetime / 2);
        self.refresh_at = now + early;
        self.expires = now + lifetime;
    }

    fn refused(&mut self, purpose: Purpose, m: &Message<'_>, now: Duration) {
        let code = m.error_code().unwrap_or(0);
        match code {
            // Unauthorized (the first answer), or a stale nonce: sign with
            // what the server says and ask again.
            401 | 438 => {
                self.auth_answers += 1;
                let realm = m.text(REALM).map(str::to_owned).or_else(|| self.realm.clone());
                let (Some(realm), Some(nonce)) = (realm, m.text(NONCE)) else {
                    return self.fail(format!("the TURN server refused with {code} and no realm or nonce"));
                };
                if self.auth_answers > AUTH_TRIES || (code == 401 && self.key.is_some() && self.auth_answers > 1) {
                    return self.fail("the TURN server refused the credentials (401)".into());
                }
                self.key = Some(long_term_key(&self.username, &realm, &self.password));
                self.realm = Some(realm);
                self.nonce = Some(nonce.to_owned());
                self.request(purpose, now);
            }
            _ => match purpose {
                Purpose::Allocate | Purpose::Refresh => self.fail(format!("the TURN server refused with {code}")),
                Purpose::Permission(ip) => {
                    self.permissions.remove(&ip);
                }
            },
        }
    }

    /// Relays `bytes` to `peer`: a Send indication to the server (after a
    /// permission for the peer's IP, asked for the first time it's needed).
    pub(super) fn send(&mut self, peer: SocketAddr, bytes: &[u8], now: Duration, out: &mut Vec<Vec<u8>>) {
        if self.relayed.is_none() {
            return;
        }
        let ip = peer.ip().to_canonical();
        if let std::collections::btree_map::Entry::Vacant(e) = self.permissions.entry(ip) {
            e.insert(now + PERMISSION_RENEW);
            self.request(Purpose::Permission(ip), now);
            self.poll(now, out);
        }
        let mut w = Writer::new(SEND, Class::Indication, self.transaction());
        w.xor_addr(XOR_PEER_ADDRESS, peer);
        w.attr(DATA_ATTR, bytes);
        out.push(w.finish());
    }

    pub(super) fn next_event(&mut self) -> Option<TurnEvent> {
        self.events.pop_front()
    }
}

/// A TURN server for the tests: allocations, long-term credentials,
/// permissions, Send and Data indications, refreshes. Sans-IO: the tests
/// carry its datagrams over the in-memory network or real sockets.
#[cfg(test)]
pub(super) mod test_server {
    use super::*;
    use std::collections::BTreeSet;

    pub(in crate::net) const REALM_NAME: &str = "example.org";

    struct Allocation {
        relay: SocketAddr,
        permissions: BTreeSet<IpAddr>,
    }

    /// What the server sends: to a client from its listening address, or
    /// from a relayed address to a peer.
    #[derive(Debug, PartialEq, Eq)]
    pub(in crate::net) enum Out {
        ToClient(SocketAddr, Vec<u8>),
        FromRelay {
            relay: SocketAddr,
            to: SocketAddr,
            bytes: Vec<u8>,
        },
    }

    pub(in crate::net) struct TestTurnServer {
        user: String,
        password: String,
        nonce: String,
        /// By client address.
        allocations: BTreeMap<SocketAddr, Allocation>,
        /// The next relayed address to hand out.
        relay: Box<dyn FnMut() -> SocketAddr + Send>,
        pub(in crate::net) relayed_to_peers: u64,
        pub(in crate::net) relayed_to_clients: u64,
        pub(in crate::net) refused: u64,
    }

    impl TestTurnServer {
        pub(in crate::net) fn new(
            user: &str,
            password: &str,
            relay: impl FnMut() -> SocketAddr + Send + 'static,
        ) -> Self {
            Self {
                user: user.into(),
                password: password.into(),
                nonce: "nonce-1".into(),
                allocations: BTreeMap::new(),
                relay: Box::new(relay),
                relayed_to_peers: 0,
                relayed_to_clients: 0,
                refused: 0,
            }
        }

        /// Makes the current nonce stale: the next signed request is told 438.
        pub(in crate::net) fn new_nonce(&mut self) {
            self.nonce.push('x');
        }

        pub(in crate::net) fn relays(&self) -> impl Iterator<Item = SocketAddr> + '_ {
            self.allocations.values().map(|a| a.relay)
        }

        fn error(m: &Message<'_>, code: u16, nonce: Option<&str>) -> Vec<u8> {
            let mut w = Writer::new(m.method, Class::Error, m.tx);
            let class = u8::try_from(code / 100).unwrap_or(4);
            let number = u8::try_from(code % 100).unwrap_or(0);
            w.attr(ERROR_CODE, &[0, 0, class, number]);
            if let Some(nonce) = nonce {
                w.attr(REALM, REALM_NAME.as_bytes());
                w.attr(NONCE, nonce.as_bytes());
            }
            w.finish()
        }

        /// A datagram from a client to the listening address.
        pub(in crate::net) fn on_client_packet(&mut self, from: SocketAddr, packet: &[u8], out: &mut Vec<Out>) {
            let Some(m) = parse(packet) else { return };
            if m.class == Class::Indication {
                if m.method == SEND
                    && let Some(a) = self.allocations.get(&from)
                    && let (Some(to), Some(bytes)) = (m.xor_addr(XOR_PEER_ADDRESS), m.get(DATA_ATTR))
                    && a.permissions.contains(&to.ip())
                {
                    self.relayed_to_peers += 1;
                    out.push(Out::FromRelay {
                        relay: a.relay,
                        to,
                        bytes: bytes.to_vec(),
                    });
                }
                return;
            }
            if m.class != Class::Request {
                return;
            }
            // Long-term credentials on every request.
            let key = long_term_key(&self.user, REALM_NAME, &self.password);
            if m.text(USERNAME) != Some(self.user.as_str()) || !m.verify(&key) {
                self.refused += 1;
                let nonce = self.nonce.clone();
                return out.push(Out::ToClient(from, Self::error(&m, 401, Some(&nonce))));
            }
            if m.text(NONCE) != Some(self.nonce.as_str()) {
                let nonce = self.nonce.clone();
                return out.push(Out::ToClient(from, Self::error(&m, 438, Some(&nonce))));
            }
            let mut w = Writer::new(m.method, Class::Success, m.tx);
            match m.method {
                ALLOCATE => {
                    if m.get(REQUESTED_TRANSPORT).and_then(|v| v.first()) != Some(&UDP) {
                        return out.push(Out::ToClient(from, Self::error(&m, 400, None)));
                    }
                    let relay = match self.allocations.get(&from) {
                        Some(_) => return out.push(Out::ToClient(from, Self::error(&m, 437, None))),
                        None => (self.relay)(),
                    };
                    self.allocations.insert(
                        from,
                        Allocation {
                            relay,
                            permissions: BTreeSet::new(),
                        },
                    );
                    w.xor_addr(XOR_RELAYED_ADDRESS, relay);
                    w.xor_addr(XOR_MAPPED_ADDRESS, from);
                    w.attr(LIFETIME, &600u32.to_be_bytes());
                }
                REFRESH => {
                    if !self.allocations.contains_key(&from) {
                        return out.push(Out::ToClient(from, Self::error(&m, 437, None)));
                    }
                    w.attr(LIFETIME, &600u32.to_be_bytes());
                }
                CREATE_PERMISSION => {
                    let (Some(a), Some(peer)) = (self.allocations.get_mut(&from), m.xor_addr(XOR_PEER_ADDRESS)) else {
                        return out.push(Out::ToClient(from, Self::error(&m, 437, None)));
                    };
                    a.permissions.insert(peer.ip());
                }
                _ => return out.push(Out::ToClient(from, Self::error(&m, 400, None))),
            }
            out.push(Out::ToClient(from, w.sign(&key)));
        }

        /// A datagram from a peer to relayed address `relay`: a Data
        /// indication to its client, if the client allowed the peer.
        pub(in crate::net) fn on_peer_packet(
            &mut self,
            relay: SocketAddr,
            from: SocketAddr,
            bytes: &[u8],
        ) -> Option<(SocketAddr, Vec<u8>)> {
            let (&client, _) = self
                .allocations
                .iter()
                .find(|(_, a)| a.relay == relay && a.permissions.contains(&from.ip()))?;
            self.relayed_to_clients += 1;
            let mut w = Writer::new(DATA, Class::Indication, [9; 12]);
            w.xor_addr(XOR_PEER_ADDRESS, from);
            w.attr(DATA_ATTR, bytes);
            Some((client, w.finish()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_server::{Out, REALM_NAME, TestTurnServer};
    use super::*;

    fn counter() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(1);
        N.fetch_add(1, Ordering::Relaxed)
    }

    const CLIENT: &str = "192.0.2.10:5000";
    const SERVER: &str = "198.51.100.1:3478";
    const RELAY: &str = "198.51.100.1:49152";
    const PEER: &str = "203.0.113.7:6000";

    fn server(password: &str) -> TestTurnServer {
        let _ = password;
        TestTurnServer::new("alice", "secret", || RELAY.parse().unwrap())
    }

    /// Carries datagrams between the client and the server until neither
    /// has more; what the server relays to peers is returned.
    fn exchange(client: &mut TurnClient, server: &mut TestTurnServer, now: Duration) -> Vec<(SocketAddr, Vec<u8>)> {
        let from: SocketAddr = CLIENT.parse().unwrap();
        let mut relayed = Vec::new();
        let mut to_server = Vec::new();
        client.poll(now, &mut to_server);
        for _ in 0..10 {
            if to_server.is_empty() {
                break;
            }
            let mut out = Vec::new();
            for packet in to_server.drain(..) {
                server.on_client_packet(from, &packet, &mut out);
            }
            for o in out {
                match o {
                    Out::ToClient(_, bytes) => assert!(client.receive(&bytes, now)),
                    Out::FromRelay { to, bytes, .. } => relayed.push((to, bytes)),
                }
            }
            client.poll(now, &mut to_server);
        }
        relayed
    }

    #[test]
    fn message_types_spread_the_class_bits_as_the_rfc_says() {
        assert_eq!(message_type(ALLOCATE, Class::Request), 0x0003);
        assert_eq!(message_type(ALLOCATE, Class::Success), 0x0103);
        assert_eq!(message_type(ALLOCATE, Class::Error), 0x0113);
        assert_eq!(message_type(SEND, Class::Indication), 0x0016);
        assert_eq!(message_type(DATA, Class::Indication), 0x0017);
        assert_eq!(message_type(CREATE_PERMISSION, Class::Success), 0x0108);
        for t in [0x0003, 0x0113, 0x0016, 0x0017, 0x0108, 0x0001, 0x0101] {
            let (m, c) = split_type(t);
            assert_eq!(message_type(m, c), t);
        }
    }

    #[test]
    fn hmac_sha1_matches_rfc_2202() {
        // RFC 2202, test cases 1 and 2.
        let mac = hmac_sha1(&[0x0b; 20], &[b"Hi There"]);
        assert_eq!(hex(&mac), "b617318655057264e28bc0b6fb378c8ef146be00");
        let mac = hmac_sha1(b"Jefe", &[b"what do ya want ", b"for nothing?"]);
        assert_eq!(hex(&mac), "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79");
    }

    #[test]
    fn the_long_term_key_is_md5_of_user_realm_and_password() {
        // MD5("user:realm:pass"), from a reference implementation.
        assert_eq!(
            hex(&long_term_key("user", "realm", "pass")),
            "8493fbc53ba582fb4c044c456bdc40eb"
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn a_signed_message_verifies_and_a_changed_one_does_not() {
        let key = long_term_key("alice", REALM_NAME, "secret");
        let mut w = Writer::new(ALLOCATE, Class::Request, [3; 12]);
        w.attr(REQUESTED_TRANSPORT, &[UDP, 0, 0, 0]).attr(USERNAME, b"alice");
        let bytes = w.sign(&key);
        assert!(parse(&bytes).unwrap().verify(&key));
        assert!(
            !parse(&bytes)
                .unwrap()
                .verify(&long_term_key("alice", REALM_NAME, "wrong"))
        );
        let mut changed = bytes.clone();
        changed[HEADER + 4] = 6; // TCP instead of UDP
        assert!(!parse(&changed).unwrap().verify(&key));
    }

    #[test]
    fn xor_addresses_round_trip_in_both_families() {
        for addr in ["192.0.2.1:32853", "[2001:db8::1]:40000"] {
            let addr: SocketAddr = addr.parse().unwrap();
            let mut w = Writer::new(DATA, Class::Indication, [7; 12]);
            w.xor_addr(XOR_PEER_ADDRESS, addr);
            let bytes = w.finish();
            assert_eq!(parse(&bytes).unwrap().xor_addr(XOR_PEER_ADDRESS), Some(addr));
        }
    }

    #[test]
    fn a_client_allocates_with_long_term_credentials_and_relays_both_ways() {
        let mut server = server("secret");
        let mut client = TurnClient::with_random(SERVER.parse().unwrap(), "alice", "secret", counter);
        assert!(exchange(&mut client, &mut server, Duration::ZERO).is_empty());
        assert_eq!(server.refused, 1, "the first, unsigned request is told the realm");
        assert_eq!(client.relayed(), Some(RELAY.parse().unwrap()));
        assert_eq!(
            client.next_event(),
            Some(TurnEvent::Allocated {
                relayed: RELAY.parse().unwrap(),
                mapped: Some(CLIENT.parse().unwrap()),
            })
        );

        let peer: SocketAddr = PEER.parse().unwrap();
        let mut out = Vec::new();
        client.send(peer, b"ping", Duration::ZERO, &mut out);
        assert_eq!(out.len(), 2, "a permission, then the packet");
        let mut relayed = Vec::new();
        for packet in out {
            let mut o = Vec::new();
            server.on_client_packet(CLIENT.parse().unwrap(), &packet, &mut o);
            for o in o {
                match o {
                    Out::ToClient(_, bytes) => assert!(client.receive(&bytes, Duration::ZERO)),
                    Out::FromRelay { to, bytes, relay } => {
                        assert_eq!(relay, RELAY.parse().unwrap());
                        relayed.push((to, bytes));
                    }
                }
            }
        }
        assert_eq!(relayed, [(peer, b"ping".to_vec())]);
        let (to, data) = server.on_peer_packet(RELAY.parse().unwrap(), peer, b"pong").unwrap();
        assert_eq!(to, CLIENT.parse().unwrap());
        assert!(client.receive(&data, Duration::ZERO));
        assert_eq!(
            client.next_event(),
            Some(TurnEvent::Data {
                peer,
                bytes: b"pong".to_vec()
            })
        );
        // A stranger the client never sent to can't reach it.
        assert!(
            server
                .on_peer_packet(RELAY.parse().unwrap(), "203.0.113.99:1".parse().unwrap(), b"x")
                .is_none()
        );
    }

    #[test]
    fn a_stale_nonce_is_replaced_and_the_request_sent_again() {
        let mut server = server("secret");
        let mut client = TurnClient::with_random(SERVER.parse().unwrap(), "alice", "secret", counter);
        exchange(&mut client, &mut server, Duration::ZERO);
        server.new_nonce();
        // The refresh, a minute before the 600 s lifetime ends.
        let later = Duration::from_secs(540);
        exchange(&mut client, &mut server, later);
        assert_eq!(client.relayed(), Some(RELAY.parse().unwrap()));
        let mut out = Vec::new();
        client.poll(Duration::from_secs(700), &mut out);
        assert!(
            client.relayed().is_some(),
            "refreshed: still allocated after the first lifetime"
        );
    }

    #[test]
    fn an_unsigned_or_forged_answer_to_a_signed_request_is_ignored() {
        let server_addr: SocketAddr = SERVER.parse().unwrap();
        let mut client = TurnClient::with_random(server_addr, "alice", "secret", counter);
        let mut out = Vec::new();
        client.poll(Duration::ZERO, &mut out);
        // The server asks for credentials; the client signs its request.
        let first = parse(&out[0]).unwrap().tx;
        let mut w = Writer::new(ALLOCATE, Class::Error, first);
        w.attr(ERROR_CODE, &[0, 0, 4, 1]);
        w.attr(REALM, REALM_NAME.as_bytes()).attr(NONCE, b"n");
        assert!(client.receive(&w.finish(), Duration::ZERO));
        out.clear();
        client.poll(Duration::ZERO, &mut out);
        let signed = parse(&out[0]).unwrap().tx;
        let forged = "203.0.113.66:4000".parse().unwrap();
        // Unsigned, and signed with another password: both ignored.
        let mut w = Writer::new(ALLOCATE, Class::Success, signed);
        w.xor_addr(XOR_RELAYED_ADDRESS, forged);
        assert!(client.receive(&w.finish(), Duration::ZERO));
        let mut w = Writer::new(ALLOCATE, Class::Success, signed);
        w.xor_addr(XOR_RELAYED_ADDRESS, forged);
        let wrong = w.sign(&long_term_key("alice", REALM_NAME, "guess"));
        assert!(client.receive(&wrong, Duration::ZERO));
        assert_eq!(client.relayed(), None);
        // The real answer still counts.
        let mut w = Writer::new(ALLOCATE, Class::Success, signed);
        w.xor_addr(XOR_RELAYED_ADDRESS, RELAY.parse().unwrap());
        let real = w.sign(&long_term_key("alice", REALM_NAME, "secret"));
        assert!(client.receive(&real, Duration::ZERO));
        assert_eq!(client.relayed(), Some(RELAY.parse().unwrap()));
    }

    #[test]
    fn permissions_are_renewed_before_they_expire() {
        let mut server = server("secret");
        let mut client = TurnClient::with_random(SERVER.parse().unwrap(), "alice", "secret", counter);
        exchange(&mut client, &mut server, Duration::ZERO);
        let peer: SocketAddr = PEER.parse().unwrap();
        let mut out = Vec::new();
        client.send(peer, b"ping", Duration::ZERO, &mut out);
        let permission = |bytes: &Vec<u8>| parse(bytes).is_some_and(|m| m.method == CREATE_PERMISSION);
        assert_eq!(out.iter().filter(|b| permission(b)).count(), 1);
        for o in &out {
            let mut answers = Vec::new();
            server.on_client_packet(CLIENT.parse().unwrap(), o, &mut answers);
            for a in answers {
                if let Out::ToClient(_, bytes) = a {
                    client.receive(&bytes, Duration::ZERO);
                }
            }
        }
        // Permissions last five minutes (RFC 8656); the client renews them
        // a minute early, and not before.
        out.clear();
        client.poll(PERMISSION_RENEW - Duration::from_secs(1), &mut out);
        assert!(!out.iter().any(permission));
        client.poll(PERMISSION_RENEW, &mut out);
        assert_eq!(out.iter().filter(|b| permission(b)).count(), 1);
    }

    #[test]
    fn wrong_credentials_fail_the_allocation() {
        let mut server = server("secret");
        let mut client = TurnClient::with_random(SERVER.parse().unwrap(), "alice", "wrong", counter);
        exchange(&mut client, &mut server, Duration::ZERO);
        assert_eq!(client.relayed(), None);
        assert!(matches!(client.next_event(), Some(TurnEvent::Failed(why)) if why.contains("401")));
    }

    #[test]
    fn a_silent_server_is_given_up_after_its_resends() {
        let mut client = TurnClient::with_random(SERVER.parse().unwrap(), "alice", "secret", counter);
        let mut out = Vec::new();
        for ms in (0..20_000).step_by(100) {
            client.poll(Duration::from_millis(ms), &mut out);
        }
        assert_eq!(out.len(), TRIES as usize, "the request and its resends");
        assert!(matches!(client.next_event(), Some(TurnEvent::Failed(_))));
    }
}
