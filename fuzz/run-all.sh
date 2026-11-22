#!/usr/bin/env bash
# Runs every fuzz target of the list (fuzz/targets.txt) for SECONDS seconds
# each, or for the seconds the list gives it when SECONDS is left out,
# starting from fuzz/corpus/<target> (fill it with fuzz/seed-corpus.sh first), and
# lists what they found in fuzz/findings.tsv: target, kind (crash,
# timeout, oom, leak, slow-unit) and the input, one per line. Each
# target's output goes to fuzz/logs/<target>.log. Exits 1 when anything
# was found. Needs nightly Rust and cargo-fuzz (fuzz/README.md).
#
#   fuzz/run-all.sh                # every target, as long as the list says
#   fuzz/run-all.sh 180            # every target, three minutes each
#   fuzz/run-all.sh 600 gltf scene # just these two
set -uo pipefail
cd "$(dirname "$0")" || exit 1

seconds=${1:-}
shift || true
# Seconds the list gives a target (60 for one it doesn't name).
listed_seconds() {
    awk -v t="$1" '$1 == t { print $2; found = 1 } END { if (!found) print 60 }' targets.txt
}
targets=()
for target in "$@"; do
    targets+=("$target")
done
if [ $# -eq 0 ]; then
    while IFS= read -r target; do
        targets+=("$target")
    done < <(awk '!/^[[:space:]]*(#|$)/ { print $1 }' targets.txt)
fi

mkdir -p logs
: > findings.tsv
for target in "${targets[@]}"; do
    mkdir -p "corpus/$target"
    secs=${seconds:-$(listed_seconds "$target")}
    echo "fuzzing $target for ${secs}s"
    # -rss_limit_mb: an input that makes a loader use 2 GB is a finding.
    # -timeout: so is one that takes 10 s. Leaks of one input are not
    # (statics, caches); memory that grows across inputs hits the RSS limit.
    cargo +nightly fuzz run "$target" "corpus/$target" -- \
        -max_total_time="$secs" -rss_limit_mb=2048 -timeout=10 \
        -detect_leaks=0 -print_final_stats=1 >"logs/$target.log" 2>&1
    status=$?
    found=0
    for input in "artifacts/$target"/*; do
        [ -f "$input" ] || continue
        name=$(basename "$input")
        printf '%s\t%s\t%s\n' "$target" "${name%%-*}" "fuzz/$input" >>findings.tsv
        found=1
    done
    if [ "$status" -ne 0 ] && [ "$found" -eq 0 ]; then
        # Failed without an input: a build or harness error.
        printf '%s\t%s\t%s\n' "$target" "error" "fuzz/logs/$target.log" >>findings.tsv
    fi
    tail -n 3 "logs/$target.log"
done

if [ -s findings.tsv ]; then
    echo "findings:"
    cat findings.tsv
    exit 1
fi
echo "no findings"
