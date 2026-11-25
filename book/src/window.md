# Window service

> **Maturity: [proven](maturity.md).** Game: Coin Dash and Ember Hop, packaged
> and played on Linux, macOS and Windows, in a window. Tests:
> `tests/window_service.rs`. Platforms: Linux, macOS and Windows, headless
> (the workspace tests in the list of checks); the packaged games also open a
> real window on each. Graphics: the packaged games run in a window on Linux
> (software Vulkan under Xvfb), macOS and Windows (DirectX 12, WARP when there
> is no GPU); Coin Dash also runs in headless Chromium and WebKit on WebGL2.

`Game::new("Play", 800, 600).run(...)` opens the ordinary window; nothing on this
page is needed for that. When a game wants an options menu (fullscreen, a mode
list, a custom cursor) or the clipboard, it calls `ctx.window`. Every call works
in headless runs and in builds without the `window` feature too: there it does
nothing, reports a default state and no monitors, and never opens a display.

## Fullscreen, size and title

<div class="lang-pair">
<div>

```rust
use ponko::window::{CursorGrab, CursorIcon, Cursor, WindowMode};

ctx.window.set_title("Play")?;
ctx.window.set_size(Vec2::new(960.0, 540.0))?; // logical pixels
// Borderless on the monitor the window is on:
ctx.window.set_mode(WindowMode::Borderless(None))?;
// Exclusive fullscreen: pick one of the modes the monitor advertises.
if let Some(monitor) = ctx.window.current_monitor() {
    if let Some(best) = ctx.window.exclusive_modes(monitor)?.last().cloned() {
        ctx.window.set_mode(WindowMode::Exclusive(monitor, best))?;
    }
}
ctx.window.set_cursor(Cursor::System(CursorIcon::Pointer))?;
ctx.window.set_cursor_grab(CursorGrab::Confined)?;
ctx.window.save_settings(&mut ctx.storage)?; // remembered for the next start
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/window.cpp:window-options}}
```

</div>
</div>

The calls, all on `ctx.window`:

| Call | What it does |
|---|---|
| `set_mode(WindowMode)` | `Windowed`, `Borderless(Some(monitor) or None)`, or `Exclusive(monitor, VideoMode)` |
| `mode()`, `current_monitor()` | How the window is shown now, and the index of the monitor it is on |
| `monitors()`, `exclusive_modes(monitor)` | The connected monitors and the exclusive modes one of them advertises |
| `set_title`, `set_size`, `set_position` | Title; logical client size (`Vec2`); physical desktop position (`IVec2`, can be negative) |
| `set_resize_limits(min, max)` | Smallest and largest logical client size; `None` removes a limit |
| `set_always_on_top`, `set_decorations` | Keep above other windows; show or hide the title bar and borders |
| `set_icon(Some(WindowImage) or None)` | The window icon from RGBA8 pixels, or the default |
| `set_cursor`, `set_cursor_visible`, `set_cursor_grab` | See [Cursor](#cursor) |
| `clipboard_read`, `clipboard_write` | See [Clipboard](#clipboard) |
| `settings()`, `state()` | What the game asked for, and what the OS reports |
| `save_settings`, `restore_settings` | See [Remember the player's choice](#remember-the-players-choice) |
| `close_requested`, `cancel_close` | See [Events](#events) |
| `request(WindowRequest)` | The low-level form of all the setters, for tools and tests |

A call that returns `Ok` was handed to the OS; the OS (or the compositor, or the
browser) may still adjust it. `state()` says what actually happened: the
physical client size, the DPI scale, focus, minimized, the mode and the monitor.
`settings()` keeps what the game asked for, which is what gets saved.

A monitor (`Monitor`) has a name if the OS gives one, a physical origin and
size (`IVec2`, `UVec2`), a scale factor, the current refresh rate in millihertz
and its exclusive modes (`VideoMode`: physical width and height, refresh in
millihertz, bit depth). The list is read from the OS on each call, so it follows
monitors being plugged in and out. A monitor index is only good until that
happens: don't store it, store the mode and check it again (the saved settings
do).

Errors are the engine's `Error` with a kind and a message that says what to do:

| Kind | When |
|---|---|
| `ErrorKind::Invalid` | A size that is not positive and finite, a minimum larger than the maximum, an image whose pixel count is not `width * height * 4`, a cursor hotspot outside its image. Nothing was changed. |
| `ErrorKind::NotFound` | The monitor is disconnected, or the exclusive mode is no longer advertised. Read the list again. |
| `ErrorKind::Unsupported` | The platform can't do it: the web (see [the web table](#on-the-web)), no clipboard, a cursor lock the OS refuses. |

## Remember the player's choice

`ctx.window.save_settings(&mut ctx.storage)` writes the requested settings
(mode, size, position, limits, title, flags) to `window.json` in the game's
player storage (the [storage](storage.md) folder, or browser storage on the
web). `ctx.window.restore_settings(&ctx.storage)` at setup puts them back. If
there is no saved file it does nothing. A stored exclusive mode or monitor that
is gone (another machine, a monitor unplugged) is not an error: the window
opens windowed and the reason is printed, so a game never starts on a black
screen. The engine never writes the file by itself.

```rust
# use ponko::prelude::*;
# let dir = std::env::temp_dir().join(format!("ponko-book-window-{}", std::process::id()));
Game::new("Play", 800, 600)
#   .data_dir(&dir)
    .run_headless(0, |ctx| {
        ctx.window.restore_settings(&ctx.storage).expect("window.json is readable");
    });
# let _ = std::fs::remove_dir_all(dir);
```

`restore_settings` changes a window that is already open, so for a moment the
player sees it at its default size and place. To open the first window with
the saved settings instead, turn on `Game::restore_window_settings(true)`:
the engine reads `window.json` before it creates the window and creates it at
the saved size and position, in the saved mode, with the saved limits and
flags. Nothing is moved or resized after it appears.

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
# let dir = std::env::temp_dir().join(format!("ponko-book-window-start-{}", std::process::id()));
Game::new("Play", 800, 600)
#   .data_dir(&dir)
    .app_id("play")
    .restore_window_settings(true)
    .run_headless(0, |ctx| {
        // Already the saved settings here; save them again when they change.
        ctx.window.save_settings(&mut ctx.storage).expect("storage is writable");
    });
# let _ = std::fs::remove_dir_all(dir);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:restore_window}}
```

</div>
</div>

- The saved choice wins over `Game::fullscreen` and the size given to
  `Game::new`. The title stays the game's: it belongs to the game, not the
  player.
- No saved file, or no `app_id`: the window opens with the game's settings.
  A file that can't be read or holds an invalid size is ignored and the reason
  is printed; a broken file never stops the game from starting.
- A saved monitor or exclusive mode that is gone opens windowed at the saved
  size, and the reason is printed.
- A headless run applies the same settings to `ctx.window` and the window size
  before setup, which is how the tests check them
  (`restore_window_settings_starts_with_the_saved_choice_before_setup` in
  `tests/window_service.rs`).
- The web reads its storage after the canvas exists, so there it restores
  just before setup (the canvas has no default window to show first).
- C turns it on with `ponko_game_restore_window_settings(game, true)` before
  the run, C++ with `game::restore_window_settings()`.

## Cursor

`set_cursor(Cursor::System(CursorIcon::Text))` picks a system cursor: the CSS
set, from `Default`, `Pointer`, `Text` and `Crosshair` to `Grab`, `Grabbing`,
`NotAllowed`, the resize arrows and `ZoomIn`/`ZoomOut` (`CursorIcon::ALL` lists
them). `set_cursor(Cursor::Custom(WindowImage::new(w, h, rgba), UVec2::new(x,
y)))` uses your own RGBA8 pixels with the hotspot at `(x, y)` in image pixels;
it must lie inside the image, and each side can be at most 65535. Custom cursors
change at the next frame.

`set_cursor_visible(false)` hides the cursor without capturing it.
`set_cursor_grab(CursorGrab::Confined)` keeps it inside the window,
`CursorGrab::Locked` locks it for relative motion (mouse look; where the OS can
only confine, it confines), `CursorGrab::None` lets it go. The existing
`ctx.input.set_cursor_locked(...)` keeps working as before, hiding the cursor
and locking again when the window gets focus back; if both are used in the same
frame, the input lock is applied last.

## Clipboard

`ctx.window.clipboard_write("text")` and `ctx.window.clipboard_read()` exchange
UTF-8 text. On the desktop they use the system clipboard (through `arboard`,
only in builds with `window`); `clipboard_read` returns `Ok(None)` when there is
no text. A focused text input of the [retained UI](ui.md) handles Ctrl/Cmd+C, X
and V with the same clipboard: copy takes the selection, cut deletes it only
after the write succeeded, paste replaces the selection and drops control
characters like typed text. During IME composition the shortcuts belong to the
IME. `Ui::selected_text()` and `cut_selection()` are there for your own
shortcuts.

On the web, reading asks the browser and returns `Ok(None)` until the answer
comes: call it again on later frames. A refused permission arrives as an
`ErrorKind::Unsupported` error from a later read, and so does a failed write.
Use the clipboard from a click or key press; nothing asks for permission at
startup.

## Events

`ctx.events.read::<WindowEvent>()` gives this frame's window events:
`Focused(bool)` and `Minimized(bool)` (each only when the state changes),
`CloseRequested`, `FileDropped(path)` and `ScaleFactorChanged(scale)`. A dropped
path comes from the OS: check it before loading it.

When the player closes the window, `ctx.window.close_requested()` is true for
the rest of the frame and the game stops after its systems ran, unless a system
calls `ctx.window.cancel_close()`, for example to ask "Quit without saving?".
`ctx.quit()` always quits.

<div class="lang-pair">
<div>

```rust
use ponko::window::WindowEvent;
# let unsaved = true;
for event in ctx.events.read::<WindowEvent>() {
    if let WindowEvent::FileDropped(path) = event {
        println!("open {}", path.display());
    }
}
if ctx.window.close_requested() && unsaved {
    ctx.window.cancel_close(); // show the dialog instead
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/window.cpp:window-events}}
```

</div>
</div>

## High DPI, pixel art and UI

Window sizes, mouse positions, 2D UI layout and the default camera use
**logical pixels** (top-left origin, y down). `state().physical_size`, monitor
sizes, positions and video modes are **physical pixels**: logical times
`state().scale_factor`. Cursor hotspots are image pixels. When the window moves
to a monitor with another scale, `ScaleFactorChanged` arrives.

- **UI**: keep sizes logical. Text is drawn at the physical density, so it stays
  sharp at 1.5× and 2× without any work.
- **Pixel art**: draw into a fixed-size render target with nearest sampling and
  scale it up by a whole number of physical pixels (letterbox the rest). Don't
  round the scale factor and treat logical pixels as physical ones; on a 1.5×
  display that gives uneven pixels. Recompute the scale on `ScaleFactorChanged`.

## On the web

| Call | Desktop | Web |
|---|---|---|
| `set_mode(Windowed)`, `set_mode(Borderless(None))` | Switches | Canvas fullscreen; needs a click or key press, and the browser may refuse |
| `set_mode(Borderless(Some(_)))`, `set_mode(Exclusive(..))` | Switches | `Unsupported`: no monitor choice, no exclusive modes |
| `set_size` | Logical client size | Canvas size; the page's CSS may override it |
| `set_title` | Title bar | Does nothing; the document title is left alone |
| `set_position`, `set_resize_limits`, `set_always_on_top`, `set_decorations`, `set_icon` | The OS may ignore a hint (macOS ignores window icons) | `Unsupported`: set these in the page instead (the icon is the favicon) |
| `request(Settings(..))` | Applies all | `Unsupported` if it has a monitor, exclusive mode or one of the desktop-only values above |
| `set_cursor`, `set_cursor_visible` | System or custom cursor | CSS cursor; browsers limit custom cursor sizes |
| `set_cursor_grab` | Confine or lock; OS refusals are errors | Lock needs a click; confine may be refused |
| `clipboard_read`, `clipboard_write` | At once | Asynchronous, see [Clipboard](#clipboard); needs HTTPS and permission |
| `monitors`, `exclusive_modes`, `state` | From the OS | What the browser exposes; no exclusive modes |
| `save_settings`, `restore_settings` | `window.json` in the data folder | Browser storage; restore drops desktop-only values |
| Events | All of them | Focus, scale and close where the browser reports them; dropped files have no file-system path |

## Testing

Tests never open a window. `WindowService::with_backend(backend)` installs any
`WindowBackend`, for example a fake that records requests and reports the
monitors a test needs; put it in `ctx.window` of a `Game::run_headless` run. A
fake can also stand in for a monitor that is gone, as in
`restart_with_the_monitor_gone_falls_back_to_windowed` in
`tests/window_service.rs`. `ctx.window.feed_event(&event)` returns whether the
event is a change and should be sent (that is how the engine sends focus and
minimized only on change).

## C and C++

C has a typed function for each call: `ponko_ctx_window_set_mode_ex` and
`ponko_ctx_window_mode_ex` with the versioned `PonkoWindowModeEx`;
`ponko_ctx_window_set_title`, `_set_size`, `_set_position`,
`_set_resize_limits`, `_set_always_on_top`, `_set_decorations`; the icon and
custom cursor from a `PonkoWindowImageEx`; `ponko_ctx_window_set_cursor_icon`
(a `PonkoCursorIcon`), `_set_cursor_visible` and `_set_cursor_grab`;
`ponko_ctx_window_monitor_count`, `_monitor_ex`, `_monitor_name`,
`_video_mode_count` and `_video_mode_ex`; `ponko_ctx_window_state_ex` and
`_properties_ex`; `ponko_ctx_window_event_count` and `_event_ex` (the dropped
path goes into a caller buffer); `_close_requested`, `_cancel_close`,
`_save_settings` and `_restore_settings`; `ponko_ctx_clipboard_write` and
`ponko_ctx_clipboard_read` (into a caller buffer, with an `available` flag).
The `*Ex` structs follow [versioned descriptors](c-cpp.md): set `struct_size`
and `version`, call the matching `_defaults`, then fill it. Text outputs use the
usual buffer rule: ask with capacity 0 for the length, then call again with
length + 1 bytes. A monitor or mode that is gone returns `PONKO_NOT_FOUND`, a
platform limit `PONKO_UNSUPPORTED`; `ponko_last_error()` has the message.

C++ wraps them on `ponko::ctx` as `window_set_mode(ponko::window_mode::borderless())`,
`window_mode()`, `window_state()`, `window_properties()`, `window_video_modes(i)`,
`window_events()`, `window_set_cursor(ponko::cursor_icon::text)`,
`clipboard_read()` (a `std::optional<std::string>`) and so on; see the
examples above.

## Needs a person

These need a real display, so the automated tests can't do them:

1. On each desktop OS, list the modes and go windowed → borderless → exclusive
   (an advertised mode) → windowed; check the monitor's real resolution and
   refresh, the saved choice after a restart, and that quitting restores the
   desktop.
2. Move and resize the window, toggle decorations and always-on-top, set an
   icon, then move it between two monitors with different scales; check the
   reported sizes and scale and that UI and pixel art stay sharp.
3. Try system and custom cursors, hide and show, confine, lock, switch away and
   back, then release; check the confinement edges and the old input lock.
4. Type non-Latin text with the OS IME; select, copy, cut and paste Unicode
   between a text input and another program (and clipboard ownership on Linux).
5. On HTTPS in each supported browser, use fullscreen, pointer lock and the
   clipboard from a click; allow and refuse permissions and check the pending
   and error results.
6. Unplug a second monitor while the game runs, then restart with the saved
   exclusive mode; the game starts windowed and says why.
7. Focus, minimize and restore, drop a file, close with and without a cancel;
   check the events and that cancelling keeps the game running while
   `ctx.quit()` exits.
8. With `Game::restore_window_settings(true)`, save a size, position and mode
   (windowed, borderless, exclusive), quit and start again; on each desktop
   OS the window must appear at the saved place and size, in the saved mode,
   without a default-sized window showing first.
