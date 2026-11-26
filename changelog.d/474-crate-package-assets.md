<!-- area: Development and releases -->
### Changed
- The published `ponko` crate no longer carries the three reference games'
  art, sound and music (`assets/coin_dash`, `assets/ember_hop`,
  `assets/shard_grove`): nothing builds from them. It packs 800 files, 4.3 MiB
  compressed, instead of 850 files, 7.5 MiB, well under crates.io's 10 MB
  limit. The games' sources stay in the package so it compiles, but running
  one of them needs the repository's `assets/`.
