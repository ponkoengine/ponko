### Added
- `ctx.window`: switch windowed / borderless / exclusive fullscreen at runtime (`set_mode`, `mode`, `current_monitor`, `monitors`, `exclusive_modes`), set the title, size, position, resize limits, icon, always-on-top and decorations, and remember the choice with `save_settings` / `restore_settings` (a monitor or mode that is gone falls back to windowed with a printed reason).
- Cursors: the system set as `CursorIcon`, custom RGBA cursors, hide, confine and lock (`set_cursor`, `set_cursor_visible`, `set_cursor_grab`); `ctx.input.set_cursor_locked` keeps working.
- Clipboard text on desktop and web (`clipboard_read`, `clipboard_write`), used by the retained UI's text input for copy, cut and paste.
- Window events through `ctx.events`: `WindowEvent::Focused` and `Minimized` (sent only when the state changes), `CloseRequested` (cancel with `ctx.window.cancel_close()`), `FileDropped`, `ScaleFactorChanged`.
- Headless and no-`window` builds keep the API and do nothing; `WindowService::with_backend` takes a fake backend for tests. Errors are `ponko::error::Error` (`Invalid`, `NotFound`, `Unsupported`).
- C and C++: typed window, monitor, cursor, clipboard and event calls with versioned `PonkoWindow*Ex` descriptors (`ponko_ctx_window_set_mode_ex`, `ponko_ctx_window_monitor_ex`, `ponko_ctx_clipboard_read`, ...), and `ponko::ctx::window_*` wrappers.
- Guide: the "Window service" chapter (direct calls, saving, high-DPI rules for pixel art and UI, what each call does on the web).
