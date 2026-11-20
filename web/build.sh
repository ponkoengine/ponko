#!/usr/bin/env bash
# Builds a game for the web, ready to serve as a static site.
#
#   web/build.sh battle                  # an example -> target/web/battle/
#   web/build.sh --bin my_game           # your game's binary, run from its folder
#   web/build.sh battle site/play        # choose the output folder
#   web/build.sh --features 3d web_smoke_3d   # with Cargo features (comma-separated)
#   web/build.sh --threads battle        # with threads (nightly; see below)
#   python3 -m http.server -d target/web/battle 8000
#
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <the wasm-bindgen in Cargo.lock>
#
# --threads builds with worker threads (the guide's "Threads on the web"):
# atomics and shared memory, which need a nightly toolchain that rebuilds
# the standard library (rustup toolchain install nightly --component
# rust-src; PONKO_WEB_TOOLCHAIN picks another, such as a dated nightly). The
# folder gets ponko-worker.js. The page must be served with the headers
# Cross-Origin-Opener-Policy: same-origin and Cross-Origin-Embedder-Policy:
# require-corp for the threads to start; without them the game runs on one
# thread (python3 -m http.server sends neither).
set -euo pipefail

kind=example
features=
threads=
while [ $# -gt 0 ]; do
  case "$1" in
    --bin) kind=bin; shift ;;
    --features) features=${2:?--features needs a list}; shift 2 ;;
    --threads) threads=1; shift ;;
    *) break ;;
  esac
done
name=${1:?usage: build.sh [--bin] [--features <list>] <name> [out-dir]}
here=$(cd "$(dirname "$0")" && pwd)
target=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
out=${2:-$target/web/$name}

feature_args=()
if [ -n "$features" ]; then
  feature_args=(--features "$features")
fi
if [ -n "$threads" ]; then
  # Atomics in every crate, std rebuilt with them (-Zbuild-std), and a
  # shared, imported memory with the thread-local symbols wasm-bindgen
  # needs (the linker doesn't add these by itself).
  flags="-C target-feature=+atomics,+bulk-memory,+mutable-globals"
  flags="$flags -C link-arg=--shared-memory -C link-arg=--max-memory=1073741824 -C link-arg=--import-memory"
  flags="$flags -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size"
  flags="$flags -C link-arg=--export=__tls_align -C link-arg=--export=__tls_base"
  CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="$flags ${CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS:-}" \
    cargo "+${PONKO_WEB_TOOLCHAIN:-nightly}" build -Zbuild-std=panic_abort,std \
    --release --target wasm32-unknown-unknown "--$kind" "$name" ${feature_args[@]+"${feature_args[@]}"}
else
  cargo build --release --target wasm32-unknown-unknown "--$kind" "$name" ${feature_args[@]+"${feature_args[@]}"}
fi
if [ "$kind" = example ]; then
  wasm="$target/wasm32-unknown-unknown/release/examples/$name.wasm"
else
  wasm="$target/wasm32-unknown-unknown/release/$name.wasm"
fi

rm -rf "$out"
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --remove-name-section --remove-producers-section \
  --out-dir "$out" --out-name game "$wasm"

# Assets, and the list of them that the engine fetches before setup.
if [ -d assets ]; then
  cp -R assets "$out/assets"
  (cd "$out/assets" && find . -type f ! -name index.json ! -name '.*' | sed 's|^\./||' | sort |
    python3 -c 'import json, sys; print(json.dumps([l.strip() for l in sys.stdin]))' > index.json)
fi

sed "s/{{TITLE}}/$name/" "$here/index.html" > "$out/index.html"
if [ -n "$threads" ]; then
  cp "$here/ponko-worker.js" "$out/ponko-worker.js"
fi
echo "built $out ($(du -sh "$out" | cut -f1)); try it: python3 -m http.server -d $out 8000"
