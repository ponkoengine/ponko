#!/usr/bin/env bash
# Makes the reference validation bundles and checks that the tool behaves:
# the deliberately broken game fails, the corrected one passes, a launched
# game observes what the in-process run observes, and two runs of one setup
# compare as the same.
#
#   scripts/validation-bundles.sh [OUT_DIR]     (default: target/validation-bundles)
#
# Headless: no window is opened. See book/src/validation-bundles.md.
set -euo pipefail

out="${1:-target/validation-bundles}"
rm -rf "$out"
mkdir -p "$out"

# One build; every run below starts the same binary.
cargo build --example validation_bundle
tool() { cargo run --quiet --example validation_bundle -- "$@"; }

# Runs the tool and requires its exit code to be exactly $1.
expect() {
  local want="$1" got=0
  shift
  tool "$@" || got=$?
  if [ "$got" != "$want" ]; then
    echo "validation-bundles: expected exit $want, got $got: $*" >&2
    exit 1
  fi
}

# The deliberate failure and the corrected run: in this process, then as a
# launched game driven through the MCP tools.
expect 1 --out "$out/in-process-buggy" --variant buggy --redact-paths
expect 0 --out "$out/in-process-fixed" --variant fixed --redact-paths --perf
expect 1 --out "$out/launched-buggy" --variant buggy --launch --redact-paths
expect 0 --out "$out/launched-fixed" --variant fixed --launch --redact-paths
expect 0 --out "$out/launched-fixed-again" --variant fixed --launch --redact-paths

# Every bundle's files are the ones its manifest lists.
for bundle in "$out"/*/; do
  expect 0 verify "$bundle"
done

# The launched game saw what the in-process run saw.
for pair in buggy fixed; do
  cmp "$out/in-process-$pair/trace.csv" "$out/launched-$pair/trace.csv"
  cmp "$out/in-process-$pair/logs/run.log" "$out/launched-$pair/logs/run.log"
done

# Repeated runs of one setup agree; the broken and the corrected run don't,
# and the comparison says how.
expect 0 compare "$out/launched-fixed" "$out/launched-fixed-again"
expect 1 compare "$out/launched-buggy" "$out/launched-fixed" > "$out/buggy-vs-fixed.txt"
expect 1 compare "$out/launched-buggy" "$out/launched-fixed" --json > "$out/buggy-vs-fixed.json"
cat "$out/buggy-vs-fixed.txt"
grep -q 'settings.variant: "buggy" -> "fixed"' "$out/buggy-vs-fixed.txt"
grep -q 'result a: failed (assertion: one-jump-per-press' "$out/buggy-vs-fixed.txt"

# Nothing of this machine is in what gets published: not the home
# directory, the temporary directory, a scratch directory or the checkout.
bash scripts/validation-bundle-guard.sh "$out"
echo "validation-bundles: ok, in $out"
