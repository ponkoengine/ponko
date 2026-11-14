#!/usr/bin/env python3
"""Fail when a C ABI export has no test and no reasoned allowlist entry.

An export (`extern "C" fn` in crates/ponko_ffi/src) counts as tested when its
name appears, outside comments and string literals
(a name in a message or a path is not a call), in an FFI test
(crates/ponko_ffi/tests/*.rs) or a native suite (examples/cpp/*.c, *.cpp).
Doctests in src/ don't count: most only pass a null pointer.

The functions that cannot be tested on a headless machine (they need a real
window, a GPU frame or a Steam client) are listed with a reason in
scripts/ffi-coverage-allowlist.txt. The check fails when
  - an export is untested and not on the allowlist (the count went up),
  - an allowlist entry is now tested or no longer exists (shrink the list),
  - an allowlist entry has no reason or appears twice.

It also holds the "Still Rust-only" table of book/src/c-cpp.md to
scripts/rust-only.txt (see that file): the table and the list name the same
Rust items, each item still exists, no C function of its own has appeared
for it, and every chapter that says "Rust-only" is one the list covers.

    python3 scripts/check-ffi-coverage.py          # check
    python3 scripts/check-ffi-coverage.py --list   # print the untested exports
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
ALLOWLIST = ROOT / 'scripts' / 'ffi-coverage-allowlist.txt'
RUST_ONLY = ROOT / 'scripts' / 'rust-only.txt'
GUIDE = ROOT / 'book' / 'src' / 'c-cpp.md'
BOOK = ROOT / 'book' / 'src'
HEADER = ROOT / 'include' / 'ponko.h'
# Chapters that say "Rust-only" about something that is not a feature of the
# engine's API: policy, the reference game, the tests.
NOT_FEATURES = {
    'contributing.md': 'contribution policy',
    'ember-hop.md': 'the second reference game is written in Rust',
    'packaging.md': 'the reference game is written in Rust',
    'render-tests.md': 'the GPU tests are written in Rust',
    'shard-grove.md': 'the 3D reference game is written in Rust',
    'tutorial-2d.md': 'the tutorial is written in Rust',
    'tutorial-3d.md': 'the tutorial is written in Rust',
}
EXPORT = re.compile(r'extern "C" fn ([a-z0-9_]+)')
# What is not code: Rust raw strings, ordinary strings (Rust and C), character
# literals (so '"' does not start a string), block comments and line comments.
# Strings and characters are blanked, comments dropped, so only calls remain.
NOT_CODE = re.compile(
    r"""(?P<raw>b?r(?P<hashes>\#*)".*?"(?P=hashes))"""
    r"""|(?P<char>'(?:\\.|[^'\\\n])')"""
    r"""|(?P<string>b?c?"(?:\\.|[^"\\])*")"""
    r"""|(?P<block>/\*.*?\*/)"""
    r"""|(?P<line>//[^\n]*)""",
    re.DOTALL,
)


def code_only(text):
    def blank(match):
        if match.lastgroup in ('block', 'line'):
            return ' '
        return '""'
    return NOT_CODE.sub(blank, text)


def exports():
    names = set()
    for source in sorted((ROOT / 'crates/ponko_ffi/src').rglob('*.rs')):
        names.update(EXPORT.findall(source.read_text()))
    return names


def test_text():
    files = sorted((ROOT / 'crates/ponko_ffi/tests').glob('*.rs'))
    files += sorted((ROOT / 'examples/cpp').glob('*.c')) + sorted((ROOT / 'examples/cpp').glob('*.cpp'))
    return '\n'.join(code_only(f.read_text()) for f in files)


def untested(names, text):
    result = set()
    for name in names:
        if not re.search(rf'(^|[^a-z0-9_]){name}([^a-z0-9_]|$)', text):
            result.add(name)
    return result


def allowlist():
    entries, problems = {}, []
    for number, line in enumerate(ALLOWLIST.read_text().splitlines(), 1):
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        name, _, reason = line.partition('#')
        name = name.strip()
        if not reason.strip():
            problems.append(f'{ALLOWLIST.name}:{number}: {name} has no reason after "#"')
        if name in entries:
            problems.append(f'{ALLOWLIST.name}:{number}: {name} is listed twice')
        entries[name] = reason.strip()
    return entries, problems


def rust_only_entries():
    entries, problems = {}, []
    for number, line in enumerate(RUST_ONLY.read_text().splitlines(), 1):
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        fields, _, reason = line.partition('#')
        parts = fields.split()
        if len(parts) != 2:
            problems.append(f'{RUST_ONLY.name}:{number}: expected "<Rust name>  <chapter>  # reason"')
            continue
        name, chapter = parts
        if not reason.strip():
            problems.append(f'{RUST_ONLY.name}:{number}: {name} has no reason after "#"')
        if name in entries:
            problems.append(f'{RUST_ONLY.name}:{number}: {name} is listed twice')
        entries[name] = chapter
    return entries, problems


def rust_only_table():
    """{name: chapter file} for the first table after "## Still Rust-only"."""
    lines = GUIDE.read_text().split('## Still Rust-only', 1)[1].splitlines()
    rows, started = {}, False
    for line in lines:
        if line.startswith('|'):
            started = True
            cells = [c.strip() for c in line.strip().strip('|').split('|')]
            if len(cells) < 3 or cells[0].startswith(('Rust feature', '---')):
                continue
            link = re.search(r'\(([a-z0-9-]+\.md)(?:#[^)]*)?\)', cells[1])
            for name in re.findall(r'`([^`]+)`', cells[0]):
                rows[name] = link.group(1) if link else None
        elif started:
            break
    return rows


def snake(name):
    return re.sub(r'(?<=[a-z0-9])(?=[A-Z])', '_', name).lower()


def rust_sources():
    files = list((ROOT / 'src').rglob('*.rs')) + list((ROOT / 'crates/ponko_ecs/src').rglob('*.rs'))
    return '\n'.join(f.read_text() for f in files)


def rust_only_problems():
    entries, problems = rust_only_entries()
    table = rust_only_table()
    for name in sorted(entries.keys() - table.keys()):
        problems.append(f'{name}: on {RUST_ONLY.name} but not in the "Still Rust-only" table of book/src/c-cpp.md.')
    for name in sorted(table.keys() - entries.keys()):
        problems.append(f'{name}: in the "Still Rust-only" table of book/src/c-cpp.md but not on scripts/{RUST_ONLY.name}.')
    source = rust_sources()
    header = HEADER.read_text()
    for name, chapter in sorted(entries.items()):
        last = name.split('::')[-1]
        if table.get(name) not in (None, chapter):
            problems.append(f'{name}: the table row links {table[name]}, the list says {chapter}.')
        elif name in table and table[name] is None:
            problems.append(f'{name}: the table row has no link to its chapter ({chapter}).')
        if not re.search(rf'\b{last}\b', source):
            problems.append(f'{name}: nothing called {last} in the Rust source any more; remove the entry and its table row.')
        chapter_path = BOOK / chapter
        if not chapter_path.exists():
            problems.append(f'{name}: {chapter} does not exist.')
        elif not re.search(rf'\b{last}\b', chapter_path.read_text()):
            problems.append(f'{name}: {chapter} never mentions {last}.')
        words = snake(last)
        if '_' in words and re.search(rf'\bponko_(?:\w*_)?{words}(?:_\w*)?\b', header, re.IGNORECASE):
            problems.append(f'{name}: ponko.h has a function named for it now. Bind it in C++ too, then remove '
                            f'the entry and its table row.')
    covered = set(entries.values()) | set(NOT_FEATURES)
    for chapter in sorted(BOOK.glob('*.md')):
        if chapter.name != GUIDE.name and chapter.name not in covered and re.search(r'rust[- ]only', chapter.read_text(), re.I):
            problems.append(f'{chapter.name}: says "Rust-only" but has no entry on scripts/{RUST_ONLY.name} '
                            f'(add one with its table row, or list the chapter in NOT_FEATURES if it is not about the API).')
    return problems


def main():
    names = exports()
    missing = untested(names, test_text())
    if '--list' in sys.argv:
        print('\n'.join(sorted(missing)))
        print(f'{len(missing)} of {len(names)} exports have no test', file=sys.stderr)
        return 0
    allowed, problems = allowlist()
    for name in sorted(missing - allowed.keys()):
        problems.append(f'{name}: no test calls it and it is not on the allowlist. Test it '
                        f'(crates/ponko_ffi/tests/) or, if it needs a window or Steam, add it to '
                        f'scripts/{ALLOWLIST.name} with the reason.')
    for name in sorted(allowed.keys() - names):
        problems.append(f'{name}: on the allowlist but not exported any more; remove the entry.')
    for name in sorted((allowed.keys() & names) - missing):
        problems.append(f'{name}: has a test now; remove it from the allowlist.')
    problems += rust_only_problems()
    if problems:
        print('\n'.join(problems))
        return 1
    print(f'{len(names) - len(missing)} of {len(names)} C exports are tested; '
          f'{len(missing)} are on the allowlist; the Rust-only table matches scripts/{RUST_ONLY.name}.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
