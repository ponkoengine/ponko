### Added
- Fuzz target `sound` for every sound format the engine reads (WAV, IMA and
  Microsoft ADPCM, AIFF, Ogg Vorbis, FLAC, MP3): a whole decode, the
  slice-by-slice decode asset groups use, a stream opened from memory and a
  stream played a little, from a small seed corpus (`fuzz/seeds/sound/`) and
  the example sounds.
- `fuzz/targets.txt` is the written fuzz list: every target with the seconds
  `fuzz/run-all.sh` runs it (`fuzz/run-all.sh` uses them). `python3
  scripts/check-fuzz-list.py` fails when a target has a file, a `[[bin]]` or
  a README row that the list doesn't name, or the reverse.

### Changed
- Sound files are checked before they are decoded, in every loader (whole
  sounds, asset groups, streams, a video's soundtrack): a sample rate over
  768 kHz, zero or more than two channels, or, for a sound decoded whole, a
  length over 64 Mi frames (512 MiB of samples) is an error that names the
  limit, instead of an allocation of that size or an error after decoding
  all of it. A sound decoded whole also stops at the length limit however
  short its header said it was (a few kilobytes of compressed silence can
  unpack to gigabytes); a longer sound is streamed. Files with more than two
  channels were already unplayable; they now fail earlier.

### Fixed
- A WAV file whose channel count makes the format reader's block size
  overflow (60 000 channels of 16 bits) panicked it in builds with overflow
  checks (debug builds, tests, the fuzzer). A panic inside any sound decoder
  is now an error naming the file as damaged, at every place the engine
  decodes sound.
