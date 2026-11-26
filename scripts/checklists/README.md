# Checklists and record sheets for checks that need a person

Some checks need a real machine, a real display or a person: no headless test can do them. This folder makes each of them quick to do and to record, so a release can say what was checked, on what, and what came out.

| Kit | For | What it holds |
|---|---|---|
| [hardware-linux.md](hardware-linux.md), [hardware-macos.md](hardware-macos.md), [hardware-windows.md](hardware-windows.md) | [#484](https://github.com/ponkoengine/ponko/issues/484) | One checklist per OS: each check with its command, what to look for and a line to fill in |
| [window_driver.rs](window_driver.rs) | #484 (window checks) | A small program to drive the window checks by key |
| [newcomer-test.md](newcomer-test.md) | [#505](https://github.com/ponkoengine/ponko/issues/505) | Tasks for someone new to the engine, a timing sheet, questions, where to report |
| [input-and-readers.md](input-and-readers.md) | [#460](https://github.com/ponkoengine/ponko/issues/460) | IME typing and screen reader checks for macOS, Windows and Linux |
| [lowend.md](lowend.md), [lowend_bench.py](lowend_bench.py), [perf_get.py](perf_get.py) | [#510](https://github.com/ponkoengine/ponko/issues/510) | The low-end and Steam Deck run: a benchmark script, an adapter query and a sheet |
| [linux-full-run.md](linux-full-run.md), [record_run.py](record_run.py) | [#500](https://github.com/ponkoengine/ponko/issues/500) | The whole list of checks on Linux, with a helper that turns the result into a record |
| [editor-and-inspector.md](editor-and-inspector.md) | #484 ([#613](https://github.com/ponkoengine/ponko/issues/613)) | The look of the editor's menus, bar, dialogs and the overlay inspector, and `ponko --help` in a terminal |
| [shard-grove.md](shard-grove.md) | [#454](https://github.com/ponkoengine/ponko/issues/454) | The 3D reference game: a frame-time baseline per GPU tier, opened in a WebGPU browser and in a window on each OS |

The scripts only run things on the machine they are on and print or write files there; nothing is sent anywhere. `python3 scripts/checklists/test_checklists.py` tests their parsing and formatting without any hardware.

## Conventions

- **A record line has three parts: the date, the hardware, the result.** Hardware means the CPU, RAM, GPU and driver, the OS and its version (Linux: distribution, kernel, session type and desktop), and for input the model of the device. `python3 scripts/checklists/record_run.py machine` prints what can be read from the machine.
- **Result** is `pass`, `fail` or `skip` (and why). A check you did not do stays blank. A skip is not a pass.
- **Every failure becomes an issue**; write its number on the line. Give the issue the revision, the machine and the exact command.
- **Record the revision** (`git rev-parse HEAD`) once per sheet. Use a clean checkout of `main` unless a sheet says otherwise.
- **Report** by pasting the filled lines into ONE comment on the issue the sheet names (each sheet ends with where). Keep personal details out: use hardware names, not people's names or addresses.
- **Windows:** start from a Visual Studio Developer PowerShell with the C++ linker and SDK on `PATH`, and replace `python3` with `py -3` (see "Local checks" in `CONTRIBUTING.md`).
- **Do not leave builds behind:** these runs build the engine; delete `target/` when you are done.
