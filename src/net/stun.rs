//! STUN binding requests and responses (RFC 8489, the part NAT traversal
//! needs): a client asks a STUN server which public address its packets come
//! from, and the server says it.
//!
//! WebRTC peers (module `rtc`, feature `webrtc`) ask this before they
//! connect, so each can tell the other the address its NAT gives it. The
//! lobby server (`ponko-lobby`, feature `lobby`) can answer these requests
//! itself, so a game needs no STUN server of its own beyond the lobby.
//!
//! Only the binding method, without authentication: what public STUN servers
//! and browsers use for address discovery. No TURN.
//!
//! ```
//! use ponko::net::stun;
//! let request = stun::binding_request([7; 12]);
//! let from = "203.0.113.5:40000".parse().unwrap();
//! let mut response = Vec::new();
//! assert!(stun::binding_response(&request, from, &mut response));
//! assert_eq!(stun::mapped_address(&response, [7; 12]), Some(from));
//! ```

#![deny(missing_docs)]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// The fixed value in every STUN message's header (RFC 8489, section 5).
/// ```
/// assert_eq!(ponko::net::stun::MAGIC_COOKIE, 0x2112_A442);
/// ```
pub const MAGIC_COOKIE: u32 = 0x2112_A442;
/// The usual STUN port.
/// ```
/// assert_eq!(ponko::net::stun::DEFAULT_PORT, 3478);
/// ```
pub const DEFAULT_PORT: u16 = 3478;

const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const MAPPED_ADDRESS: u16 = 0x0001;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;
const HEADER: usize = 20;

/// A binding request with the given transaction id: 20 bytes to send to a
/// STUN server over UDP. Pick a fresh random id per request and keep it to
/// match the response with [`mapped_address`].
/// ```
/// let request = ponko::net::stun::binding_request([1; 12]);
/// assert!(ponko::net::stun::is_binding_request(&request));
/// ```
pub fn binding_request(transaction: [u8; 12]) -> [u8; HEADER] {
    let mut out = [0u8; HEADER];
    out[0..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    // Length 0: no attributes.
    out[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    out[8..20].copy_from_slice(&transaction);
    out
}

/// Whether `packet` is a STUN binding request (any attributes).
/// ```
/// assert!(!ponko::net::stun::is_binding_request(b"hello"));
/// ```
pub fn is_binding_request(packet: &[u8]) -> bool {
    header(packet).is_some_and(|(kind, _)| kind == BINDING_REQUEST)
}

/// Whether `packet` looks like any STUN message: the header's magic cookie,
/// the two zero top bits and a length that matches. Lets a socket that
/// carries other traffic too pick STUN out.
/// ```
/// let request = ponko::net::stun::binding_request([1; 12]);
/// assert!(ponko::net::stun::is_stun(&request));
/// assert!(!ponko::net::stun::is_stun(&[0u8; 20]));
/// ```
pub fn is_stun(packet: &[u8]) -> bool {
    header(packet).is_some()
}

/// The message type and the transaction id, for a well-formed STUN header.
fn header(packet: &[u8]) -> Option<(u16, [u8; 12])> {
    if packet.len() < HEADER || packet[0] & 0xC0 != 0 {
        return None;
    }
    let kind = u16::from_be_bytes([packet[0], packet[1]]);
    let length = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]) != MAGIC_COOKIE
        || length != packet.len() - HEADER
        || !length.is_multiple_of(4)
    {
        return None;
    }
    let mut transaction = [0u8; 12];
    transaction.copy_from_slice(&packet[8..20]);
    Some((kind, transaction))
}

/// The server side: if `request` is a binding request, writes the success
/// response that tells its sender it came from `from` into `out` (replacing
/// what was there) and returns `true`; anything else returns `false` and
/// leaves `out` empty.
/// ```
/// let mut out = Vec::new();
/// assert!(!ponko::net::stun::binding_response(b"not stun", "127.0.0.1:1".parse().unwrap(), &mut out));
/// assert!(out.is_empty());
/// ```
pub fn binding_response(request: &[u8], from: SocketAddr, out: &mut Vec<u8>) -> bool {
    out.clear();
    let Some((BINDING_REQUEST, transaction)) = header(request) else {
        return false;
    };
    let (family, address): (u8, Vec<u8>) = match xor_address(from, &transaction) {
        (IpAddr::V4(ip), _) => (1, ip.octets().to_vec()),
        (IpAddr::V6(ip), _) => (2, ip.octets().to_vec()),
    };
    let port = from.port() ^ (MAGIC_COOKIE >> 16) as u16;
    let value_len = 4 + address.len();
    out.extend_from_slice(&BINDING_SUCCESS.to_be_bytes());
    out.extend_from_slice(&((4 + value_len) as u16).to_be_bytes());
    out.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
    out.extend_from_slice(&transaction);
    out.extend_from_slice(&XOR_MAPPED_ADDRESS.to_be_bytes());
    out.extend_from_slice(&(value_len as u16).to_be_bytes());
    out.extend_from_slice(&[0, family]);
    out.extend_from_slice(&port.to_be_bytes());
    out.extend_from_slice(&address);
    true
}

/// The client side: the address a binding success `response` to the
/// request with `transaction` reports (its XOR-MAPPED-ADDRESS, or a plain
/// MAPPED-ADDRESS from an old server). `None` for anything else, including
/// a response to another request.
/// ```
/// assert_eq!(ponko::net::stun::mapped_address(&[0u8; 40], [0; 12]), None);
/// ```
pub fn mapped_address(response: &[u8], transaction: [u8; 12]) -> Option<SocketAddr> {
    let (kind, id) = header(response)?;
    if kind != BINDING_SUCCESS || id != transaction {
        return None;
    }
    let mut plain = None;
    let mut rest = &response[HEADER..];
    while rest.len() >= 4 {
        let kind = u16::from_be_bytes([rest[0], rest[1]]);
        let len = usize::from(u16::from_be_bytes([rest[2], rest[3]]));
        let value = rest.get(4..4 + len)?;
        match kind {
            XOR_MAPPED_ADDRESS => {
                let addr = read_address(value)?;
                let (ip, _) = xor_address(addr, &transaction);
                let port = addr.port() ^ (MAGIC_COOKIE >> 16) as u16;
                return Some(SocketAddr::new(ip, port));
            }
            MAPPED_ADDRESS => plain = read_address(value),
            _ => {}
        }
        // Attributes are padded to four bytes.
        rest = rest.get((4 + len).next_multiple_of(4)..).unwrap_or_default();
    }
    plain
}

fn read_address(value: &[u8]) -> Option<SocketAddr> {
    let port = u16::from_be_bytes([*value.get(2)?, *value.get(3)?]);
    let ip = match (value.get(1)?, value.get(4..)?) {
        (1, ip) if ip.len() == 4 => IpAddr::V4(Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3])),
        (2, ip) if ip.len() == 16 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(ip);
            IpAddr::V6(Ipv6Addr::from(octets))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

/// XORs an address's IP with the cookie (and, for IPv6, the transaction
/// id): the same operation encodes and decodes. The port is left alone.
fn xor_address(addr: SocketAddr, transaction: &[u8; 12]) -> (IpAddr, u16) {
    let cookie = MAGIC_COOKIE.to_be_bytes();
    let ip = match addr.ip() {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            IpAddr::V4(Ipv4Addr::new(
                o[0] ^ cookie[0],
                o[1] ^ cookie[1],
                o[2] ^ cookie[2],
                o[3] ^ cookie[3],
            ))
        }
        IpAddr::V6(ip) => {
            let mut key = [0u8; 16];
            key[..4].copy_from_slice(&cookie);
            key[4..].copy_from_slice(transaction);
            let mut o = ip.octets();
            for (b, k) in o.iter_mut().zip(key) {
                *b ^= k;
            }
            IpAddr::V6(Ipv6Addr::from(o))
        }
    };
    (ip, addr.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_response_round_trips_the_senders_address_for_both_families() {
        for from in ["198.51.100.7:51000", "[2001:db8::42]:3479"] {
            let from: SocketAddr = from.parse().unwrap();
            let request = binding_request([9; 12]);
            let mut response = Vec::new();
            assert!(binding_response(&request, from, &mut response));
            assert!(is_stun(&response));
            assert_eq!(mapped_address(&response, [9; 12]), Some(from));
            assert_eq!(mapped_address(&response, [8; 12]), None, "another request's response");
        }
    }

    #[test]
    fn the_response_matches_the_rfc_5769_ipv4_encoding() {
        // RFC 5769 section 2.2: 192.0.2.1:32853 encodes as port 0xA147 and
        // address 0xE112A643.
        let mut response = Vec::new();
        assert!(binding_response(
            &binding_request([0; 12]),
            "192.0.2.1:32853".parse().unwrap(),
            &mut response
        ));
        assert_eq!(&response[24..32], &[0x00, 0x01, 0xA1, 0x47, 0xE1, 0x12, 0xA6, 0x43]);
    }

    #[test]
    fn foreign_and_malformed_packets_are_not_stun() {
        let mut out = vec![1];
        assert!(!binding_response(&[0u8; 20], "127.0.0.1:1".parse().unwrap(), &mut out));
        assert!(out.is_empty());
        let mut bad_length = binding_request([1; 12]).to_vec();
        bad_length[3] = 4;
        assert!(!is_stun(&bad_length));
        // A response whose attribute claims more bytes than there are.
        let mut response = Vec::new();
        binding_response(&binding_request([2; 12]), "127.0.0.1:9".parse().unwrap(), &mut response);
        response[23] = 200;
        assert_eq!(mapped_address(&response, [2; 12]), None);
    }
}
