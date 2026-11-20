### Added
- Video playback (feature `video`, off by default): `ponko::video::Video`
  plays WebM files with AV1 pictures into a texture, to draw as an intro
  or cutscene or to put on a screen in the world, with play, pause, seek,
  looping and an end. The sound (Vorbis) plays through the mixer on a
  chosen bus with its own volume, starts and stops with the picture and
  seeks with it, decoded a packet at a time as it plays. Pictures are
  decoded on the CPU by rav1d (the Rust port of dav1d, without its
  assembly: pure Rust, nothing to install). Other codecs (VP9, H.264,
  Opus sound, 10-bit pictures) fail to load with an `Unsupported` error
  naming the ffmpeg option to use instead. Desktop only: the web build
  compiles the API and loading reports `Unsupported`; the guide's new
  "Video" chapter compares the decoders considered (licenses, patents,
  size, web) and gives the web recipe. Not bound for C and C++ yet.
- `ponko_bench`: `video/frame`, the time to decode and convert one frame
  of the clip named by `PONKO_BENCH_VIDEO` (feature `video`).
