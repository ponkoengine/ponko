# Retained native ABI baseline (version 2)

What the C ABI looked like at a recorded engine revision, kept so that later
revisions are checked against something they did not generate themselves.
[STABILITY.md](../../../../../STABILITY.md#c-abi) says that within an ABI version
existing functions and struct layouts never change, and that releases only add
functions and enum values.

| File | What it is |
|---|---|
| `ponko_abi2.h` | `include/ponko.h` at the baseline revision (see `provenance.json` for the one mechanical rename) |
| `abi_v2_layout.json` | Measured by a C compiler reading that header: size, alignment and field offsets of every struct, every enum constant, every numeric `#define`, the ABI version |
| `layout_table.rs` | Generated; names the Rust types for the test to read their layout. It holds no numbers |
| `abi_v2.provenance.json` | Revision, compiler, toolchain, command, date |
| `generate_baseline.py` | Produces the layout file and the table from a header |

The baseline comes from the engine's `main` branch, **not from a published
release**, and no released native artifact exists to compile an old client
against. It shows that the current header and library honor what that revision
promised; it does not show that any earlier binary release is covered.

## What checks it

- `crates/ponko_ffi/tests/abi_compat.rs`
  - Rust struct sizes, alignments and field offsets, enum constants and
    `#define` values equal what the C compiler measured (64-bit desktop
    targets; the same numbers hold on macOS, Linux and Windows).
  - `include/ponko.h` still declares everything the baseline header declares:
    functions (ignoring parameter names), struct definitions, typedefs, enum
    constants with their values, `#define`s. New functions, types and enum
    constants pass; a removed or changed function, a changed struct, or a
    shifted enum value fails and names the item.
  - `ponko_abi_version()` still equals the baseline's version, the number
    programs check at startup.
- `examples/cpp/abi_baseline.c` (the native check, `ctest -R c_abi_baseline`): a C
  client compiled against `ponko_abi2.h` only, linked to the current library.
  It checks the startup version, a query over `PonkoChunk`, a built-in
  component in `PonkoTransform2D`, structs passed and returned by value, and a
  headless game run. It fails if a layout, a signature or a status code the
  baseline header promised no longer matches the library.

Limits, stated plainly: the layout check covers 64-bit targets only; the C
client covers a small slice of the ~620 functions (the header comparison covers
all of their signatures); calling-convention changes on a platform that the list of checks does
not build are not seen.

## Rules

- **Never edit or regenerate the baseline** to make a test pass. A failure means
  programs built against the old header can break: fix the library, or make an
  explicit compatibility decision (STABILITY.md: an ABI bump, in a minor
  release while at 0.x).
- A new function, type or enum constant needs no change here.

## Retaining a new baseline (after an intentional ABI bump)

1. Check out the revision to record, clean.
2. Copy `include/ponko.h` to `ponko_abi<N>.h` here, then
   `python3 generate_baseline.py ponko_abi<N>.h <out-dir> <N>`. It writes
   `abi_v<N>_layout.json` and `layout_table.rs` and refuses to overwrite files, so
   write them to a scratch directory and move them in under versioned names
   (and rename the macro/file in `abi_compat.rs` for the new version).
3. Write `abi_v<N>.provenance.json` like the version-2 one.
4. Keep the version-2 files and tests: old programs still exist.

## Why the version stays 2

The `crabby_*` compatibility exports were removed before 0.1.0. They were a
rename shim over the `ponko_*` functions, and no native artifact was ever
released with them, so no client binary can depend on them. Every `ponko_*`
symbol, struct layout and numeric value is unchanged, so the first released ABI
is version 2 and the baseline stays valid. Bump the version only for a change
that breaks a program built against a released header.
