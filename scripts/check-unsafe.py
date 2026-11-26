#!/usr/bin/env python3
"""Hold every unsafe site outside the ECS to its test (the project rule).

"Every `unsafe` block needs a test that exercises it" (AGENTS.md). The ECS
(crates/ponko_ecs) runs its own tests under Miri; this script covers the rest
of the workspace: the engine (src/) and the crates' library and binary
sources (crates/*/src, the benchmark binary crates/ponko_bench included),
except
  - crates/ponko_ecs (its own Miri jobs),
  - the bodies of `extern "C" fn` exports in crates/ponko_ffi/src: each export
    is tested or on a reasoned allowlist, which scripts/check-ffi-coverage.py
    holds (the helpers of the C API crate are listed here like any other code),
  - test code: tests/, examples/, benches/, `#[cfg(test)]` modules and
    `#[test]` functions (a test needs no test).

An unsafe site is an `unsafe { .. }` block, an `unsafe fn` with a body, an
`unsafe impl` or an `unsafe extern` block. Sites are grouped by the item that
holds them (`function`, `Type::method`, `impl Send for Type`), and each item
has one line in scripts/unsafe-inventory.txt:

    <path> | <item> | <test> | <miri>

<test> is `<test file>::<test function>` (a Rust test, a check of a test
program without the harness (`harness = false` in Cargo.toml: any function
of the file), or a function of a C or C++ test program in examples/cpp;
`<file>::<module>::<function>` for a test a macro makes in a module), a
browser runner (`web/smoke/*.mjs`) or a device runner (`scripts/*.sh`, such
as the iOS simulator check), and must exist. <miri> is `miri` (the
test runs under Miri: no GPU, window, audio device or C library),
`miri(<features>)` (the same, with those engine features on), or the reason
it can't: `c-library` (it calls a C library), `gpu`, `os` (signals, sockets,
processes), `windows`, `macos`, `wasm`, `ios` (it runs on the iOS
simulator), `android` (it runs in an app on the Android emulator),
`window` (a real window) or
`sanitizers` (C API code, run under AddressSanitizer with the C and C++ test
programs instead). `window` is a check a person runs, in a real window
(the guide's "Unsafe code" lists them).
The check fails for an unsafe item without a line (with the line to add), a
line whose item has no unsafe code any more, a test that does not exist, and
a malformed line.

    python3 scripts/check-unsafe.py              # check
    python3 scripts/check-unsafe.py --list       # every unsafe site, by item
    python3 scripts/check-unsafe.py --miri       # the Miri commands, one per line
    python3 scripts/check-unsafe.py --selftest   # the parser on fixed snippets
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
INVENTORY = ROOT / 'scripts' / 'unsafe-inventory.txt'
NO_MIRI = {'c-library', 'gpu', 'os', 'windows', 'macos', 'wasm', 'ios', 'android', 'window', 'sanitizers'}
TEST_DIRS = {'tests', 'examples', 'benches'}
# The ECS has its own Miri jobs.
SKIPPED_CRATES = {'ponko_ecs'}

# What is not code: raw strings, strings, byte and character literals, block
# and line comments. They are blanked (newlines kept) so offsets still match.
NOT_CODE = re.compile(
    r"""(?P<raw>b?r(?P<hashes>\#*)".*?"(?P=hashes))"""
    r"""|(?P<char>b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^'\\\n])')"""
    r"""|(?P<string>b?c?"(?:\\.|[^"\\])*")"""
    r"""|(?P<block>/\*(?:[^*]|\*(?!/))*\*/)"""
    r"""|(?P<line>//[^\n]*)""",
    re.DOTALL,
)


def code_only(text):
    def blank(match):
        s = match.group(0)
        kept = re.sub(r'[^\n]', ' ', s)
        if match.lastgroup in ('raw', 'string'):
            return '"' + kept[1:-1] + '"'
        return kept
    return NOT_CODE.sub(blank, text)


_PAIRS = {}


def matching(code, open_at):
    """Offset of the brace closing the one at `open_at`."""
    key = id(code)
    if key not in _PAIRS or _PAIRS[key][0] is not code:
        pairs, stack = {}, []
        for m in re.finditer(r'[{}]', code):
            if m.group() == '{':
                stack.append(m.start())
            elif stack:
                pairs[stack.pop()] = m.start()
        _PAIRS.clear()
        _PAIRS[key] = (code, pairs)
    return _PAIRS[key][1].get(open_at, len(code))


def body_start(code, at):
    """Offset of the `{` opening the item whose header starts at `at`, or None
    when a `;` comes first (a declaration). Parentheses, brackets and angle
    brackets of the signature are skipped."""
    depth = 0
    i = at
    while i < len(code):
        c = code[i]
        if c in '([':
            depth += 1
        elif c in ')]':
            depth -= 1
        elif depth == 0 and c == '{':
            return i
        elif depth == 0 and c == ';':
            return None
        i += 1
    return None


def attributes_before(code, at):
    """The attributes (`#[..]`) right before offset `at`."""
    i = at
    while True:
        j = i
        while j > 0 and code[j - 1].isspace():
            j -= 1
        if j == 0 or code[j - 1] != ']':
            break
        depth, k = 0, j - 1
        while k >= 0:
            if code[k] == ']':
                depth += 1
            elif code[k] == '[':
                depth -= 1
                if depth == 0:
                    break
            k -= 1
        m = k - 1
        if m >= 0 and code[m] == '!':
            m -= 1
        if m < 0 or code[m] != '#':
            break
        i = m
    return code[i:at]


def is_test_attr(attrs):
    if re.search(r'#\[\s*(?:\w+::)*test\b', attrs):
        return True
    for cfg in re.findall(r'#\[\s*cfg\s*\((.*?)\)\s*\]', attrs, re.DOTALL):
        if re.search(r'\btest\b', cfg) and not re.search(r'not\s*\(\s*test\s*\)', cfg):
            return True
    return False


ITEM = re.compile(r'\b(?:(fn)\s+(\w+)|(impl)\b|(mod)\s+(\w+)|(trait)\s+(\w+)|(macro_rules)!\s*(\w+))')


def impl_name(header):
    """`impl<T: X> Trait<T> for Type<T> where ..` -> `Type`; `impl Type` -> `Type`."""
    header = re.sub(r'\bwhere\b.*', '', header, flags=re.DOTALL)
    header = header[4:].strip()
    if header.startswith('<'):
        depth = 0
        for i, c in enumerate(header):
            depth += c == '<'
            depth -= c == '>'
            if depth == 0:
                header = header[i + 1:]
                break
    target = re.split(r'\bfor\b', header)[-1]
    words = re.findall(r'[A-Za-z_]\w*', re.sub(r'<.*', '', target, flags=re.DOTALL))
    return words[-1] if words else '?'


def scopes(code):
    """[(start, end, kind, name, test)] for every fn, impl, mod and trait body."""
    found = []
    for m in ITEM.finditer(code):
        kind = m.group(1) or m.group(3) or m.group(4) or m.group(6) or m.group(8)
        name = m.group(2) or m.group(5) or m.group(7) or m.group(9)
        if kind == 'macro_rules':
            kind, name = 'macro', name + '!'
        start = body_start(code, m.end())
        if start is None:
            continue
        if kind == 'impl':
            name = impl_name(code[m.start():start])
        head = m.start()
        window = code[max(0, head - 120):head]
        prefix = re.search(r'(?:(?:pub(?:\s*\([^)]*\))?|const|async|unsafe|extern\s+"[^"]*"|default)\s+)*$', window)
        head -= len(window) - prefix.start()
        found.append((start, matching(code, start), kind, name, is_test_attr(attributes_before(code, head))))
    return found


UNSAFE = re.compile(r'\bunsafe\b')


def unsafe_sites(path, text):
    """[(line, item, form, inside_export)] for every unsafe site of production
    code in one file."""
    code = code_only(text)
    spans = scopes(code)
    sites = []
    for m in UNSAFE.finditer(code):
        after = code[m.end():]
        rest = after.lstrip()
        form = None
        name = None
        if rest.startswith('{'):
            form = 'block'
        elif rest.startswith('impl'):
            form = 'impl'
        elif rest.startswith('trait'):
            form = 'trait'
        else:
            fn = re.match(r'(?:extern\s+"[^"]*"\s+)?fn\s+(\w+)', rest)
            if fn:
                if body_start(code, m.end() + len(after) - len(rest) + fn.end()) is None:
                    continue  # a declaration in a trait: its impls are the sites
                form, name = 'fn', fn.group(1)
            elif re.match(r'extern\s+"[^"]*"\s*\{', rest):
                form = 'extern block'
        if form is None:
            continue  # `unsafe(no_mangle)`, a fn-pointer type, a lint name
        offset = m.start()
        inside = [s for s in spans if s[0] < offset < s[1]]
        if any(s[4] for s in inside):
            continue
        attrs = attributes_before(code, offset)
        if is_test_attr(attrs):
            continue
        fns = [s for s in inside if s[2] == 'fn']
        impls = [s for s in inside if s[2] in ('impl', 'trait')]
        export = False
        macros = [x for x in inside if x[2] == 'macro']
        if macros:
            item = 'macro ' + macros[-1][3]
        elif form == 'impl':
            header = code[m.end():body_start(code, m.end()) or m.end() + 80].strip()[len('impl'):].lstrip()
            if header.startswith('<'):  # the impl's own generics: `impl<T: X> Trait for Type<T>`
                depth = 0
                for i, c in enumerate(header):
                    depth += (c == '<') - (c == '>')
                    if depth == 0:
                        header = header[i + 1:]
                        break
            item = 'impl ' + ' '.join(header.split())
            item = re.sub(r'\s*\{.*', '', item)
        elif form == 'trait':
            item = 'trait ' + re.match(r'trait\s+(\w+)', rest).group(1)
        else:
            if form == 'fn':
                owner = impls[-1] if impls and (not fns or impls[-1][0] > fns[-1][0]) else None
                item = f'{owner[3]}::{name}' if owner else name
                export = bool(re.match(r'\s*extern\s+"C"', text[m.end():]))
            elif fns:
                fn = fns[-1]
                owner = [s for s in impls if s[0] < fn[0] < s[1]]
                outer = [s for s in fns if s is not fn]
                if owner and not (outer and owner[-1][0] < outer[-1][0]):
                    item = f'{owner[-1][3]}::{fn[3]}'
                else:
                    item = fn[3]
                header = code[max(0, fn[0] - 2000):fn[0]]
                last = list(re.finditer(rf'\bfn\s+{fn[3]}\b', header))
                if last:
                    lead = text[max(0, fn[0] - 2000):fn[0]][:last[-1].start()][-40:]
                    export = bool(re.search(r'extern\s+"C"\s*$', lead))
            elif [x for x in inside if x[2] == 'mod']:
                item = 'mod ' + [x for x in inside if x[2] == 'mod'][-1][3]
            else:
                item = '(top level)'
            if form == 'extern block':
                item = f'extern block in {item}'
        line = code.count('\n', 0, offset) + 1
        sites.append((line, item, form, export))
    return sites


def source_files():
    files = sorted((ROOT / 'src').rglob('*.rs'))
    for crate in sorted((ROOT / 'crates').iterdir()):
        if crate.name in SKIPPED_CRATES or not (crate / 'src').is_dir():
            continue
        files += sorted((crate / 'src').rglob('*.rs'))
    files = [f for f in files if not TEST_DIRS & set(f.relative_to(ROOT).parts)]
    tests_only = test_modules(files)
    return [f for f in files if f not in tests_only]


def test_modules(files):
    """The files of modules declared under a test cfg (`#[cfg(test)] mod x;`),
    with their submodules: test code."""
    found = set()
    for f in files:
        code = code_only(f.read_text(encoding='utf-8'))
        here = f.parent if f.name in ('mod.rs', 'lib.rs', 'main.rs') else f.parent / f.stem
        for m in re.finditer(r'\bmod\s+(\w+)\s*;', code):
            if is_test_attr(attributes_before(code, m.start())):
                found.update(p for p in (here / f'{m.group(1)}.rs', here / m.group(1) / 'mod.rs') if p.exists())
                found.update((here / m.group(1)).rglob('*.rs'))
    return found


FFI_SRC = ROOT / 'crates' / 'ponko_ffi' / 'src'
FFI_TESTS = ROOT / 'crates' / 'ponko_ffi' / 'tests'
CALL = re.compile(r'\b([A-Za-z_]\w*)\s*(?:!|\(|::<)')


def ffi_functions():
    """Every fn and macro of the C API crate: dicts of file, item, name,
    export, private and the names its body calls."""
    found = []
    for f in sorted(FFI_SRC.rglob('*.rs')):
        rel = f.relative_to(ROOT).as_posix()
        text = f.read_text(encoding='utf-8')
        code = code_only(text)
        spans = scopes(code)
        for start, end, kind, name, test in spans:
            if test or kind not in ('fn', 'macro'):
                continue
            owner = [s for s in spans if s[2] in ('impl', 'trait') and s[0] < start < s[1]]
            outer_fn = [s for s in spans if s[2] == 'fn' and s[0] < start < s[1]]
            if owner and not (outer_fn and owner[-1][0] < outer_fn[-1][0]):
                item = f'{owner[-1][3]}::{name}'
            elif kind == 'macro':
                item = f'macro {name}'
            else:
                item = name
            head = code[:start]
            at = head.rfind('fn ' + name) if kind == 'fn' else head.rfind('macro_rules')
            lead = text[max(0, at - 60):at]
            found.append({
                'file': rel,
                'item': item,
                'name': name.rstrip('!'),
                'export': kind == 'fn' and bool(re.search(r'extern\s+"C"\s*$', lead)),
                # Callers in other files only for `pub` items of the file itself.
                'private': kind == 'macro' or bool(outer_fn) or any(x[2] == 'mod' and x[0] < start < x[1] for x in spans)
                or not re.search(r'\bpub\b[^\n;{}]*$', head[max(0, at - 60):at]),
                'calls': set(CALL.findall(code[start:end])),
            })
    return found


def ffi_tests():
    """{export name: '<test file>::<test fn>'}: the first test that calls the
    export, itself or through a function of its file it names (a system it
    registers, a helper it calls)."""
    tests = {}
    for f in sorted(FFI_TESTS.glob('*.rs')):
        rel = f.relative_to(ROOT).as_posix()
        code = code_only(f.read_text(encoding='utf-8'))
        fns = {}
        roots = []
        for start, end, kind, name, test in scopes(code):
            if kind != 'fn':
                continue
            names = set(re.findall(r'\b[A-Za-z_]\w*\b', code[start:end]))
            fns.setdefault(name, set()).update(names)
            if re.search(r'#\[\s*test\b', attributes_before(code, code.rfind('fn ' + name, 0, start))):
                roots.append(name)
        for root in roots:
            seen, queue = {root}, [root]
            while queue:
                for called in fns.get(queue.pop(), ()):
                    if called.startswith('ponko_'):
                        tests.setdefault(called, f'{rel}::{root}')
                    elif called in fns and called not in seen:
                        seen.add(called)
                        queue.append(called)
    native = ROOT / 'examples' / 'cpp'
    for f in sorted(native.glob('*.c')) + sorted(native.glob('*.cpp')):
        rel = f.relative_to(ROOT).as_posix()
        code = code_only(f.read_text(encoding='utf-8'))
        for name, start, end in c_functions(code):
            for called in re.findall(r'\b(ponko_\w+)', code[start:end]):
                tests.setdefault(called, f'{rel}::{name}')
    return tests


C_FUNCTION = re.compile(r'^[A-Za-z_][^\n;=]*?\b([A-Za-z_]\w*)\s*\([^;{}]*\)\s*(?:const\s*)?\{', re.MULTILINE)


def c_functions(code):
    """[(name, body start, body end)] of the top-level functions of a C or
    C++ file (definitions that start a line)."""
    found = []
    for m in C_FUNCTION.finditer(code):
        if m.group(1) in ('if', 'for', 'while', 'switch', 'return'):
            continue
        start = m.end() - 1
        found.append((m.group(1), start, matching(code, start)))
    return found


def ffi_derived():
    """{(path, item): (test, via export)} for the C API crate's helpers that a
    tested export reaches through the calls in its body."""
    fns = ffi_functions()
    tests = ffi_tests()
    by_item = {(f['file'], f['item']): f for f in fns}
    callers = {}
    for f in fns:
        for name in f['calls']:
            callers.setdefault(name, []).append(f)
    derived = {}
    for key, helper in by_item.items():
        if helper['export']:
            continue
        seen, queue = {id(helper)}, [helper]
        while queue:
            current = queue.pop(0)
            if current['export'] and current['name'] in tests:
                derived[key] = (tests[current['name']], current['name'])
                break
            for caller in callers.get(current['name'], ()):
                if id(caller) in seen:
                    continue
                if current['private'] and caller['file'] != current['file']:
                    continue
                seen.add(id(caller))
                queue.append(caller)
    return derived


def all_sites():
    """{(path, item): [lines]} of every unsafe site this script covers."""
    items = {}
    for f in source_files():
        rel = f.relative_to(ROOT).as_posix()
        for line, item, _form, export in unsafe_sites(rel, f.read_text(encoding='utf-8')):
            if export and rel.startswith('crates/ponko_ffi/'):
                continue
            items.setdefault((rel, item), []).append(line)
    return items


def inventory():
    entries, problems = {}, []
    for number, raw in enumerate(INVENTORY.read_text(encoding='utf-8').splitlines(), 1):
        line = raw.split('#', 1)[0].strip() if raw.lstrip().startswith('#') else raw.strip()
        if not line:
            continue
        cells = [c.strip() for c in line.split('|')]
        where = f'{INVENTORY.name}:{number}'
        if len(cells) != 4 or not all(cells):
            problems.append(f'{where}: expected "<path> | <item> | <test file>::<test> | <miri>"')
            continue
        path, item, test, miri = cells
        if (path, item) in entries:
            problems.append(f'{where}: {path} | {item} is listed twice')
        if not (miri == 'miri' or re.fullmatch(r'miri\([\w,-]+\)', miri) or miri in NO_MIRI):
            problems.append(f'{where}: Miri column "{miri}": write miri, miri(<features>) or one of {", ".join(sorted(NO_MIRI))}')
        entries[(path, item)] = (test, miri, number)
    return entries, problems


def split_test(test):
    """'<file>::<mod>::<fn>' -> (file, [mods], fn), or None."""
    m = re.fullmatch(r'(.+?\.(?:rs|c|cpp))::((?:\w+::)*)(\w+)', test)
    if not m:
        return None
    return m.group(1), [x for x in m.group(2).split('::') if x], m.group(3)


def harnessless_tests():
    """The files of test programs without the test harness (`[[test]]` with
    `harness = false` in a Cargo.toml): their checks are plain functions."""
    found = set()
    for manifest in [ROOT / 'Cargo.toml'] + sorted((ROOT / 'crates').glob('*/Cargo.toml')):
        text = manifest.read_text(encoding='utf-8')
        for block in re.split(r'^\[', text, flags=re.MULTILINE):
            if not block.startswith('[test]]') or not re.search(r'^harness\s*=\s*false', block, re.MULTILINE):
                continue
            name = re.search(r'^name\s*=\s*"([^"]+)"', block, re.MULTILINE)
            path = re.search(r'^path\s*=\s*"([^"]+)"', block, re.MULTILINE)
            if path:
                file = manifest.parent / path.group(1)
            elif name:
                file = manifest.parent / 'tests' / f'{name.group(1)}.rs'
            else:
                continue
            found.add(file.relative_to(ROOT).as_posix())
    return found


def test_exists(test):
    """None if `<file>::<test fn>` names a test function, else the problem.
    In a Rust file the function is a `#[test]`, or one that a macro of the
    file turns into tests (it is named in the macro's call); in a C or C++
    test program it is a function of the program."""
    if test.endswith('.mjs') or (test.startswith('scripts/') and test.endswith('.sh')):
        return None if (ROOT / test).is_file() else f'{test} does not exist'
    parts = split_test(test)
    if not parts:
        return 'write the test as <file>::<test function> (or <file>::<module>::<test function>)'
    file, _mods, name = parts
    path = ROOT / file
    if not path.is_file():
        return f'{file} does not exist'
    code = code_only(path.read_text(encoding='utf-8'))
    if file in harnessless_tests():
        if re.search(rf'\bfn\s+{re.escape(name)}\s*[(<]', code):
            return None
        return f'no check {name} in {file} (a test program without the harness)'
    if not file.endswith('.rs'):
        if any(n == name for n, _s, _e in c_functions(code)):
            return None
        return f'no function {name} in {file}'
    for m in re.finditer(rf'\bfn\s+{re.escape(name)}\s*[(<]', code):
        if re.search(r'#\[\s*(?:\w+::)*test\b', attributes_before(code, m.start())):
            return None
        for call in re.finditer(r'\b\w+!\s*\(', code):
            close = code.find(');', call.end())
            if re.search(rf'\b{re.escape(name)}\b', code[call.end():close]):
                return None
    return f'no #[test] fn {name} in {file}'


def check():
    sites = all_sites()
    entries, problems = inventory()
    derived = ffi_derived()
    for key in sorted(sites.keys() - entries.keys() - derived.keys()):
        path, item = key
        lines = ', '.join(map(str, sites[key]))
        problems.append(
            f'{path}:{sites[key][0]}: unsafe code in {item} (line {lines}) has no test: no line in '
            f'scripts/{INVENTORY.name}'
            + (', and no tested C function reaches it' if path.startswith('crates/ponko_ffi/') else '')
            + '. Write a test that runs it, then add:\n'
            f'    {path} | {item} | <test file>::<test fn> | <miri, miri(<features>) or why not>')
    for key in sorted(entries.keys() - sites.keys()):
        path, item = key
        problems.append(f'{INVENTORY.name}:{entries[key][2]}: {path} | {item} has no unsafe code any more; remove the line.')
    harnessless = harnessless_tests()
    for key, (test, miri, number) in sorted(entries.items(), key=lambda kv: kv[1][2]):
        problem = test_exists(test)
        if not problem and miri.startswith('miri') and (split_test(test) or ('',))[0] in harnessless:
            problem = ('a check of a test program without the harness is not run under Miri by name: '
                       'give the reason it cannot run instead')
        if problem:
            problems.append(f'{INVENTORY.name}:{number}: {key[0]} | {key[1]}: {problem}')
    for p in problems:
        print(p)
    if problems:
        print(f'\n{len(problems)} problem(s). See the docstring of scripts/check-unsafe.py and '
              '"Unsafe code" in book/src/contributing.md.')
        return 1
    listed = len(sites.keys() & entries.keys())
    print(f'{len(sites)} unsafe items outside the ECS, each with a test: {listed} listed, '
          f'{len(sites) - listed} C API helpers reached from a tested C function.')
    return 0


def unit_test_path(test):
    """(package, cargo target args, full test path) for an inventory test."""
    file, mods, name = split_test(test)
    parts = Path(file).parts
    if parts[0] == 'crates':
        package, rest = parts[1], parts[2:]
    else:
        package, rest = 'ponko', parts
    crate = ROOT / Path(*parts[:len(parts) - len(rest)])
    if rest[0] == 'tests':
        target = f'--test {Path(rest[1]).stem}'
        modules = []
    else:
        if rest[:2] == ('src', 'bin'):
            # A binary of its own (`src/bin/<name>.rs` or `src/bin/<name>/main.rs`).
            target = f'--bin {Path(rest[2]).stem}'
            modules = list(rest[3:]) or ['main']
        else:
            # A library, or a crate that is only a binary (`src/main.rs`).
            target = '--lib' if (crate / 'src' / 'lib.rs').is_file() else f'--bin {package}'
            modules = list(rest[1:])
        modules[-1] = Path(modules[-1]).stem
        if modules[-1] in ('mod', 'lib', 'main'):
            modules.pop()
    if mods:
        modules += mods  # a test a macro makes in a module of its own
    else:
        code = code_only((ROOT / file).read_text(encoding='utf-8'))
        at = re.search(rf'\bfn\s+{re.escape(name)}\s*[(<]', code).start()
        modules += [mod for start, end, kind, mod, _t in scopes(code) if kind == 'mod' and start < at < end]
    return package, target, '::'.join(modules + [name])


def miri_commands():
    entries, _ = inventory()
    groups = {}
    for (path, item), (test, miri, _n) in entries.items():
        if not miri.startswith('miri') or test.endswith('.mjs'):
            continue
        features = miri[5:-1] if miri != 'miri' else ''
        package, target, test_path = unit_test_path(test)
        groups.setdefault((package, features, target), set()).add(test_path)
    # One feature set per package, so its targets share one Miri build.
    union = {}
    for package, features, _target in groups:
        union.setdefault(package, set()).update(f for f in features.split(',') if f)
    merged = {}
    for (package, _features, target), tests in groups.items():
        merged.setdefault((package, ','.join(sorted(union[package])), target), set()).update(tests)
    groups = merged
    lines = []
    for (package, features, target), tests in sorted(groups.items()):
        flags = '--no-default-features' + (f' --features {features}' if features else '')
        lines.append(f'cargo +nightly miri test -p {package} {flags} {target} -- --exact '
                     + ' '.join(sorted(tests)))
    return lines


SELFTEST = r'''
fn plain() { let s = "unsafe { not code }"; } // unsafe in a comment
/* unsafe { block comment } */
fn reads(p: *const u8) -> u8 { unsafe { *p } }
struct Slots;
unsafe impl Send for Slots {}
unsafe impl<T: Copy> Sync for Wrap<T> {}
impl Slots {
    pub unsafe fn write(&self) { unsafe { core::hint::black_box(0) }; }
    fn helper(&self) { let f: unsafe fn() = noop; let c = 'x'; unsafe { f() } }
}
trait Raw { unsafe fn raw(&self); }
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_thing() { unsafe { core::hint::black_box(1) }; }
unsafe extern "C" { fn puts(); }
#[cfg(test)]
mod tests {
    #[test]
    fn exercises() { unsafe { super::reads(core::ptr::null()) }; }
}
#[test]
fn top_level_test() { unsafe { core::hint::black_box(2) }; }
'''


def selftest():
    got = [(item, form, export) for _l, item, form, export in unsafe_sites('x.rs', SELFTEST)]
    want = [
        ('reads', 'block', False),
        ('impl Send for Slots', 'impl', False),
        ('impl Sync for Wrap<T>', 'impl', False),
        ('Slots::write', 'fn', False),
        ('Slots::write', 'block', False),
        ('Slots::helper', 'block', False),
        ('ponko_thing', 'fn', True),
        ('ponko_thing', 'block', True),
        ('extern block in (top level)', 'extern block', False),
    ]
    if got != want:
        print('selftest failed:')
        print('  got ', got)
        print('  want', want)
        return 1
    print('selftest passed')
    return 0


def main():
    args = sys.argv[1:]
    if args == ['--selftest']:
        return selftest()
    if args == ['--list']:
        entries, _ = inventory()
        derived = ffi_derived()
        for (path, item), lines in sorted(all_sites().items()):
            if (path, item) in entries:
                test = f'{entries[(path, item)][0]}  [{entries[(path, item)][1]}]'
            elif (path, item) in derived:
                test = f'{derived[(path, item)][0]}  (through {derived[(path, item)][1]})'
            else:
                test = '(no test)'
            print(f'{path}:{",".join(map(str, lines))}  {item}  ->  {test}')
        return 0
    if args == ['--miri']:
        print('\n'.join(miri_commands()))
        return 0
    if args:
        print(__doc__)
        return 2
    return check()


if __name__ == '__main__':
    sys.exit(main())
