#!/usr/bin/env bash
# Fails if anything under DIR (validation bundles about to be published)
# names a place on the machine that made it: the home directory, the
# temporary directory, the run's scratch directories, or the checkout.
#
#   scripts/validation-bundle-guard.sh DIR
#
# Used by validation-bundles.sh and validation-bundle-native.sh.
set -euo pipefail

dir="${1:?usage: validation-bundle-guard.sh DIR}"
if [ ! -d "$dir" ]; then
  echo "validation-bundle-guard: no such directory: $dir" >&2
  exit 1
fi

# Directories of this machine, where known. A trailing slash is dropped so
# "/tmp/x/" and "/tmp/x" are the same thing; "/" alone is never searched for.
places=()
add() {
  local place="${1%/}"
  if [ -n "$place" ] && [ "$place" != "/" ]; then
    places+=("$place")
  fi
}
add "${HOME:-}"
add "${TMPDIR:-}"
add "$(pwd -P)"
# The temporary directory as the system resolves it (macOS: /private/var/...).
if [ -n "${TMPDIR:-}" ] && [ -d "$TMPDIR" ]; then
  add "$(cd "$TMPDIR" && pwd -P)"
fi
# The usual temporary roots, whatever this machine's own is called.
places+=("/tmp/" "/var/folders/" "/var/tmp/")

found=0
# The tool's scratch directories: ponko-validation-<pid>-<time>-<run>. (The
# manifest's schema name starts the same way, so the numbers are required.)
if hits="$(grep -rIlE -- 'ponko-validation-[0-9]+-[0-9]+' "$dir")"; then
  echo "validation-bundle-guard: a published file names a scratch directory:" >&2
  echo "$hits" >&2
  found=1
fi
for place in "${places[@]}"; do
  if hits="$(grep -rIlF -- "$place" "$dir")"; then
    echo "validation-bundle-guard: a published file names '$place':" >&2
    echo "$hits" >&2
    found=1
  fi
done
if [ "$found" != 0 ]; then
  exit 1
fi
