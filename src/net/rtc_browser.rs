//! The browser side of the WebRTC transport (`rtc.rs`): the page's own
//! `RTCPeerConnection` and `RTCDataChannel`, with the same calls as the
//! desktop agent (`rtc_native.rs`).
//!
//! The browser runs ICE, DTLS and SCTP itself, asks the STUN and TURN
//! servers of the configuration, and hands results over in callbacks and
//! promises on the page's event loop; they wait in the agent's state until
//! the game polls. The state lives in a thread-local table (a web build has
//! one thread), so the agent itself is a plain id and endpoints stay `Send`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::time::Duration;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    MessageEvent, RtcConfiguration, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelInit, RtcDataChannelState,
    RtcDataChannelType, RtcIceCandidateInit, RtcIceServer, RtcPeerConnection, RtcPeerConnectionIceEvent,
    RtcPeerConnectionState, RtcSdpType, RtcSessionDescriptionInit,
};

use super::rtc::{CHANNEL_LABEL, RtcConfig, Signal};

/// Channel messages waiting for the session at most.
const INBOX: usize = 4096;
/// The largest data channel message passed up (the session's datagrams are
/// at most 1500 bytes).
const MAX_MESSAGE: usize = 1500;
/// Bytes queued on a slow channel before more sends count as lost.
const MAX_BUFFERED: u32 = 1024 * 1024;

/// Callbacks a connection keeps alive while the browser may call them.
type Callbacks = Vec<Closure<dyn FnMut(JsValue)>>;

/// One connection and the callbacks that keep its state up to date.
struct Conn {
    pc: RtcPeerConnection,
    dc: Option<RtcDataChannel>,
    open: bool,
    closed: bool,
    /// The remote description is set: candidates can be added.
    remote_set: bool,
    /// Candidates that came before the remote description.
    early: Vec<String>,
    _pc_callbacks: Callbacks,
    _dc_callbacks: Callbacks,
}

#[derive(Default)]
struct State {
    servers: JsValue,
    conns: BTreeMap<u64, Conn>,
    signals: VecDeque<(u64, Signal)>,
    inbox: VecDeque<(u64, Vec<u8>)>,
}

thread_local! {
    static AGENTS: RefCell<BTreeMap<u32, State>> = const { RefCell::new(BTreeMap::new()) };
    static NEXT: Cell<u32> = const { Cell::new(1) };
}

fn with<R>(agent: u32, f: impl FnOnce(&mut State) -> R) -> Option<R> {
    AGENTS.with(|a| a.borrow_mut().get_mut(&agent).map(f))
}

fn with_conn(agent: u32, conn: u64, f: impl FnOnce(&mut Conn)) {
    with(agent, |s| {
        if let Some(c) = s.conns.get_mut(&conn) {
            f(c);
        }
    });
}

fn push_signal(agent: u32, conn: u64, signal: Signal) {
    with(agent, |s| {
        if s.conns.contains_key(&conn) {
            s.signals.push_back((conn, signal));
        }
    });
}

fn fail(agent: u32, conn: u64) {
    with_conn(agent, conn, |c| c.closed = true);
}

/// Every WebRTC connection of one endpoint, in this page.
pub(in crate::net) struct Agent {
    id: u32,
}

impl Agent {
    pub(in crate::net) fn new(config: &RtcConfig) -> io::Result<Self> {
        let servers = js_sys::Array::new();
        for (url, username, credential) in config.servers() {
            let server = RtcIceServer::new();
            server.set_urls_str(url);
            if let Some(username) = username {
                server.set_username(username);
            }
            if let Some(credential) = credential {
                server.set_credential(credential);
            }
            servers.push(&server);
        }
        let id = NEXT.with(|n| {
            let id = n.get();
            n.set(id.wrapping_add(1).max(1));
            id
        });
        AGENTS.with(|a| {
            a.borrow_mut().insert(
                id,
                State {
                    servers: servers.into(),
                    ..State::default()
                },
            )
        });
        Ok(Self { id })
    }

    /// A peer connection with its candidate and state callbacks.
    fn peer_connection(&self, conn: u64) -> Option<(RtcPeerConnection, Callbacks)> {
        let config = RtcConfiguration::new();
        with(self.id, |s| config.set_ice_servers(&s.servers))?;
        let pc = RtcPeerConnection::new_with_configuration(&config).ok()?;
        let agent = self.id;
        let on_candidate = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
            let Ok(event) = event.dyn_into::<RtcPeerConnectionIceEvent>() else {
                return;
            };
            // `None` (or an empty line) marks the end of gathering.
            if let Some(candidate) = event.candidate().map(|c| c.candidate()).filter(|c| !c.is_empty()) {
                push_signal(agent, conn, Signal::Candidate { candidate });
            }
        });
        pc.set_onicecandidate(Some(on_candidate.as_ref().unchecked_ref()));
        let watched = pc.clone();
        let on_state = Closure::<dyn FnMut(JsValue)>::new(move |_| {
            if matches!(
                watched.connection_state(),
                RtcPeerConnectionState::Failed | RtcPeerConnectionState::Closed
            ) {
                fail(agent, conn);
            }
        });
        pc.set_onconnectionstatechange(Some(on_state.as_ref().unchecked_ref()));
        Some((pc, vec![on_candidate, on_state]))
    }

    /// Starts connection `conn` from this side: the data channel, then the
    /// offer, which goes out as a signal once the browser made it.
    pub(in crate::net) fn connect(&mut self, conn: u64) {
        let Some((pc, callbacks)) = self.peer_connection(conn) else {
            self.insert_failed(conn);
            return;
        };
        let init = RtcDataChannelInit::new();
        init.set_ordered(false);
        init.set_max_retransmits(0);
        let dc = pc.create_data_channel_with_data_channel_dict(CHANNEL_LABEL, &init);
        let dc_callbacks = channel_callbacks(self.id, conn, &dc);
        self.insert(conn, pc.clone(), Some(dc), callbacks, dc_callbacks);
        let agent = self.id;
        wasm_bindgen_futures::spawn_local(async move {
            let made = async {
                let offer = JsFuture::from(pc.create_offer()).await?;
                let sdp = js_sys::Reflect::get(&offer, &"sdp".into())?
                    .as_string()
                    .unwrap_or_default();
                let local = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
                local.set_sdp(&sdp);
                JsFuture::from(pc.set_local_description(&local)).await?;
                Ok::<_, JsValue>(sdp)
            };
            match made.await {
                Ok(sdp) => push_signal(agent, conn, Signal::Offer { sdp }),
                Err(_) => fail(agent, conn),
            }
        });
    }

    fn insert(
        &mut self,
        conn: u64,
        pc: RtcPeerConnection,
        dc: Option<RtcDataChannel>,
        pc_callbacks: Callbacks,
        dc_callbacks: Callbacks,
    ) {
        with(self.id, |s| {
            s.conns.insert(
                conn,
                Conn {
                    pc,
                    dc,
                    open: false,
                    closed: false,
                    remote_set: false,
                    early: Vec::new(),
                    _pc_callbacks: pc_callbacks,
                    _dc_callbacks: dc_callbacks,
                },
            )
        });
    }

    fn insert_failed(&mut self, conn: u64) {
        if let Ok(pc) = RtcPeerConnection::new() {
            self.insert(conn, pc, None, Vec::new(), Vec::new());
        }
        fail(self.id, conn);
    }

    /// A signal from the other end of `conn`; an offer starts it.
    pub(in crate::net) fn signal(&mut self, conn: u64, signal: Signal) {
        let agent = self.id;
        match signal {
            Signal::Offer { sdp } => {
                let Some((pc, mut callbacks)) = self.peer_connection(conn) else {
                    self.insert_failed(conn);
                    return;
                };
                // The joining side made the channel; it arrives here.
                let on_channel = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
                    let Ok(event) = event.dyn_into::<RtcDataChannelEvent>() else {
                        return;
                    };
                    let dc = event.channel();
                    let dc_callbacks = channel_callbacks(agent, conn, &dc);
                    with_conn(agent, conn, |c| {
                        c.dc = Some(dc);
                        c._dc_callbacks = dc_callbacks;
                    });
                });
                pc.set_ondatachannel(Some(on_channel.as_ref().unchecked_ref()));
                callbacks.push(on_channel);
                self.insert(conn, pc.clone(), None, callbacks, Vec::new());
                wasm_bindgen_futures::spawn_local(async move {
                    let made = async {
                        let remote = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
                        remote.set_sdp(&sdp);
                        JsFuture::from(pc.set_remote_description(&remote)).await?;
                        add_early_candidates(agent, conn, &pc);
                        let answer = JsFuture::from(pc.create_answer()).await?;
                        let sdp = js_sys::Reflect::get(&answer, &"sdp".into())?
                            .as_string()
                            .unwrap_or_default();
                        let local = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
                        local.set_sdp(&sdp);
                        JsFuture::from(pc.set_local_description(&local)).await?;
                        Ok::<_, JsValue>(sdp)
                    };
                    match made.await {
                        Ok(sdp) => push_signal(agent, conn, Signal::Answer { sdp }),
                        Err(_) => fail(agent, conn),
                    }
                });
            }
            Signal::Answer { sdp } => {
                let Some(pc) = with(agent, |s| s.conns.get(&conn).map(|c| c.pc.clone())).flatten() else {
                    return;
                };
                wasm_bindgen_futures::spawn_local(async move {
                    let remote = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
                    remote.set_sdp(&sdp);
                    match JsFuture::from(pc.set_remote_description(&remote)).await {
                        Ok(_) => add_early_candidates(agent, conn, &pc),
                        Err(_) => fail(agent, conn),
                    }
                });
            }
            Signal::Candidate { candidate } => {
                let candidate = candidate.strip_prefix("a=").unwrap_or(&candidate).to_string();
                let mut ready = None;
                with_conn(agent, conn, |c| {
                    if c.remote_set {
                        ready = Some(c.pc.clone());
                    } else {
                        c.early.push(candidate.clone());
                    }
                });
                if let Some(pc) = ready {
                    add_candidate(&pc, &candidate);
                }
            }
            Signal::Bye => {}
        }
    }

    /// The browser keeps its own time: nothing to run here.
    pub(in crate::net) fn advance(&mut self, _now: Duration) {}

    pub(in crate::net) fn send(&mut self, conn: u64, bytes: &[u8]) -> bool {
        with(self.id, |s| {
            let Some(dc) = s
                .conns
                .get(&conn)
                .filter(|c| c.open && !c.closed)
                .and_then(|c| c.dc.as_ref())
            else {
                return false;
            };
            dc.ready_state() == RtcDataChannelState::Open
                && dc.buffered_amount() <= MAX_BUFFERED
                && dc.send_with_u8_array(bytes).is_ok()
        })
        .unwrap_or(false)
    }

    pub(in crate::net) fn recv(&mut self, buf: &mut [u8]) -> Option<(u64, usize)> {
        with(self.id, |s| {
            while let Some((conn, bytes)) = s.inbox.pop_front() {
                if bytes.len() <= buf.len() {
                    buf[..bytes.len()].copy_from_slice(&bytes);
                    return Some((conn, bytes.len()));
                }
            }
            None
        })
        .flatten()
    }

    /// A failure of the agent's own (a desktop TURN allocation); a
    /// browser reports none.
    #[allow(clippy::unused_self)]
    pub(in crate::net) fn take_error(&mut self) -> Option<String> {
        None
    }

    pub(in crate::net) fn take_signal(&mut self) -> Option<(u64, Signal)> {
        with(self.id, |s| s.signals.pop_front()).flatten()
    }

    pub(in crate::net) fn is_open(&self, conn: u64) -> bool {
        with(self.id, |s| s.conns.get(&conn).is_some_and(|c| c.open && !c.closed)).unwrap_or(false)
    }

    pub(in crate::net) fn is_closed(&self, conn: u64) -> bool {
        with(self.id, |s| s.conns.get(&conn).is_none_or(|c| c.closed)).unwrap_or(true)
    }

    pub(in crate::net) fn close(&mut self, conn: u64) {
        let removed = with(self.id, |s| {
            s.inbox.retain(|(c, _)| *c != conn);
            s.signals.retain(|(c, _)| *c != conn);
            s.conns.remove(&conn)
        })
        .flatten();
        if let Some(c) = removed {
            shut(&c);
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        if let Some(state) = AGENTS.with(|a| a.borrow_mut().remove(&self.id)) {
            for c in state.conns.values() {
                shut(c);
            }
        }
    }
}

/// Detaches a connection's callbacks and closes it.
fn shut(c: &Conn) {
    if let Some(dc) = &c.dc {
        dc.set_onopen(None);
        dc.set_onclose(None);
        dc.set_onmessage(None);
        dc.close();
    }
    c.pc.set_onicecandidate(None);
    c.pc.set_onconnectionstatechange(None);
    c.pc.set_ondatachannel(None);
    c.pc.close();
}

fn add_candidate(pc: &RtcPeerConnection, candidate: &str) {
    let init = RtcIceCandidateInit::new(candidate);
    // One media line: the data channel's.
    init.set_sdp_m_line_index(Some(0));
    let promise = pc.add_ice_candidate_with_opt_rtc_ice_candidate_init(Some(&init));
    // A candidate the browser can't use is skipped, as on the desktop.
    wasm_bindgen_futures::spawn_local(async move {
        let _ = JsFuture::from(promise).await;
    });
}

fn add_early_candidates(agent: u32, conn: u64, pc: &RtcPeerConnection) {
    let mut early = Vec::new();
    with_conn(agent, conn, |c| {
        c.remote_set = true;
        early = std::mem::take(&mut c.early);
    });
    for candidate in early {
        add_candidate(pc, &candidate);
    }
}

/// Binary messages into the agent's inbox; open and close into the state.
fn channel_callbacks(agent: u32, conn: u64, dc: &RtcDataChannel) -> Callbacks {
    dc.set_binary_type(RtcDataChannelType::Arraybuffer);
    let on_open = Closure::<dyn FnMut(JsValue)>::new(move |_| with_conn(agent, conn, |c| c.open = true));
    let on_close = Closure::<dyn FnMut(JsValue)>::new(move |_| fail(agent, conn));
    let on_message = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
        let Ok(event) = event.dyn_into::<MessageEvent>() else {
            return;
        };
        let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() else {
            return; // text: not ours
        };
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
        if bytes.len() > MAX_MESSAGE {
            return;
        }
        with(agent, |s| {
            if s.inbox.len() < INBOX {
                s.inbox.push_back((conn, bytes));
            }
        });
    });
    dc.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    dc.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    dc.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    // A channel that opened before these callbacks were set (the host's,
    // handed over already open) is open now.
    if dc.ready_state() == RtcDataChannelState::Open {
        with_conn(agent, conn, |c| c.open = true);
    }
    vec![on_open, on_close, on_message]
}
