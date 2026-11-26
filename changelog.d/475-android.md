### Added
- Android: the engine builds for `aarch64-linux-android` (the other Android
  ABIs package too, untested) and runs full screen as a native activity, with Vulkan or,
  where wgpu picks it or `WGPU_BACKEND=gl` asks for it, OpenGL ES.
  `ponko package --target android` makes an `.apk` (Android 11, API 30, or
  newer) with the game's library, its asset pack, notices and launcher icons,
  aligned for 16 KB pages and signed with the Android debug key, or left
  unsigned for the `--sign` hook. The `orientation` key sets the
  orientations. The SDK, build tools and NDK are found through
  `ANDROID_HOME`. See the guide's new Android chapter for the toolchain, the
  release-signing steps and what has been checked where.
- On Android: suspend and resume release and recreate the GPU surface;
  `safe_insets` and `keyboard_inset` come from the system bars, cutouts and
  the on-screen keyboard; the game takes audio focus (none for `Ambient`),
  gives it back while away, and holds its sound during a call; storage,
  the clipboard and crash reports work; debuggable builds read `PONKO_*`,
  `WGPU_*` and `RUST_*` intent extras as environment variables.
- `WindowEvent::BackRequested`, `WindowService::back_requested` and
  `WindowService::cancel_back`: the system Back button or gesture as an
  event. By default the app leaves after the frame; a game that cancels it
  handles Back itself.
- C and C++: `PONKO_WINDOW_EVENT_BACK_REQUESTED`,
  `ponko_ctx_window_back_requested` and `ponko_ctx_window_cancel_back`, with
  their C++ methods.
- `scripts/android-emulator.sh`, which runs the engine's unit tests, the
  phone check and a reference game on a headless emulator, and lints the
  Android target with `--clippy`; an `android` job in `scripts/checks.yml`.

### Changed
- On Android, storage keeps a game's files in the app's files folder and
  replaces a file's backup by copying (the system denies hard links there).
- Crash reports list the loaded modules on Android, as on Linux.

### Fixed
- A window surface without sRGB view formats (OpenGL ES on some Android
  devices and emulators) no longer panics; it draws without the sRGB view
  and warns that colors come out darker.
- The C API tests build where `c_char` is unsigned.
