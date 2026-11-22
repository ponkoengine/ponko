### Added
- Asset packs have absolute caps on what they may unpack to, next to the deflate
  ratio check: `PackLimits` sets the size of one file (default 1 GiB, enforced
  while inflating, so a file that grows past it fails without holding more than
  the cap), the sum of the sizes a pack's index declares (16 GiB) and its number
  of files (1,000,000), the last two checked when the pack is opened. Errors
  name the file, the cap and how to raise it. Set them for the game with
  `Game::pack_limits` (covers `assets.pack`, mod packs and `PackSource::open`
  and `from_bytes`), or for one pack with `PackSource::open_with_limits` and
  `from_bytes_with_limits`.
- `ponko pack`, `ponko cook --pack` and `ponko package` refuse to write a pack
  the engine's default caps would refuse to read, unless told higher caps with
  `--max-entry-size`, `--max-pack-size` and `--max-entries`.

### Changed
- A pack whose index declares more than 16 GiB, more than a million files or a
  file over 1 GiB is now refused (before, only the 1032:1 deflate ratio bounded
  it). Games shipping a pack that large raise the caps with `Game::pack_limits`.

### Fixed
- `ponko pack --list` no longer inflates a file past the size its index declares.
