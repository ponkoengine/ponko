<!-- area: Web, mobile and platforms -->
### Added
- `PONKO_MUTE=1` makes the audio output play silence, for automated runs
  on a machine or phone someone is using; the stream, mixer and music
  clock run as usual. See the guide's Audio chapter.
- `PONKO_IOS_ORIENTATION=landscape` (or `portrait`) asks an iOS app's
  scene to turn as the game starts; the simulator check uses it to check
  the safe area turned.

### Fixed
- iOS: `ctx.window.safe_insets()` follows the phone as it turns (it kept
  its upright values, so the touch controls sat under the Dynamic Island
  and the rounded corners in landscape). (#641)
- iOS: the audio output closes while the app is suspended and opens again
  on resume, as on Android; the game's sound no longer plays over the
  home screen or another app. (#642)
