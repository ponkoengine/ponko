#!/usr/bin/env bash
# Builds a game for the web, ready to serve as a static site.
#
#   web/build.sh battle                  # an example -> target/web/battle/
#   web/build.sh --bin my_game           # your game's binary, run from its folder
#   web/build.sh battle site/play        # choose the output folder
#   web/build.sh --features 3d web_smoke_3d   # with Cargo features (comma-separated)
#   web/build.sh --threads battle        # with threads (nightly; see below)
#   web/build.sh --threads --print-env   # what a threaded build would set, no build
#   web/build.sh --threads --clippy      # nightly clippy on the threaded configuration, no build
#   web/build.sh --threads --isolation-worker battle   # threads on hosts that can't send headers
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
#
# --isolation-worker (with --threads) adds ponko-isolate.js and its script
# tag to index.html: on a host that can't send those headers, a service
# worker adds them, at the cost of one reload on the first visit (the
# guide's "Hosts that can't send the headers").
#
# Cargo reads the first of CARGO_ENCODED_RUSTFLAGS, RUSTFLAGS and
# CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS that is set and ignores the
# others, so --threads adds its flags to that one and keeps what it holds:
# RUSTFLAGS="-D warnings" still fails a threaded build on a warning.
# --print-env prints the toolchain and that variable instead of building
# (web/smoke/build.test.mjs checks it).
#
# --clippy (with --threads) lints the engine and the threads fixture with
# the same toolchain and flags, with -D warnings: the nightly clippy has
# lints the stable one doesn't, and some code only compiles with atomics.
# It builds nothing to serve (the check list runs it as its own step).
set -euo pipefail

kind=example
features=
threads=
print_env=
clippy=
isolation_worker=
while [ $# -gt 0 ]; do
  case "$1" in
    --bin) kind=bin; shift ;;
    --features) features=${2:?--features needs a list}; shift 2 ;;
    --threads) threads=1; shift ;;
    --print-env) print_env=1; shift ;;
    --clippy) clippy=1; shift ;;
    --isolation-worker) isolation_worker=1; shift ;;
    *) break ;;
  esac
done

if [ -n "$threads" ]; then
  # Atomics in every crate, std rebuilt with them (-Zbuild-std), and a
  # shared, imported memory with the thread-local symbols wasm-bindgen
  # needs (the linker doesn't add these by itself). The same words as
  # `ponko package --web --threads`, so the two builds share cargo's cache.
  flags="-C target-feature=+atomics,+bulk-memory,+mutable-globals"
  flags="$flags -C link-arg=--shared-memory -C link-arg=--max-memory=1073741824 -C link-arg=--import-memory"
  flags="$flags -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size"
  flags="$flags -C link-arg=--export=__tls_align -C link-arg=--export=__tls_base"
  toolchain=${PONKO_WEB_TOOLCHAIN:-nightly}
  if [ -n "${CARGO_ENCODED_RUSTFLAGS:-}" ]; then
    flags_var=CARGO_ENCODED_RUSTFLAGS
    flags_value="$(printf '%s' "$flags" | tr ' ' '\037')"$'\037'"$CARGO_ENCODED_RUSTFLAGS"
  elif [ -n "${RUSTFLAGS:-}" ]; then
    flags_var=RUSTFLAGS
    flags_value="$flags $RUSTFLAGS"
  else
    flags_var=CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS
    flags_value="$flags${CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS:+ $CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS}"
  fi
fi
if [ -n "$isolation_worker" ] && [ -z "$threads" ]; then
  echo "--isolation-worker is for --threads (the ordinary web build needs no isolation)" >&2
  exit 2
fi
if [ -n "$clippy" ] && [ -z "$threads" ]; then
  echo "--clippy is for --threads (the ordinary web build is linted by 'cargo clippy --target wasm32-unknown-unknown')" >&2
  exit 2
fi
clippy_args=(-Zbuild-std=panic_abort,std --target wasm32-unknown-unknown -p ponko --lib --example web_threads)
if [ -n "$features" ]; then
  clippy_args+=(--features "$features")
fi
if [ -n "$print_env" ]; then
  if [ -n "$threads" ]; then
    printf 'toolchain=%s\n%s=%s\n' "$toolchain" "$flags_var" "$flags_value"
    if [ -n "$clippy" ]; then
      echo "clippy=cargo +$toolchain clippy ${clippy_args[*]} -- -D warnings"
    fi
  else
    echo "toolchain=default"
  fi
  exit 0
fi
if [ -n "$clippy" ]; then
  exec env "$flags_var=$flags_value" cargo "+$toolchain" clippy "${clippy_args[@]}" -- -D warnings
fi
name=${1:?usage: build.sh [--bin] [--features <list>] <name> [out-dir]}
here=$(cd "$(dirname "$0")" && pwd)
target=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
out=${2:-$target/web/$name}

feature_args=()
if [ -n "$features" ]; then
  feature_args=(--features "$features")
fi
if [ -n "$threads" ]; then
  env "$flags_var=$flags_value" cargo "+$toolchain" build -Zbuild-std=panic_abort,std \
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
if [ -n "$isolation_worker" ]; then
  cp "$here/ponko-isolate.js" "$out/ponko-isolate.js"
  # Before the game's script: it registers the service worker that sends
  # the cross-origin isolation headers, and reloads the page once.
  awk '/<\/head>/ { print "  <script src=\"ponko-isolate.js\"></script>" } { print }' "$out/index.html" > "$out/index.html.tmp"
  mv "$out/index.html.tmp" "$out/index.html"
fi
echo "built $out ($(du -sh "$out" | cut -f1)); try it: python3 -m http.server -d $out 8000"
