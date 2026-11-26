### Added
- `ponko package --target android --format aab` makes an Android App
  Bundle, what Google Play takes, with Google's `bundletool` (`BUNDLETOOL`
  names its jar, else `bundletool` on the `PATH`): the library and asset
  pack stored and the library on 16 KiB pages in the apks made from it,
  signed with the debug key or by the `--sign` program (`archive <path>`,
  as for the apk). The Android guide has the release-signing and Google
  Play upload steps; the emulator check installs the phone check from its
  bundle.

### Fixed
- A window with no sRGB format and no sRGB view (OpenGL ES whose EGL has
  no sRGB color space, such as Android's emulator with `WGPU_BACKEND=gl`)
  showed colors too dark: the frame is now drawn into an sRGB image and
  encoded into the window by a last pass, so it matches Vulkan. Windows
  that offer sRGB, desktop OpenGL with it and WebGL2 draw as before.
