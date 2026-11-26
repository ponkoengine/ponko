#!/usr/bin/env python3
"""Where every public item is called from, and which items nothing calls.

    api-usage.py report            # write scripts/api-usage.md (the list by module)
    api-usage.py report --stdout   # print it instead
    api-usage.py item <path>       # where one item is called, file by file
    api-usage.py check             # every decision names an item, with a known word
    api-usage.py selftest          # the parsing and matching, with made-up input

The freeze review (#478) looks at names and shapes. This looks at use: an
item that no example, reference game, cookbook recipe, guide snippet, test
or binding calls is unproven, and after 1.0 it has to be kept anyway. What
was decided for each one (a use added, renamed `unstable_`, removed, or kept
with a reason) is in scripts/api-usage-decisions.txt, and the report shows it.

Items come from the API snapshots in api/ (scripts/api-snapshots.py), the
covered API of every published crate: types, traits, functions, methods,
constants, statics and macros. Fields and enum variants belong to their type
and are not listed on their own; neither are trait impls' methods (`clone`,
`fmt`, a plugin's `build`), which the trait already names. Experimental
items (api/<crate>/experimental.txt) and `unstable_` names are listed apart:
they are already outside the promise.

Where they are called, by kind of place:

  game       the reference games: examples/coin_dash, examples/ember_hop
  example    the other examples (examples/, crates/*/examples) and the C and
             C++ example programs
  cookbook   the cookbook recipes (tests/cookbook_*.rs, examples/cpp/cookbook.cpp)
  guide      the guide's inline snippets (book/src) and the files it includes
             only for the guide (examples/cpp/guide.c, guide.cpp)
  test       integration tests (tests/, crates/*/tests, the C and C++ test
             programs, the offscreen GPU tests src/**/*_test.rs); not unit
             tests inside the item's own source file, not doctests
  binding    the C API (crates/ponko_ffi/src) for Rust items, the C++ wrapper
             (include/ponko.hpp) for C functions
  tool       the other crates' sources (ponko_mcp, ponko_unity, ponko_cli,
             ...) and, for ponko_ecs items, the engine's own sources

Matching is by name, not by type resolution, so it errs on the side of
"called": `Type::name` or `name(` counts, and a method name several types
share counts only in a file that also names the type (otherwise the item is
`ambiguous`: its name is called somewhere, but maybe on another type). An
item listed as not called is not called by name anywhere in those places.
"""
from collections import defaultdict
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
API = ROOT / 'api'
REPORT = ROOT / 'scripts' / 'api-usage.md'
ISSUE = '#508'

KINDS = ('game', 'example', 'cookbook', 'guide', 'test', 'binding', 'tool')
GAMES = ('examples/coin_dash/', 'examples/ember_hop/')

# The methods a trait impl gives a type, by the trait's name, for the
# standard library's and serde's traits (a crate's own traits are read from
# the listing). Unlisted traits: their snake_case name.
TRAIT_METHODS = {
    'Clone': {'clone', 'clone_from'}, 'Debug': {'fmt'}, 'Display': {'fmt'},
    'PartialEq': {'eq', 'ne'}, 'PartialOrd': {'partial_cmp', 'lt', 'le', 'gt', 'ge'},
    'Ord': {'cmp', 'max', 'min', 'clamp'}, 'Hash': {'hash'}, 'Hasher': {'finish', 'write'},
    'Iterator': {'next', 'size_hint', 'nth', 'count', 'last', 'fold'},
    'DoubleEndedIterator': {'next_back'}, 'ExactSizeIterator': {'len'},
    'Error': {'source', 'description', 'cause'}, 'Write': {'write_str', 'write_char', 'write_fmt', 'write', 'flush'},
    'Read': {'read'}, 'Seek': {'seek'}, 'Future': {'poll'}, 'Serialize': {'serialize'},
    'Deserialize': {'deserialize'}, 'BitOr': {'bitor'}, 'BitAnd': {'bitand'}, 'BitXor': {'bitxor'},
    'BitOrAssign': {'bitor_assign'}, 'BitAndAssign': {'bitand_assign'}, 'BitXorAssign': {'bitxor_assign'},
    'ToOwned': {'to_owned'}, 'ToString': {'to_string'},
}


def trait_method_names(trait_path):
    name = trait_path.split('::')[-1]
    if name in TRAIT_METHODS:
        return TRAIT_METHODS[name]
    return {re.sub(r'(?<!^)([A-Z])', r'_\1', name).lower()}


KEYWORDS = {'pub', 'fn', 'const', 'unsafe', 'async', 'extern', 'c', '"C"', 'struct', 'enum',
            'union', 'trait', 'type', 'static', 'mut', 'macro', 'mod', 'use', 'auto'}
ATTRS = re.compile(r'^(?:#\[[^\]]*(?:\[[^\]]*\][^\]]*)*\]\s*)+')
PATH = re.compile(r'(?<![\w:])([a-z_][a-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+)')
WORD = re.compile(r'[A-Za-z_][A-Za-z0-9_]*')
# Overlapping: `ponko_mcp::Config::from_env` holds both `ponko_mcp::Config` and `Config::from_env`.
PAIR = re.compile(r'(?<![A-Za-z0-9_])(?=([A-Za-z_][A-Za-z0-9_]*)\s*::\s*(?:<[^>]*>\s*::\s*)?([A-Za-z_][A-Za-z0-9_]*))')
DOT = re.compile(r'\.\s*([a-z_][a-z0-9_]*)\b')


# Parsing: plain functions of their input, checked by `selftest`.

def path_at(body, match):
    """The path a PATH match starts, with generic arguments inside it dropped:
    `ponko::a::B<'a>::c` -> `ponko::a::B::c`."""
    path, i = match.group(1), match.end()
    while i < len(body) and body[i] == '<':
        depth = 0
        for j in range(i, len(body)):
            depth += body[j] == '<'
            depth -= body[j] == '>'
            if depth == 0:
                break
        rest = re.match(r'::([A-Za-z_][A-Za-z0-9_]*)', body[j + 1:])
        if not rest:
            break
        path += '::' + rest.group(1)
        i = j + 1 + rest.end()
    return path


def parse_line(line):
    """(kind, path) for one api/ line, or None for what isn't an item of its own.

    kind: fn, type, trait, const, static, macro, field, variant, impl, mod,
    assoc (an associated type of an impl), use.
    """
    body = ATTRS.sub('', line.strip())
    if body.startswith('impl'):
        return ('impl', body)
    match = PATH.search(body)
    if not match:
        return None
    path = path_at(body, match)
    head = body[:match.start()].split()
    if any(w not in KEYWORDS for w in head):
        return None  # `pub fn F::run_if(self, ponko::x::Y)`: a generic type's method
    words = set(head)
    if 'fn' in words:
        return ('fn', path)
    if words & {'struct', 'enum', 'union'}:
        return ('type', path)
    if 'trait' in words:
        return ('trait', path)
    if 'type' in words:
        parent = path.split('::')[-2]
        return ('assoc' if parent[:1].isupper() else 'type', path)
    if 'const' in words:
        return ('const', path)
    if 'static' in words:
        return ('static', path)
    if 'macro' in words:
        return ('macro', path)
    if 'mod' in words:
        return ('mod', path)
    if 'use' in words:
        return ('use', path)
    if head == ['pub']:
        last = path.split('::')[-1]
        return ('variant' if last[:1].isupper() else 'field', path)
    return None


def impl_parts(body):
    """(trait path or None, self type path) of an `impl ...` line."""
    rest = body[4:]
    if rest.startswith('<'):
        depth = 0
        for i, ch in enumerate(rest):
            depth += ch == '<'
            depth -= ch == '>'
            if depth == 0:
                rest = rest[i + 1:]
                break
    depth, split = 0, None
    for i, ch in enumerate(rest):
        depth += ch == '<'
        depth -= ch == '>'
        if depth == 0 and rest.startswith(' for ', i):
            split = i
    trait = None
    if split is not None:
        trait_match = PATH.search(rest[:split])
        trait = trait_match.group(1) if trait_match else None
        rest = rest[split + 5:]
    self_match = PATH.search(rest)
    return trait, self_match.group(1) if self_match else None


def strip_generics(path):
    return path.split('<', 1)[0]


def items_of(lines, crate=None):
    """The items of one crate's covered listing: {path: kind}, without trait
    impls' methods, fields, variants and associated types."""
    items, trait_methods, impls = {}, defaultdict(set), defaultdict(set)
    parsed = [p for p in (parse_line(l) for l in lines) if p]
    traits = {path for kind, path in parsed if kind == 'trait'}
    for kind, path in parsed:
        if kind == 'fn' and path.rsplit('::', 1)[0] in traits:
            trait_methods[path.rsplit('::', 1)[0]].add(path.rsplit('::', 1)[1])
        if kind == 'impl':
            trait, self_type = impl_parts(path)
            if trait and self_type:
                impls[self_type].add(trait)
    for kind, path in parsed:
        if kind in ('impl', 'field', 'variant', 'assoc', 'mod', 'use'):
            continue
        if crate and not (path == crate or path.startswith(crate + '::')):
            continue  # a trait impl's method on a foreign type (`f32::lerp`)
        parent, name = path.rsplit('::', 1)
        if kind in ('fn', 'const') and parent in impls and parent not in traits:
            if any(name in (trait_methods[t] if t in traits else trait_method_names(t))
                   for t in impls[parent]):
                continue
        items.setdefault(path, kind)
    # The prelude re-exports: an item the listing shows under the prelude and
    # under its own module is the same item.
    tails = defaultdict(set)
    for path in items:
        if '::prelude' not in path:
            tails[path.split('::')[-1] if not is_member(path) else '::'.join(path.split('::')[-2:])].add(path)
    for path in [p for p in items if '::prelude' in p]:
        tail = path.split('::')[-1] if not is_member(path) else '::'.join(path.split('::')[-2:])
        if tails.get(tail):
            del items[path]
    return items


def signatures(lines):
    """{owner: [lines]}: the lines that mention other types for each item, a
    field or variant's line under its type."""
    out = defaultdict(list)
    for line in lines:
        parsed = parse_line(line)
        if not parsed or parsed[0] in ('mod', 'use'):
            continue
        kind, path = parsed
        if kind == 'impl':  # its bounds name traits: `impl<T: OrTerms> Filter for Or<T>`
            _, self_type = impl_parts(path)
            if self_type:
                out[self_type].append(line)
            continue
        owner = path.rsplit('::', 1)[0] if kind in ('field', 'variant', 'assoc') else path
        out[owner].append(line)
    return out


def reach_through_signatures(results, sigs):
    """Mark `reached` the uncalled types and traits that a called (or reached)
    item takes or returns, or a field of one holds: games use them without
    naming them (`report.frames.p99`). Repeats until nothing changes."""
    pending = {p for p, r in results.items() if r[2] != 'called' and r[1] in ('type', 'trait')}
    users = {p for p, r in results.items() if r[2] == 'called'}
    patterns = {p: re.compile(rf'(?<![\w:]){re.escape(p)}(?![\w])') for p in pending}
    changed = True
    while changed and pending:
        changed = False
        for path in sorted(pending):
            for owner in users:
                if owner != path and any(patterns[path].search(l) for l in sigs.get(owner, ())):
                    crate, kind, _, _ = results[path]
                    results[path] = (crate, kind, 'reached', {f'via {owner}': 'signature'})
                    pending.discard(path)
                    users.add(path)
                    changed = True
                    break
    # A trait's methods are its shape, when the trait is used: implemented by
    # games, called by the engine (`IntoSystem::into_system` behind `.sim_system(f)`).
    for path, (crate, kind, status, _) in list(results.items()):
        parent = results.get(path.rsplit('::', 1)[0])
        if status != 'called' and kind == 'fn' and parent and parent[1] == 'trait' \
                and parent[2] in ('called', 'reached'):
            results[path] = (crate, kind, 'trait', {f'via {path.rsplit("::", 1)[0]}': 'signature'})


def is_member(path):
    parts = path.split('::')
    return len(parts) >= 3 and parts[-2][:1].isupper()


def scan(text):
    """(words, pairs, dotted names) of one file's text."""
    words = set(WORD.findall(text))
    pairs = {f'{a}::{b}' for a, b in PAIR.findall(text)}
    dots = set(DOT.findall(text))
    return words, pairs, dots


class Index:
    """Which files of which kind name what."""

    def __init__(self):
        self.words = defaultdict(set)
        self.pairs = defaultdict(set)
        self.dots = defaultdict(set)
        self.kind = {}

    def add(self, name, kind, text):
        self.kind[name] = kind
        words, pairs, dots = scan(text)
        for w in words:
            self.words[w].add(name)
        for p in pairs:
            self.pairs[p].add(name)
        for d in dots:
            self.dots[d].add(name)


def member_owners(items):
    """{member name: number of types that have a member of that name}."""
    owners = defaultdict(set)
    for path in items:
        if is_member(path):
            parent, name = path.rsplit('::', 1)
            owners[name].add(parent)
    return {name: len(types) for name, types in owners.items()}


def item_names(items):
    """{last segment: number of non-member items with that name}."""
    counts = defaultdict(int)
    for path in items:
        if not is_member(path):
            counts[path.rsplit('::', 1)[1]] += 1
    return counts


def aliases_in(text):
    """{public name: {source names}} from the renaming re-exports of a source
    file (`pub use joints::Joint3D as Joint`): callers inside the crate and
    the prelude often use the source name."""
    out = defaultdict(set)
    for use in re.findall(r'\bpub(?:\([^)]*\))?\s+use\s+[^;]*;', text, re.S):
        for source, public in re.findall(r'\b([A-Za-z_]\w*)\s+as\s+([A-Za-z_]\w*)', use):
            if source != public and public != '_':
                out[public].add(source)
    return out


def where_called(path, kind, index, owners, names, exclude=(), aliases=None):
    """(status, {file: kind}) for one item: status is `called`, `ambiguous`
    or `not called`. Files in `exclude` don't count (the item's own crate)."""
    parts = path.split('::')
    name = parts[-1]
    aliases = aliases or {}
    spellings = lambda n: {n} | aliases.get(n, set())
    files = set()
    maybe = set()
    words = lambda ns: set().union(*(index.words.get(n, set()) for n in ns))
    if kind == 'macro':
        files = words(spellings(name))
    elif is_member(path):
        owner_names = spellings(parts[-2])
        for owner in owner_names:
            files |= index.pairs.get(f'{owner}::{name}', set())
        called = index.dots.get(name, set()) | index.pairs.get(f'Self::{name}', set())
        if owners.get(name, 1) <= 1:
            files |= called
        else:
            with_owner = called & words(owner_names)
            files |= with_owner
            maybe = called - with_owner
    else:
        named = words(spellings(name))
        if names.get(name, 1) <= 1 or parts[0] == 'ponko_ffi':
            files |= named
        else:
            module = parts[-2]
            for n in spellings(name):
                files |= index.pairs.get(f'{module}::{n}', set())
            files |= named & index.words.get(module, set())
            maybe = named - files
    files = {f for f in files if f not in exclude}
    maybe = {f for f in maybe if f not in exclude}
    if files:
        return 'called', {f: index.kind[f] for f in files}
    if maybe:
        return 'ambiguous', {f: index.kind[f] for f in maybe}
    return 'not called', {}


def unstable(path):
    return any('unstable_' in part for part in path.split('::'))


# Side effects: reading the repository.

def rust_test_code(text):
    """A source file without its `#[cfg(test)]` modules and doc comments
    (the item's own unit tests and doctests)."""
    text = re.sub(r'^\s*//[/!].*$', '', text, flags=re.M)
    match = re.search(r'^\s*#\[cfg\(test\)\]\s*\n\s*mod \w+', text, re.M)
    return text[:match.start()] if match else text


def guide_snippets(text):
    """The code blocks of a guide page (inline snippets; included files are
    indexed as what they are)."""
    blocks = re.findall(r'^```[^\n]*\n(.*?)^```', text, re.M | re.S)
    inline = re.findall(r'`([^`\n]+)`', re.sub(r'^```.*?^```', '', text, flags=re.M | re.S))
    return '\n'.join(blocks + inline)


def build_index():
    """{crate: Index}: an index per crate, since a crate's own sources don't count."""
    index = Index()
    index.aliases = defaultdict(set)
    for path in list((ROOT / 'src').rglob('*.rs')) + list((ROOT / 'crates').glob('*/src/**/*.rs')):
        for public, sources in aliases_in(path.read_text(encoding='utf-8', errors='replace')).items():
            index.aliases[public] |= sources

    def add(path, kind, text=None):
        rel = path.relative_to(ROOT).as_posix()
        index.add(rel, kind, text if text is not None else path.read_text(encoding='utf-8', errors='replace'))

    for path in sorted((ROOT / 'examples').rglob('*')):
        rel = path.relative_to(ROOT).as_posix()
        if not path.is_file() or path.suffix not in ('.rs', '.c', '.cpp', '.h', '.hpp'):
            continue
        if rel.startswith(GAMES):
            add(path, 'game')
        elif rel.startswith('examples/cpp/'):
            name = path.name
            if name.startswith('cookbook'):
                add(path, 'cookbook')
            elif name.startswith('guide'):
                add(path, 'guide')
            elif name.startswith(('tests', 'abi_baseline', 'capability_schema', 'dimensions')):
                add(path, 'test')
            else:
                add(path, 'example')
        else:
            add(path, 'example')
    for path in sorted((ROOT / 'crates').glob('*/examples/**/*.rs')):
        add(path, 'example')
    for path in sorted((ROOT / 'tests').rglob('*.rs')):
        add(path, 'cookbook' if path.name.startswith('cookbook') else 'test')
    for path in sorted((ROOT / 'crates').glob('*/tests/**/*.rs')):
        add(path, 'test')
    for path in sorted((ROOT / 'src').rglob('*_test.rs')):
        add(path, 'test')
    for path in sorted((ROOT / 'book' / 'src').rglob('*.md')):
        add(path, 'guide', guide_snippets(path.read_text(encoding='utf-8')))
    for path in sorted((ROOT / 'crates' / 'ponko_ffi' / 'src').rglob('*.rs')):
        add(path, 'binding', rust_test_code(path.read_text(encoding='utf-8')))
    add(ROOT / 'include' / 'ponko.hpp', 'binding')
    for path in sorted((ROOT / 'crates').glob('*/src/**/*.rs')):
        rel = path.relative_to(ROOT).as_posix()
        if rel.startswith('crates/ponko_ffi/'):
            continue
        add(path, 'tool', rust_test_code(path.read_text(encoding='utf-8')))
    for path in sorted((ROOT / 'src').rglob('*.rs')):
        if not path.name.endswith('_test.rs'):
            add(path, 'tool', rust_test_code(path.read_text(encoding='utf-8')))
    return index


def own_files(crate, index):
    """The files that don't count for a crate's items: its own sources."""
    prefix = 'src/' if crate == 'ponko' else f'crates/{crate}/src/'
    # The crate's own binaries (`src/main.rs`, `src/bin/`) are users of its library.
    return {f for f in index.kind if f.startswith(prefix) and index.kind[f] != 'test'
            and f != f'{prefix}main.rs' and not f.startswith(f'{prefix}bin/')}


def load_items():
    """{crate: {path: kind}} of the covered API, {crate: [paths]} set apart,
    and the signature lines of every item."""
    covered, apart, sigs = {}, defaultdict(list), {}
    for crate_dir in sorted(API.iterdir()):
        if not crate_dir.is_dir():
            continue
        lines = []
        for path in sorted(crate_dir.glob('*.txt')):
            if path.stem == 'experimental':
                apart[crate_dir.name] += sorted(items_of(path.read_text(encoding='utf-8').splitlines(), crate_dir.name))
            else:
                lines += path.read_text(encoding='utf-8').splitlines()
        items = items_of(sorted(set(lines)), crate_dir.name)
        sigs.update(signatures(sorted(set(lines))))
        for path in [p for p in items if unstable(p)]:
            apart[crate_dir.name].append(path)
            del items[path]
        covered[crate_dir.name] = items
    return covered, apart, sigs


def module_of(path):
    """The module an item belongs to, for grouping: up to its type."""
    parts = path.split('::')
    while len(parts) > 1 and (parts[-1][:1].isupper() or len(parts) > 2 and parts[-2][:1].isupper()):
        parts = parts[:-1]
    if len(parts) > 1 and not path.split('::')[len(parts) - 1:][0][:1].isupper():
        parts = parts[:-1] if parts == path.split('::') else parts
    return '::'.join(parts)


def analyse():
    covered, apart, sigs = load_items()
    index = build_index()
    results = {}
    for crate, items in covered.items():
        owners = member_owners(items)
        names = item_names(items)
        exclude = own_files(crate, index)
        for path, kind in items.items():
            status, files = where_called(path, kind, index, owners, names, exclude, index.aliases)
            results[path] = (crate, kind, status, files)
    reach_through_signatures(results, sigs)
    return results, apart


def render(results, apart, decisions):
    total = len(results)
    by_status = defaultdict(list)
    for path, (crate, kind, status, files) in results.items():
        by_status[status].append(path)
    kinds = defaultdict(int)
    for path, (_, _, status, files) in results.items():
        for kind in set(files.values()) & set(KINDS):
            kinds[kind] += 1
    out = [
        '# Public API by use',
        '',
        f'Generated by `python3 scripts/api-usage.py report` ({ISSUE}); do not edit by hand.',
        'It lists, from the API snapshots in `api/`, the public items that nothing calls by name',
        'outside their own crate: no example, reference game, cookbook recipe, guide snippet,',
        'test, binding or other crate. The freeze review (#478) reads it next to the names and',
        'shapes. `scripts/api-usage.py`\'s docstring says what counts as a call; matching is by',
        'name, so the list errs on the side of "called".',
        '',
        f'{total} covered items (types, traits, functions, methods, constants, statics, macros;',
        'not fields, variants or trait impls\' methods):',
        '',
        '| Status | Items |',
        '|---|---|',
        f'| called | {len(by_status["called"])} |',
        f'| reached: a type or trait no one names, but a called item takes or returns it | {len(by_status["reached"])} |',
        f'| a trait\'s method: part of its trait, which is called or reached | {len(by_status["trait"])} |',
        f'| ambiguous: only a name other types share is called | {len(by_status["ambiguous"])} |',
        f'| not called | {len(by_status["not called"])} |',
        '',
        'Items called from each kind of place (an item can count in several):',
        '',
        '| Place | Items |',
        '|---|---|',
    ]
    out += [f'| {kind} | {kinds[kind]} |' for kind in KINDS]
    out += ['', f'{sum(len(v) for v in apart.values())} more items are outside the promise already '
            '(experimental, or named `unstable_`) and are not listed.', '']
    out += ['## Not called, by module', '',
            'Each item carries the decision recorded for it in `scripts/api-usage-decisions.txt`:',
            '**use** (a real use was added), **unstable** (renamed `unstable_`), **remove** (gone, with',
            'a migration note), **keep** (part of a type\'s shape, with the reason). An item without',
            'one needs a decision before the freeze.', '']
    modules = defaultdict(list)
    for path in by_status['not called']:
        modules[module_of(path)].append(path)
    for module in sorted(modules):
        out += [f'### `{module}`', '', '| Item | Kind | Decision |', '|---|---|---|']
        for path in sorted(modules[module]):
            decision = decisions.get(path) or decisions.get(module) or '**undecided**'
            out.append(f'| `{path}` | {results[path][1]} | {decision} |')
        out.append('')
    out += ['## Ambiguous', '',
            'Their name is called somewhere, but only on a type it may not be (several types have',
            'a member of that name, and the file never names this one). Most are called through a',
            'value whose type the file doesn\'t spell out; not checked one by one, so the freeze',
            'review can look at any it doubts with `api-usage.py item <path>`.', '']
    modules = defaultdict(list)
    for path in by_status['ambiguous']:
        modules[module_of(path)].append(path)
    for module in sorted(modules):
        out.append(f'- `{module}`: ' + ', '.join(f'`{p.split("::")[-2]}::{p.split("::")[-1]}`'
                                                  if is_member(p) else f'`{p.split("::")[-1]}`'
                                                  for p in sorted(modules[module])))
    return '\n'.join(out) + '\n'


DECISIONS = ROOT / 'scripts' / 'api-usage-decisions.txt'
DECISION_WORDS = ('**use**', '**unstable**', '**remove**', '**keep**')


def parse_decisions(text):
    """{item or module path: decision} from the decisions file's text."""
    decisions = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        item, _, decision = line.partition(' ')
        decisions[item] = decision.strip()
    return decisions


def load_decisions():
    return parse_decisions(DECISIONS.read_text(encoding='utf-8')) if DECISIONS.exists() else {}


def decision_errors(decisions, paths):
    """What is wrong with `decisions` for the covered item `paths`: a key
    that is neither an item nor a module of one, or a decision without a
    known first word."""
    modules = {module_of(p) for p in paths}
    errors = []
    for key, decision in decisions.items():
        if key not in paths and key not in modules:
            errors.append(f'{key}: not a covered item or module (renamed, removed or experimental?)')
        if not decision.startswith(DECISION_WORDS):
            errors.append(f'{key}: the decision starts with one of {", ".join(DECISION_WORDS)}')
    return errors


def main(argv):
    command = argv[1] if len(argv) > 1 else ''
    if command == 'report':
        results, apart = analyse()
        text = render(results, apart, load_decisions())
        if '--stdout' in argv:
            print(text, end='')
        else:
            REPORT.write_text(text, encoding='utf-8')
            print(f'api-usage.py: wrote {REPORT.relative_to(ROOT)}')
        return 0
    if command == 'item' and len(argv) == 3:
        results, _ = analyse()
        if argv[2] not in results:
            print(f'{argv[2]} is not a covered item')
            return 1
        crate, kind, status, files = results[argv[2]]
        print(f'{argv[2]} ({crate}, {kind}): {status}')
        for f, k in sorted(files.items()):
            print(f'  {k:8} {f}')
        return 0
    if command == 'check':
        covered, _, _ = load_items()
        paths = {p for items in covered.values() for p in items}
        errors = decision_errors(load_decisions(), paths)
        for e in errors:
            print(f'api-usage.py: {DECISIONS.relative_to(ROOT)}: {e}')
        if not errors:
            print(f'api-usage.py: {len(load_decisions())} decisions, each for a covered item or module')
        return 1 if errors else 0
    if command == 'selftest':
        return selftest()
    print(__doc__)
    return 2


def selftest():
    decisions = parse_decisions('# a comment\n\nponko::a::B::c **keep**: why\nponko_ffi **use**\nponko::x::y oops\n')
    assert decisions == {'ponko::a::B::c': '**keep**: why', 'ponko_ffi': '**use**', 'ponko::x::y': 'oops'}
    errors = decision_errors(decisions, {'ponko::a::B::c', 'ponko_ffi::PONKO_X'})
    assert errors == ['ponko::x::y: not a covered item or module (renamed, removed or experimental?)',
                      'ponko::x::y: the decision starts with one of **use**, **unstable**, **remove**, **keep**'], errors
    assert parse_line('pub fn ponko::game::Game::new() -> Self') == ('fn', 'ponko::game::Game::new')
    assert parse_line('#[non_exhaustive] pub struct ponko::render::Camera') == ('type', 'ponko::render::Camera')
    assert parse_line('pub ponko::render::Camera::fov: f32') == ('field', 'ponko::render::Camera::fov')
    assert parse_line('pub ponko::render::Mode::Fast = 1') == ('variant', 'ponko::render::Mode::Fast')
    assert parse_line('pub type ponko::a::B::Output = f32') == ('assoc', 'ponko::a::B::Output')
    assert parse_line('pub type ponko::a::Result<T> = core::result::Result<T>') == ('type', 'ponko::a::Result')
    assert parse_line('pub trait ponko::plugin::Plugin') == ('trait', 'ponko::plugin::Plugin')
    assert parse_line('pub macro ponko::profile_scope!') == ('macro', 'ponko::profile_scope')
    assert parse_line('#[no_mangle] pub unsafe c fn ponko_ffi::ponko_spawn(*mut u8)') == ('fn', 'ponko_ffi::ponko_spawn')
    assert parse_line('pub const ponko_ffi::PONKO_ABI_VERSION: u32') == ('const', 'ponko_ffi::PONKO_ABI_VERSION')
    assert parse_line('pub mod ponko') is None
    assert parse_line('pub fn F::run_if<W>(self, impl ponko::state::IntoCondition<W>)') is None
    assert parse_line("pub fn ponko::ai::TraceLine<'a>::clone(&self) -> Self") == ('fn', 'ponko::ai::TraceLine::clone')
    assert impl_parts('impl core::clone::Clone for ponko::a::B') == ('core::clone::Clone', 'ponko::a::B')
    assert impl_parts('impl<T> ponko::a::C<T>') == (None, 'ponko::a::C')

    lines = [
        'pub trait ponko::plugin::Plugin',
        'pub fn ponko::plugin::Plugin::build(&self)',
        'pub struct ponko::a::Thing',
        'impl ponko::a::Thing',
        'impl core::clone::Clone for ponko::a::Thing',
        'impl ponko::plugin::Plugin for ponko::a::Thing',
        'pub fn ponko::a::Thing::clone(&self) -> Self',
        'pub fn ponko::a::Thing::build(&self)',
        'pub fn ponko::a::Thing::spin(&mut self)',
        'pub fn ponko::a::Thing::len(&self) -> usize',
        'pub ponko::a::Thing::speed: f32',
        'pub struct ponko::b::Other',
        'impl ponko::b::Other',
        'pub fn ponko::b::Other::spin(&mut self)',
        'pub fn ponko::b::helper() -> u32',
        'pub fn ponko::c::helper() -> u32',
        'impl ponko::a::Lerp for f32',
        'pub fn f32::lerp(&self)',
        'pub struct ponko::prelude::Other',
    ]
    items = items_of(lines, 'ponko')
    assert 'f32::lerp' not in items and 'ponko::prelude::Other' not in items
    assert 'ponko::a::Thing::clone' not in items and 'ponko::a::Thing::build' not in items
    assert 'ponko::a::Thing::speed' not in items
    assert items['ponko::a::Thing::spin'] == 'fn' and items['ponko::plugin::Plugin::build'] == 'fn'
    assert 'ponko::a::Thing::len' in items, 'an inherent method keeps its std-sounding name when no std trait is implemented'
    owners = member_owners(items)
    names = item_names(items)
    assert owners['spin'] == 2 and names['helper'] == 2

    index = Index()
    index.add('tests/a.rs', 'test', 'let t = Thing::default(); t.spin(); b::helper();')
    index.add('examples/x.rs', 'example', 'other.spin(); let o = make(); ponko::d::Deep::call();')
    index.add('book/src/g.md', 'guide', 'thing.len()')
    status, files = where_called('ponko::a::Thing::spin', 'fn', index, owners, names)
    assert status == 'called' and files == {'tests/a.rs': 'test'}, files
    status, files = where_called('ponko::b::Other::spin', 'fn', index, owners, names)
    assert status == 'ambiguous', 'spin is called, but never next to the name Other'
    assert where_called('ponko::b::helper', 'fn', index, owners, names)[0] == 'called'
    assert 'Deep::call' in index.pairs and 'd::Deep' in index.pairs, 'pairs overlap'
    assert where_called('ponko::c::helper', 'fn', index, owners, names)[0] == 'ambiguous'
    assert where_called('ponko::a::Thing::len', 'fn', index, owners, names)[0] == 'called', 'unique member name'
    assert where_called('ponko::a::Thing::spin', 'fn', index, owners, names, {'tests/a.rs'})[0] == 'ambiguous'
    assert where_called('ponko::plugin::Plugin', 'trait', index, owners, names)[0] == 'not called'
    aliases = aliases_in('pub use crate::a::{Plugin2 as Plugin, X};\nuse y::Z as W;')
    assert aliases == {'Plugin': {'Plugin2'}}, aliases
    index.add('tests/b.rs', 'test', 'impl Plugin2 for Mine {}')
    assert where_called('ponko::plugin::Plugin', 'trait', index, owners, names, (), aliases)[0] == 'called'

    sigs = signatures(['pub fn ponko::a::Thing::report(&self) -> ponko::a::Report',
                       'pub ponko::a::Report::frames: ponko::a::Frames',
                       'pub struct ponko::a::Lonely'])
    results = {'ponko::a::Thing::report': ('ponko', 'fn', 'called', {}),
               'ponko::a::Report': ('ponko', 'type', 'not called', {}),
               'ponko::a::Frames': ('ponko', 'type', 'not called', {}),
               'ponko::a::Lonely': ('ponko', 'type', 'not called', {})}
    reach_through_signatures(results, sigs)
    assert results['ponko::a::Report'][2] == 'reached' and results['ponko::a::Frames'][2] == 'reached'
    assert results['ponko::a::Lonely'][2] == 'not called'
    code = 'pub fn a() {}\n/// b()\nfn c() { d() }\n#[cfg(test)]\nmod tests {\n    fn t() { e() }\n}\n'
    assert 'e()' not in rust_test_code(code) and 'b()' not in rust_test_code(code) and 'd()' in rust_test_code(code)
    assert 'x()' in guide_snippets('text `x()` more\n```rust\ny()\n```\n')
    assert 'y()' in guide_snippets('text `x()` more\n```rust\ny()\n```\n')
    assert unstable('ponko::ui::UiLayout::unstable_style') and not unstable('ponko::ui::UiLayout::style')
    assert module_of('ponko::a::Thing::spin') == 'ponko::a', module_of('ponko::a::Thing::spin')
    assert module_of('ponko::b::helper') == 'ponko::b', module_of('ponko::b::helper')
    assert module_of('ponko::a::Thing') == 'ponko::a'
    print('api-usage.py: self-test passed')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
