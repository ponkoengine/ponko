# Security policy

## Supported versions

Security fixes go to the latest 0.x release (as a patch release) and to
`main`. Older releases don't get fixes: upgrade to the latest one.

## Reporting a vulnerability

Please **don't** open a public issue. Report it privately through
[a confidential issue](https://gitlab.com/ponkoengine/ponko/-/issues/new?issue%5Bconfidential%5D=true)
instead. Include what's affected, how to reproduce it, and what an attacker
could do with it.

We aim to reply within a week. Once a fix is ready, it's released, and
then the advisory is published, crediting you unless you'd rather not be
named.

## What is in scope

Anything in this repository that reads bytes the game didn't write, when
those bytes can make it crash, hang, use memory without bound, read or
write files outside the game's folders, or act for another player:

- **The network:** the lobby and relay server (`ponko_lobby`, including
  its STUN responder), the message endpoint and its UDP, WebSocket,
  WebRTC and Steam transports, lockstep, replication.
- **Files a player, a mod or a download brings:** images and textures
  (KTX2, DDS), video, glTF models, Aseprite files and sheets, LDtk and
  Tiled levels, asset packs, scenes, saves, prefabs, action maps, the
  Unity importer (`ponko_unity`).
- **The control endpoint** (below) and the command-line tools' handling
  of the projects and assets they are given.

The guide's chapter "Security: what reads untrusted bytes"
(`book/src/security.md`) lists each of these with what it promises and
what stops a bad actor. Out of scope: the examples and test programs,
denial of service by sheer volume against a server that has no network
filtering in front of it, and the absence of encryption and accounts in
the network transports (documented as the game's to add).

## Areas worth knowing about

- **The control endpoint** (`PONKO_CONTROL=1` in debug builds) lets tools
  and AI agents read and change a running game. It's off by default; a
  release build opens it only when the game asks for it in code or is
  built with the `control-env` feature. It listens only on localhost, needs
  a random token written to `~/.ponko/instances`, and reads and writes
  files only below the game's folder. Never expose it to a network.
- **The C API** (`ponko_ffi`) trusts its caller like any C library:
  passing invalid pointers is undefined behavior, not a vulnerability.
- **Save files and data files** are parsed as JSON. Crashes or hangs on
  malformed input are bugs worth reporting.
