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
