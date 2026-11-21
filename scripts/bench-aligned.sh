#!/usr/bin/env bash
# Runs cargo with every function of the build aligned to 128 bytes, so two
# builds compared by a benchmark lay out their code the same way:
#
#   scripts/bench-aligned.sh build --release -p ponko_bench
#   scripts/bench-aligned.sh run --release -p ponko_bench -- 'ecs/'
#
# Without it, a change to one function moves every function after it in the
# binary, and where a hot loop falls against cache lines and fetch blocks can
# change its speed by a few percent, in either direction, with no change to
# its code. With it, every function compiled here starts on a 128-byte
# boundary (a cache line on Apple chips, two on x86), so only the changed
# functions themselves move. The standard library is prebuilt and stays as
# it is.
#
# The flags are appended to RUSTFLAGS (which replaces any `build.rustflags`
# from a cargo config). Changing RUSTFLAGS rebuilds every crate, so keep a copy
# of each side's binary instead of switching back and forth.
# PONKO_BENCH_CODE_ALIGN is recorded as `env.code_align` in ponko_bench's
# JSON, and `bench_track compare` warns when two sides differ in it.
set -euo pipefail
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C llvm-args=-align-all-functions=7"
export PONKO_BENCH_CODE_ALIGN=128
exec cargo "$@"
