### Added

- `scripts/check-unsafe.py` holds every unsafe item outside the ECS to the test
  that runs it: `scripts/unsafe-inventory.txt` lists each item of the engine and
  the tools with its test and whether that test runs under Miri (or why not),
  and the C API crate's helpers are matched to a tested C function that calls
  them. It fails for unsafe code without a test and prints the line to add;
  `--list` shows every item with its test, `--miri` the Miri commands.
- The engine's unsafe code runs under Miri: the counting allocator, the
  threaded sprite and skeleton draw-list fills, components read and written as
  bytes (reflection, world hashes, C events), hierarchies, the retained 3D
  frame's parallel compare and `Ui` across threads. New tests cover what had
  none: `the_counting_allocator_forwards_every_call_to_the_system_allocator`,
  `many_sprites_are_queued_once_each_from_many_threads` and `tests/ui_threads.rs`.
- The C and C++ test programs build with AddressSanitizer and
  UndefinedBehaviorSanitizer (`-DPONKO_SANITIZE=ON` in `examples/cpp`), and the
  `ponko_ffi` tests run with AddressSanitizer; the commands are in
  `CONTRIBUTING.md`, "Unsafe code and sanitizers".
- Every C function says who owns each pointer it takes or returns: the rules at
  the top of `ponko.h` ("Pointers and who owns them") cover most by type, and
  returned pointers, `T **out` parameters, callback user data and objects a
  function takes over have an `Ownership:` paragraph in the API docs.
  `scripts/check-ffi-ownership.py` fails for a function that leaves one unsaid
  and says what to write.
