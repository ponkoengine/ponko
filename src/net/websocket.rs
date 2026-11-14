//! Browser players over WebSocket: the packet transport under lockstep
//! ([`Net`](super::Net)) and messages ([`NetMessages`](super::NetMessages))
//! for clients that can't open a UDP socket.
//!
//! Every datagram the sessions send is one binary WebSocket message, so the
//! session protocols above don't change: they resend what's lost on UDP and
//! simply find nothing lost here. A host keeps its UDP socket and adds a TCP
//! listener ([`WebListener`]); browser connections show up in the session as
//! addresses in a private IPv6 range ([`conn_addr`]), next to the UDP
//! players' real ones, so desktop and browser players share one session.
//!
//! The desktop side is `tungstenite` on std sockets, nonblocking and polled
//! from the game thread like the UDP transports. A desktop client connects
//! on a short-lived thread (std has no nonblocking connect) that retries
//! until the host is up, the way a UDP client resends its hello. The browser
//! side is the browser's own `WebSocket` (`websocket_browser.rs`), with the
//! same [`WebClient`] calls.
//!
//! Plain `ws://` only: a page served over `https://` needs `wss://`, which a
//! TLS proxy in front of the host (Caddy, nginx) provides. Like UDP sessions,
//! connections are unauthenticated and any page may connect.

use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};

#[cfg(not(target_arch = "wasm32"))]
pub(super) use native::{WebClient, WebListener};

#[cfg(target_arch = "wasm32")]
pub(super) use super::websocket_browser::WebClient;

/// The largest packet either side accepts, far above the sessions' own
/// (lockstep's 1400 bytes, messages' 1500): a bound on what one peer can
/// make the other buffer.
pub(super) const MAX_PACKET: usize = 64 * 1024;
/// Bytes queued for a slow connection before further packets are dropped
/// (the sessions resend what matters).
pub(super) const MAX_BUFFERED: usize = 1024 * 1024;

/// A browser connection's id travels in the session as an address in this
/// private IPv6 range (`fd77:7765:6273:6f63::/64`, "wwebsoc"), so the
/// sessions treat browser and UDP players alike.
const PREFIX: [u16; 4] = [0xfd77, 0x7765, 0x6273, 0x6f63];

/// The address a WebSocket client's session sends to and hears its host
/// from: there is only the one connection.
pub(super) const HOST: SocketAddr = SocketAddr::V6(SocketAddrV6::new(
    Ipv6Addr::new(PREFIX[0], PREFIX[1], PREFIX[2], PREFIX[3], 0, 0, 0, 0),
    1,
    0,
    0,
));

/// The session address of the host's connection `id` (ids start at 1).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(super) fn conn_addr(id: u64) -> SocketAddr {
    let ip = Ipv6Addr::new(
        PREFIX[0],
        PREFIX[1],
        PREFIX[2],
        PREFIX[3],
        (id >> 48) as u16,
        (id >> 32) as u16,
        (id >> 16) as u16,
        id as u16,
    );
    SocketAddr::V6(SocketAddrV6::new(ip, 0, 0, 0))
}

/// The connection id behind a session address, if it is one.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(super) fn conn_id(addr: SocketAddr) -> Option<u64> {
    let SocketAddr::V6(v6) = addr else { return None };
    let s = v6.ip().segments();
    if s[..4] != PREFIX || addr == HOST {
        return None;
    }
    Some(u64::from(s[4]) << 48 | u64::from(s[5]) << 32 | u64::from(s[6]) << 16 | u64::from(s[7]))
}

/// What a `ws://host:port/path` URL names, checked before connecting, so a
/// typo fails at once instead of timing out.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct WebUrl {
    pub host: String,
    pub port: u16,
}

/// Checks a WebSocket URL: `ws://` (and `wss://` where `secure` is
/// allowed, in the browser), a host, an optional port (80, or 443 for
/// `wss`).
pub(super) fn parse_url(url: &str, secure: bool) -> std::io::Result<WebUrl> {
    let invalid = |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("{url}: {why}"));
    let (rest, default_port) = if let Some(rest) = url.strip_prefix("ws://") {
        (rest, 80)
    } else if let Some(rest) = url.strip_prefix("wss://") {
        if !secure {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!("{url}: the desktop client speaks plain ws://; wss:// needs a browser or a TLS proxy"),
            ));
        }
        (rest, 443)
    } else {
        return Err(invalid("not a ws:// URL"));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return Err(invalid("user names are not supported"));
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (ip, after) = v6.split_once(']').ok_or_else(|| invalid("unclosed IPv6 address"))?;
        (ip, after.strip_prefix(':'))
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    if host.is_empty() {
        return Err(invalid("no host"));
    }
    let port = match port {
        Some(p) => p.parse().map_err(|_| invalid("bad port"))?,
        None => default_port,
    };
    Ok(WebUrl {
        host: host.to_string(),
        port,
    })
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::BTreeMap;
    use std::io;
    use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    use tungstenite::handshake::MidHandshake;
    use tungstenite::handshake::server::{NoCallback, ServerHandshake};
    use tungstenite::protocol::WebSocketConfig;
    use tungstenite::{HandshakeError, Message, WebSocket};

    use super::{MAX_BUFFERED, MAX_PACKET, conn_addr, conn_id, parse_url};
    use crate::perf::Instant;

    /// Handshakes in flight at once; more connections wait in the OS backlog.
    const MAX_HANDSHAKES: usize = 64;
    /// Open connections at most (lockstep has 8 players, messages 1024 clients).
    const MAX_CONNECTIONS: usize = 1100;
    /// A connection that hasn't finished its handshake by then is dropped.
    const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
    /// A desktop client retries connecting this long, then reports the failure.
    const CONNECT_PATIENCE: Duration = Duration::from_secs(300);
    const CONNECT_RETRY: Duration = Duration::from_millis(100);

    fn config() -> WebSocketConfig {
        WebSocketConfig::default()
            .read_buffer_size(16 * 1024)
            // Every packet goes out at once; a full socket keeps the rest
            // queued, up to MAX_BUFFERED.
            .write_buffer_size(0)
            .max_write_buffer_size(MAX_BUFFERED)
            .max_message_size(Some(MAX_PACKET))
            .max_frame_size(Some(MAX_PACKET))
    }

    /// Sends one packet: `Ok(true)` once it is written or queued, `Ok(false)`
    /// when the connection's queue is full (the packet is dropped, as UDP
    /// would), `Err` when the connection is dead.
    fn send(ws: &mut WebSocket<TcpStream>, bytes: &[u8]) -> Result<bool, tungstenite::Error> {
        match ws.send(Message::binary(bytes.to_vec())) {
            Ok(()) => Ok(true),
            Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => Ok(true),
            Err(tungstenite::Error::WriteBufferFull(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// The next binary packet on `ws` copied into `buf`: `Ok(Some(n))`,
    /// `Ok(None)` when nothing more is waiting, `Err` once the connection is
    /// closed or broken. Text, pings (answered by tungstenite) and packets
    /// larger than `buf` are skipped.
    fn read(ws: &mut WebSocket<TcpStream>, buf: &mut [u8]) -> Result<Option<usize>, tungstenite::Error> {
        loop {
            match ws.read() {
                Ok(Message::Binary(bytes)) if bytes.len() <= buf.len() => {
                    buf[..bytes.len()].copy_from_slice(&bytes);
                    return Ok(Some(bytes.len()));
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(e) => return Err(e),
            }
        }
    }

    type Handshake = MidHandshake<ServerHandshake<TcpStream, NoCallback>>;

    /// A host's WebSocket side: accepts browser connections on a TCP port and
    /// moves their packets. Polled; never blocks.
    pub(in crate::net) struct WebListener {
        listener: TcpListener,
        handshakes: Vec<(Instant, Handshake)>,
        conns: BTreeMap<u64, WebSocket<TcpStream>>,
        next_id: u64,
        /// The connection read first on the next poll, so one busy browser
        /// can't starve the others.
        cursor: u64,
    }

    impl WebListener {
        pub(in crate::net) fn bind(addr: impl ToSocketAddrs) -> io::Result<Self> {
            let listener = TcpListener::bind(addr)?;
            listener.set_nonblocking(true)?;
            Ok(Self {
                listener,
                handshakes: Vec::new(),
                conns: BTreeMap::new(),
                next_id: 1,
                cursor: 0,
            })
        }

        pub(in crate::net) fn local_addr(&self) -> io::Result<SocketAddr> {
            self.listener.local_addr()
        }

        /// Whether `addr` is one of this listener's connections (or could be).
        pub(in crate::net) fn owns(addr: SocketAddr) -> bool {
            conn_id(addr).is_some()
        }

        /// Sends to connection `to`: `false` if it is gone or its queue is full.
        pub(in crate::net) fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> bool {
            let Some(id) = conn_id(to) else { return false };
            let Some(ws) = self.conns.get_mut(&id) else {
                return false;
            };
            match send(ws, bytes) {
                Ok(sent) => sent,
                Err(_) => {
                    self.conns.remove(&id);
                    false
                }
            }
        }

        /// The next packet from any connection, after accepting new ones.
        pub(in crate::net) fn recv(&mut self, buf: &mut [u8]) -> Option<(SocketAddr, usize)> {
            self.accept();
            // Round robin from the cursor, without collecting the ids: this
            // runs every poll.
            let mut next = self.cursor;
            for _ in 0..self.conns.len() {
                let Some(id) = self
                    .conns
                    .range(next..)
                    .next()
                    .or_else(|| self.conns.iter().next())
                    .map(|(&id, _)| id)
                else {
                    break;
                };
                next = id + 1;
                let ws = self.conns.get_mut(&id).expect("found");
                match read(ws, buf) {
                    Ok(Some(n)) => {
                        self.cursor = next;
                        return Some((conn_addr(id), n));
                    }
                    Ok(None) => {}
                    Err(_) => {
                        self.conns.remove(&id);
                    }
                }
            }
            None
        }

        /// Takes new connections and moves their handshakes along.
        fn accept(&mut self) {
            let now = Instant::now();
            while self.handshakes.len() < MAX_HANDSHAKES {
                let Ok((stream, _)) = self.listener.accept() else { break };
                if self.conns.len() >= MAX_CONNECTIONS
                    || stream.set_nonblocking(true).is_err()
                    || stream.set_nodelay(true).is_err()
                {
                    continue; // dropped: closes the connection
                }
                self.handshake(now, tungstenite::accept_with_config(stream, Some(config())));
            }
            for (started, mid) in std::mem::take(&mut self.handshakes) {
                if now - started > HANDSHAKE_TIMEOUT {
                    continue;
                }
                let result = mid.handshake();
                self.handshake(started, result);
            }
        }

        fn handshake(
            &mut self,
            started: Instant,
            result: Result<WebSocket<TcpStream>, HandshakeError<ServerHandshake<TcpStream, NoCallback>>>,
        ) {
            match result {
                Ok(ws) => {
                    self.conns.insert(self.next_id, ws);
                    self.next_id += 1;
                }
                Err(HandshakeError::Interrupted(mid)) => self.handshakes.push((started, mid)),
                Err(HandshakeError::Failure(_)) => {} // not a WebSocket client
            }
        }
    }

    enum State {
        /// The connect thread is trying; it sends the open socket or why it gave up.
        Connecting(mpsc::Receiver<Result<WebSocket<TcpStream>, String>>),
        Open(Box<WebSocket<TcpStream>>),
        Closed,
    }

    /// A desktop client's connection to a host's [`WebListener`].
    pub(in crate::net) struct WebClient {
        state: State,
        error: Option<String>,
        /// Tells the connect thread to give up (on drop).
        stop: Arc<AtomicBool>,
    }

    impl WebClient {
        /// Starts connecting to `url` (`ws://host:port/path`) in the
        /// background. Fails at once only on a URL it can't use.
        pub(in crate::net) fn connect(url: &str) -> io::Result<Self> {
            let target = parse_url(url, false)?;
            let url = url.to_string();
            let stop = Arc::new(AtomicBool::new(false));
            let (tx, rx) = mpsc::channel();
            let stopped = stop.clone();
            std::thread::Builder::new()
                .name("ponko websocket connect".into())
                .spawn(move || {
                    let _ = tx.send(connect(&url, (target.host.as_str(), target.port), &stopped));
                })?;
            Ok(Self {
                state: State::Connecting(rx),
                error: None,
                stop,
            })
        }

        /// Sends a packet to the host; `false` while connecting, once closed
        /// or when the queue is full.
        pub(in crate::net) fn send(&mut self, bytes: &[u8]) -> bool {
            self.update();
            let State::Open(ws) = &mut self.state else { return false };
            match send(ws, bytes) {
                Ok(sent) => sent,
                Err(e) => {
                    self.close(format!("the connection to the host broke: {e}"));
                    false
                }
            }
        }

        /// The next packet from the host.
        pub(in crate::net) fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
            self.update();
            let State::Open(ws) = &mut self.state else { return None };
            match read(ws, buf) {
                Ok(n) => n,
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    self.close("the host closed the connection".into());
                    None
                }
                Err(e) => {
                    self.close(format!("the connection to the host broke: {e}"));
                    None
                }
            }
        }

        /// Why the connection failed or closed, once.
        pub(in crate::net) fn take_error(&mut self) -> Option<String> {
            self.update();
            self.error.take()
        }

        fn update(&mut self) {
            if let State::Connecting(rx) = &self.state {
                match rx.try_recv() {
                    Ok(Ok(ws)) => self.state = State::Open(Box::new(ws)),
                    Ok(Err(e)) => self.close(e),
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => self.close("the connect thread stopped".into()),
                }
            }
        }

        fn close(&mut self, why: String) {
            self.state = State::Closed;
            self.error = Some(why);
        }
    }

    impl Drop for WebClient {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let State::Open(ws) = &mut self.state {
                // Best effort: the host notices a dropped socket either way.
                let _ = ws.close(None);
                let _ = ws.flush();
            }
        }
    }

    /// The connect thread: TCP, then the WebSocket handshake, retrying while
    /// nobody listens yet.
    fn connect(url: &str, target: (&str, u16), stop: &AtomicBool) -> Result<WebSocket<TcpStream>, String> {
        let start = Instant::now();
        loop {
            if stop.load(Ordering::Relaxed) {
                return Err("stopped".into());
            }
            let stream = match TcpStream::connect(target) {
                Ok(s) => s,
                Err(e) => {
                    if start.elapsed() > CONNECT_PATIENCE {
                        return Err(format!("could not connect to {url}: {e}"));
                    }
                    std::thread::sleep(CONNECT_RETRY);
                    continue;
                }
            };
            let _ = stream.set_nodelay(true);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let (ws, _) = tungstenite::client::client_with_config(url, stream, Some(config()))
                .map_err(|e| format!("{url} refused the WebSocket handshake: {e}"))?;
            ws.get_ref()
                .set_nonblocking(true)
                .map_err(|e| format!("could not use the connection to {url}: {e}"))?;
            return Ok(ws);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_addresses_round_trip_and_stay_apart_from_the_host() {
        for id in [1, 2, 0xffff, 0x1_0000, u64::MAX] {
            assert_eq!(conn_id(conn_addr(id)), Some(id));
        }
        assert_eq!(conn_id(HOST), None, "the client's host is no connection");
        assert_eq!(conn_id("127.0.0.1:7777".parse().unwrap()), None);
        assert_eq!(conn_id("[::1]:7777".parse().unwrap()), None);
    }

    #[test]
    fn urls_name_a_host_and_port() {
        let url = |host: &str, port| WebUrl {
            host: host.into(),
            port,
        };
        assert_eq!(parse_url("ws://127.0.0.1:7778", false).unwrap(), url("127.0.0.1", 7778));
        assert_eq!(
            parse_url("ws://example.com/game?x=1", false).unwrap(),
            url("example.com", 80)
        );
        assert_eq!(parse_url("ws://[::1]:9000/", false).unwrap(), url("::1", 9000));
        assert_eq!(parse_url("wss://example.com", true).unwrap(), url("example.com", 443));
        assert_eq!(
            parse_url("wss://example.com", false).unwrap_err().kind(),
            std::io::ErrorKind::Unsupported
        );
        for bad in ["http://example.com", "ws://", "ws://:80", "ws://host:port", "ws://a@b"] {
            assert!(parse_url(bad, true).is_err(), "{bad}");
        }
    }
}
