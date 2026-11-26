# Android: games on phones and tablets

> **Maturity: [experimental](maturity.md).** Game: none (the phone check,
> `examples/phone.rs`, is a test app; Coin Dash and the project template are
> packaged and started by the check). Tests: the android job in
> `scripts/checks.yml` (clippy for `aarch64-linux-android`, and
> `scripts/android-emulator.sh`: the engine's unit tests and the phone check
> on the Android emulator); the package and its manifest in
> `crates/ponko_cli` (`cargo test -p ponko_cli package`); Back from Rust in
> `src/window.rs`, from C in `crates/ponko_ffi/tests/lifecycle.rs`.
> Platforms: the Android 16 (API 36) emulator, arm64, on an Apple silicon
> Mac. Graphics: Vulkan (the emulator's SwiftShader), and OpenGL ES through
> ANGLE when asked for. Missing: a run on a phone (touch with fingers,
> multi-touch, calls, memory pressure, performance, heat and power),
> release signing, Google Play (an app bundle).

The same game runs on Android phones and tablets. `ponko package --target
android` builds it as an apk; the window is the screen, the renderer draws
with Vulkan (or OpenGL ES), fingers are [touches](touch.md), and the engine
tells the game when the system sends it away, when the keyboard covers part
of the screen and when the player presses Back. This chapter is what is
Android-specific, and what has and hasn't been checked; the [packaging
chapter](packaging.md) covers the command itself, and the [iOS
chapter](ios.md) the events and calls the two share.

## Setting up the tools

Once, on the machine that builds (macOS, Linux or Windows; the commands
below were run on a Mac):

1. A JDK, 17 or later (Android's tools run on Java): `brew install
   openjdk@21`, a distribution's package, or Android Studio's own.
2. The Android SDK command-line tools, from Android Studio or
   [developer.android.com/studio](https://developer.android.com/studio#command-line-tools-only)
   (unpack them as `<sdk>/cmdline-tools/latest`), and `ANDROID_HOME` set to
   the SDK folder (`~/Library/Android/sdk` is Android Studio's on a Mac,
   `~/Android/Sdk` on Linux; `ponko package` looks there too).
3. The packages, with the SDK's `sdkmanager`:

   ```sh
   sdkmanager "platform-tools" "platforms;android-36" "build-tools;36.1.0" "ndk;29.0.14206865"
   # only for the emulator:
   sdkmanager "emulator" "system-images;android-36;google_apis;arm64-v8a"
   ```

4. The Rust target: `rustup target add aarch64-linux-android`.

What this chapter was checked with: Temurin JDK 21.0.13, command-line tools
13114758, platform-tools 37.0.3, platform android-36, build-tools 36.1.0,
NDK r29 (29.0.14206865), emulator 37.3.10 and the android-36 Google APIs
arm64 image. Newer ones should do; `ponko package` takes the newest build
tools, platform and NDK it finds (`ANDROID_NDK_HOME` names another NDK).
No Gradle, no Android Studio project and no Java code are involved.

## Building and packaging

In the game's folder:

```sh
ponko package --target android              # an apk, signed with the debug key
ponko package --target android --format folder   # only the files it is made of
```

`android` stands for `aarch64-linux-android`, the 64-bit ARM every phone
of the last years has. (`x86_64-linux-android` and the 32-bit triples
package too, untested.) The command builds the game's program with the
NDK's compilers, linked as the shared library Android's `NativeActivity`
loads (`lib/arm64-v8a/libmain.so`, its symbols stripped in the package;
the build's copy keeps them to read crash reports with), and makes the apk
of:

| In the apk | from | |
|---|---|---|
| `AndroidManifest.xml` | `[package.metadata.ponko]` and the engine | below |
| `lib/arm64-v8a/libmain.so` | the game's program | stored on 16 KiB pages, loaded from the apk as it is |
| `assets/assets.pack` | the files under `assets/`, [cooked](packaging.md#cooking-assets-ponko-cook) for `mobile` | stored, read from the apk as it is |
| `assets/THIRD-PARTY-NOTICES.txt`, the licenses | the crate | |
| `res/mipmap-*/ic_launcher.png` | `icon` | 48, 72, 96, 144 and 192 pixels |

An Android game reads its assets only from the pack: `--no-pack`,
`pack = false` and asset folders outside `assets/` are refused for Android.
The phone check's apk is 20 MB, nearly all of it the engine's code.

The manifest:

| It says | from | why |
|---|---|---|
| `package`, `android:label` | `id` (`-` becomes `_`), `name` | the app's identity |
| `versionCode` | the crate's version: `major * 1000000 + minor * 1000 + patch` | a later version must have a larger code |
| `minSdkVersion` 30 | the engine | Android 11: the first with the window-inset calls the safe area and the keyboard come from |
| `targetSdkVersion` 36 | the engine | Android 16, what it was checked on |
| `hasCode="false"`, `NativeActivity`, `android.app.lib_name` `main` | the engine | the game is the library; no Java |
| `debuggable` | signed with the debug key? | debug builds only; it lets the emulator check pass variables (below) |
| `appCategory="game"`, `isGame` | the engine | the system's game settings, and Android 16 keeps a game's orientation on large screens |
| `enableOnBackInvokedCallback="false"` | the engine | Back reaches the game as an event (Android 16's predictive Back would leave first) |
| `configChanges` (rotation, size, keyboard, density, ...), `launchMode` `singleTask` | the engine | the game follows a rotation or a resize itself instead of being restarted |
| `screenOrientation` | `orientation` | `any` is `fullUser`, `landscape` `userLandscape`, `portrait` `userPortrait`: the player's rotation lock is kept |
| `windowSoftInputMode="adjustNothing"`, a full-screen theme | the engine | the keyboard covers the game, which keeps its text above it ([below](#the-keyboard-the-safe-area-and-turning-the-phone)) |
| `glEsVersion` 3.0 required | the engine | the least the renderer draws with |

The smoke run is skipped (it would need a phone); the command prints how
to run one on the emulator instead.

## How the game starts and ends

Android starts the game's library in a process of its own. The engine's
`android_main` (android-activity's entry point, through winit) runs the
program's `main`, so the game's code is the same as on a desktop:
`Game::new(...).run(...)`. What the game prints goes to the log, tag
`ponko` (`adb logcat -s ponko`). The process's home folder is the app's
files folder and its temporary folder the app's cache.

When the game quits (`ctx.quit()`, or a smoke run ends), the engine
finishes the activity and ends the process; starting the app again starts
it fresh. An apk signed with the debug key (debuggable) takes variables as
the activity's extras, as a test passes them:

```sh
adb install -r target/package/<name>-<version>-android-aarch64.apk
adb shell am start -n <package>/android.app.NativeActivity -e PONKO_SMOKE 120
adb logcat -s ponko
```

Extras whose names start with `PONKO_`, `WGPU_` or `RUST_` become
environment variables; a release-signed apk ignores them.

## Lifecycle: sent away and brought back

The events are the [iOS chapter's](ios.md#lifecycle-sent-away-and-brought-back):
`WindowEvent::Suspended` when the game loses the screen (home, the app
switcher, a call, locking the phone), `WindowEvent::Resumed` when it is in
front again, `WindowEvent::LowMemory`, and `ctx.window.suspended()`. On
Android the system also takes the window's drawing surface away while the
game is suspended: the renderer lets it go then and makes a new one on
resume, with everything the game loaded kept. No frames run in between.

Android sends `LowMemory` only through the old `onLowMemory` call, which
recent versions rarely make; a game should not count on it.

## Back

Android's back button or back gesture is `WindowEvent::BackRequested`.
Unless a system calls `ctx.window.cancel_back()` in the frame that sees it,
the app leaves to the background after that frame, as players expect (the
game keeps running, suspended, and comes back where it was). A game that
uses Back itself, to close a menu or pause, cancels it then:

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
# use ponko::window::WindowEvent;
fn handle_back(ctx: &mut Ctx, menu_open: &mut bool) {
    for event in ctx.events.read::<WindowEvent>() {
        if matches!(event, WindowEvent::BackRequested) && *menu_open {
            *menu_open = false;
            ctx.window.cancel_back();
        }
    }
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:phone_back}}
```

</div>
</div>

`ctx.window.back_requested()` says whether a Back is still pending this
frame. Other systems never send the event. Tests feed it with
`ctx.window.feed_event(&WindowEvent::BackRequested)` (C:
`ponko_ctx_window_feed_lifecycle` with `PONKO_WINDOW_EVENT_BACK_REQUESTED`).

## The keyboard, the safe area and turning the phone

The on-screen keyboard comes up with `ctx.input.set_ime_allowed(true)`;
what is typed arrives as on a desktop, and `ctx.window.keyboard_inset()` is
how far up from the bottom edge it covers the window, in logical pixels,
while it is open (the engine follows it every frame then, as it slides).
On the emulated Pixel 7 it covers 336 of the 914 logical pixels.

The [safe area](touch.md#notches-home-bars-and-rounded-corners) is what
the system bars and the camera cutout leave: read from the system when the
window is made, on each resize, twice a second, and every frame while the
keyboard may be open. The game draws under the bars (Android 15 and later
draw every app edge to edge); upright on the emulated Pixel 7 that leaves
52 logical pixels at the top (the cutout) and 24 at the bottom (the
gesture bar). Turned, the cutout's inset moves to the side it is on.

Turning the phone resizes the window, with no restart: the game sees a new
`ctx.window_size()` and safe area. `orientation` in the metadata limits
which ways it turns. Tablets and foldables give the game any size,
including split screen; Android 16 honors a game's orientation there.

## Sound: audio focus and calls

Android asks apps to take the audio focus before they play, and other apps
(the player's music) pause or duck for the one that holds it. The engine
follows the [audio session category](ios.md#sound-the-audio-session):

| `AudioSessionCategory` | Audio focus | Other apps' audio |
|---|---|---|
| `SoloAmbient` (the default) | taken while the game is in front, as a game's sound | pauses |
| `Playback` | the same | pauses |
| `Ambient` | not taken | plays on, mixed with the game |

The focus is let go when the game is suspended and taken again on resume.
While the game is suspended, or while a call has the phone's audio, the
engine closes the audio output (the game's voices go on silently, keeping
their places) and opens it again after. Losing the focus to another app
while in front is not reported: the engine has no Java code to hear of it.

## Files and the clipboard

[Storage](storage.md) keeps a game's files in `files/<app id>` in the app's
own data folder, private to it and backed up with the device; they go with
the app. The clipboard (`ctx.window.clipboard_read` and `clipboard_write`)
is the system's; Android lets only the app in front with the input focus
read it, so a read in the first frames, before the window has the focus,
is `None`. Android also shows the player a notice when an app reads it.

## The GPU

wgpu draws with **Vulkan** where the phone has it (every 64-bit phone that
shipped with Android 10 or later must), and with **OpenGL ES 3** where it
doesn't. On the emulator Vulkan is SwiftShader, a CPU renderer: the
phone check draws 30 to 50 frames a second there, the
[tier](3d.md#gpu-tiers) is `desktop`. With `WGPU_BACKEND=gl` it draws
through OpenGL ES (ANGLE over the same SwiftShader), tier `baseline`.
That OpenGL ES window has no sRGB format, so the colors are written without
sRGB encoding there and look darker (the engine says so in the log); a
driver with EGL's sRGB color space doesn't have that problem. Which tier
real phones reach, and how fast, needs phones.

Debug builds on the emulator crash in its Vulkan driver, which fails on
the names wgpu gives its objects in debug builds; packages are release
builds, which run.

**A phone's settings.** There is no separate mobile preset: a 3D game on a
phone starts at `GraphicsQuality::Low` (two-thirds render scale with
sharpening, two shadow cascades, no ambient occlusion, reflections or
global illumination; [Graphics quality](3d.md#graphics-quality)) and lets
the [quality autotuner](3d.md#optional-scene-quality-autotuning) move the
render scale and shadow resolution to hold its frame rate. 2D games need
nothing. Phones keep a frame rate only as long as they stay cool, so check
on a phone over minutes, not seconds.

## Checking on the emulator

`scripts/android-emulator.sh` runs the whole check; the emulator runs
without a window:

```sh
scripts/android-emulator.sh            # results and screenshots in target/android-emulator
scripts/android-emulator.sh --clippy   # only clippy for Android
```

It packages the phone check, boots a fresh emulated Pixel 7 (Android 16,
arm64; `PONKO_ANDROID_AVD` boots one of yours instead), and:

0. builds the engine's library tests for Android and runs them on the
   emulator (`adb shell`), all but those that read this checkout's files
   by their path on the build machine;
1. starts the app headless for 120 ticks and requires the smoke run to
   pass once, with no restart after the process ends;
2. starts it with its window and requires steady frames on the GPU for 10
   seconds, the safe area, the clipboard round trip, a file in the storage
   folder, the audio focus held, let go on the home screen and taken again
   on resume (one suspend and one resume), a turned window with the cutout
   on its side, and leaving on Back;
3. starts it with the keyboard open and requires its inset, typed text,
   a tap closing it, and a Back the game cancels keeping it in front;
4. packages Coin Dash and runs it headless from its apk, its assets read
   from the pack inside it.

The emulator is stopped at the end, also when a step fails. By hand (an
emulated phone with a window: leave out `-no-window`):

```sh
emulator -avd <avd> -no-window -no-audio -gpu swiftshader_indirect &
adb install -r target/package/<name>-<version>-android-aarch64.apk
adb shell am start -n <package>/android.app.NativeActivity
adb exec-out screencap -p > game.png
adb shell input tap 540 1200          # a touch
adb emu kill
```

## On a phone, and signing for players

A phone with developer options and USB debugging on installs the debug
apk the same way (`adb install -r`). The debug key is
`~/.android/debug.keystore`, the one Android Studio uses (the command makes
it when it is missing); an apk signed with it is for testing only.

A release is signed with the developer's own key, which only they hold:

1. Once, make the key (and keep it safe: an app's updates must be signed
   with the same key):

   ```sh
   keytool -genkeypair -keystore release.jks -alias game -keyalg RSA -keysize 4096 -validity 10000
   ```

2. A signing program, which `ponko package` calls with the aligned,
   unsigned apk (`archive <path>`); with `--sign` the apk is not debuggable:

   ```sh
   #!/bin/sh
   # sign-android.sh <stage> <path>; KEYSTORE and KEY_ALIAS are yours, the
   # passwords come from the environment, never the command line.
   set -e
   [ "$1" = archive ] || exit 0
   "$ANDROID_HOME/build-tools/36.1.0/apksigner" sign --ks "$KEYSTORE" --ks-key-alias "$KEY_ALIAS" \
       --ks-pass env:KEYSTORE_PASS --key-pass env:KEY_PASS "$2"
   "$ANDROID_HOME/build-tools/36.1.0/apksigner" verify "$2"
   ```

3. `ponko package --target android --sign ./sign-android.sh`

Google Play takes Android App Bundles (`.aab`), not apks, for new apps,
and re-signs them with a key it keeps (Play App Signing, with the key
above as the upload key). `ponko package` makes apks only: an app bundle
needs Google's `bundletool`, which the SDK doesn't include. Other stores
and direct downloads take the signed apk.

## C and C++

The calls of this chapter are in the C API and the C++ wrapper:
`PONKO_WINDOW_EVENT_BACK_REQUESTED`, `ponko_ctx_window_back_requested` and
`ponko_ctx_window_cancel_back` (C++: `window_event_kind::back_requested`,
`ctx.window_back_requested()`, `ctx.window_cancel_back()`), with the
lifecycle, keyboard, safe area and audio session calls of the
[iOS chapter](ios.md#c-and-c). The C library builds for
`aarch64-linux-android` (clippy checks it), but a C or C++ game for Android
needs its own build that makes a `NativeActivity` library linking it:
`ponko package` packages Cargo targets only, and that path has not been
tried.

## What has been checked

| What | Checked | How |
|---|---|---|
| Builds for Android | yes | clippy for `aarch64-linux-android` (engine and C API) |
| The package, its manifest and alignment | yes | unit tests (`cargo test -p ponko_cli package`); `zipalign -c -P 16 4` and `apksigner verify` pass; Android installs it |
| The project template and a reference game (Coin Dash) | emulator | packaged, installed and started; Coin Dash draws its coins from the apk's pack, and a swipe on its on-screen stick moves the player |
| The engine's unit tests | emulator | 1156 pass on Android (18 skipped: they read the checkout's files) |
| Vulkan rendering, full screen | emulator | the phone check draws its frame around the safe area; screenshots |
| OpenGL ES | emulator | with `WGPU_BACKEND=gl`: draws, without sRGB encoding |
| Steady frames | emulator | 30 to 50 frames a second on the CPU renderer |
| Safe area, rotation | emulator | 52 / 0 / 24 / 0 upright on a Pixel 7; turned, 52 on the left |
| Suspend and resume, the surface made again | emulator | home, then the game again: one event each, frames go on |
| Back: leaving, and kept by the game | emulator | the back key and the back gesture |
| On-screen keyboard, its inset, typing | emulator | 336 logical pixels; `adb shell input text` arrives as typed text |
| Touch (one finger) | emulator | `adb shell input` taps and swipes, as touches |
| Audio focus | emulator | taken, let go when suspended, taken again (`dumpsys audio`); `Ambient` takes none |
| Clipboard, storage folder | emulator | round trips in the phone check |
| Smoke runs end once | emulator | the activity is finished; Android does not start it again |
| Multi-touch, gestures with fingers | **needs a phone and a person** | the shared touch code is tested on the desktop and the web |
| A call taking the audio, other apps taking the focus | **needs a phone** | the call check reads the system's audio mode once a second |
| Memory warnings | **needs a phone** | recent Android rarely sends them |
| Performance, power, heat, the GPU tier of real GPUs | **needs phones** | the emulator draws on the CPU |
| Release signing, Google Play | **needs the developer's key and account** | the steps above; no app bundle yet |

## Notes for other platforms

`WindowEvent::BackRequested`, `cancel_back` and `back_requested` are
platform-neutral, like the other lifecycle calls: a game handles Back the
same way whatever it runs on, and only Android sends it.
