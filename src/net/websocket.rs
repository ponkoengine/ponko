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

use std::net::{IpAddr, Ipv6Addr, SocketAddr, SocketAddrV6};

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

/// Where a browser (WebSocket) client's connection comes from, as a host
/// sees it: the TCP peer, and the client addresses a reverse proxy in front
/// of the host wrote into the upgrade request's `X-Forwarded-For` header.
/// From [`NetMessages::web_peer`](super::NetMessages::web_peer).
///
/// The header is only as honest as whoever wrote it: anyone may send one.
/// [`client_ip`](Self::client_ip) believes it only from proxies the caller
/// trusts.
///
/// ```
/// use ponko::net::WebPeer;
/// let peer = WebPeer::new("10.0.0.2:41000".parse().unwrap(), Some("203.0.113.7"));
/// assert_eq!(peer.forwarded_for(), Some("203.0.113.7"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebPeer {
    remote: SocketAddr,
    forwarded_for: Option<String>,
}

/// Hops of an `X-Forwarded-For` header read at most.
const MAX_HOPS: usize = 32;

impl WebPeer {
    /// A peer with this TCP address and `X-Forwarded-For` header (several
    /// header lines joined with commas, in order), for tests and servers of
    /// your own.
    /// ```
    /// let peer = ponko::net::WebPeer::new("192.0.2.1:5000".parse().unwrap(), None);
    /// ```
    pub fn new(remote: SocketAddr, forwarded_for: Option<&str>) -> Self {
        Self {
            remote,
            forwarded_for: forwarded_for.map(str::to_owned),
        }
    }

    /// The address the TCP connection comes from: the browser, or the
    /// proxy in front of the host.
    /// ```
    /// let peer = ponko::net::WebPeer::new("192.0.2.1:5000".parse().unwrap(), None);
    /// assert_eq!(peer.remote().port(), 5000);
    /// ```
    pub fn remote(&self) -> SocketAddr {
        self.remote
    }

    /// The request's `X-Forwarded-For` header as sent, if any.
    /// ```
    /// assert_eq!(ponko::net::WebPeer::new("192.0.2.1:5000".parse().unwrap(), None).forwarded_for(), None);
    /// ```
    pub fn forwarded_for(&self) -> Option<&str> {
        self.forwarded_for.as_deref()
    }

    /// The client's IP address, believing the `X-Forwarded-For` header only
    /// as far as `trusted` says: starting from the TCP peer, each hop that
    /// is a trusted proxy hands over to the address it wrote (the header's
    /// last entry first). The first address that isn't trusted is the
    /// client. A connection from an untrusted peer is the peer itself,
    /// whatever its header says, so a client can't pick an address by
    /// sending the header.
    ///
    /// `None` when a trusted proxy sent no header or an entry that isn't an
    /// IP address (`unknown`, a name): the client can't be told apart from
    /// the proxy. Entries may carry a port (`192.0.2.1:5000`,
    /// `[2001:db8::1]:5000`). When every hop is trusted, the first entry.
    ///
    /// ```
    /// use std::net::IpAddr;
    /// use ponko::net::WebPeer;
    /// let proxy: IpAddr = "10.0.0.2".parse().unwrap();
    /// let trusted = |ip: IpAddr| ip == proxy;
    /// // Through the proxy, which appended the address it saw:
    /// let peer = WebPeer::new("10.0.0.2:41000".parse().unwrap(), Some("198.51.100.1, 203.0.113.7"));
    /// assert_eq!(peer.client_ip(trusted), Some("203.0.113.7".parse().unwrap()));
    /// // Straight from a client that wrote the header itself: ignored.
    /// let peer = WebPeer::new("203.0.113.9:5000".parse().unwrap(), Some("198.51.100.1"));
    /// assert_eq!(peer.client_ip(trusted), Some("203.0.113.9".parse().unwrap()));
    /// ```
    pub fn client_ip(&self, trusted: impl Fn(IpAddr) -> bool) -> Option<IpAddr> {
        let mut ip = self.remote.ip().to_canonical();
        if !trusted(ip) {
            return Some(ip);
        }
        let header = self.forwarded_for.as_deref()?;
        for entry in header.rsplit(',').take(MAX_HOPS) {
            ip = parse_hop(entry.trim())?;
            if !trusted(ip) {
                return Some(ip);
            }
        }
        if header.split(',').nth(MAX_HOPS).is_some() {
            return None; // Longer than any real chain of proxies.
        }
        Some(ip)
    }
}

/// One `X-Forwarded-For` entry: an IP address, with or without a port.
fn parse_hop(entry: &str) -> Option<IpAddr> {
    if let Ok(ip) = entry.parse::<IpAddr>() {
        return Some(ip.to_canonical());
    }
    if let Some(v6) = entry.strip_prefix('[') {
        let (ip, _) = v6.split_once(']')?;
        return ip.parse::<IpAddr>().ok().map(|ip| ip.to_canonical());
    }
    entry.parse::<SocketAddr>().ok().map(|a| a.ip().to_canonical())
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tungstenite::handshake::MidHandshake;
    use tungstenite::handshake::server::{Callback, ErrorResponse, Request, Response, ServerHandshake};
    use tungstenite::protocol::WebSocketConfig;
    use tungstenite::{HandshakeError, Message, WebSocket};

    use super::{MAX_BUFFERED, MAX_PACKET, WebPeer, conn_addr, conn_id, parse_url};
    use crate::perf::Instant;

    /// Handshakes in flight at once; more connections wait in the OS backlog.
    const MAX_HANDSHAKES: usize = 64;
    /// Open connections at most (lockstep has 8 players, messages 1024 clients).
    const MAX_CONNECTIONS: usize = 1100;
    /// A connection that hasn't finished its handshake by then is dropped.
    const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
    /// An open connection that sends no packet for this long is dropped:
    /// the sessions send several a second (their heartbeats), so only a
    /// client that opened a socket to hold a slot goes this quiet.
    pub(in crate::net) const SILENCE_TIMEOUT: Duration = Duration::from_secs(30);
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

    /// The longest `X-Forwarded-For` header kept; a longer one is dropped.
    const MAX_FORWARDED: usize = 1024;

    /// Keeps the upgrade request's `X-Forwarded-For` header (every line of
    /// it, joined in order) for [`WebPeer`].
    struct Capture(Arc<Mutex<Option<String>>>);

    impl Callback for Capture {
        fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
            let lines: Vec<&str> = request
                .headers()
                .get_all("x-forwarded-for")
                .iter()
                .filter_map(|v| v.to_str().ok())
                .collect();
            let joined = lines.join(", ");
            if !lines.is_empty() && joined.len() <= MAX_FORWARDED {
                *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(joined);
            }
            Ok(response)
        }
    }

    type Handshake = MidHandshake<ServerHandshake<TcpStream, Capture>>;

    /// A handshake in flight: when it started, the TCP peer, and where its
    /// proxy header lands.
    struct Pending {
        started: Instant,
        remote: SocketAddr,
        forwarded: Arc<Mutex<Option<String>>>,
    }

    impl Pending {
        fn peer(&self) -> WebPeer {
            let forwarded = self.forwarded.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            WebPeer::new(self.remote, forwarded.as_deref())
        }
    }

    /// A host's WebSocket side: accepts browser connections on a TCP port and
    /// moves their packets. Polled; never blocks.
    pub(in crate::net) struct WebListener {
        listener: TcpListener,
        handshakes: Vec<(Pending, Handshake)>,
        conns: BTreeMap<u64, WebSocket<TcpStream>>,
        /// When each connection last sent a packet (or opened).
        heard: BTreeMap<u64, Instant>,
        /// Connections silent past this are dropped ([`SILENCE_TIMEOUT`]).
        silence: Duration,
        /// Where each connection comes from.
        peers: BTreeMap<u64, WebPeer>,
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
                heard: BTreeMap::new(),
                silence: SILENCE_TIMEOUT,
                peers: BTreeMap::new(),
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
                    self.peers.remove(&id);
                    false
                }
            }
        }

        /// Where connection `addr` comes from, while it is open.
        pub(in crate::net) fn peer(&self, addr: SocketAddr) -> Option<WebPeer> {
            conn_id(addr).and_then(|id| self.peers.get(&id)).cloned()
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
                        self.heard.insert(id, Instant::now());
                        return Some((conn_addr(id), n));
                    }
                    Ok(None) => {}
                    Err(_) => {
                        self.conns.remove(&id);
                        self.peers.remove(&id);
                    }
                }
            }
            None
        }

        /// Takes new connections and moves their handshakes along.
        fn accept(&mut self) {
            let now = Instant::now();
            while self.handshakes.len() < MAX_HANDSHAKES {
                let Ok((stream, remote)) = self.listener.accept() else {
                    break;
                };
                if self.conns.len() >= MAX_CONNECTIONS
                    || stream.set_nonblocking(true).is_err()
                    || stream.set_nodelay(true).is_err()
                {
                    continue; // dropped: closes the connection
                }
                let pending = Pending {
                    started: now,
                    remote,
                    forwarded: Arc::default(),
                };
                let capture = Capture(pending.forwarded.clone());
                self.handshake(
                    pending,
                    tungstenite::accept_hdr_with_config(stream, capture, Some(config())),
                );
            }
            for (pending, mid) in std::mem::take(&mut self.handshakes) {
                if now - pending.started > HANDSHAKE_TIMEOUT {
                    continue;
                }
                let result = mid.handshake();
                self.handshake(pending, result);
            }
            // Gone connections (closed elsewhere) and silent ones.
            let (conns, peers, silence) = (&mut self.conns, &mut self.peers, self.silence);
            self.heard.retain(|id, heard| {
                let keep = conns.contains_key(id) && now - *heard <= silence;
                if !keep {
                    conns.remove(id);
                    peers.remove(id);
                }
                keep
            });
        }

        /// Drops connections silent for `silence` instead of [`SILENCE_TIMEOUT`] (tests).
        #[cfg(test)]
        pub(in crate::net) fn set_silence_timeout(&mut self, silence: Duration) {
            self.silence = silence;
        }

        /// Open connections (tests).
        #[cfg(test)]
        pub(in crate::net) fn connections(&self) -> usize {
            self.conns.len()
        }

        fn handshake(
            &mut self,
            pending: Pending,
            result: Result<WebSocket<TcpStream>, HandshakeError<ServerHandshake<TcpStream, Capture>>>,
        ) {
            match result {
                Ok(ws) => {
                    self.conns.insert(self.next_id, ws);
                    self.heard.insert(self.next_id, Instant::now());
                    self.peers.insert(self.next_id, pending.peer());
                    self.next_id += 1;
                }
                Err(HandshakeError::Interrupted(mid)) => self.handshakes.push((pending, mid)),
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn an_open_connection_that_sends_nothing_is_dropped() {
        use std::io::{Read, Write};
        use std::time::Duration;
        let mut listener = WebListener::bind("127.0.0.1:0").unwrap();
        listener.set_silence_timeout(Duration::from_millis(200));
        let mut socket = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        socket
            .write_all(
                b"GET / HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                  Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            )
            .unwrap();
        let mut buf = [0u8; 2048];
        let poll = |listener: &mut WebListener, buf: &mut [u8]| while listener.recv(buf).is_some() {};
        let start = std::time::Instant::now();
        while listener.connections() == 0 {
            assert!(start.elapsed() < Duration::from_secs(5), "the handshake finished");
            poll(&mut listener, &mut buf);
            std::thread::sleep(Duration::from_millis(5));
        }
        // Open, then silent: dropped once the timeout passes, not before.
        poll(&mut listener, &mut buf);
        assert_eq!(listener.connections(), 1);
        std::thread::sleep(Duration::from_millis(300));
        poll(&mut listener, &mut buf);
        assert_eq!(listener.connections(), 0, "a silent connection kept its slot");
        // The client sees the socket closed: the 101 reply, then the end.
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut reply = Vec::new();
        let _ = socket.read_to_end(&mut reply);
        assert!(
            reply.starts_with(b"HTTP/1.1 101"),
            "{}",
            String::from_utf8_lossy(&reply)
        );
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
