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

- **A crash that is not a panic**: a segmentation fault or an `abort()` in
  a native library, an access violation on Windows. Its report is finished
  at the game's next start ([below](#crashes-that-are-not-panics)).

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
| `kind` | `panic`, `error` or `native` (a crash that is not a panic, see below). |
| `signal` | For a `native` crash only: the signal or the Windows exception (`SIGSEGV`, `SIGABRT`, `EXCEPTION_ACCESS_VIOLATION`). |
| `message` | The panic's message, or the error's; for a `native` crash, the signal, what it means and, for a memory fault, the address it touched. |
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
`engine_version`, `os`, `gpu`, `settings`, `backtrace`, `log`, `file` (its
name in `ctx.storage`) and `signal`.

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

## Crashes that are not panics

A segmentation fault in a C library, an `abort()` in a GPU driver or an
access violation on Windows stops the game without a panic. With reports on,
these leave a report too, of kind `native`, on Linux, macOS and Windows. It
says which signal (or Windows exception) stopped the game, when, the game's
and the engine's versions, the system, the GPU and the settings of the last
change, and the last lines the game printed. It has no backtrace, no thread
name and no panic location.

The report is finished at the **next start** of the game: `saved_reports`
lists it, and the game's `on_crash` hook runs with it, early in that start,
with `report.kind()` being `CrashKind::Native`. That is where a game shows
"the game stopped last time" or offers to send it.

### How it is written (the design)

A crashing process can't be trusted. When the signal arrives, the thread
that crashed may hold the allocator's lock, the lock of standard output or
any of the engine's own locks, and the memory near the crash may be
damaged. Code that runs then may only do what is safe at any instruction:

- **Linux and macOS (a signal handler).** Only async-signal-safe calls:
  `write` to a file descriptor, `clock_gettime`, `sigaction`, `raise`. No
  allocation (`malloc` may hold its lock), no locks (`Mutex`, `println!`,
  `Backtrace`), no formatting machinery (`format!` allocates), nothing that
  loads symbols. The handler reads only memory that was set up for it, with
  atomic loads.
- **Windows (an unhandled exception filter, and the C runtime's `SIGABRT`
  handler).** The same rules: the filter runs on the thread that faulted, the
  process heap may be locked, and the loader lock may be held. Only
  `WriteFile` to a handle opened earlier, `GetSystemTimeAsFileTime`, and
  plain reads of prepared memory.

So everything is **prepared ahead of time**, while the game runs normally:

1. When a run with reports on starts, the engine opens a file of its own,
   `crash-reports/.native-<pid>` in the game's data folder, and keeps it open
   (and locked: `flock` on Unix, no sharing on Windows, so a second copy of
   the game running at the same time leaves it alone).
2. It writes the report's fixed part as text into a buffer in static memory:
   the game, the versions, the system, the start time, and later the GPU
   once the renderer opened one, and the settings each time they change (not
   each frame: only when a value changed). The buffer is two halves of
   atomic bytes; the engine fills the idle half and then switches, so the
   handler always reads a whole one.
3. Every line the game prints also goes into a ring of the last 16 KiB of
   output in static memory, as atomic bytes, next to the log the control
   endpoint reads.
4. It installs the handlers for `SIGSEGV`, `SIGBUS`, `SIGILL`, `SIGFPE` and
   `SIGABRT` (Unix, on the alternate signal stack where the thread has one,
   so a stack overflow still gets one), or the unhandled exception filter and
   a `SIGABRT` handler (Windows), and remembers the ones that were there.

When the game crashes, the handler only copies bytes: the prepared fixed
part, the signal's name and description from a constant table, the signal's
number, the address for a memory fault and the time, as digits it writes
itself into a small buffer on the stack, then the ring of output. It writes
them to the open file with `write` (or `WriteFile`). Only the first crash of
a run writes; a second one (the `abort` after a stack overflow message, a
second thread) doesn't. Then it puts back the handler that was there before
and hands the crash on: Rust's own stack overflow message, a handler of
another library, or the system's default, so the process still ends as a
crash, with its exit status and the system's own crash report.

**At the next start**, the engine looks in `crash-reports/` for files left
by earlier runs that no running copy of the game holds: an empty one was a
run that ended without crashing (it is deleted), a full one is a crash. It
turns that into an ordinary report: the times become dates, the paths in the
log are made relative (as above), the output is cut to the last 100 whole
lines, and the report is saved as `crash-reports/<date>_<time>.txt`, at the
time of the crash. A run that ends normally deletes its own file.

What this can't catch: `SIGKILL` and Windows' `TerminateProcess` (the system
killing the game for using too much memory, or the player ending it), a
crash in the first moments before the run started, and Windows' fail-fast
exits (`__fastfail`, which Rust's own `abort` uses), which skip every
handler. The lines printed in the last moment before the crash may be
missing when the engine's copy thread hadn't passed them on yet (the
handler gives it 50 ms). A library that installs its own handler after the
engine started comes first, and decides itself whether to hand a crash on.

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

## C and C++

A C or C++ game gets the same default (reports in a release build of the
engine's library, for a game with a window and an app id). The settings, the
game's version and the hook are one versioned descriptor,
`PonkoCrashReports`, given to `ponko_game_crash_reports` before the run; C++
wraps it as `game::crash_reports`. `mode` is `PONKO_CRASH_REPORTS_DEFAULT`
(the build's default), `_ON` or `_OFF`.

<div class="lang-pair">
<div>

```rust,no_run
# use ponko::prelude::*;
use ponko::crash::CrashReports;
# fn main() {
Game::new("Space Crab", 1280, 720)
    .app_id("space-crab")
    .crash_reports(CrashReports::default().game_version("1.4.2").on_crash(|report| {
        if let Some(path) = report.path() {
            eprintln!("Space Crab stopped. A report is in {}", path.display());
        }
    }))
    .run(|ctx| {
        for name in ponko::crash::saved_reports(&ctx.storage) {
            let text = ctx.storage.read(&name);
            // Ask the player, then send it with an HTTP client of your choice.
#           let _ = text;
            let _ = ctx.storage.delete(&name);
        }
    });
# }
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:crash_reports}}
```

</div>
</div>

The hook gets a `PonkoCrashReport`: the fields of the table above as
strings (`NULL` where a report has none: a `location` for an error, a `path`
without storage), `kind` (`PONKO_CRASH_PANIC`, `_ERROR` or `_NATIVE`),
`uptime_secs`, and `text`, the whole file. The report and its strings live
only during the call: copy what you keep. The hook may run on any thread,
inside the panic hook, so the same rules hold as for Rust's: quick, and it
must not unwind (C++'s wrapper catches what the function throws). `user` is
the game's own and is passed back to each call; it must stay valid for the
run.

`ponko_ctx_saved_crash_reports` lists the saved reports, newest first, one
name a line, to read with `ponko_ctx_storage_read`; C++'s
`ctx.saved_crash_reports()` splits them. The game's own fatal error is
`ponko_crash_report("why")` (C++ `ponko::report_crash`), which says
`PONKO_NOT_FOUND` (C++: returns `false`) while reports are off:

```cpp
{{#include ../../examples/cpp/guide.cpp:crash_report_own}}
```

## Limits

- A crash that is not a panic writes its report at the next start, without
  a backtrace (see [Crashes that are not panics](#crashes-that-are-not-panics));
  a process the system kills (`SIGKILL`, running out of memory) writes none.
- In a browser, the log isn't kept, and a backtrace has no frames (wasm has
  no backtraces); the message and location are there.
- A Windows game built for the GUI subsystem (`windows_subsystem = "windows"`)
  has no standard output to keep unless its launcher gives it one.
