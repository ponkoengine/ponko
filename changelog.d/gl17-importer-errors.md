### Added
- `ponko_unity::Error` can be matched on: `kind()` returns an
  `ErrorKind` (`Io`, `Decode`, `Unsupported`, `Invalid`, `Other`;
  non-exhaustive), `path()` the file, `line()` the line of a text file and
  `message()` the cause alone. The `std::io::Error` of a failed read or
  write is kept as the error's `source()`. Every error the importer makes
  has a kind. The guide's Unity chapter lists what each kind means.

### Breaking changes
- `ponko_unity::Error`'s public `message` field is the `message()` method now.
  The error keeps its text, path, line and kind private so more can be added
  without breaking callers; `Error` is still `Clone`, `Eq`, `Display` and
  `std::error::Error`.
  Instead: call `error.message()` for the cause alone (it no longer holds the
  file name or `line N:` prefix), `error.to_string()` for the whole text, and
  match on `error.kind()`, `error.path()` and `error.line()`.
  Before:
  ```rust
  if err.message.contains("no Assets folder") { /* ... */ }
  ```
  After:
  ```rust
  # let err: ponko_unity::Error = todo!();
  if err.kind() == ponko_unity::ErrorKind::Invalid { /* ... */ }
  ```
