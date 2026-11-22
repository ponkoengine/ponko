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
