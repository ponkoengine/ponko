# Hardware checks: Linux

Issue [#484](https://github.com/ponkoengine/ponko/issues/484). Things no headless test can show. Do the checks that apply to your machine, in any order; each has the command, what to look for and one line to fill in. A check you cannot do (no such hardware) stays blank: do not mark it. Read [the conventions](README.md#conventions) first.

Machine (run `python3 scripts/checklists/record_run.py machine` and paste the lines):

```text
revision: ______  date: ______
OS and version: ______
CPU / RAM: ______
GPU and driver: ______
displays (size, scale): ______
```

For minor releases, also run the [four-hour reference-game soaks](soak.md).
Record their results on that sheet.

## W1. Window modes

- **Run:** Copy `scripts/checklists/window_driver.rs` to `examples/window_driver.rs` (do not commit it) and run `cargo run --example window_driver`; its header lists the keys (F1 mode, F2 title bar, F3 always on top, F4 cursor grab, F5 cursor shown, F6 system cursors, F7 save settings, F8 print the state). Press F1 repeatedly: windowed → borderless → exclusive (an advertised mode) → windowed.
- **Look for:** The monitor's real resolution and refresh rate match what F8 prints (the state); the choice is kept after a restart; quitting puts the desktop back as it was.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## W2. Move, resize, decorations, always on top, icon

- **Run:** The window driver (see W1): move and resize the window; F2 and F3 toggle the title bar and always-on-top. (The icon has no key: skip it, or call `ctx.window.set_icon` from the driver.)
- **Look for:** Reported sizes and scale are right. With two monitors of different scale: drag the window between them; UI and pixel art stay sharp.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## W3. Cursors

- **Run:** The window driver (see W1): F6 cycles system cursors, F5 hides and shows, F4 cycles none → confined → locked; switch to another app and back, then release.
- **Look for:** The confinement edges are right; the lock lets go when you switch away and takes hold again when you come back; no input stays locked afterwards.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## W4. Unicode text and the clipboard

- **Run:** `cargo run --example ui_menu --features accessibility` (its text field), or any example with a text input: type non-Latin text, select, copy, cut and paste between the game and another program.
- **Look for:** Text arrives intact both ways. On Linux also: the clipboard still holds the text after the game closes (clipboard ownership).
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## W5. Unplug a monitor

- **Run:** With two monitors: in the window driver (see W1) go to exclusive mode, press F7, quit, and start it again; then unplug the second monitor while it runs, quit and restart.
- **Look for:** The game starts windowed and says why.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## W6. Focus, minimize, drop a file, close

- **Run:** The window driver (see W1): focus and unfocus, minimize and restore, drop a file onto the window (watch the terminal), click the window's close button (the driver cancels the first close request and prints it), then press Esc, which calls `ctx.quit()`.
- **Look for:** The events show in the example; cancelling the close keeps the game running; `ctx.quit()` exits.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## W7. Saved window settings

- **Run:** The window driver (see W1) uses `restore_window_settings(true)`: set a size and position by hand (and each mode with F1), press F7, quit with Esc, start it again. Repeat for windowed, borderless and exclusive.
- **Look for:** The window appears at the saved place and size, in the saved mode, with no default-size window showing first.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## E1. Editor: file pickers

- **Run:** `ponko editor --example editor` (run from the repository): **Open** and **Save as**.
- **Look for:** The system picker opens in the document's folder with the name filled in for a save; a file outside `assets/` is refused; cancelling changes nothing. Linux: try with the desktop portal and again with Zenity.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## E2. Editor: no key stays held after a picker

- **Run:** Open the picker with Ctrl+O (⌘O on macOS) and with Enter on **Choose file…**; choose a file, and also cancel. Then press plain O, S and Enter. Also switch to another window with Ctrl or Alt held and come back.
- **Look for:** No second picker, no save, no confirmed dialog: nothing acts as if Ctrl (⌘) or Enter were still held.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## E3. Editor: the manual list

- **Run:** `ponko editor --example editor`, then `ponko editor --last`: title bar, close dialog, gizmo feel, menus, drag hover, external reload (change the file in another program), shortcuts (⌘ on macOS), sound preview aloud, real mouse drag, rebinding, HiDPI, where `editor.json` is written, the 3D camera centring.
- **Look for:** Each item behaves as the editor chapter says (book/src/editor.md). Note the ones that do not.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## C1. Controllers: two real pads

- **Run:** `cargo run --example gamepad` with two pads (use `--features gamepad-sdl` too, where the pads need it): buttons, sticks, triggers, rumble, glyphs; plug one in and out while running.
- **Look for:** Both are listed; the glyphs match the pad; rumble is felt; hot-plug is noticed. Write down the pad models.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## C2. Controllers: motion and trigger rumble

- **Run:** Same example with a DualSense, a DualShock 4 and a Switch Pro pad (whichever you have): gyro and accelerometer.
- **Look for:** Axis signs and values make sense when you tilt the pad; body and trigger rumble are felt.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## T1. Touch (a touch screen on this OS)

- **Run:** Play the three reference games with the touch controls (`cargo run --release --example coin_dash`, `... --example ember_hop --features physics`, `... --example shard_grove --features navmesh,physics3d`), and try the on-screen sticks, buttons, gestures and the layout edit mode.
- **Look for:** A tap clicks once; sticks and buttons respond; pinch and rotate work; the prompts do not switch to keyboard glyphs. Macs have no touch screen: skip there.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## K1. Keyboard layout labels

- **Run:** Run `cargo run --release --example coin_dash` (or any game with key prompts) and switch the keyboard layout with the desktop's own shortcut. Linux: GNOME and KDE under Wayland and an X11 session (`setxkbmap fr,de -option grp:alt_shift_toggle`); also load a keymap while a game runs on X11.
- **Look for:** The key labels change on the next frame (`Key::W` shows `Ц` after switching to Russian, `Z` for a German layout).
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## R1. Real crash reports

- **Run:** `PONKO_CRASH_TESTS_DIE=1 cargo test --test native_crash` (macOS and Windows leave a system crash entry each time; Linux needs no variable). Then start the crashed game again.
- **Look for:** The next start shows the report; the backtrace names the crashing function. Linux: check the frame-pointer line of the backtrace tests.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## G1. GPU timing on this backend

- **Run:** `WGPU_BACKEND=<backend> cargo test -p ponko --lib --features 3d a_compute_pass_with_real_work_gets_a_time_within_the_frame -- --nocapture`, once for each backend you have (`vulkan`, `dx12` on Windows, `gl`, `metal` on macOS).
- **Look for:** It passes and prints the adapter. A skip because there are no timestamp queries is recorded as a skip, not a pass.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## G2. Query-set limits

- **Run:** `WGPU_BACKEND=<backend> cargo test -p ponko --lib --features 3d -- more_timed_devices_than_metal_has_query_sets_for_all_keep_working passes_beyond_the_query_sets_limit_are_counted_not_silently_dropped a_quick_drop_and_create_cycle_never_holds_more_sets_than_it_counts --nocapture` for each backend you have.
- **Look for:** All pass with no device lost and no refused timer.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## G3. Crowd, instance-cell and GI render tests

- **Run:** `WGPU_BACKEND=<backend> cargo test -p ponko --lib --features 3d render_test -- --test-threads=4 --nocapture 2>&1 | tee render-<backend>.log`, then `python3 scripts/checklists/record_run.py render render-<backend>.log`.
- **Look for:** The counts of ran, skipped (with the reasons) and failed. A failure needs an issue with the test name and the adapter.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## B1. Blender

- **Run:** Follow the Blender chapter (book/src/blender.md): export a real level from Blender, and install the add-on in a real Blender (Preferences → Add-ons → Install…).
- **Look for:** The export loads in the game as the chapter says; the add-on enables, shows its properties and writes `ponko-schema.json`.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## D1. Lobby server in a container

- **Run:** `docker build -f crates/ponko_lobby/Dockerfile -t ponko-lobby .` (from the repository root), then `docker run --rm --name lobby -p 7700:7700/udp -p 7701:7701/tcp ponko-lobby` and `docker stop lobby`.
- **Look for:** It builds, a game can use it as its lobby, and `docker stop` makes it tell its members and exit.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## N1. Replication: a browser and two machines

- **Run:** `cargo run --example net_arena -- server`; build `net_arena` for the web (book/src/web.md) and open it in a browser; join a native client too. Then, on two machines: `net_arena server` on one, `net_arena join <its address>:7777` on the other; pull the network cable for a couple of seconds.
- **Look for:** Both see each other move smoothly, scores match, a wave shows on both. After the cable is pulled the other players freeze, yours keeps moving, and everything catches up within a second of reconnecting.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## N2. WebRTC across a real NAT

- **Run:** Run the lobby on a public address with `LOBBY_STUN=0.0.0.0:3478 cargo run -p ponko_lobby` (book/src/lobby.md, book/src/webrtc.md), then two copies of a game that hosts and joins with the chapter's code (`lobby.rtc_host` and `lobby.rtc_join`) on two different networks, one of them in a browser. No shipped example does this across networks (`net_webrtc` runs both ends in one process): use your own game, or the chapter's few lines in a scratch example.
- **Look for:** They connect; note whether STUN alone was enough or TURN was needed, the browser, and both networks' kinds of NAT if you know them.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## S1. Steam: a match between two accounts

- **Run:** Two machines, two Steam accounts, a build with `--features steam` (book/src/steam.md): host, join through an invite, play, open the overlay (Shift+Tab).
- **Look for:** Relay traffic works, lobby callbacks fire, the overlay invite joins, the game pauses under the overlay, and with Steam closed the game still starts and plays.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## WB1. Web threads in Firefox and Safari

- **Run:** Build a threaded web game (`web/build.sh --threads --isolation-worker --bin <game>`, book/src/web-threads.md), serve it over HTTP on localhost, and open it in Firefox and in Safari (Safari: macOS only), and behind a real host's headers if you have one.
- **Look for:** The page reports `crossOriginIsolated`, the game runs on several threads, and without isolation it stays on one thread and says so.
- **Record:** date ______ · hardware ______ · result ______ · issue # if it failed ______


## Linux only: sessions

Repeat W1 to W4 and K1 on each session type you have: GNOME on Wayland, KDE on Wayland, and an X11 session. Record the session next to each result (`echo $XDG_SESSION_TYPE $XDG_CURRENT_DESKTOP`).

The full list of checks, and the Linux-wide items (Mesa and NVIDIA runs, packages on fresh installs), are in [linux-full-run.md](linux-full-run.md) (#500).

## Where to report

Paste the filled lines (or a screenshot of them) as ONE comment on [#484](https://github.com/ponkoengine/ponko/issues/484), and open an issue for each failure; put its number on the line.
