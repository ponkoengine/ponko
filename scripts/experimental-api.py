#!/usr/bin/env python3
"""The experimental public items, outside the versions' promise.

    experimental-api.py check      # the list against the guide, api/ and the C header
    experimental-api.py list       # every listed item, by chapter
    experimental-api.py selftest   # the parsing and matching, with made-up input

scripts/experimental-api.txt lists the public items of the features whose
guide chapter is experimental (STABILITY.md, "Maturity levels"). They keep
their names; instead this file is read by:

- scripts/api-snapshots.py, which lists them apart, in
  api/<crate>/experimental.txt, so the other files of api/ are exactly the
  covered API;
- scripts/semver-checks.py, which does not count a break whose every
  `Failed in:` line names a listed item;
- crates/ponko_ffi/tests/abi_compat.rs, which lets a listed C item change.

`check` fails when the list and the repository disagree:

- an experimental chapter has no section, or a section names a chapter that
  is not experimental (any more), or a top-level module is experimental but
  not listed;
- a listed item is not in api/<crate>/experimental.txt (it was removed or
  renamed: update the list), or a covered file of api/ still lists one
  (regenerate api/);
- a covered item names an experimental type or module in its signature (a
  stable function that returns an experimental type is experimental too:
  list it, or make the type stable);
- a listed cargo feature is not in Cargo.toml or not under "Unstable" in
  STABILITY.md;
- the `/* Unstable: ... */` notes in include/ponko.h don't mark exactly the
  listed C functions, or a C++ wrapper that calls one doesn't say "Unstable"
  in its doc comment.
"""
import importlib.util
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
LIST = ROOT / 'scripts' / 'experimental-api.txt'
API = ROOT / 'api'
HEADER = ROOT / 'include' / 'ponko.h'
CPP = ROOT / 'include' / 'ponko.hpp'
STABILITY = ROOT / 'STABILITY.md'
CARGO = ROOT / 'Cargo.toml'
SNAPSHOT_NAME = 'experimental.txt'
UNSTABLE_NOTE = '/* Unstable:'

SECTION = re.compile(r'^\[([\w./-]+\.md)\]\s+(\S.*)$')
ATTRS = re.compile(r'^(?:#\[[^\]]*(?:\[[^\]]*\][^\]]*)*\]\s*)+')
PATH = re.compile(r'(?<![\w:])([a-z_][a-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+)')
KEYWORDS = {'pub', 'fn', 'const', 'unsafe', 'async', 'extern', 'c', '"C"', 'struct', 'enum',
            'union', 'trait', 'type', 'static', 'mut', 'macro', 'mod', 'use', 'auto'}
TYPE_KINDS = ('struct', 'enum', 'mod', 'trait', 'type', 'union')


class Problem(Exception):
    pass


# Parsing and matching: plain functions of their input, checked by `selftest`.

def parse_list(text):
    """[{chapter, reason, items: [(path, note)], features: [(name, note)],
    web: [(path, note)], none: note or None}] from the list file."""
    sections = []
    for number, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith('#'):
            continue
        where = f'scripts/experimental-api.txt:{number}'
        match = SECTION.match(line)
        if match:
            sections.append({'chapter': match.group(1), 'reason': match.group(2),
                             'items': [], 'features': [], 'web': [], 'none': None})
            continue
        if line.startswith('['):
            raise Problem(f'{where}: a section is `[chapter.md] why it is experimental`')
        if not sections:
            raise Problem(f'{where}: an entry before the first `[chapter.md]` section')
        section = sections[-1]
        words = line.split(None, 1)
        if words[0] == 'none':
            if len(words) < 2:
                raise Problem(f'{where}: `none` needs a reason')
            section['none'] = words[1]
            continue
        if words[0] in ('feature', 'web'):
            rest = words[1].split(None, 1) if len(words) > 1 else []
            if len(rest) < 2:
                raise Problem(f'{where}: `{words[0]} <name> <why>`')
            section['features' if words[0] == 'feature' else 'web'].append((rest[0], rest[1]))
            continue
        if len(words) < 2:
            raise Problem(f'{where}: `{words[0]}` needs a reason after it')
        if '::' not in words[0]:
            raise Problem(f'{where}: `{words[0]}` is not an item path (`crate::module::Item`)')
        section['items'].append((words[0], words[1]))
    for section in sections:
        if not (section['items'] or section['features'] or section['web'] or section['none']):
            raise Problem(f'[{section["chapter"]}] lists nothing: add its items, or `none <why>`')
        if section['none'] and (section['items'] or section['features'] or section['web']):
            raise Problem(f'[{section["chapter"]}] says `none` but lists items')
    return sections


def load_list(path=LIST):
    return parse_list(path.read_text(encoding='utf-8'))


def item_paths(sections, crate=None):
    """The listed item paths (snapshot-checked ones), optionally of one crate."""
    paths = [p for s in sections for p, _ in s['items']]
    return [p for p in paths if crate is None or p.split('::', 1)[0] == crate]


def features(sections):
    return [f for s in sections for f, _ in s['features']]


def line_path(line):
    """The item a line of api/ is about: its own path, or the type an impl is for."""
    line = ATTRS.sub('', line.strip())
    if line.startswith('impl'):
        rest = line[4:]
        if rest.startswith('<'):
            depth = 0
            for i, ch in enumerate(rest):
                depth += ch == '<'
                depth -= ch == '>'
                if depth == 0:
                    rest = rest[i + 1:]
                    break
        top, depth = [], 0
        # ` for ` outside generic brackets separates the trait from the type.
        for i, ch in enumerate(rest):
            depth += ch == '<'
            depth -= ch == '>'
            if depth == 0 and rest.startswith(' for ', i):
                top.append(i)
        if top:
            rest = rest[top[-1] + 5:]
    if line.startswith('impl'):
        match = PATH.search(rest)
        return match.group(1) if match else None
    match = PATH.search(line)
    # The item's own path comes right after the keywords; a path further in
    # is a parameter (`pub fn F::run_if(self, ponko::x::Y)` is a method of a
    # generic type, not of `ponko::x::Y`).
    if not match or any(w not in KEYWORDS for w in line[:match.start()].split()):
        return None
    return match.group(1)


def covers(entry, path):
    return path is not None and (path == entry or path.startswith(entry + '::'))


def is_experimental(line, entries):
    path = line_path(line)
    return any(covers(e, path) for e in entries)


def split_listing(text, entries):
    """(covered text, experimental lines) of one api/ listing."""
    covered, experimental = [], []
    for line in text.splitlines(keepends=True):
        (experimental if line.strip() and is_experimental(line, entries) else covered).append(line)
    return ''.join(covered), [l.rstrip('\n') for l in experimental]


def type_like(entries, experimental_lines):
    """The entries that name a type or a module (what a signature can mention)."""
    found = set()
    for line in experimental_lines:
        body = ATTRS.sub('', line.strip())
        for kind in TYPE_KINDS:
            match = re.match(rf'pub {kind} ([\w:]+)', body)
            if match and match.group(1) in entries:
                found.add(match.group(1))
    return sorted(found)


def mention_pattern(types):
    if not types:
        return None
    alternatives = '|'.join(re.escape(t) for t in sorted(types, key=len, reverse=True))
    return re.compile(rf'(?<![\w:])(?:{alternatives})(?![\w])')


def leaks(covered_lines, types):
    """Covered lines that name an experimental type or module."""
    pattern = mention_pattern(types)
    return [l for l in covered_lines if pattern and pattern.search(l)] if pattern else []


def short_names(entries, experimental_lines, covered_lines):
    """Names cargo-semver-checks may print for listed items, minus any that a
    covered item shares (so an ambiguous name never excuses a real break)."""
    def names_of(path, typeish):
        parts = path.split('::')
        out = {path}
        if typeish:
            out.add(parts[-1])
        elif len(parts) >= 3:
            out.add('::'.join(parts[-2:]))
        if len(parts) == 2:  # a crate-root item: C functions, constants
            out.add(parts[-1])
        return out

    type_paths = set(type_like(sorted({line_path(l) for l in experimental_lines} - {None}),
                               experimental_lines))
    experimental_paths = {line_path(l) for l in experimental_lines} - {None}
    names = set()
    for entry in entries:
        names |= names_of(entry, entry in type_paths)
        # What a listed module or type holds, by the names the tool prints.
        for path in experimental_paths:
            if covers(entry, path) and path != entry:
                names |= names_of(path, path in type_paths)
    covered_names = set()
    for line in covered_lines:
        path = line_path(line)
        if path:
            body = ATTRS.sub('', line.strip())
            typeish = any(body.startswith(f'pub {k} ') for k in TYPE_KINDS)
            covered_names |= names_of(path, typeish) - {path}
    return sorted(names - covered_names)


def failed_in(output):
    """[(lint, line)] of every `Failed in:` entry in cargo-semver-checks output."""
    found = []
    lint = None
    lines = output.splitlines()
    i = 0
    while i < len(lines):
        match = re.match(r'--- failure ([\w-]+):', lines[i].strip())
        if match:
            lint = match.group(1)
        elif lines[i].strip() == 'Failed in:':
            i += 1
            while i < len(lines) and lines[i].startswith((' ', '\t')) and lines[i].strip():
                found.append((lint, lines[i].strip()))
                i += 1
            continue
        i += 1
    return found


def classify(output, names, feature_names=()):
    """(covered failures, experimental failures) of one crate's run."""
    pattern = mention_pattern(names)
    covered, experimental = [], []
    for lint, line in failed_in(output):
        hit = bool(pattern and pattern.search(line)) or any(
            re.search(rf'\bfeature {re.escape(f)}\b', line) for f in feature_names)
        (experimental if hit else covered).append((lint, line))
    return covered, experimental


def header_unstable_functions(header):
    """The C functions include/ponko.h marks `/* Unstable: ... */`."""
    return sorted(set(re.findall(r'/\* Unstable:.*?\*/\s*[^;(]*?\b(ponko_\w+)\s*\(', header, re.S)))


def cpp_unmarked_calls(hpp, functions):
    """C++ wrapper declarations that call a listed C function without saying
    "Unstable" in their doc comment: [(line number, function)]."""
    lines = hpp.splitlines()
    out = []
    for i, line in enumerate(lines):
        for name in functions:
            if not re.search(rf'\b{re.escape(name)}\s*\(', line):
                continue
            j = i
            while j >= 0 and not lines[j].lstrip().startswith('///'):
                j -= 1
            doc = []
            while j >= 0 and lines[j].lstrip().startswith('///'):
                doc.append(lines[j])
                j -= 1
            if not any('Unstable' in d for d in doc):
                out.append((i + 1, name))
    return out


def snapshot_c_functions(experimental_lines):
    return sorted({m.group(1) for l in experimental_lines
                   for m in [re.search(r'\bc fn ponko_ffi::(\w+)\(', l)] if m})


def section_of(text, heading):
    match = re.search(rf'^## {re.escape(heading)}\n(.*?)(?=^## |\Z)', text, re.M | re.S)
    return match.group(1) if match else ''


# Side effects: reading the repository.

def load_maturity():
    spec = importlib.util.spec_from_file_location('maturity', ROOT / 'scripts' / 'maturity.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def snapshot_files():
    """{crate: {set name: lines}} of api/."""
    out = {}
    for path in sorted(API.glob('*/*.txt')):
        out.setdefault(path.parent.name, {})[path.stem] = path.read_text(encoding='utf-8').splitlines()
    return out


def run_check():
    problems = []
    try:
        sections = load_list()
    except Problem as e:
        print(f'experimental-api.py: {e}')
        return 1

    # The list and the maturity levels.
    maturity = load_maturity()
    level_problems, levels = maturity.collect()
    problems += level_problems
    experimental_chapters = {p for p, (level, _) in levels.items() if level == 'experimental'}
    listed_chapters = [s['chapter'] for s in sections]
    for chapter in sorted(experimental_chapters - set(listed_chapters)):
        problems.append(f'book/src/{chapter} is experimental but has no `[{chapter}]` section '
                        'in scripts/experimental-api.txt (list its public items, or `none <why>`)')
    for chapter in listed_chapters:
        if chapter not in levels:
            problems.append(f'[{chapter}] is not a chapter of the guide')
        elif chapter not in experimental_chapters:
            problems.append(f'[{chapter}]: book/src/{chapter} is {levels[chapter][0]} now; its items '
                            'are covered by the versions, so remove its section')
        if listed_chapters.count(chapter) > 1:
            problems.append(f'[{chapter}] appears twice')
    entries = item_paths(sections)
    _, rows = maturity.check_modules(levels, False)
    for name, level, _ in rows:
        if level == 'experimental' and f'ponko::{name}' not in entries:
            problems.append(f'ponko::{name} is an experimental module but not listed')

    # The list and api/.
    files = snapshot_files()
    for entry in entries:
        crate = entry.split('::', 1)[0]
        listed = files.get(crate, {}).get('experimental', [])
        if not any(is_experimental(l, [entry]) for l in listed):
            problems.append(f'{entry} is listed but not in api/{crate}/{SNAPSHOT_NAME}: it was removed or '
                            'renamed (update the list), or api/ is not regenerated '
                            '(`python3 scripts/api-snapshots.py update`)')
    all_experimental = [l for sets in files.values() for l in sets.get('experimental', [])]
    types = type_like(entries, all_experimental)
    for crate, sets in files.items():
        crate_entries = item_paths(sections, crate)
        for set_name, lines in sets.items():
            if set_name == 'experimental':
                continue
            stale = [l for l in lines if is_experimental(l, crate_entries)]
            for line in stale[:5]:
                problems.append(f'api/{crate}/{set_name}.txt lists an experimental item, regenerate api/: {line}')
            for line in leaks(lines, types):
                problems.append(f'api/{crate}/{set_name}.txt: a covered item names an experimental '
                                f'type (list the item, or make the type stable): {line}')

    # Cargo features.
    cargo = CARGO.read_text(encoding='utf-8')
    unstable = section_of(STABILITY.read_text(encoding='utf-8'), 'Unstable')
    for feature in features(sections):
        if not re.search(rf'^{re.escape(feature)}\s*=', cargo, re.M):
            problems.append(f'feature {feature} is listed but not in Cargo.toml')
        if f'`{feature}`' not in unstable:
            problems.append(f'feature {feature}: STABILITY.md, "Unstable", must name `{feature}`')

    # C and C++.
    c_functions = snapshot_c_functions(all_experimental)
    marked = header_unstable_functions(HEADER.read_text(encoding='utf-8'))
    for name in sorted(set(c_functions) - set(marked)):
        problems.append(f'include/ponko.h: {name} is experimental but has no `{UNSTABLE_NOTE} ... */` note '
                        '(`/// cbindgen:prefix=/* Unstable: <why> */` on the Rust function, then '
                        'regenerate the header)')
    for name in sorted(set(marked) - set(c_functions)):
        problems.append(f'include/ponko.h: {name} is marked Unstable but not listed in '
                        'scripts/experimental-api.txt')
    for number, name in cpp_unmarked_calls(CPP.read_text(encoding='utf-8'), c_functions):
        problems.append(f'include/ponko.hpp:{number}: calls {name}, which is unstable; its doc '
                        'comment must say "Unstable"')

    if problems:
        print('experimental-api.py: the list of experimental items and the repository disagree:')
        for p in problems:
            print(f'  {p}')
        return 1
    print(f'experimental-api.py: {len(entries)} items and {len(features(sections))} features in '
          f'{len(sections)} experimental chapters, {len(c_functions)} unstable C functions')
    return 0


def run_list():
    for section in load_list():
        print(f'{section["chapter"]}: {section["reason"]}')
        for kind in ('features', 'items', 'web'):
            for name, note in section[kind]:
                prefix = {'features': 'feature ', 'web': 'web '}.get(kind, '')
                print(f'  {prefix}{name}  ({note})')
        if section['none']:
            print(f'  none ({section["none"]})')
    return 0


def selftest():
    sections = parse_list(
        '# comment\n[a.md] why a\nponko::x::Thing  the thing\nfeature fast  a feature\n'
        'web ponko::web::f  web only\n\n[b.md] why b\nnone  nothing public\n')
    assert [s['chapter'] for s in sections] == ['a.md', 'b.md']
    assert sections[0]['items'] == [('ponko::x::Thing', 'the thing')]
    assert features(sections) == ['fast'] and sections[0]['web'][0][0] == 'ponko::web::f'
    assert sections[1]['none'] == 'nothing public'
    for bad in ('ponko::x::Thing why\n', '[a.md] why\nponko::x::Thing\n', '[a.md] why\n',
                '[a.md] why\nnone x\nponko::a::B b\n', '[a.md] why\nThing why\n', '[a.md]\n'):
        try:
            parse_list(bad)
        except Problem:
            pass
        else:
            raise AssertionError(f'accepted {bad!r}')

    assert line_path('pub fn ponko::game::Game::hdr_output(self, ponko::render::HdrOutput) -> Self') == \
        'ponko::game::Game::hdr_output'
    assert line_path('#[non_exhaustive] #[repr(u32)] pub enum ponko::render::HdrEncoding') == \
        'ponko::render::HdrEncoding'
    assert line_path('impl core::clone::Clone for ponko::render::HdrOutput') == 'ponko::render::HdrOutput'
    assert line_path('impl ponko::net::rtc::RtcConfig') == 'ponko::net::rtc::RtcConfig'
    assert line_path('impl<T: core::fmt::Debug> core::convert::From<T> for ponko::a::B<T>') == 'ponko::a::B'
    assert line_path("impl<'de> serde::Deserialize<'de> for ponko::a::C") == 'ponko::a::C'
    assert line_path('pub ponko::render::HdrOutput::enabled: bool') == 'ponko::render::HdrOutput::enabled'
    assert line_path('#[no_mangle] pub unsafe c fn ponko_ffi::ponko_ctx_hdr_status(*mut ponko_ffi::PonkoCtx)') \
        == 'ponko_ffi::ponko_ctx_hdr_status'
    assert line_path('pub mod ponko') is None
    assert line_path('pub fn F::run_if<W>(self, impl ponko::render::HdrOutput) -> Self') is None, \
        'a parameter is not the item'

    entries = ['ponko::render::HdrOutput', 'ponko::net::rtc', 'ponko::game::Game::hdr_output']
    listing = ('pub fn ponko::game::Game::hdr_output(self, ponko::render::HdrOutput) -> Self\n'
               'pub fn ponko::game::Game::new() -> Self\n'
               'pub struct ponko::render::HdrOutput\n'
               'pub struct ponko::render::HdrOutputs\n'
               'impl core::clone::Clone for ponko::render::HdrOutput\n'
               'pub mod ponko::net::rtc\n'
               'pub struct ponko::net::rtc::Signaling(_)\n'
               'pub fn ponko::net::rtc::Signaling::new() -> Self\n'
               'pub fn ponko::net::lobby::LobbyClient::signaling(&mut self) -> ponko::net::rtc::Signaling\n')
    covered, experimental = split_listing(listing, entries)
    assert covered.splitlines() == [
        'pub fn ponko::game::Game::new() -> Self',
        'pub struct ponko::render::HdrOutputs',
        'pub fn ponko::net::lobby::LobbyClient::signaling(&mut self) -> ponko::net::rtc::Signaling'], covered
    assert len(experimental) == 6
    types = type_like(entries, experimental)
    assert types == ['ponko::net::rtc', 'ponko::render::HdrOutput'], types
    assert leaks(covered.splitlines(), types) == [covered.splitlines()[2]], 'a stable item handing out rtc'
    assert leaks(['pub struct ponko::render::HdrOutputs'], types) == [], 'a longer name is another type'

    names = short_names(entries, experimental, covered.splitlines() + ['pub struct ponko::other::Signaling'])
    assert 'HdrOutput' in names and 'Game::hdr_output' in names and 'ponko::net::rtc' in names
    assert 'Signaling' not in names, 'a covered type shares the name: never excuse it'
    assert 'Game::new' not in names

    output = '''--- failure inherent_method_missing: pub method removed or renamed ---

Description:
A publicly-visible method or associated fn is no longer available.

Failed in:
  Game::hdr_output, previously in file src/game/mod.rs:10
  Game::new, previously in file src/game/mod.rs:5

--- failure struct_pub_field_missing: pub struct's pub field removed or renamed ---

Failed in:
  field peak of struct HdrOutput, previously in file src/render/hdr.rs:3
  struct ponko::net::rtc::Signaling, previously in file src/net/rtc.rs:4

--- failure feature_missing: package feature removed or renamed ---

Failed in:
  feature fast in the package's Cargo.toml
'''
    assert len(failed_in(output)) == 5
    covered_fail, experimental_fail = classify(output, names, ['fast'])
    assert covered_fail == [('inherent_method_missing', 'Game::new, previously in file src/game/mod.rs:5')]
    assert len(experimental_fail) == 4
    assert classify(output.replace('Game::new', 'ConfiguredGame::hdr_outputs'), names, ['fast'])[0], \
        'a longer name is not a listed one'

    header = ('enum PonkoStatus ponko_a(int x);\n'
              '/* Unstable: never checked */ enum PonkoStatus ponko_b(int x);\n'
              '/* Unstable: never checked */\nenum PonkoStatus ponko_c(struct PonkoX *x,\n    int y);\n')
    assert header_unstable_functions(header) == ['ponko_b', 'ponko_c']
    hpp = ('    /// Does a. Unstable: see the guide.\n    void a() { ponko_b(1); }\n'
           '    /// Does c.\n    void c() {\n        ponko_c(nullptr, 1);\n    }\n')
    assert cpp_unmarked_calls(hpp, ['ponko_b', 'ponko_c']) == [(5, 'ponko_c')]
    assert snapshot_c_functions(['#[no_mangle] pub unsafe c fn ponko_ffi::ponko_b(i32) -> u32',
                                 'pub struct ponko_ffi::PonkoX']) == ['ponko_b']
    assert 'b' in section_of('## A\na\n## Unstable\nb\n## C\nc\n', 'Unstable')
    assert 'c' not in section_of('## A\na\n## Unstable\nb\n## C\nc\n', 'Unstable')
    print('experimental-api.py: self-test passed')
    return 0


if __name__ == '__main__':
    command = sys.argv[1] if len(sys.argv) == 2 else ''
    if command == 'check':
        sys.exit(run_check())
    if command == 'list':
        sys.exit(run_list())
    if command == 'selftest':
        sys.exit(selftest())
    sys.exit(__doc__)
