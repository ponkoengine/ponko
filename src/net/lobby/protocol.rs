//! The lobby's wire protocol: what [`LobbyClient`](super::LobbyClient) and a
//! lobby server (the `ponko_lobby` crate) say to each other, for a server or
//! a client written in another language.
//!
//! Every message travels over a [message endpoint](crate::net::NetMessages)
//! (UDP, or WebSocket for browsers) as one byte saying what follows:
//!
//! | First byte | Then | Lane |
//! |---|---|---|
//! | [`CONTROL`] (1) | A JSON object: a [`Request`] from a client, a [`Reply`] from the server | Reliable |
//! | [`RELAY`] (2) | A member id, 4 bytes little-endian, then the payload | The sender's choice; the server forwards on the same lane |
//!
//! A client's relay frame names the member it is for ([`EVERYONE`], 0, for
//! every other member of its room); the server's names the member it came
//! from. The payload is the game's own bytes: the server never reads them.
//!
//! The protocol has a [`VERSION`], sent in the server's
//! [`Reply::Welcome`]. Fields and variants may be added within a version;
//! readers ignore fields they don't know, and unknown errors and reasons read
//! as `Other`.
//!
//! ```
//! use ponko::net::lobby::protocol::{self, Frame, Request};
//!
//! let bytes = protocol::encode_control(&Request::Join { code: "KQ7X2M".into() });
//! assert_eq!(bytes[0], protocol::CONTROL);
//! assert_eq!(
//!     protocol::decode::<Request>(&bytes),
//!     Some(Frame::Control(Request::Join { code: "KQ7X2M".into() }))
//! );
//!
//! let mut relay = Vec::new();
//! protocol::encode_relay(protocol::EVERYONE, b"opaque", &mut relay);
//! assert_eq!(
//!     protocol::decode::<Request>(&relay),
//!     Some(Frame::Relay { member: 0, payload: &b"opaque"[..] })
//! );
//! ```

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::{LeaveReason, LobbyError, MemberId, RoomInfo};

/// The protocol version this engine speaks.
/// ```
/// assert_eq!(ponko::net::lobby::protocol::VERSION, 1);
/// ```
pub const VERSION: u32 = 1;

/// First byte of a control message (a JSON [`Request`] or [`Reply`]).
/// ```
/// assert_eq!(ponko::net::lobby::protocol::CONTROL, 1);
/// ```
pub const CONTROL: u8 = 1;

/// First byte of a relay frame (member id, then the opaque payload).
/// ```
/// assert_eq!(ponko::net::lobby::protocol::RELAY, 2);
/// ```
pub const RELAY: u8 = 2;

/// The member id a client's relay frame uses for "every other member of my
/// room".
/// ```
/// assert_eq!(ponko::net::lobby::protocol::EVERYONE, 0);
/// ```
pub const EVERYONE: u32 = 0;

/// Bytes in front of a relay frame's payload: the kind and the member id.
/// ```
/// assert_eq!(ponko::net::lobby::protocol::RELAY_HEADER, 5);
/// ```
pub const RELAY_HEADER: usize = 5;

/// What a client asks of the server, as JSON after [`CONTROL`]. The server
/// answers a request it can't do with [`Reply::Refused`].
/// ```
/// use ponko::net::lobby::protocol::Request;
/// let json = serde_json::to_string(&Request::Create { size: 4, meta: String::new() }).unwrap();
/// assert_eq!(json, r#"{"op":"create","size":4,"meta":""}"#);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Request {
    /// Make a room for up to `size` members (the creator included) and join
    /// it as its host. `meta` is the room's text for its members (a game
    /// mode, the host's own address for a direct connection, ...).
    /// ```
    /// let r = ponko::net::lobby::protocol::Request::Create { size: 2, meta: "duel".into() };
    /// ```
    Create {
        /// Most members, 2 up to the server's room size cap.
        /// ```
        /// let size: u16 = 4;
        /// ```
        size: u16,
        /// The room's text, up to the server's meta cap.
        /// ```
        /// let meta = String::from("mode=duel");
        /// ```
        #[serde(default)]
        meta: String,
    },
    /// Join the room with this code (letters in any case).
    /// ```
    /// let r = ponko::net::lobby::protocol::Request::Join { code: "kq7x2m".into() };
    /// ```
    Join {
        /// The room's code.
        /// ```
        /// let code = String::from("KQ7X2M");
        /// ```
        code: String,
    },
    /// Leave the current room.
    /// ```
    /// let r = ponko::net::lobby::protocol::Request::Leave;
    /// ```
    Leave,
    /// Wait in the matchmaking queue `mode` for a room of `size`.
    /// ```
    /// let r = ponko::net::lobby::protocol::Request::Queue { mode: "duel".into(), size: 2, rating: 1500 };
    /// ```
    Queue {
        /// The queue's name: players only meet others in the same one.
        /// ```
        /// let mode = String::from("duel");
        /// ```
        mode: String,
        /// The room size this queue fills.
        /// ```
        /// let size: u16 = 2;
        /// ```
        size: u16,
        /// A number for the server's matchmaking rule (a skill rating); the
        /// default rule ignores it.
        /// ```
        /// let rating: i32 = 1500;
        /// ```
        #[serde(default)]
        rating: i32,
    },
    /// Leave the matchmaking queue.
    /// ```
    /// let r = ponko::net::lobby::protocol::Request::CancelQueue;
    /// ```
    CancelQueue,
    /// Change the room's text (host only).
    /// ```
    /// let r = ponko::net::lobby::protocol::Request::SetMeta { meta: "started".into() };
    /// ```
    SetMeta {
        /// The new text.
        /// ```
        /// let meta = String::from("started");
        /// ```
        meta: String,
    },
}

/// The limits a server announces in [`Reply::Welcome`].
/// ```
/// let limits = ponko::net::lobby::protocol::ServerLimits::default();
/// assert_eq!(limits.max_relay_bytes, 0);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ServerLimits {
    /// The largest relay payload the server forwards.
    /// ```
    /// let n = ponko::net::lobby::protocol::ServerLimits::default().max_relay_bytes;
    /// ```
    #[serde(default)]
    pub max_relay_bytes: u32,
    /// The largest room.
    /// ```
    /// let n = ponko::net::lobby::protocol::ServerLimits::default().max_room_size;
    /// ```
    #[serde(default)]
    pub max_room_size: u16,
    /// The longest room text, in bytes.
    /// ```
    /// let n = ponko::net::lobby::protocol::ServerLimits::default().max_meta_bytes;
    /// ```
    #[serde(default)]
    pub max_meta_bytes: u32,
    /// How long a connection may stay outside any room and queue before the
    /// server closes it, in milliseconds.
    /// ```
    /// let n = ponko::net::lobby::protocol::ServerLimits::default().idle_timeout_ms;
    /// ```
    #[serde(default)]
    pub idle_timeout_ms: u64,
}

impl ServerLimits {
    /// Limits with every field set, for a server implementation.
    /// ```
    /// let l = ponko::net::lobby::protocol::ServerLimits::new(4096, 16, 256, 60_000);
    /// assert_eq!(l.max_room_size, 16);
    /// ```
    pub const fn new(max_relay_bytes: u32, max_room_size: u16, max_meta_bytes: u32, idle_timeout_ms: u64) -> Self {
        Self {
            max_relay_bytes,
            max_room_size,
            max_meta_bytes,
            idle_timeout_ms,
        }
    }
}

/// What the server tells a client, as JSON after [`CONTROL`].
/// ```
/// use ponko::net::lobby::protocol::Reply;
/// use ponko::net::lobby::MemberId;
/// let json = serde_json::to_string(&Reply::MemberLeft { member: MemberId::from_bits(3) }).unwrap();
/// assert_eq!(json, r#"{"ev":"member_left","member":3}"#);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Reply {
    /// The first message on a new connection: the protocol version, the
    /// client's own member id, and the server's limits.
    /// ```
    /// use ponko::net::lobby::{MemberId, protocol::{Reply, ServerLimits, VERSION}};
    /// let r = Reply::Welcome { version: VERSION, member: MemberId::from_bits(1), limits: ServerLimits::default() };
    /// ```
    Welcome {
        /// The server's protocol version.
        /// ```
        /// let v = ponko::net::lobby::protocol::VERSION;
        /// ```
        version: u32,
        /// The client's member id, for as long as it stays connected.
        /// ```
        /// let m = ponko::net::lobby::MemberId::from_bits(1);
        /// ```
        member: MemberId,
        /// What the server accepts.
        /// ```
        /// let l = ponko::net::lobby::protocol::ServerLimits::default();
        /// ```
        #[serde(default)]
        limits: ServerLimits,
    },
    /// The client is in a room now (it made it, joined it, or matchmaking
    /// put it there).
    /// ```
    /// # use ponko::net::lobby::{RoomInfo, protocol::Reply};
    /// # fn f(room: RoomInfo) -> Reply {
    /// Reply::Joined { room }
    /// # }
    /// ```
    Joined {
        /// The room as it is now.
        /// ```
        /// # fn f(room: ponko::net::lobby::RoomInfo) {}
        /// ```
        room: RoomInfo,
    },
    /// Someone joined the client's room.
    /// ```
    /// let r = ponko::net::lobby::protocol::Reply::MemberJoined { member: ponko::net::lobby::MemberId::from_bits(2) };
    /// ```
    MemberJoined {
        /// Who.
        /// ```
        /// let m = ponko::net::lobby::MemberId::from_bits(2);
        /// ```
        member: MemberId,
    },
    /// Someone left the client's room.
    /// ```
    /// let r = ponko::net::lobby::protocol::Reply::MemberLeft { member: ponko::net::lobby::MemberId::from_bits(2) };
    /// ```
    MemberLeft {
        /// Who.
        /// ```
        /// let m = ponko::net::lobby::MemberId::from_bits(2);
        /// ```
        member: MemberId,
    },
    /// The room has a new host (the old one left).
    /// ```
    /// let r = ponko::net::lobby::protocol::Reply::HostChanged { host: ponko::net::lobby::MemberId::from_bits(2) };
    /// ```
    HostChanged {
        /// The new host.
        /// ```
        /// let m = ponko::net::lobby::MemberId::from_bits(2);
        /// ```
        host: MemberId,
    },
    /// The host changed the room's text.
    /// ```
    /// let r = ponko::net::lobby::protocol::Reply::MetaChanged { meta: "started".into() };
    /// ```
    MetaChanged {
        /// The new text.
        /// ```
        /// let meta = String::from("started");
        /// ```
        meta: String,
    },
    /// The client is out of its room.
    /// ```
    /// let r = ponko::net::lobby::protocol::Reply::Left { reason: ponko::net::lobby::LeaveReason::Requested };
    /// ```
    Left {
        /// Why.
        /// ```
        /// let r = ponko::net::lobby::LeaveReason::Requested;
        /// ```
        reason: LeaveReason,
    },
    /// The client waits in a matchmaking queue.
    /// ```
    /// let r = ponko::net::lobby::protocol::Reply::Queued { mode: "duel".into(), size: 2 };
    /// ```
    Queued {
        /// The queue.
        /// ```
        /// let mode = String::from("duel");
        /// ```
        mode: String,
        /// Its room size.
        /// ```
        /// let size: u16 = 2;
        /// ```
        size: u16,
    },
    /// The client left the matchmaking queue (it asked to).
    /// ```
    /// let r = ponko::net::lobby::protocol::Reply::Unqueued;
    /// ```
    Unqueued,
    /// A request was not done.
    /// ```
    /// let r = ponko::net::lobby::protocol::Reply::Refused { error: ponko::net::lobby::LobbyError::RoomFull };
    /// ```
    Refused {
        /// Why.
        /// ```
        /// let e = ponko::net::lobby::LobbyError::RoomFull;
        /// ```
        error: LobbyError,
    },
}

/// One decoded message: a control message, or a relay frame borrowing the
/// payload from the received bytes.
/// ```
/// use ponko::net::lobby::protocol::{Frame, Request};
/// let f: Frame<'_, Request> = Frame::Relay { member: 2, payload: b"hi" };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame<'a, T> {
    /// A [`Request`] or [`Reply`].
    /// ```
    /// let f: ponko::net::lobby::protocol::Frame<'_, _> = ponko::net::lobby::protocol::Frame::Control(ponko::net::lobby::protocol::Request::Leave);
    /// ```
    Control(T),
    /// Opaque bytes for (from a client) or from (from the server) `member`.
    /// ```
    /// let f: ponko::net::lobby::protocol::Frame<'_, ()> = ponko::net::lobby::protocol::Frame::Relay { member: 0, payload: b"" };
    /// ```
    Relay {
        /// The member id: the target in a client's frame ([`EVERYONE`] for
        /// the whole room), the sender in the server's.
        /// ```
        /// let member: u32 = 0;
        /// ```
        member: u32,
        /// The game's bytes.
        /// ```
        /// let payload: &[u8] = b"";
        /// ```
        payload: &'a [u8],
    },
}

/// A [`Request`] or [`Reply`] as a control message: [`CONTROL`], then JSON.
/// ```
/// let bytes = ponko::net::lobby::protocol::encode_control(&ponko::net::lobby::protocol::Request::Leave);
/// assert_eq!(&bytes[1..], br#"{"op":"leave"}"#);
/// ```
pub fn encode_control<T: Serialize>(message: &T) -> Vec<u8> {
    let mut out = vec![CONTROL];
    // Writing these plain enums of strings and numbers to a Vec can't fail.
    if serde_json::to_writer(&mut out, message).is_err() {
        out.truncate(1);
    }
    out
}

/// Writes a relay frame for `member` into `out` (cleared first), so a
/// sender can reuse one buffer.
/// ```
/// let mut out = Vec::new();
/// ponko::net::lobby::protocol::encode_relay(7, b"xy", &mut out);
/// assert_eq!(out, [2, 7, 0, 0, 0, b'x', b'y']);
/// ```
pub fn encode_relay(member: u32, payload: &[u8], out: &mut Vec<u8>) {
    out.clear();
    out.reserve(RELAY_HEADER + payload.len());
    out.push(RELAY);
    out.extend_from_slice(&member.to_le_bytes());
    out.extend_from_slice(payload);
}

/// Reads one message: `None` for an empty, unknown or malformed one.
/// `T` is [`Request`] on the server and [`Reply`] on a client.
/// ```
/// use ponko::net::lobby::protocol::{decode, Reply};
/// assert_eq!(decode::<Reply>(&[9, 1, 2]), None);
/// assert_eq!(decode::<Reply>(&[1, b'{']), None);
/// ```
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Option<Frame<'_, T>> {
    match *bytes.first()? {
        CONTROL => serde_json::from_slice(&bytes[1..]).ok().map(Frame::Control),
        RELAY => {
            let member = u32::from_le_bytes(bytes.get(1..RELAY_HEADER)?.try_into().ok()?);
            Some(Frame::Relay {
                member,
                payload: &bytes[RELAY_HEADER..],
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reply_reads_back_as_written() {
        let room = RoomInfo {
            code: "ABCDEF".into(),
            host: MemberId::from_bits(1),
            members: vec![MemberId::from_bits(1), MemberId::from_bits(4)],
            size: 4,
            meta: "m".into(),
        };
        let replies = [
            Reply::Welcome {
                version: VERSION,
                member: MemberId::from_bits(9),
                limits: ServerLimits::new(1, 2, 3, 4),
            },
            Reply::Joined { room },
            Reply::MemberJoined {
                member: MemberId::from_bits(2),
            },
            Reply::MemberLeft {
                member: MemberId::from_bits(2),
            },
            Reply::HostChanged {
                host: MemberId::from_bits(2),
            },
            Reply::MetaChanged { meta: "x".into() },
            Reply::Left {
                reason: LeaveReason::RoomClosed,
            },
            Reply::Queued {
                mode: "duel".into(),
                size: 2,
            },
            Reply::Unqueued,
            Reply::Refused {
                error: LobbyError::RateLimited,
            },
        ];
        for reply in replies {
            let bytes = encode_control(&reply);
            assert_eq!(decode::<Reply>(&bytes), Some(Frame::Control(reply)));
        }
    }

    #[test]
    fn unknown_errors_and_reasons_from_a_newer_server_read_as_other() {
        let bytes = [&[CONTROL][..], br#"{"ev":"refused","error":"solar_flare"}"#].concat();
        assert_eq!(
            decode::<Reply>(&bytes),
            Some(Frame::Control(Reply::Refused {
                error: LobbyError::Other
            }))
        );
        let bytes = [&[CONTROL][..], br#"{"ev":"left","reason":"moon","extra":1}"#].concat();
        assert_eq!(
            decode::<Reply>(&bytes),
            Some(Frame::Control(Reply::Left {
                reason: LeaveReason::Other
            }))
        );
    }

    #[test]
    fn short_relay_frames_are_rejected() {
        assert_eq!(decode::<Request>(&[RELAY, 1, 0, 0]), None);
        assert_eq!(
            decode::<Request>(&[RELAY, 1, 0, 0, 0]),
            Some(Frame::Relay {
                member: 1,
                payload: &[]
            })
        );
        assert_eq!(decode::<Request>(&[]), None);
    }
}
