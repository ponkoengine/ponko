#!/bin/sh
# Fails if this public repo would carry a word from an optional local list: a
# list of words a maintainer never wants committed. The list itself never lives
# here: it comes from $LEAK_WORDS, one word per line, or the first of these
# files that holds a word: ~/.config/ponko/leak-words.txt, then
# ~/.config/crabby/leak-words.txt (the older location). Without any of them it
# does nothing.
#
# Hits are reported by place only (file:line), never the text, so logs don't
# repeat what they caught.
#
#   scripts/leak-check.sh files               tracked files as staged
#   scripts/leak-check.sh messages <range>    commit messages, e.g. origin/main..HEAD
#   scripts/leak-check.sh text <file>         any text (a commit message, a PR body)
#   scripts/leak-check.sh install             run it before every local commit

set -eu

list=$(mktemp)
trap 'rm -f "$list"' EXIT
if [ -n "${LEAK_WORDS:-}" ]; then
    printf '%s\n' "$LEAK_WORDS" | sed '/^[[:space:]]*$/d' > "$list"
else
    # The first file that holds a word wins: an empty new file never hides
    # the old one.
    for f in "$HOME/.config/ponko/leak-words.txt" "$HOME/.config/crabby/leak-words.txt"; do
        if [ -f "$f" ]; then
            sed '/^[[:space:]]*$/d' "$f" > "$list"
            [ ! -s "$list" ] || break
        fi
    done
fi
if [ ! -s "$list" ] && [ "${1:-}" != install ]; then
    echo "leak check: no word list, skipped"
    exit 0
fi

found() {
    echo "leak check: private words found at:" >&2
    cat >&2
    exit 1
}

case "${1:-files}" in
files)
    hits=$(git grep --cached -n -i -I -F -f "$list" -- . | cut -d: -f1,2 || true)
    [ -z "$hits" ] || printf '%s\n' "$hits" | found
    ;;
messages)
    for c in $(git rev-list "${2:?a commit range}"); do
        if git log -1 --format=%B "$c" | grep -q -i -F -f "$list"; then
            echo "commit $c message"
        fi
    done | { hits=$(cat); [ -z "$hits" ] || printf '%s\n' "$hits" | found; }
    ;;
text)
    hits=$(grep -n -i -F -f "$list" "${2:?a file}" | cut -d: -f1 | sed 's/^/line /' || true)
    [ -z "$hits" ] || printf '%s\n' "$hits" | found
    ;;
install)
    hooks=$(git rev-parse --git-path hooks)
    printf '#!/bin/sh\nexec scripts/leak-check.sh files\n' > "$hooks/pre-commit"
    printf '#!/bin/sh\nexec scripts/leak-check.sh text "$1"\n' > "$hooks/commit-msg"
    chmod +x "$hooks/pre-commit" "$hooks/commit-msg"
    echo "leak check: runs before every commit now"
    ;;
*)
    echo "usage: scripts/leak-check.sh files | messages <range> | text <file> | install" >&2
    exit 2
    ;;
esac
