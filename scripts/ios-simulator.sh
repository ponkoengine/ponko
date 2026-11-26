#!/usr/bin/env bash
# The iOS simulator check (book/src/ios.md, "Checking on the simulator"):
# packages the phone check (`examples/phone.rs`) for the simulator, makes a
# fresh simulated iPhone, and on it
#   0. runs the engine's iOS unit tests (`src/platform/ios.rs`),
#   1. runs the app headless for 120 ticks (`PONKO_SMOKE`),
#   2. starts it with its window: it must draw frames, read the safe area,
#      use the clipboard, keep a file in its storage folder, be suspended
#      when another app comes to the front and resumed after,
#   3. starts it with the keyboard open: the keyboard's inset must show,
# saving a screenshot of 2 and 3. The simulated iPhone is shut down and
# deleted at the end, also when a step fails. No window opens on the Mac:
# the simulator runs without Simulator.app.
#
# Needs a Mac with Xcode, its iOS simulator runtime
# (`xcodebuild -downloadPlatform iOS`) and the Rust target
# (`rustup target add aarch64-apple-ios-sim`).
#
#   scripts/ios-simulator.sh [out-dir]       (default target/ios-simulator)
#
# PONKO_IOS_DEVICE names the device type ("iPhone 17"); the default is the
# first iPhone the runtime offers.
set -euo pipefail
cd "$(dirname "$0")/.."

out=${1:-target/ios-simulator}
id=dev.ponko.phone-check

fail() {
    echo "ios-simulator: FAILED: $*" >&2
    exit 1
}

command -v xcrun >/dev/null || fail "no xcrun: this check needs a Mac with Xcode"
xcrun simctl list runtimes | grep -q "iOS" ||
    fail "no iOS simulator runtime: install one with \`xcodebuild -downloadPlatform iOS\`"
rustup target list --installed 2>/dev/null | grep -q aarch64-apple-ios-sim ||
    fail "no Rust target: \`rustup target add aarch64-apple-ios-sim\`"

device_type=${PONKO_IOS_DEVICE:-$(xcrun simctl list devices available | grep -m1 -o "iPhone [^(]*" | sed 's/ *$//')}
[ -n "$device_type" ] || fail "the runtime offers no iPhone"

echo "ios-simulator: packaging the phone check"
rm -rf "$out"
cargo run -q -p ponko_cli -- package --example phone --target ios-sim --format folder --out "$out"
app=$(ls -d "$out"/phone-check-*-ios-simulator-*/"Phone Check.app")

udid=$(xcrun simctl create "ponko-check-$$" "$device_type")
console=""
cleanup() {
    [ -n "$console" ] && kill "$console" 2>/dev/null || true
    xcrun simctl shutdown "$udid" >/dev/null 2>&1 || true
    xcrun simctl delete "$udid" >/dev/null 2>&1 || true
}
trap cleanup EXIT
echo "ios-simulator: booting a $device_type ($udid)"
xcrun simctl boot "$udid"
xcrun simctl bootstatus "$udid" -b >/dev/null
xcrun simctl install "$udid" "$app"

echo "ios-simulator: 0. the engine's iOS unit tests"
tests=$(cargo test -q -p ponko --lib --target aarch64-apple-ios-sim --no-run --message-format=json 2>/dev/null |
    python3 -c 'import json,sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == "ponko":
        print(m["executable"])')
[ -n "$tests" ] || fail "the library's tests did not build for the simulator"
case $tests in /*) ;; *) tests=$PWD/$tests ;; esac
xcrun simctl spawn "$udid" "$tests" platform::ios >"$out/unit.log" 2>&1 || fail "unit tests: $(cat "$out/unit.log")"
grep -q "test result: ok. [1-9]" "$out/unit.log" || fail "no unit test ran: $(cat "$out/unit.log")"

# Starts the app with its output in $1 (extra variables after it).
start() {
    local log=$1
    shift
    env "$@" xcrun simctl launch --console-pty --terminate-running-process "$udid" "$id" >"$log" 2>&1 &
    console=$!
}
stop() {
    xcrun simctl terminate "$udid" "$id" >/dev/null 2>&1 || true
    kill "$console" 2>/dev/null || true
    wait "$console" 2>/dev/null || true
    console=""
}

echo "ios-simulator: 1. headless smoke run"
SIMCTL_CHILD_PONKO_SMOKE=120 xcrun simctl launch --console-pty "$udid" "$id" >"$out/smoke.log" 2>&1
grep -q "ponko: smoke run passed" "$out/smoke.log" || fail "smoke run: $(cat "$out/smoke.log")"

echo "ios-simulator: 2. windowed: frames, safe area, suspend and resume"
start "$out/window.log"
sleep 12
xcrun simctl io "$udid" screenshot "$out/phone.png" >/dev/null
xcrun simctl launch "$udid" com.apple.Preferences >/dev/null
sleep 4
xcrun simctl launch "$udid" "$id" >/dev/null
sleep 6
stop
cat "$out/window.log"
grep -q "frames in 10" "$out/window.log" || fail "the frames stopped (no report at 10 s)"
grep -Eq "safe insets top [1-9]" "$out/window.log" || fail "no safe area inset read"
grep -q "phone: suspended" "$out/window.log" || fail "no Suspended event"
grep -q "phone: resumed" "$out/window.log" || fail "no Resumed event"
grep -q 'clipboard holds Some("ponko phone check")' "$out/window.log" || fail "the clipboard"
grep -q "storage kept a file in .*Library/Application Support/dev.ponko.phone-check" "$out/window.log" ||
    fail "storage in the app's Application Support folder"

echo "ios-simulator: 3. the on-screen keyboard"
start "$out/keyboard.log" SIMCTL_CHILD_PONKO_PHONE_KEYBOARD=1
sleep 8
xcrun simctl io "$udid" screenshot "$out/keyboard.png" >/dev/null
stop
grep -Eq "keyboard [1-9]" "$out/keyboard.log" || fail "no keyboard inset: $(cat "$out/keyboard.log")"

echo "ios-simulator: passed; screenshots in $out (phone.png, keyboard.png)"
