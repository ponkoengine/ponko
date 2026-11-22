# Security: what reads untrusted bytes

A game reads bytes it didn't write: packets from other players and from
strangers, files a player, a mod or a download brings, and requests from
tools. This chapter lists every place in the engine that does, who can send
it bytes, what someone sending bad bytes would want, and what stops them.
`SECURITY.md` at the root of the repository says what is in scope and how
to report a problem.

What every entry point promises, whatever the bytes:

- **No crash and no hang.** A malformed packet or file is an error (a
  `Result`, a refused request, a dropped packet), never a panic, and no
  input makes a loader or a server loop for long.
- **No unbounded memory.** Sizes and counts written in a header are
  checked against limits *before* anything is allocated for them, and
  queues of anything a stranger can send are capped.
- **No reading or writing outside the game's folders.** Only the control
  endpoint writes files for others, and only below the game's folder.
- **No acting as another player.** Where the engine knows who a peer is
  (the session's link), a packet can only speak for its own link.

The fuzz targets in `fuzz/` (its README says how to run them) feed each
entry point random and mutated bytes; a finding is fixed in the loader and
kept fixed by a regression test next to it.

## Network entry points

| Entry point | Who can send it bytes | What an attacker wants | What stops it | Checked by |
|---|---|---|---|---|
| Lobby and relay server (`ponko_lobby`) | Anyone who can reach its port | Crash it, exhaust its memory or sockets, flood rooms, use the relay to flood players, speak for another member | Limits per address on requests, relay bytes and connections; caps on request size, relay payload, rooms, room size and text; idle connections and rooms closed; silent links dropped; relay frames carry the sender's id as the server knows it, never one the client wrote ([Lobbies](lobby.md#limits)) | `tests/limits.rs`, `tests/hostile.rs`, fuzz target `lobby` |
| STUN responder (`ponko_lobby`) | Anyone, with any source address (UDP) | Use the server to flood a third party (reflection) | Binding requests only, answered at most 20 a second per IP; the address table is capped | `tests/stun.rs` |
| Message endpoint (`NetMessages`, UDP) | Anyone who can reach the port, with forged source addresses | Crash the game, hold every client slot, make it buffer without bound, inject messages into another player's link | Datagrams of 1500 bytes at most; packets only count for a link whose address and both nonces match; messages over 1 MiB close the link; per-channel send and receive memory capped (4 MiB); undrained events capped (4096, about 5 MiB); a link silent for 10 s is dropped; at most `max_clients` (≤ 1024) links | `net/message_conformance.rs`, fuzz target `messages` |
| WebSocket transport (browsers, `accept_web`) | Any web page, any TCP client | Hold sockets with slow handshakes or silent connections, send huge frames | 64 handshakes in flight, each dropped after 5 s; 1100 connections at most; a connection that sends nothing for 30 s is dropped; frames over 64 KiB refused; 1 MiB queued for a slow reader, then packets are dropped | `net/websocket.rs` tests |
| WebRTC endpoints (feature `webrtc`) | Whoever the signaling channel lets through; then the peer | Crash the game through malformed signaling or data | Signaling text is parsed by the WebRTC stack (str0m, or the browser's); data channel messages go through the same message protocol and its limits | `net/rtc_tests.rs` |
| Lockstep (`Net`) | The session's peers; strangers' datagrams are ignored | Desync or stall the game, crash it with malformed inputs | Datagrams from the session's peers only (others are ignored), parsed with bounds checks; state checksums catch desyncs | `net/lockstep_conformance.rs`, fuzz target `lockstep` |
| Replication (`Replication`) | The server, for a client; any connected client, for the server | Crash or desync peers, make the server apply another player's input, grow memory with events | Snapshots read with a bounds-checked bit reader; a client's input only drives the entities it owns; events capped per tick (`OUTBOX_LIMIT`) and raw events in size; a client with other declarations is refused | `net/replication_conformance.rs`, fuzz target `replication` |
| Steam transport (feature `steam`) | Steam friends and lobby members | As for the message endpoint | The same message protocol over Steam's relay | `net/steam_messages.rs` |

## Files

Every loader below gets its bytes from an asset source: `assets/`, a
pack, a download, a mod. A file can lie in its header about anything.

| Loader | What an attacker wants | What stops it | Checked by |
|---|---|---|---|
| Images (PNG, JPEG and the `image-*` formats) | Make the game allocate gigabytes from a small file, crash the decoder | The size in the header is checked before decoding: 16 384 pixels a side and 512 MiB of RGBA8 at most by default, and a game can change both (`ErrorKind::Limit`, see "Image size limits" below) | `assets.rs` tests, fuzz target `cooked_texture` |
| Cooked and compressed textures (KTX2, DDS) | The same; a supercompressed level that unpacks to terabytes | The same size limits; every level must unpack to exactly the bytes its size needs, and unpacking stops as soon as it passes them | `compressed.rs` tests, fuzz target `cooked_texture` |
| Video (WebM with AV1) | A picture size that allocates gigabytes before the first frame | The container's picture size is checked against the image limits; the AV1 decoder refuses larger frames itself | `video.rs` tests |
| glTF models (`.glb`, `.gltf`) | Crash the loader, allocate without bound from accessor counts | Accessors and buffers are bounds-checked against the file; images go through the image limits | fuzz target `gltf` |
| Aseprite files and cooked Aseprite sheets | Huge canvases, frame and tile counts | Sheets are limited in size and in the bytes they decode to; cooked sheets are a JSON file and an image (image limits) | `ponko_aseprite` tests, fuzz target `aseprite` |
| Levels (LDtk `.ldtk`, Tiled `.tmx` / `.tmj`) | Layers or tilesets of billions of cells from a few bytes of text; compressed layer data that unpacks to gigabytes | Tile layers of 16 777 216 cells at most, tilesets of 1 048 576 tiles, tiles and sheets within the image limits (`ErrorKind::Limit`); compressed layer data stops unpacking at the cell limit | `tests/tilemaps.rs`, `tilemap/tiled.rs` tests, fuzz target `levels` |
| Asset packs (`ponko pack`) | Overflowing sizes in the header, an index that claims more entries than it holds | Header sizes checked, entries limited by the index's length, unpacked sizes limited by the deflate ratio | fuzz target `asset_pack` |
| Scenes, world saves and prefabs (JSON) | Crash the reader, a prefab that includes itself forever | Errors for malformed JSON and unknown types; prefab nesting depth is limited | fuzz targets `scene`, `prefab` |
| Action maps (players' key bindings) | Crash the input system | Malformed bindings are an error; nothing else is read | fuzz target `action_map` |
| Unity imports (`ponko_unity`) | Crash the importer with a malformed project | Every read is bounds-checked | fuzz targets `unity_yaml`, `unity_binary` |
| Sounds (WAV, OGG, FLAC, MP3) | Crash the decoder | Decoded by symphonia, which is fuzzed upstream | — |

### Image size limits

Every image decoder reads one setting, `ImageLimits`: the longest side in
pixels and the most bytes the decoded pixels may take (RGBA8, four bytes a
pixel). The defaults are 16 384 pixels, what nearly every desktop GPU takes as
a texture, and 512 MiB, past any game texture but small enough that a hostile
file can't exhaust a normal machine. A game that loads pictures from players or
the network (avatars, mods, user levels) should lower them for everything it
loads, and decode each untrusted picture under tighter limits still, which
doesn't change the game's. A tool that reads large source art (a height map, a
panorama to slice) raises them. Set them once at startup with
`Game::image_limits` (C: `ponko_game_image_limits`, C++: `game::image_limits`)
or later with `Assets::set_image_limits` (`ponko_ctx_set_image_limits`,
`ctx::set_image_limits`); they are for the whole process, not one game, and
`Graphics::try_decode_texture` (`ponko_ctx_try_decode_texture`,
`ctx::try_decode_texture`) takes limits of its own for one picture. A picture
past them fails with `ErrorKind::Limit` (`PONKO_ERROR_LIMIT`) before its
pixels are allocated, and the message names the limits in force. Raising a
limit doesn't promise a texture: a picture wider than the device's own
texture limit still fails at upload, with the usual error.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/image_limits.rs:image_limits}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:image_limits}}
```

</div>
</div>

## The control endpoint

The control endpoint (`PONKO_CONTROL=1`, [the control endpoint](control.md))
lets tools drive a running game, so it is the one entry point that can
change the game and write files.

- **Who can send it bytes:** anything on the same machine that can open a
  TCP connection to `127.0.0.1`, which includes web pages in a browser.
- **What an attacker wants:** run requests without the token, guess the
  token, hold its connection slots, write or read files outside the game's
  folder.
- **What stops it:**
  - It is **off by default in every build.** `PONKO_CONTROL=1` opens it
    only in debug builds and in release builds with the `control-env`
    feature; a shipped release build without that feature opens it only if
    the game calls `Game::control_endpoint(true)`.
  - Every connection must send the instance's random 128-bit token (in the
    registry file only its user can read) with its first request, within
    10 s; the token is compared in time that doesn't depend on where it
    differs. Lines are 4 MiB at most, read before the token is checked;
    32 connections at most.
  - File paths in requests must be relative, made of plain names (no
    `..`, no hidden or device names) with the expected extension, and are
    resolved below the game's working folder (or `assets/`) with links
    followed: a link inside the folder can't lead out of it. New files are
    created without following links; overwriting refuses links, hard
    links and special files.
- **Checked by:** `tests/control.rs` and the `control.rs` tests
  (`the_endpoint_is_off_by_default`, `tokens_are_compared_whole`,
  `file_methods_refuse_paths_outside_the_game`), fuzz target `control`.

## What the engine leaves to the game

- **Encryption and authentication.** The network transports carry plain,
  unauthenticated traffic, like most game protocols: run them over a
  trusted network or a secure tunnel, and serve browsers through a TLS
  proxy. Who a player *is* (accounts, bans) is the game's to decide.
- **Game rules.** The engine checks that a packet is well formed and
  speaks only for its own link; whether the move it describes is allowed
  is up to the game (the host's or server's simulation decides).
- **Spoofed UDP floods.** A stranger who forges source addresses can make
  a message endpoint open links that time out after 10 s; behind a lobby
  each costs one of the endpoint's slots for that long. Put a public
  server behind a firewall or provider that filters spoofed traffic.
