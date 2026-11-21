### Added
- Video in the browser: in the web build `ponko::video::Video` plays the
  same WebM files through the same calls, its pictures decoded by the
  browser's own AV1 decoder (WebCodecs) and its sound through the mixer.
  Frames show once the browser has decoded them, usually by the next
  update. A headless Chromium runner (`web/smoke/video.mjs`, fixture
  `examples/web_video.rs`) checks every frame shown against its time and
  two seeks while paused.
- `ponko_bench`: `video/convert`, converting a decoded frame to RGBA on
  the CPU (what `Video::frame_rgba` does when asked).

### Changed
- Faster video on the desktop: rav1d's assembly is on for arm64 (built by
  the system's C compiler; x86-64 can turn it on with nasm, see the guide),
  and frames are converted to RGBA on the GPU instead of the CPU. A 1080p
  frame took 3.5 to 4.5 ms of the game's thread instead of 18 to 22 ms
  (median, on a busy machine: the guide's "What it costs"). The decoder is
  rav1d as Rerun publishes it (`re_rav1d` 0.1.3), whose package builds the
  arm64 assembly.
- `Video::update` keeps the picture with the sound: more than 0.1 s apart,
  the one behind catches up (after frames stopped while the sound played
  on, or a sound that started late). Before, a frame longer than 0.25 s
  left the picture behind until the game sought.
- `Video::frame_rgba` converts the frame on the CPU when first asked for
  after a new frame, instead of every frame.

### Fixed
- A video's sound, and every sound streamed a packet at a time (on the
  web), now lands where a seek or a loop asks: it landed up to 33 ms off,
  and up to 180 ms off in places, because the stream's timestamps were
  read as samples (a WebM file's are milliseconds) and a reset Vorbis
  decoder gives nothing for its first packet.
