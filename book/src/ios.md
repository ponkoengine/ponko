# iOS: games on iPhone and iPad

> **Maturity: [experimental](maturity.md).** Game: none (the phone check,
> `examples/phone.rs`, is a test app). Tests: the iOS job in
> `scripts/checks.yml` (clippy for both iOS targets, and
> `scripts/ios-simulator.sh`: the engine's iOS unit tests and the phone check
> on the simulator); the lifecycle, safe area and keyboard inset math run
> natively in `src/window.rs`, the C API in `crates/ponko_ffi/tests/lifecycle.rs`.
> Platforms: the iOS 27 simulator on an Apple silicon Mac. Graphics: Metal,
> on the simulator's GPU. Missing: a run on a device (touch, typing,
> interruptions, the silent switch, memory warnings, performance and power),
> signing, and the App Store.

The same game runs on an iPhone or iPad. `ponko package --target ios`
builds it as an app; the window is the screen, the renderer draws with
Metal, fingers are [touches](touch.md), and the engine tells the game when
the system sends it away and brings it back. This chapter is what is
iOS-specific, and what has and hasn't been checked; the [packaging
chapter](packaging.md) covers the command itself.

## Building and packaging

Once, on a Mac with Xcode:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
xcodebuild -downloadPlatform iOS     # the simulator, several GB; only to run on it
```

Then, in the game's folder:

```sh
ponko package --target ios-sim       # for the simulator: a .app, zipped
ponko package --target ios           # for a device: a .app in an .ipa
```

`ios` and `ios-sim` stand for `aarch64-apple-ios` and
`aarch64-apple-ios-sim`. The package is a flat iOS app bundle,
`<Name>.app`: the program, `assets.pack`, the icons, the licenses and
notices, and an `Info.plist` written from `[package.metadata.ponko]`:

| `Info.plist` says | from | why |
|---|---|---|
| `CFBundleIdentifier`, `CFBundleName`, `CFBundleDisplayName`, versions | `id`, `name`, the crate's version | the app's identity |
| `MinimumOSVersion` 16.0 | the engine | the build also targets it (`IPHONEOS_DEPLOYMENT_TARGET`, unless set) |
| `UIApplicationSceneManifest`, one scene | the engine | apps built with the iOS 27 SDK must use scenes, or iOS doesn't start them |
| `UIRequiredDeviceCapabilities` `arm64`, `metal` | the engine | the renderer needs Metal |
| `UIDeviceFamily` iPhone and iPad, `UIRequiresFullScreen`, `UIStatusBarHidden`, `UILaunchScreen` | the engine | a full-screen game without a status bar, with an empty launch screen |
| `UISupportedInterfaceOrientations` (and `~ipad`) | `orientation` | which ways the game turns |
| `CFBundleIcons` | `icon` | the icon at 120, 180 (iPhone), 152 and 167 (iPad) pixels, opaque, from the game's PNG |

```toml
[package.metadata.ponko]
name = "Coin Dash"
id = "com.example.coin-dash"
icon = "assets/icon.png"
orientation = "landscape"     # any (the default), landscape or portrait
```

Assets are [cooked](packaging.md#cooking-assets-ponko-cook) for the
`mobile` platform. The smoke run is skipped (an iOS program can't run on
the Mac); the simulator check below is how a package is run.

## Lifecycle: sent away and brought back

A phone takes the screen from a game whenever the player goes home, opens
the app switcher, takes a call or locks it. The engine sends a
`WindowEvent`, the same on iOS and [Android](android.md):

| Event | When | What a game does |
|---|---|---|
| `WindowEvent::Suspended` | the app lost the screen | save now: the system may end a suspended app without another word |
| `WindowEvent::Resumed` | it is in front again | carry on (a pause menu is a good idea) |
| `WindowEvent::LowMemory` | the system is short of memory | free what can be loaded again |

While suspended, no frames run and nothing is drawn; the suspend event is
read in one last frame, so a system that saves on it runs. The first frame
after a resume has the usual clamped time step (a quarter of a second), not
the time the app was away. `ctx.window.suspended()` says whether the app is
suspended now. A suspend and a resume each come once: the platform's
repeats are dropped. A desktop or a browser never sends them.

The on-screen keyboard comes up with `ctx.input.set_ime_allowed(true)`,
and its typed text arrives as on a desktop. `ctx.window.keyboard_inset()`
is how far up from the window's bottom edge it covers the window, in the
same logical pixels as `ctx.window_size()`: 0 while it is hidden, floating
(an iPad's), or on a desktop. A text field keeps above it, and above the
[safe area](touch.md#notches-home-bars-and-rounded-corners)'s bottom edge:

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
# use ponko::audio::AudioSessionCategory;
# use ponko::window::WindowEvent;
fn phone_setup(ctx: &mut Ctx) {
    // The player's own music plays on under the game.
    if let Err(e) = ctx.audio.set_session_category(AudioSessionCategory::Ambient) {
        eprintln!("audio session: {e}");
    }
}

fn chat_line_bottom(ctx: &mut Ctx, progress: &str) -> f32 {
    for event in ctx.events.read::<WindowEvent>() {
        if matches!(event, WindowEvent::Suspended) {
            // The system may end the app from here on.
            if let Err(e) = ctx.storage.write("progress.txt", progress.as_bytes()) {
                eprintln!("not saved: {e}");
            }
        }
    }
    ctx.window.safe_area().max.y.min(ctx.window_size().y - ctx.window.keyboard_inset())
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:phone_lifecycle}}
```

</div>
</div>

Tests feed what a phone would report: `ctx.window.feed_event(&WindowEvent::Suspended)`
(and send it with `ctx.events.send`), `ctx.window.feed_keyboard_inset(336.0)`
and `ctx.window.feed_safe_insets(...)`. From C and C++ the same are
`ponko_ctx_window_feed_lifecycle` (`ctx.window_feed_lifecycle(kind)`) and
`ponko_ctx_window_feed_keyboard_inset`; the guide's C++ example above runs
that way in the native check.

## The safe area

On iOS the [safe area](touch.md#notches-home-bars-and-rounded-corners) comes
from the system: the game's window covers the whole screen, and the part
the notch or Dynamic Island, the home indicator and the rounded corners
leave is read once a frame, so it follows the phone as it turns. On the
simulated iPhone 18 Pro that is 62 points at the top and 34 at the bottom
held upright, and 62 on each side and 20 at the bottom turned either way
(top, right, bottom, left: `62 0 34 0` and `0 62 20 62`). UI roots made
with `.inside_safe_area()` and the engine's touch controls keep to it.

winit makes its window before the app's scene exists, and UIKit keeps such
a window at its upright safe area. The engine therefore reads the safe area
from a second, empty window made in the scene and kept under the game's;
it is never seen and takes no touches.

## Sound: the audio session

iOS asks every app what kind of sound it makes, its audio session category:
whether it stops the player's music and whether the ring/silent switch
mutes it. `ctx.audio.set_session_category` sets it (C:
`ponko_ctx_audio_set_session_category`, C++: `ctx.set_audio_session_category`):

| `AudioSessionCategory` | Other apps' audio | The silent switch |
|---|---|---|
| `SoloAmbient` (the default, iOS's own) | stops | mutes the game |
| `Ambient` | plays on, mixed with the game | mutes the game |
| `Playback` | stops | the game is still heard |

The category is kept and has no effect on other systems, so a game sets it
once at setup everywhere. A category the system refuses is an
`ErrorKind::Unsupported` error and the old one stays.

While the app is suspended (`WindowEvent::Suspended`, when it loses the
screen) the engine closes the audio output, as on Android: nothing plays
over the home screen or another app, and no audio stream runs. On
`WindowEvent::Resumed` it opens again, and every sound and the music go on
from where they were.

Interruptions (a call, an alarm, Siri) are the audio backend's: it stops the
output while one lasts and starts it again after. The game's mix keeps its
state, and its clock doesn't run on while the app is suspended.

## Files and the clipboard

[Storage](storage.md) keeps a game's files in `Library/Application Support/<app id>`
inside the app's own container, which the device backs up; they go with the
app when it is deleted. The clipboard (`ctx.window.clipboard_read` and
`clipboard_write`) is the general pasteboard. iOS shows the player a notice
when an app reads text another app copied.

## Checking on the simulator

`scripts/ios-simulator.sh` runs the whole check headlessly; no window opens
on the Mac, as the simulator runs without Simulator.app:

```sh
scripts/ios-simulator.sh            # results and screenshots in target/ios-simulator
```

It packages the phone check (`examples/phone.rs`) for the simulator, makes a
fresh simulated iPhone, and:

0. builds the engine's library tests for the simulator and runs the iOS
   ones in it (`xcrun simctl spawn`): the keyboard's notifications and every
   audio session category;
1. starts the app headless for 120 ticks (`PONKO_SMOKE`, passed as
   `SIMCTL_CHILD_PONKO_SMOKE`) and requires the smoke run to pass;
2. starts it with its window and requires steady frames for 10 seconds,
   the safe area read from the system, the clipboard round trip, a file kept
   in the storage folder, and a suspend and a resume when another app comes
   to the front and the game is opened again;
3. starts it turned to landscape (`PONKO_IOS_ORIENTATION=landscape`, which
   asks the app's scene to turn as the game starts) and requires the safe
   area to follow: a window wider than tall, with insets at its sides;
4. starts it with the keyboard open and requires its inset.

Every run is muted with `PONKO_MUTE=1`. The simulated iPhone is shut down
and deleted at the end, also when a step fails. By hand:

```sh
xcrun simctl boot "iPhone 18 Pro"
xcrun simctl install booted "target/package/<name>-<version>-ios-simulator-aarch64/<Name>.app"
SIMCTL_CHILD_PONKO_MUTE=1 SIMCTL_CHILD_PONKO_SMOKE=120 xcrun simctl launch --console-pty booted <app id>
xcrun simctl io booted screenshot game.png
xcrun simctl shutdown booted
```

**Muting a check.** `PONKO_MUTE=1` makes the audio output play silence
(see [Audio](audio.md#muting-automated-runs-ponko_mute)), so a check doesn't
play through the speakers of the Mac it runs on. `simctl launch` passes it as
`SIMCTL_CHILD_PONKO_MUTE=1`. For apps started any other way (from the home
screen, or by a UI-testing tool that taps and turns the simulator), set it in
the simulator's own environment once after booting:
`xcrun simctl spawn booted launchctl setenv PONKO_MUTE 1` (and
`launchctl unsetenv PONKO_MUTE` to hear the game again).

## On a device: signing

A device installs only an app signed with a developer certificate and a
provisioning profile that names the device. `ponko package --target ios`
leaves that to the [signing hook](packaging.md#signing-and-notarizing-the-hook)
and says so when it is missing. The steps, for whoever holds the Apple
developer account:

1. In the developer account, register the app id (the game's `id`) and the
   device (its UDID, from `xcrun devicectl list devices`), and make an
   "Apple Development" certificate; install it in the login keychain
   (`security find-identity -v -p codesigning` lists it).
2. Make an iOS App Development provisioning profile for that app id,
   certificate and device, and download it (`profile.mobileprovision`).
3. A signing program, called by `ponko package` with the `.app` before the
   `.ipa` is made:

   ```sh
   #!/bin/sh
   # sign-ios.sh <stage> <path>; IOS_PROFILE and IOS_IDENTITY are yours.
   set -e
   [ "$1" = sign ] || exit 0
   cp "$IOS_PROFILE" "$2/embedded.mobileprovision"
   security cms -D -i "$IOS_PROFILE" > "$PONKO_PACKAGE_DIR/profile.plist"
   /usr/libexec/PlistBuddy -x -c 'Print :Entitlements' "$PONKO_PACKAGE_DIR/profile.plist" \
       > "$PONKO_PACKAGE_DIR/entitlements.plist"
   codesign --force --sign "$IOS_IDENTITY" --generate-entitlement-der \
       --entitlements "$PONKO_PACKAGE_DIR/entitlements.plist" "$2"
   rm "$PONKO_PACKAGE_DIR/profile.plist" "$PONKO_PACKAGE_DIR/entitlements.plist"
   ```

4. Package, check and install:

   ```sh
   IOS_PROFILE=profile.mobileprovision IOS_IDENTITY="Apple Development: …" \
       ponko package --target ios --sign ./sign-ios.sh
   codesign --verify --verbose "target/package/<name>-<version>-ios-aarch64/<Name>.app"
   xcrun devicectl device install app --device <device> "target/package/<name>-<version>-ios-aarch64/<Name>.app"
   xcrun devicectl device process launch --console --device <device> <app id>
   ```

`--console` shows the game's printed output, as the simulator's
`--console-pty` does. The device must be in Developer Mode (Settings,
Privacy & Security).

The App Store needs more than the command makes today: an asset catalog
(`Assets.car`) with a 1024-pixel icon, a privacy manifest
(`PrivacyInfo.xcprivacy`), a distribution certificate and profile, and an
upload with Xcode's Transporter or `xcrun altool`.

## C and C++

Everything in this chapter is in the C API and the C++ wrapper:
`PONKO_WINDOW_EVENT_SUSPENDED`, `_RESUMED` and `_LOW_MEMORY` among the window
events (`ctx.window_events()` in C++), `ponko_ctx_window_suspended`,
`ponko_ctx_window_keyboard_inset`, `PonkoAudioSessionCategory` with
`ponko_ctx_audio_set_session_category`, and the feeds for tests. A C or C++
game for iOS still needs its own Xcode project that links the engine's
static library built for `aarch64-apple-ios`; `ponko package` packages
Cargo targets only.

## What has been checked

| What | Checked | How |
|---|---|---|
| Builds for devices and the simulator | yes | clippy for `aarch64-apple-ios` and `aarch64-apple-ios-sim` (engine and C API), the simulator package |
| The package and its `Info.plist` | yes | unit tests (`cargo test -p ponko_cli`), and iOS installs and starts it on the simulator |
| Metal rendering, full screen | simulator | the phone check draws its frame around the safe area; screenshot |
| Steady frames | simulator | about 60 frames a second for 25 seconds |
| Safe area | simulator | 62 / 0 / 34 / 0 points on an iPhone 18 Pro upright, 0 / 62 / 20 / 62 turned either way, also when started turned |
| Suspend and resume | simulator | another app in front, then the game again: one event each, no frames between |
| Sound while away | simulator | at the home screen the game's audio render thread stops (process samples), and it runs again on return |
| On-screen keyboard and its inset | simulator | opened by the game; 301 points |
| Clipboard, storage folder | simulator | round trips in the phone check |
| Audio session categories | simulator | each one set on the system's session |
| Touches, gestures, typing | **needs a device and a person** | the simulator can't be touched headlessly; the shared touch code is tested on the desktop and the web |
| Memory warnings | **needs a person** | Simulator.app's Debug menu, or a device |
| Interruptions (a call, an alarm), the silent switch | **needs a device and a person** | |
| Turning the phone | simulator | turned both ways and back by a UI-testing tool, and started turned (step 3 of the check); still to confirm on a device |
| Signing, install on a device | **needs the developer account** | the steps above |
| Performance, power, heat | **needs a device** | the simulator's numbers say nothing about a phone |

## Notes for other platforms

The lifecycle events, `suspended()`, the keyboard inset, the platform's
safe insets and the audio session category are platform-neutral: Android
reports the same events and insets through the same calls
([Android](android.md)), and the
category is kept, without effect, where a system has no such session.
