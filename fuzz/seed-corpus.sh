#!/bin/sh
# Fills fuzz/corpus/<target>/ (git-ignored) with the seeds committed in
# fuzz/seeds/<target>/ and the repository's test fixtures and assets of each
# format. Run from anywhere; `cargo fuzz run <target>` then starts from them.
set -eu
cd "$(dirname "$0")/.."

add() {
    target=$1
    shift
    mkdir -p "fuzz/corpus/$target"
    for f in "$@"; do
        # Unmatched globs stay as they are: skip them.
        [ -f "$f" ] || continue
        cp "$f" "fuzz/corpus/$target/$(printf '%s' "$f" | tr '/' '_')"
    done
}

add scene tests/fixtures/saves/world_v1.json tests/fixtures/game-saves/*.json assets/levels/*.json assets/*/scenes/*.json
add prefab tests/fixtures/saves/prefab_v1.json assets/prefabs/*.json assets/*/prefabs/*.json
add gltf assets/models/*.glb assets/*/*.glb
add unity_yaml crates/ponko_unity/tests/fixtures/*.asset
add asset_pack tests/fixtures/packs/*.pack
add aseprite tests/fixtures/aseprite/*.json tests/fixtures/aseprite-linked/*.aseprite tests/fixtures/cook/source/*.aseprite
add levels tests/fixtures/tilemaps/*.ldtk tests/fixtures/tilemaps/*.tmj tests/fixtures/tilemaps/*.tmx tests/fixtures/tilemaps/*.tsx
add cooked_texture tests/fixtures/textures/*.ktx2 tests/fixtures/textures/*.png tests/fixtures/images/*
for dir in fuzz/seeds/*/; do
    add "$(basename "$dir")" "$dir"*
done
for dir in fuzz/corpus/*/; do
    echo "$(basename "$dir"): $(find "$dir" -type f | wc -l | tr -d ' ') seeds"
done
