#!/usr/bin/env bash
# The Android emulator check (book/src/android.md, "Checking on the
# emulator"): packages the phone check (`examples/phone.rs`) with `ponko
# package --target android`, boots an emulated phone without a window, and
# on it
#   0. runs the engine's unit tests,
#   1. runs the app headless for 120 ticks (`PONKO_SMOKE`), once: the
#      process must end without Android starting it again,
#   2. starts it with its window: it must draw frames on the GPU, read the
#      safe area, use the clipboard, keep a file in its storage folder,
#      hold the audio focus, be suspended (and let the focus go) when the
#      home screen comes to the front and resumed after, follow a rotation,
#      and leave on Back,
#   3. starts it with the keyboard open: the keyboard's inset must show,
#      typed text must arrive, a tap must close it, and a Back the game
#      cancels must keep it in front,
#   4. packages Coin Dash, a reference game with assets, and runs it
#      headless from its apk (its asset pack read from inside the apk),
# saving screenshots of 2 and 3. The emulator is stopped at the end, also
# when a step fails, with the adb server if this script started it.
#
# Needs the Android SDK (ANDROID_HOME, else Android Studio's folder) with
# platform-tools, build-tools, a platform, the NDK, the emulator and an
# arm64 (or, on an Intel machine, x86_64) system image; a JDK; and
# `rustup target add aarch64-linux-android`. The guide lists the
# `sdkmanager` packages.
#
#   scripts/android-emulator.sh [out-dir]   (default target/android-emulator)
#   scripts/android-emulator.sh --clippy    (only clippy for Android, with
#                                            the NDK's compilers; no emulator)
#
# PONKO_ANDROID_AVD names an existing virtual device to boot instead of a
# fresh one; PONKO_ANDROID_IMAGE the system image of a fresh one (default:
# the newest arm64 Google APIs image installed); PONKO_ANDROID_GPU the
# emulator's -gpu mode (default swiftshader_indirect, which needs no
# display).
set -euo pipefail
cd "$(dirname "$0")/.."

clippy=no
if [ "${1:-}" = --clippy ]; then
    clippy=yes
    shift
fi
out=${1:-target/android-emulator}
id=dev.ponko.phone_check
activity=android.app.NativeActivity
port=5584
serial=emulator-$port

fail() {
    echo "android-emulator: FAILED: $*" >&2
    exit 1
}

sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}
if [ -z "$sdk" ]; then
    case $(uname) in
    Darwin) sdk=$HOME/Library/Android/sdk ;;
    *) sdk=$HOME/Android/Sdk ;;
    esac
fi
[ -d "$sdk" ] || fail "no Android SDK: set ANDROID_HOME (see the guide's Android chapter)"
export ANDROID_HOME=$sdk
adb=$sdk/platform-tools/adb
emulator=$sdk/emulator/emulator
avdmanager=$(ls "$sdk"/cmdline-tools/*/bin/avdmanager 2>/dev/null | head -1)
rustup target list --installed 2>/dev/null | grep -q aarch64-linux-android ||
    fail "no Rust target: \`rustup target add aarch64-linux-android\`"

# The NDK's compilers, for the unit tests and clippy (`ponko package` sets
# the same for the package).
ndk=${ANDROID_NDK_HOME:-$(ls -d "$sdk"/ndk/* 2>/dev/null | sort -V | tail -1)}
[ -d "$ndk" ] || fail "no NDK: \`sdkmanager \"ndk;29.0.14206865\"\`, or set ANDROID_NDK_HOME"
bin=$(ls -d "$ndk"/toolchains/llvm/prebuilt/*/bin | head -1)
export CC_aarch64_linux_android=${CC_aarch64_linux_android:-$bin/aarch64-linux-android30-clang}
export CXX_aarch64_linux_android=${CXX_aarch64_linux_android:-$bin/aarch64-linux-android30-clang++}
export AR_aarch64_linux_android=${AR_aarch64_linux_android:-$bin/llvm-ar}
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=${CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER:-$bin/aarch64-linux-android30-clang}
if [ "$clippy" = yes ]; then
    # Android's thread locals are emulated, so clippy asks for `const`
    # initializers that are already there: that lint is off here.
    exec cargo clippy -p ponko -p ponko_ffi --target aarch64-linux-android --all-targets "$@" -- \
        -A clippy::missing_const_for_thread_local
fi
[ -x "$adb" ] || fail "no adb: \`sdkmanager platform-tools\`"
[ -x "$emulator" ] || fail "no emulator: \`sdkmanager emulator\`"

echo "android-emulator: packaging the phone check"
rm -rf "$out"
mkdir -p "$out"
cargo run -q -p ponko_cli -- package --example phone --target android --out "$out/package" >"$out/package.log" ||
    fail "packaging: $(cat "$out/package.log")"
apk=$(ls "$out"/package/phone-check-*-android-aarch64.apk)

echo "android-emulator: building the engine's unit tests for Android"
tests=$(cargo test -q -p ponko --lib --target aarch64-linux-android --no-run --message-format=json 2>/dev/null |
    python3 -c 'import json,sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == "ponko":
        print(m["executable"])')
[ -n "$tests" ] || fail "the library's tests did not build for Android"

adb_was_running=no
pgrep -x adb >/dev/null && adb_was_running=yes
avd=${PONKO_ANDROID_AVD:-}
made_avd=""
emu=""
cleanup() {
    "$adb" -s "$serial" emu kill >/dev/null 2>&1 || true
    [ -n "$emu" ] && { kill "$emu" 2>/dev/null || true; wait "$emu" 2>/dev/null || true; }
    [ -n "$made_avd" ] && "$avdmanager" delete avd -n "$made_avd" >/dev/null 2>&1 || true
    [ "$adb_was_running" = no ] && "$adb" kill-server >/dev/null 2>&1 || true
}
trap cleanup EXIT
if [ -z "$avd" ]; then
    [ -n "$avdmanager" ] || fail "no avdmanager: \`sdkmanager \"cmdline-tools;latest\"\`"
    image=${PONKO_ANDROID_IMAGE:-$(ls -d "$sdk"/system-images/android-*/google_apis/arm64-v8a 2>/dev/null |
        sort -V | tail -1 | sed "s|^$sdk/||; s|/|;|g")}
    [ -n "$image" ] || fail "no system image: \`sdkmanager \"system-images;android-36;google_apis;arm64-v8a\"\`"
    made_avd=ponko-check-$$
    avd=$made_avd
    echo no | "$avdmanager" create avd -n "$avd" -k "$image" -d pixel_7 >/dev/null
fi
echo "android-emulator: booting $avd without a window"
"$emulator" -avd "$avd" -port "$port" -no-window -no-audio -no-boot-anim -no-snapshot \
    -gpu "${PONKO_ANDROID_GPU:-swiftshader_indirect}" >"$out/emulator.log" 2>&1 &
emu=$!
"$adb" -s "$serial" wait-for-device
for _ in $(seq 120); do
    [ "$("$adb" -s "$serial" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ] && break
    sleep 2
done
[ "$("$adb" -s "$serial" shell getprop sys.boot_completed | tr -d '\r')" = 1 ] || fail "the emulator did not boot"
sh() { "$adb" -s "$serial" shell "$@"; }
sh settings put global window_animation_scale 0
sh settings put global transition_animation_scale 0
sh wm user-rotation lock 0
"$adb" -s "$serial" install -r "$apk" >/dev/null

echo "android-emulator: 0. the engine's unit tests"
"$adb" -s "$serial" push "$tests" /data/local/tmp/ponko-tests >/dev/null 2>&1
# Left out: tests that read this checkout's files (fonts, scenes, shaders,
# assets/) by their path on the build machine, and hard links, which the
# shell user may not make in /data/local/tmp.
sh "mkdir -p /data/local/tmp/t && cd /data/local/tmp && HOME=/data/local/tmp/t TMPDIR=/data/local/tmp/t \
    timeout 300 ./ponko-tests --test-threads 4 \
    --skip assets::tests::loose_files_win_over_default_packs_which_fill_in_the_rest \
    --skip control::tests::overwrite_refuses_pipes_and_hard_links \
    --skip editor::document::tests --skip render::gpu::browser_gpu::tests \
    --skip render::shader_diagnostics::cook_rules --skip scene::lockstep_tests \
    --skip ui::pixel_snap_tests::a_retained_label_draws_missing_glyphs_with_its_fallback_font \
    --skip ui::pixel_snap_tests::named_rich_faces_and_synthetic_bold_reach_the_draw_list" \
    >"$out/unit.log" 2>&1 || fail "unit tests: $(tail -40 "$out/unit.log")"
grep -q "test result: ok. [1-9]" "$out/unit.log" || fail "no unit test ran: $(tail -20 "$out/unit.log")"

# The app's output since `mark`.
mark() { "$adb" -s "$serial" logcat -c; }
log() { "$adb" -s "$serial" logcat -d -s 'ponko:*' | tr -d '\r'; }
start() { sh am start -W -n "$id/$activity" "$@" >/dev/null; }
top() { sh dumpsys activity activities | grep -m1 topResumedActivity; }
focus() { sh dumpsys audio | grep -A3 "Audio Focus stack" | grep -c "pack: $id" || true; }
shot() { "$adb" -s "$serial" exec-out screencap -p >"$1"; }
# Waits up to a minute for a line in the app's output.
await() {
    for _ in $(seq 60); do
        log | grep -q "$1" && return 0
        sleep 1
    done
    return 1
}

echo "android-emulator: 1. headless smoke run"
mark
start -e PONKO_SMOKE 120
await "ponko: smoke run passed" || fail "smoke run: $(log)"
sleep 8
log >"$out/smoke.log"
[ "$(grep -c "ponko: smoke run passed" "$out/smoke.log")" = 1 ] ||
    fail "smoke run (once, without a restart): $(cat "$out/smoke.log")"

echo "android-emulator: 2. windowed: frames, safe area, focus, suspend and resume, rotation, Back"
sh am force-stop "$id"
mark
start -e PONKO_PHONE_AUDIO solo
await "frames in 10" || fail "no frames for 10 s: $(log)"
shot "$out/phone.png"
[ "$(focus)" = 1 ] || fail "the audio focus was not taken"
sh input keyevent KEYCODE_HOME
sleep 3
[ "$(focus)" = 0 ] || fail "the audio focus was kept while away"
start
sleep 5
[ "$(focus)" = 1 ] || fail "the audio focus was not taken back"
sh wm user-rotation lock 1
sleep 4
shot "$out/landscape.png"
sh wm user-rotation lock 0
sleep 3
sh input keyevent KEYCODE_BACK
sleep 3
top | grep -q "$id" && fail "Back did not leave the app"
log >"$out/window.log"
cat "$out/window.log"
grep -q "frames in 10" "$out/window.log" || fail "the frames stopped (no report at 10 s)"
grep -Eq "phone: gpu .* on (vulkan|gl)" "$out/window.log" || fail "no GPU adapter named"
grep -Eq "window ([0-9]+) x ([0-9]+), safe insets top [1-9]" "$out/window.log" || fail "no safe area inset read"
grep -q "phone: suspended" "$out/window.log" || fail "no Suspended event"
grep -q "phone: resumed" "$out/window.log" || fail "no Resumed event"
grep -Eq "window ([0-9]+) x ([0-9]+), safe insets top 0 right 0 bottom [0-9]+ left [1-9]" "$out/window.log" ||
    fail "no landscape window with the cutout on the left"
grep -q 'clipboard holds Some("ponko phone check")' "$out/window.log" || fail "the clipboard"
grep -q "storage kept a file in \"/data/user/0/$id/files/dev.ponko.phone-check\"" "$out/window.log" ||
    fail "storage in the app's files folder"
grep -q "phone: back, leaving" "$out/window.log" || fail "no BackRequested event"

echo "android-emulator: 3. the on-screen keyboard, typing and a kept Back"
sh am force-stop "$id"
mark
start -e PONKO_PHONE_KEYBOARD 1 -e PONKO_PHONE_KEEP_BACK 1
sleep 8
sh input text ponko
sleep 2
shot "$out/keyboard.png"
sh input tap 500 400
sleep 2
sh input keyevent KEYCODE_BACK
sleep 2
top | grep -q "$id" || fail "a canceled Back left the app"
log >"$out/keyboard.log"
grep -Eq "keyboard [1-9]" "$out/keyboard.log" || fail "no keyboard inset: $(cat "$out/keyboard.log")"
awk '/keyboard [1-9]/ { open = 1 } open && /keyboard 0$/ { closed = 1 } END { exit !closed }' "$out/keyboard.log" ||
    fail "the tap did not close the keyboard"
grep -q 'phone: typed "ponko"' "$out/keyboard.log" || fail "the typed text did not arrive"
grep -q "phone: back, kept" "$out/keyboard.log" || fail "no kept Back"
sh am force-stop "$id"

echo "android-emulator: 4. a reference game with assets"
cargo run -q -p ponko_cli -- package --example coin_dash --target android --out "$out/package" >"$out/coin-package.log" ||
    fail "packaging Coin Dash: $(cat "$out/coin-package.log")"
"$adb" -s "$serial" install -r "$(ls "$out"/package/coin-dash-*-android-aarch64.apk)" >/dev/null
mark
sh am start -W -n "dev.ponko.coin_dash/$activity" -e PONKO_SMOKE 120 >/dev/null
await "ponko: smoke run passed" || true
log >"$out/coin.log"
grep -q "ponko: smoke run passed" "$out/coin.log" || fail "Coin Dash smoke run: $(cat "$out/coin.log")"
grep -qi "not found\|can't read\|panicked" "$out/coin.log" && fail "Coin Dash's assets: $(cat "$out/coin.log")"

echo "android-emulator: passed; screenshots in $out (phone.png, landscape.png, keyboard.png)"
