#!/usr/bin/env bash
# Builds a C or C++ game for the web, ready to serve as a static site (the
# guide's "C and C++ on the web"). Run it from the game's folder: its
# assets/ folder is copied like web/build.sh does.
#
#   web/build-cpp.sh game.cpp                         # -> target/web/game/
#   web/build-cpp.sh -o site/play main.cpp world.c    # choose the output folder
#   web/build-cpp.sh --features physics game.cpp      # ponko_ffi features (comma-separated)
#   web/build-cpp.sh game.cpp -- -DNDEBUG -Isrc       # more compiler flags
#   python3 -m http.server -d target/web/game 8000
#
# Needs: the WASI SDK, 33 or newer (its C++ library with exceptions), in
#        $WASI_SDK_PATH or /opt/wasi-sdk (https://github.com/WebAssembly/wasi-sdk)
#        rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <the wasm-bindgen in Cargo.lock>
#
# The game's sources are compiled by the WASI SDK's clang for wasm32 with
# WebAssembly exception handling, and linked with its C and C++ libraries
# into ponko_ffi's examples/web_main.rs, the engine's ordinary web build
# (wasm32-unknown-unknown, wasm-bindgen) that calls the game's main. The
# module then needs a browser with the current exception handling (Chrome
# 137, Firefox 131, Safari 18.4). Emscripten can't build the engine: its
# window, input and graphics run on winit and wasm-bindgen, which have no
# Emscripten backend.
set -euo pipefail

# Called as the linker (cargo rustc ... -C linker=build-cpp.sh, below): the
# library exports its whole C API for C callers, which would keep all of it
# in the module. The page calls only wasm-bindgen's exports, so leave those
# out (and the WASI functions web_main defines) and the linker drops the
# engine code the game doesn't use.
if [ -n "${PONKO_WEB_CPP_LINKER:-}" ]; then
  args=()
  while [ $# -gt 0 ]; do
    if [ "$1" = "--export" ]; then
      case "${2:-}" in
        ponko_* | __imported_wasi_snapshot_preview1_*) shift 2; continue ;;
      esac
    fi
    args+=("$1")
    shift
  done
  exec "$PONKO_WEB_CPP_LINKER" "${args[@]}"
fi

out=
features=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=${2:?-o needs a folder}; shift 2 ;;
    --features) features=${2:?--features needs a list}; shift 2 ;;
    *) break ;;
  esac
done
sources=()
while [ $# -gt 0 ] && [ "$1" != "--" ]; do
  sources+=("$1")
  shift
done
[ $# -gt 0 ] && shift
extra=("$@")
if [ ${#sources[@]} -eq 0 ]; then
  echo "usage: build-cpp.sh [-o out-dir] [--features <list>] <source.c|.cpp>... [-- <compiler flags>]" >&2
  exit 2
fi

sdk=${WASI_SDK_PATH:-/opt/wasi-sdk}
if [ ! -x "$sdk/bin/clang++" ]; then
  echo "build-cpp.sh: no WASI SDK in $sdk; install it there or set WASI_SDK_PATH" >&2
  exit 1
fi
sysroot="$sdk/share/wasi-sysroot"
libs="$sysroot/lib/wasm32-wasip1"
if [ ! -f "$libs/eh/libc++.a" ]; then
  echo "build-cpp.sh: $sdk has no C++ library with exceptions ($libs/eh); use WASI SDK 33 or newer" >&2
  exit 1
fi

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
target=$(cargo metadata --manifest-path "$repo/Cargo.toml" --format-version 1 --no-deps |
  python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
name=$(basename "${sources[0]}"); name=${name%.*}
out=${out:-$target/web/$name}
objects="$target/web-cpp/$name"
rm -rf "$objects"
mkdir -p "$objects"

# The game's objects. The same exception handling everywhere: the C++
# library in eh/ is built with it.
flags=(--target=wasm32-wasip1 --sysroot="$sysroot" -O2 -fwasm-exceptions -mllvm -wasm-use-legacy-eh=false
  -I "$repo/include")
link_args=()
i=0
for source in "${sources[@]}"; do
  object="$objects/$i-$(basename "$source").o"
  case "$source" in
    *.c) "$sdk/bin/clang" -std=c11 "${flags[@]}" ${extra[@]+"${extra[@]}"} -c "$source" -o "$object" ;;
    *) "$sdk/bin/clang++" -std=c++17 "${flags[@]}" ${extra[@]+"${extra[@]}"} -c "$source" -o "$object" ;;
  esac
  link_args+=(-C "link-arg=$object")
  i=$((i + 1))
done
builtins=$("$sdk/bin/clang" --target=wasm32-wasip1 -print-libgcc-file-name)
# Warnings fail the link: a WASI function web_main defines with another
# signature than the C library's would otherwise become a trap.
link_args+=(-C link-arg=--fatal-warnings)
link_args+=(-L "native=$libs/eh" -L "native=$libs" -L "native=$(dirname "$builtins")")
link_args+=(-l static=c++ -l static=c++abi -l static=unwind -l static=c -l "static=$(basename "$builtins" .a | sed 's/^lib//')")

link_args+=(-C "linker=$here/build-cpp.sh")
lld="$(cd "$repo" && rustc --print sysroot)/lib/rustlib/$(cd "$repo" && rustc -vV | sed -n 's/^host: //p')/bin/rust-lld"

feature_args=()
if [ -n "$features" ]; then
  feature_args=(--features "$features")
fi
PONKO_WEB_CPP_LINKER=$lld cargo rustc --manifest-path "$repo/Cargo.toml" -p ponko_ffi --example web_main --release \
  --target wasm32-unknown-unknown ${feature_args[@]+"${feature_args[@]}"} -- "${link_args[@]}"
wasm="$target/wasm32-unknown-unknown/release/examples/web_main.wasm"

# Every WASI function the game calls must be one web_main defines: the page
# has no others.
missing=$(node -e '
  const m = new WebAssembly.Module(require("fs").readFileSync(process.argv[1]));
  for (const i of WebAssembly.Module.imports(m))
    if (i.module === "wasi_snapshot_preview1" || i.module === "env") console.log(i.module + "." + i.name);
' "$wasm")
if [ -n "$missing" ]; then
  echo "build-cpp.sh: the game calls functions a browser doesn't have:" >&2
  echo "$missing" | sed 's/^/  /' >&2
  echo "(env.*: a function declared but not defined; wasi_snapshot_preview1.*: files, sleeping, sockets)" >&2
  exit 1
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
rm -rf "$objects"
echo "built $out ($(du -sh "$out" | cut -f1)); try it: python3 -m http.server -d $out 8000"
