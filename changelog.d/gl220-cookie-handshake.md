### Security
- A UDP message host (`NetMessages::host`) checks that a joining client
  can receive at its address before it gives the client anything: a hello
  without a valid cookie gets one challenge (41 bytes, smaller than the
  64-byte padded hello, at most 256 a second) and the host keeps no slot
  and no memory for it. The cookie is SipHash-2-4-128 over the client's
  address, port, protocol version, nonce and key period, with keys made at
  start and every 5 s (a cookie is good for 5 to 10 s), compared in
  constant time. Forged source addresses no longer fill slots or use the
  host as a reflector. Always on; C and C++ need no new call. WebSocket,
  WebRTC and Steam relay clients are unchanged (book/src/security.md,
  "Forged hellos").

### Changed
- **Breaking:** the message protocol is version 2 (packets start with
  `CRM2`; the Steam lobby's `ponko_protocol` is `ponko-messages-2`).
  Connecting takes one extra round trip on UDP. Clients and hosts from
  before this version can't talk to new ones: a version-1 client joining a
  new host, or a new client joining a version-1 host, times out ("peer
  timed out"), and Steam joins fail with "incompatible message protocol".
  Migration: ship the host and every client with the same engine version.
  From version 2 on, a host answers a hello of another version with a
  refusal the client reports as `Disconnected` with a reason starting
  "version mismatch".
