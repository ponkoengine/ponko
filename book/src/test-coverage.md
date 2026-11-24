# Workspace test coverage

Coverage records which Rust source lines the tests execute. It includes the
headless and offscreen GPU tests in the default workspace suite and the
doctests. Run it on a machine where the offscreen tests can acquire a GPU
adapter, and check the test log for adapter skips before recording a baseline.

```sh
rustup component add llvm-tools-preview --toolchain nightly
cargo install --locked cargo-llvm-cov --version 0.9.2
cargo +nightly llvm-cov --workspace --doctests --json --output-path target/coverage.json
python3 scripts/coverage-report.py target/coverage.json
```

The Rust compiler guide explains the instrumentation. The cargo-llvm-cov
documentation describes nightly doctest support. [Rust compiler guide](https://doc.rust-lang.org/rustc/instrument-coverage.html) · [cargo-llvm-cov documentation](https://github.com/taiki-e/cargo-llvm-cov)

The table groups mapped Rust lines by crate and top-level source module.
It excludes integration tests, examples and separate `*_test.rs` files.
Inline tests can contribute lines when LLVM maps them in the source file.
The total divides executed lines by mapped lines across the workspace.
Source compiled out on the host or by the selected features is absent from
the denominator. Record the revision, toolchain, OS and GPU adapter with
each baseline. Keep the raw export in `target/`.

## Public functions

Build rustdoc JSON with the default features for the workspace's Rust
libraries, then pass those files alongside the coverage export.

```sh
for crate in ponko ponko_aseprite ponko_ecs ponko_ffi ponko_image_diff ponko_lobby ponko_mcp ponko_unity; do
    cargo +nightly rustdoc --lib -p "$crate" -- -Z unstable-options --output-format json || exit
done
python3 scripts/coverage-report.py target/coverage.json --rustdoc target/doc/ponko*.json
```

The inventory includes public free functions, public inherent methods and
public trait defaults with bodies. It uses rustdoc's public paths and LLVM's
function counts. Any executed generic instantiation counts as evidence for
that source function. An **unexecuted** body has a mapped entry with zero
executions. An **unmapped** body has no matching entry. That can happen when
a generic function has no compiled instantiation or the source is emitted
by a macro. Inspect the source and any platform-specific uses before using
either result in an API stability decision.
