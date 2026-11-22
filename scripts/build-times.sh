#!/usr/bin/env bash
# Measures a game project's build times and binary size, the way the guide's
# "Build times and download sizes" (book/src/performance.md) was measured.
#
#   ponko new /tmp/bt --path . && scripts/build-times.sh /tmp/bt          # debug
#   scripts/build-times.sh /tmp/bt release 3                               # release, 3 clean builds
#
# For each run: delete the project's target folder, wait 20 s for the
# machine to settle, build clean (with --timings: the report of the last run
# is kept as build-times-<profile>.html in the project). Then four
# one-line changes in src/lib.rs (a constant), four in src/main.rs (a
# comment), and four link times: rustc's `link_binary` pass from
# `-Z time-passes` (RUSTC_BOOTSTRAP=1 allows it on stable for that one
# crate). Prints one line per measurement with the 1-minute load average,
# then the binary size, stripped size and target folder size, and deletes
# the target folder. The project's own files are put back as they were.
#
# Times are cargo's own ("Finished ... in"). Run nothing else meanwhile, and
# report the median and range.
set -euo pipefail

dir=${1:?usage: build-times.sh <project> [dev|release] [clean runs]}
profile=${2:-dev}
runs=${3:-3}
cd "$dir"
name=$(grep -m1 '^name = ' Cargo.toml | cut -d'"' -f2)
case "$profile" in
  dev) flags=(); out=debug ;;
  release) flags=(--release); out=release ;;
  *) echo "profile: dev or release" >&2; exit 2 ;;
esac

load() { uptime | sed 's/.*averages*: *//' | tr -s ', ' ' ' | cut -d' ' -f1; }
finished() { grep 'Finished' | sed 's/.* in //'; }
cp src/lib.rs src/lib.rs.build-times
cp src/main.rs src/main.rs.build-times
restore() {
  mv src/lib.rs.build-times src/lib.rs
  mv src/main.rs.build-times src/main.rs
}
trap restore EXIT

cargo fetch -q
for i in $(seq 1 "$runs"); do
  rm -rf target
  sleep 20
  l=$(load)
  t=$(cargo build ${flags[@]+"${flags[@]}"} --timings 2>&1 | finished)
  echo "clean-$out run$i $t load=$l"
done
cp "$(ls -t target/cargo-timings/*.html | head -1)" "build-times-$out.html"

printf '\npub const BUILD_TIMES_EDIT: u32 = 0;\n' >> src/lib.rs
cargo build ${flags[@]+"${flags[@]}"} -q
for i in 1 2 3 4; do
  perl -pi -e "s/BUILD_TIMES_EDIT: u32 = \d+/BUILD_TIMES_EDIT: u32 = $i/" src/lib.rs
  sleep 5
  l=$(load)
  echo "edit-lib-$out run$i $(cargo build ${flags[@]+"${flags[@]}"} 2>&1 | finished) load=$l"
done
for i in 1 2 3 4; do
  echo "// $i" >> src/main.rs
  sleep 5
  l=$(load)
  echo "edit-main-$out run$i $(cargo build ${flags[@]+"${flags[@]}"} 2>&1 | finished) load=$l"
done
for i in 1 2 3 4; do
  echo "// link $i" >> src/main.rs
  sleep 5
  t=$(RUSTC_BOOTSTRAP=1 cargo rustc --bin "$name" ${flags[@]+"${flags[@]}"} -- -Z time-passes 2>&1 |
    grep -E 'time: +[0-9.]+; .*link_binary' | sed 's/.*time: *\([0-9.]*\);.*/\1/')
  echo "link-$out run$i ${t}s"
done

bin=target/$out/$name
[ -f "$bin" ] || bin=$bin.exe
cp "$bin" target/stripped
strip target/stripped 2>/dev/null || true
echo "binary-$out $(wc -c < "$bin" | tr -d ' ') bytes, stripped $(wc -c < target/stripped | tr -d ' ') bytes"
echo "target folder $(du -sh target | cut -f1)"
rm -rf target
