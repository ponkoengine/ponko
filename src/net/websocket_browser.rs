//! The browser side of the WebSocket transport (`websocket.rs`): the
//! page's own `WebSocket`, with the same calls as the desktop client.
//!
//! The browser hands packets over in callbacks on the page's event loop;
//! they wait in an inbox until the game polls. The socket and its callbacks
//! live in a thread-local table (a web build has one thread), so the client
//! itself is a plain id and the sessions stay `Send`.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::time::Duration;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{BinaryType, CloseEvent, MessageEvent, WebSocket};

use super::websocket::{MAX_BUFFERED, MAX_PACKET, parse_url};
use crate::perf::Instant;

/// Packets waiting for the game at most; a page that stops polling drops the rest.
const INBOX: usize = 4096;
/// A connection that closed before it ever opened (the host isn't up yet)
/// is tried again this often, as a UDP client resends its hello.
const RETRY: Duration = Duration::from_millis(500);

struct Socket {
    ws: WebSocket,
    inbox: VecDeque<Vec<u8>>,
    opened: bool,
    /// Set by the close callback: why it closed.
    closed: Option<String>,
    since: Instant,
    _on_open: Closure<dyn FnMut()>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_close: Closure<dyn FnMut(CloseEvent)>,
}

thread_local! {
    static SOCKETS: RefCell<BTreeMap<u32, Socket>> = const { RefCell::new(BTreeMap::new()) };
    static NEXT: std::cell::Cell<u32> = const { std::cell::Cell::new(1) };
}

/// A browser client's connection to a host's WebSocket listener.
pub(in crate::net) struct WebClient {
    id: u32,
    url: String,
    error: Option<String>,
    /// Closed for good: an error was reported, nothing more is tried.
    done: bool,
}

impl WebClient {
    /// Opens a WebSocket to `url` (`ws://` or `wss://`). Fails at once only
    /// on a URL it can't use.
    pub(in crate::net) fn connect(url: &str) -> io::Result<Self> {
        parse_url(url, true)?;
        let id = NEXT.with(|n| {
            let id = n.get();
            n.set(id + 1);
            id
        });
        open(id, url)?;
        Ok(Self {
            id,
            url: url.to_string(),
            error: None,
            done: false,
        })
    }

    pub(in crate::net) fn send(&mut self, bytes: &[u8]) -> bool {
        self.update();
        SOCKETS.with(|s| {
            let sockets = s.borrow();
            let Some(socket) = sockets.get(&self.id) else {
                return false;
            };
            if socket.ws.ready_state() != WebSocket::OPEN || socket.ws.buffered_amount() as usize > MAX_BUFFERED {
                return false;
            }
            socket.ws.send_with_u8_array(bytes).is_ok()
        })
    }

    pub(in crate::net) fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        self.update();
        SOCKETS.with(|s| {
            let mut sockets = s.borrow_mut();
            let socket = sockets.get_mut(&self.id)?;
            while let Some(packet) = socket.inbox.pop_front() {
                if packet.len() <= buf.len() {
                    buf[..packet.len()].copy_from_slice(&packet);
                    return Some(packet.len());
                }
            }
            None
        })
    }

    pub(in crate::net) fn take_error(&mut self) -> Option<String> {
        self.update();
        self.error.take()
    }

    /// Reports a close after the connection opened; retries one that never did.
    fn update(&mut self) {
        if self.done {
            return;
        }
        let (closed, opened, since) = SOCKETS.with(|s| {
            let sockets = s.borrow();
            sockets
                .get(&self.id)
                .map_or((Some(String::new()), false, Instant::now()), |socket| {
                    (socket.closed.clone(), socket.opened, socket.since)
                })
        });
        let Some(why) = closed else { return };
        if opened {
            self.done = true;
            self.error = Some(why);
        } else if since.elapsed() >= RETRY && open(self.id, &self.url).is_err() {
            self.done = true;
            self.error = Some(format!("could not open a WebSocket to {}", self.url));
        }
    }
}

impl Drop for WebClient {
    fn drop(&mut self) {
        if let Some(socket) = SOCKETS.with(|s| s.borrow_mut().remove(&self.id)) {
            socket.ws.set_onopen(None);
            socket.ws.set_onmessage(None);
            socket.ws.set_onclose(None);
            let _ = socket.ws.close();
        }
    }
}

/// Opens (or reopens) socket `id`, replacing what was there.
fn open(id: u32, url: &str) -> io::Result<()> {
    let ws = WebSocket::new(url).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("{url}: {e:?}")))?;
    ws.set_binary_type(BinaryType::Arraybuffer);
    let on_open = Closure::<dyn FnMut()>::new(move || {
        SOCKETS.with(|s| {
            if let Some(socket) = s.borrow_mut().get_mut(&id) {
                socket.opened = true;
            }
        });
    });
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() else {
            return; // text: not ours
        };
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
        if bytes.len() > MAX_PACKET {
            return;
        }
        SOCKETS.with(|s| {
            if let Some(socket) = s.borrow_mut().get_mut(&id)
                && socket.inbox.len() < INBOX
            {
                socket.inbox.push_back(bytes);
            }
        });
    });
    let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |event: CloseEvent| {
        SOCKETS.with(|s| {
            if let Some(socket) = s.borrow_mut().get_mut(&id) {
                socket.closed = Some(if socket.opened {
                    format!("the host closed the connection (code {})", event.code())
                } else {
                    "could not connect".into()
                });
            }
        });
    });
    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    let socket = Socket {
        ws,
        inbox: VecDeque::new(),
        opened: false,
        closed: None,
        since: Instant::now(),
        _on_open: on_open,
        _on_message: on_message,
        _on_close: on_close,
    };
    if let Some(old) = SOCKETS.with(|s| s.borrow_mut().insert(id, socket)) {
        old.ws.set_onopen(None);
        old.ws.set_onmessage(None);
        old.ws.set_onclose(None);
    }
    Ok(())
}
