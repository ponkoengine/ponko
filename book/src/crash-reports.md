# Crash reports

When a shipped game panics on a player's machine, the panic message goes to a
terminal nobody looks at. With crash reports, the game leaves a text file in
the player's data folder that says what happened, on which system and GPU,
with which settings, and what the game printed just before. The player can
send it; the game can offer to send it for them. The engine itself never
sends anything anywhere.

## Turning them on

Reports are on by default in **release builds of a game with a window**
(what `ponko package` ships), and off in development builds (with debug
assertions) and in headless runs. One setting changes that:

```rust,no_run
# use ponko::prelude::*;
# fn main() {
Game::new("Space Crab", 1280, 720)
    .app_id("space-crab")
    .crash_reports(true) // on in every build, also headless (a dedicated server)
    .run(|_ctx| {});
# }
```

`.crash_reports(false)` turns them off everywhere. The full form,
`ponko::crash::CrashReports`, also names the game's version and adds a hook:

```rust,no_run
# use ponko::prelude::*;
use ponko::crash::CrashReports;
# fn main() {
Game::new("Space Crab", 1280, 720)
    .app_id("space-crab")
    .crash_reports(
        CrashReports::default() // the build's default: on in release
            .game_version(env!("CARGO_PKG_VERSION"))
            .on_crash(|report| {
                if let Some(path) = report.path() {
                    eprintln!("Space Crab stopped. A report is in {}", path.display());
                }
            }),
    )
    .run(|_ctx| {});
# }
```

Reports are written into `ctx.storage` ([Saves and settings](storage.md)), so
the game needs an app id (or `Game::data_dir`): each one is
`crash-reports/<date>_<time>.txt` (UTC) in the game's data folder, for example
`~/Library/Application Support/space-crab/crash-reports/2026-11-21_14-03-07.txt`.
The newest ten are kept (`ponko::crash::KEPT`); writing one deletes the
oldest beyond that. Without storage, the hook still gets the report, with no
file.

## When a report is written

- **A panic**, in the game's code or the engine's (a GPU validation error, a
  lost surface): the engine's panic hook writes the report before the game
  stops, then the panic goes on as before (the message on standard error, the
  unwinding or the abort). Only the first panic of a run gets a report; the
  ones that follow it (another thread that saw it) say less. A panic the game
  catches with `catch_unwind` gets one too.
- **A fatal engine error**: `Game::run` or `try_run` ends with an error (no
  usable GPU, the window couldn't be made, a setup callback failed), or a
  headless run's setup failed; in a browser, also the failures that stop a
  running game (the WebGL2 context lost, a worker thread that stopped). The
  report's kind is `error`.
- **The game's own fatal error**: `ponko::crash::report("why")` writes one
  for a game that stops without panicking.

Reports start when the run starts and end with it: a panic after `run`
returned gets none.

## The hook: the game's own message, or sending the file

`CrashReports::on_crash` runs the game's code with each report, right after
the file was written. That's where a game shows its own message (a dialog
through its own crate, a page it opens) or hands the report to its
developers. `report.text()` is the file's text, `report.path()` the file on
the disk, and the report serializes to JSON with serde (`serde_json::to_string(report)`)
with the field names below.

For a panic, the hook runs inside the panic hook, on the thread that
panicked, before the game stops: it must not panic itself (a panic inside a
panic hook aborts at once) and should be quick. Sending over the network is
better done at the next start, when the game runs normally and can ask the
player first:

```rust,no_run
# use ponko::prelude::*;
fn offer_crash_reports(ctx: &mut Ctx) {
    for name in ponko::crash::saved_reports(&ctx.storage) {
        let Ok(text) = ctx.storage.read(&name) else { continue };
        // Ask the player, then send `text` to your own server with the HTTP
        // client of your choice. The engine has none and sends nothing.
#       let _ = text;
        let _ = ctx.storage.delete(&name);
    }
}
# fn main() {
Game::new("Space Crab", 1280, 720)
    .app_id("space-crab")
    .run(offer_crash_reports);
# }
```

`saved_reports` lists the reports newest first. The engine's tests send a
report from the hook to a server of the test's own, in the same process
(`tests/crash_reports.rs`), to show the hook can; a real game picks its own
endpoint, and nothing is sent unless it does.

## What a report holds

Every field, in the file's order. This is all there is: no user name, no
machine name, no network address, no hardware serial numbers.

| Field | What it is |
|---|---|
| `kind` | `panic` or `error`. |
| `message` | The panic's message, or the error's. |
| `location` | For a panic, the source position it came from (`src/main.rs:12:5`). |
| `thread` | The name of the thread that panicked (`main`), when it has one. |
| `time` | When, in UTC (`2026-11-21 14:03:07 UTC`). |
| `uptime` | Seconds since the game started. |
| `game` | The game's title, from `Game::new`. |
| `game version` | From `CrashReports::game_version`; `not set` without it. |
| `engine version` | The engine's version. |
| `os` | The operating system, its version and the CPU architecture, with the kernel's name and release on Unix (`macos 15.1 aarch64 (Darwin 24.1.0)`); in a browser, the browser's user agent. |
| `gpu`, `gpu driver` | The GPU the renderer opened: its name, graphics API, type, and the driver's name and version where the driver reports them. `not opened` before the first frame and in headless runs. |
| `settings` | The graphics settings at the last frame: the window's size, scale and mode, vsync, and in 3D builds the quality preset (or `custom`), render scale, multisampling, shadows, antialiasing, ambient occlusion and automatic quality. |
| `backtrace` | Where the panic happened, frame by frame, from the panicking frame down. A release build without debug info shows fewer names (`debug = "line-tables-only"` in the release profile gives file names and lines for little size). |
| `log` | The last 100 lines the game printed on standard output and error (Rust's `println!`, C's `printf` and the engine's messages), oldest first. Not kept in browsers. |

The JSON form (`serde_json`) has the same fields, named `kind`, `message`,
`location`, `thread`, `time`, `uptime_secs`, `game`, `game_version`,
`engine_version`, `os`, `gpu`, `settings`, `backtrace`, `log` and `file` (its
name in `ctx.storage`).

**Paths are made relative to the game's folders.** In every field, the data
folder becomes `<data>`, the program's folder `<game>`, the working folder
`<cwd>` and the home folder `~`; any other user's home folder (a library's
source path in a backtrace, `/Users/<name>`, `/home/<name>`,
`C:\Users\<name>`) becomes `~` too. What the game prints is its own business:
the log holds whatever the game wrote, so a game that prints a player's name
or e-mail address puts it in its reports.

To keep the log, the engine passes standard output and error through pipes
while reports are on, the way the [control endpoint](control.md) does; the
terminal still shows everything.

## In the browser

A web build writes its report into the page's storage, in the same
`crash-reports/` folder of `ctx.storage` (a panic in a worker thread gets
none: the worker reaches neither), and hands it to the page: a
`ponkocrashreport` event on `window`, whose `detail` is
`{ name, text, kind, message }`, also kept as `window.ponkoCrashReport` for a
page that looks later. The page `ponko package --web` writes (and the
engine's `web/index.html`) shows a **Save the crash report** link when one
arrives, which saves the text as a file. A page of your own can do the same,
or send it:

```js
window.addEventListener("ponkocrashreport", (event) => {
  const { name, text } = event.detail;
  // Show your own message, offer `text` as a download, or send it.
});
```

The report comes before the page's own failure message (`onPonkoFailure`,
see [Web builds](web.md)), for the same panic or failure: the page shows
the short message for the player, and the report is the longer version, for
the developer.

## Limits

- Rust only for now: the C and C++ API has no crash report calls. A C or
  C++ game gets the default (reports in a release build of the engine's
  library, for a game with a window and an app id), without a hook or a
  game version.
- A crash that is not a panic (a segmentation fault in a C library, the GPU
  driver killing the process, running out of memory) writes no report.
- In a browser, the log isn't kept, and a backtrace has no frames (wasm has
  no backtraces); the message and location are there.
- A Windows game built for the GUI subsystem (`windows_subsystem = "windows"`)
  has no standard output to keep unless its launcher gives it one.
