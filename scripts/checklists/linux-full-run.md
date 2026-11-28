# Linux: the whole list of checks

Issue [#500](https://github.com/ponkoengine/ponko/issues/500): no recorded run of the full list of checks on Linux since the hosted checks went away. This is the whole list, once, on real desktops. The single Linux items of #484 (keyboard layouts, crash backtraces, windowing) are in [hardware-linux.md](hardware-linux.md); do both.

Read [the conventions](README.md#conventions) first.

## What is needed

- **Two machines** (or one machine and two disks): one with a **Mesa driver** (Intel or AMD) and one with the **NVIDIA driver**.
- Sessions to try on each: **X11**, and **Wayland** with **GNOME** and with **KDE**.
- A **fresh install** of Ubuntu, Fedora and Arch for the package check (a container is not enough for the games; a virtual machine is enough for the package check only).

## 1. The whole list

On a clean checkout of `main` (note its revision), with Python 3.10 or later, Git, Bash, the Rust toolchains the list asks for, and the ALSA and udev headers (book/src/getting-started.md; AT-SPI and D-Bus for the accessibility lane). See "Running local checks" in `book/src/contributing.md` for the tools of each job.

```sh
git clone https://github.com/ponkoengine/ponko && cd ponko
python3 scripts/run-checks.py --list            # what will run, and what is skipped and why
python3 scripts/run-checks.py                   # the whole list; writes target/check-results/
python3 scripts/run-checks.py --include-windowed   # also the steps that open windows (on a session with a display)
```

Both full runs are wanted: the first (no windows) and, on a desktop session, the second. Exit code 1 means a check failed, 2 means a usage error or that nothing ran. A failed step is an issue each (title: the step, the distribution, the driver). Then make the record:

```sh
python3 scripts/checklists/record_run.py checks --hardware "<CPU, RAM, GPU, driver>" > record.md
```

`record.md` has the date, revision, machine (distribution, kernel, session, GPU), the counts, every failed step and every GPU test that skipped. Add the issue numbers by hand. Keep `target/check-results/results.json` too: it has the failed steps' full output.

## 2. The `render::` tests on Vulkan and on OpenGL

How many passed, failed, and were skipped for a missing capability, per backend. Run each once and keep the output:

```sh
WGPU_BACKEND=vulkan PONKO_REQUIRE_GPU=1 cargo test -p ponko --lib --features 3d render_test -- --test-threads=4 --nocapture 2>&1 | tee render-vulkan.log
WGPU_BACKEND=gl     PONKO_REQUIRE_GPU=1 cargo test -p ponko --lib --features 3d render_test -- --test-threads=4 --nocapture 2>&1 | tee render-gl.log
python3 scripts/checklists/record_run.py render render-vulkan.log render-gl.log
```

`PONKO_REQUIRE_GPU=1` makes a missing adapter a failure instead of a silent skip; `--test-threads=4` keeps a small card from running out of memory. The last command prints, per file, passed, failed and ignored by cargo, how many tests ran on a GPU and how many skipped, the adapters, and the reasons for the skips (book/src/render-tests.md says what each kind of skip means). Paste its output.

## 3. The three reference games, played

On each session type (X11; Wayland on GNOME; Wayland on KDE) and on each of the two machines where it applies, play each game for a few minutes: `cargo run --release --example coin_dash`, `... --example ember_hop --features physics`, `... --example shard_grove --features navmesh,physics3d`.

| Check | Look for |
|---|---|
| Window | opens at the right size, title is right, nothing flickers |
| Fullscreen | borderless and exclusive (see W1 in [hardware-linux.md](hardware-linux.md)) |
| HiDPI scaling | on a scaled display the game is sharp and the right size |
| Cursor grab | confined and locked (W3) release when you switch away |
| Clipboard | copy and paste Unicode with another program (W4) |
| Sound | sound comes out through PipeWire (`pactl info` shows PipeWire); volume and mute work |
| Gamepad | a pad works, glyphs match, rumble is felt (C1) |

Record one line per game and session: date ______ · hardware ______ · distribution and session ______ · game ______ · each row above pass/fail ______ · issue # ______.

## 4. A packaged build on another distribution

```sh
cargo run -p ponko_cli -- package --example coin_dash          # on the build machine
```

Copy the archive to a different distribution (a different family if you can: for example built on Ubuntu, run on Fedora or Arch), unpack it somewhere else and start it. Look for: it starts with no Rust installed; missing system libraries are named by the error, not by a crash. Record: build distribution, run distribution, game, result, the library that was missing if one was.

## 5. The package list in the getting-started chapter, on fresh installs

`book/src/getting-started.md` lists the Debian and Ubuntu packages (`libasound2-dev libudev-dev`). On a fresh install of each of Ubuntu, Fedora and Arch with only Git, a C compiler and Rust installed, find out what is needed to build and run an example, and fix the chapter from what you find. Starting points (confirm each; they are the usual names, not tested here):

| Distribution | Try |
|---|---|
| Ubuntu | `sudo apt-get install libasound2-dev libudev-dev` |
| Fedora | `sudo dnf install alsa-lib-devel systemd-devel` |
| Arch | `sudo pacman -S alsa-lib systemd-libs` (and `base-devel`) |

Then `cargo run --release --example coin_dash`. Record, for each distribution: the version, the packages you needed beyond the list, whether the game started, and any error that did not name the missing package.

## Where to report

ONE comment on [#500](https://github.com/ponkoengine/ponko/issues/500) per machine and distribution: `record.md` (section 1), the `record_run.py render` output (section 2), the lines of sections 3 to 5, with an issue number for every failure. When one distribution with a Mesa driver and one with the NVIDIA driver have a complete comment each, the issue can be closed.
