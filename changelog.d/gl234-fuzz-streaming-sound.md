### Added
- Fuzz target `webm_audio` for the sound of WebM videos (feature `video`):
  the tracks a video reads first, then its soundtrack's header and a little
  of it played with seeks and a loop, from seeds cut from the video test
  files (`fuzz/seeds/webm_audio/`) and the whole files. The `sound` target
  now also runs the decoder that desktop streams play through, decoding and
  seeking, with no audio device.

### Fixed
- A sound decoder that panicked part-way through a streamed sound on the
  desktop ended Kira's decoder thread and left the sound silent, still
  "playing", for good. Desktop streams (from a file or from memory) now
  play through the engine's own decoder, with the same readers, codecs and
  header limits, in which every call is guarded: a panic ends that sound
  with an error in the log (`ponko: a streamed sound stopped: ...`), and
  the other sounds and the game go on. Seeking, looping and stream lengths
  behave as before.
- A looping stream decoded a packet at a time (every stream on the web,
  a video's soundtrack everywhere) whose packets decoded to no frames, or
  whose loop start was past its end, went back to its start forever,
  rebuilding the decoder each time and freezing the thread that mixes:
  found by the new `webm_audio` fuzz target in its first run. It now ends
  like a sound that has nothing left to play.
- A WebM file with a block group whose size overflows the Matroska
  reader's arithmetic panicked it in builds with overflow checks, when a
  video loaded or played (`Video::from_bytes`, `advance`, `seek`): found by
  the new `webm_audio` fuzz target. A panic inside the reader is now a
  `Decode` error ("its data is damaged").
