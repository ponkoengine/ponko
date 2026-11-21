#!/usr/bin/env python3
"""Fail when a C function leaves the ownership of one of its pointers unsaid.

Every C function (`extern "C" fn` in crates/ponko_ffi/src, declared in
include/ponko.h) documents who owns each pointer it takes or returns. Most
pointers follow the rules written at the top of ponko.h ("Pointers and who
owns them"), decided by the parameter's type:

  handle     a `Ponko*` object pointer (PonkoCtx, PonkoWorld, a builder...):
             borrowed for the call; the caller keeps it. A `ponko_*_free`
             function takes its first handle and frees it.
  input      any `const` pointer (strings, arrays, descriptors, bytes, an
             array of strings or handles): read during the call only; the
             engine copies what it keeps.
  output     a non-const pointer to plain data (numbers, `char` buffers,
             value structs, `void *out...`): the caller's memory, written
             (and maybe read) during the call only.

Everything else needs an `Ownership:` paragraph in the function's doc comment
(the ponko_ffi API docs) that names it:

  - a returned pointer: who owns it; an owned object names the function that
    frees it (`ponko_*_free`), a borrowed one says how long it stays valid,
  - a pointer to a pointer (`T **out`): what the caller receives,
  - a `void *` that is not an output (callback user data): how long it must
    stay valid, and who frees it,
  - a handle the function takes over (its Rust body frees it with
    `Box::from_raw`) when the function is not a `ponko_*_free`.

    python3 scripts/check-ffi-ownership.py            # check
    python3 scripts/check-ffi-ownership.py --list     # every pointer and its rule
    python3 scripts/check-ffi-ownership.py --selftest # the rules on fixed signatures
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / 'crates' / 'ponko_ffi' / 'src'
HEADER = ROOT / 'include' / 'ponko.h'

EXPORT = re.compile(r'extern\s+"C"\s+fn\s+(ponko_\w+)\s*\(')


def opaque_types():
    """The `Ponko*` types C sees only through pointers (`typedef struct X X;`)."""
    return set(re.findall(r'^typedef struct (Ponko\w+) \1;', HEADER.read_text(), re.MULTILINE))


def split_params(text):
    params, depth, cur = [], 0, ''
    for c in text:
        if c in '(<[':
            depth += 1
        elif c in ')>]':
            depth -= 1
        if c == ',' and depth == 0:
            params.append(cur)
            cur = ''
        else:
            cur += c
    if cur.strip():
        params.append(cur)
    return [p.strip() for p in params if p.strip()]


def closing(text, open_at, pair='()'):
    depth = 0
    for i in range(open_at, len(text)):
        if text[i] == pair[0]:
            depth += 1
        elif text[i] == pair[1]:
            depth -= 1
            if depth == 0:
                return i
    return len(text)


def doc_before(text, at):
    """The `///` doc lines above the item starting at `at`, attributes skipped."""
    lines = text[:at].rstrip().split('\n')
    while lines and lines[-1].strip() and not lines[-1].strip().startswith('///'):
        line = lines[-1].strip()
        if line.startswith(('#[', 'pub ', 'unsafe', 'extern')) or line.endswith(')]'):
            lines.pop()
            continue
        break
    doc = []
    while lines and lines[-1].strip().startswith('///'):
        doc.append(lines.pop().strip()[3:].strip())
    doc.reverse()
    prose, fenced = [], False
    for line in doc:
        if line.startswith('```'):
            fenced = not fenced
            continue
        if not fenced:
            prose.append(line)
    return prose


def ownership_text(prose):
    """The `Ownership:` paragraph (to the next blank line or heading)."""
    out, on = [], False
    for line in prose:
        if line.startswith('Ownership:'):
            on = True
        elif on and (not line or line.startswith('#')):
            break
        if on:
            out.append(line)
    return ' '.join(out)


def exports():
    """[(name, file, params [(name, type)], return type, body, doc prose)]."""
    found = []
    for f in sorted(SRC.rglob('*.rs')):
        text = f.read_text()
        for m in EXPORT.finditer(text):
            line_start = text.rfind('\n', 0, m.start()) + 1
            if text[line_start:m.start()].lstrip().startswith('//'):
                continue
            close = closing(text, m.end() - 1)
            params = []
            for p in split_params(text[m.end():close]):
                name, _, ty = p.partition(':')
                params.append((name.strip().lstrip('_') or '_', ' '.join(ty.split())))
            brace = text.find('{', close)
            ret = ' '.join(text[close + 1:brace].replace('->', ' ', 1).split()) if '->' in text[close:brace] else ''
            body = text[brace:closing(text, brace, '{}') + 1]
            found.append((m.group(1), f.relative_to(ROOT).as_posix(), params, ret, body, doc_before(text, line_start)))
    return found


def consuming_helpers():
    """Functions of the C API crate that free their first argument with
    `Box::from_raw` (as `take(game)` does): an export passing a handle to
    one takes that handle over."""
    found = set()
    for f in sorted(SRC.rglob('*.rs')):
        text = f.read_text()
        for m in re.finditer(r'\bfn\s+(\w+)\s*(?:<[^>]*>)?\(\s*(\w+)\s*:', text):
            if m.group(1).startswith('ponko_'):
                continue
            brace = text.find('{', m.end())
            body = text[brace:closing(text, brace, '{}') + 1]
            if re.search(rf'Box::from_raw\(\s*{re.escape(m.group(2))}\b', body):
                found.add(m.group(1))
    return found


_HELPERS = None


def takes_over(name, body):
    """The body frees handle `name`, itself or through a consuming helper."""
    global _HELPERS
    if _HELPERS is None:
        _HELPERS = consuming_helpers()
    if re.search(rf'Box::from_raw\(\s*{re.escape(name)}\b', body):
        return True
    return any(re.search(rf'\b{h}\(\s*{re.escape(name)}\s*[,)]', body) for h in _HELPERS)


def pointee(ty):
    """('const'|'mut', inner type) of a raw pointer type, else None."""
    m = re.fullmatch(r'\*(const|mut)\s+(.+)', ty)
    return (m.group(1), m.group(2).strip()) if m else None


def base_name(ty):
    return ty.split('::')[-1].strip()


def classify(fn, params, ret, body, opaque):
    """[(what, rule)] for every pointer of a function: rule is 'handle',
    'consumed', 'input', 'output' (no doc needed) or 'doc: <what to say>'."""
    rules = []
    first_handle = True
    for name, ty in params:
        p = pointee(ty)
        if not p:
            continue
        kind, inner = p
        inner_ptr = pointee(inner)
        if inner_ptr and kind == 'const':
            rules.append((name, 'input'))  # an array of pointers (strings, handles) the call reads
            continue
        if inner_ptr:
            rules.append((name, f'doc: what `{name}` receives (an owned object and the function that frees it, or a borrowed view and how long it stays valid)'))
            continue
        target = base_name(inner)
        if target in opaque:
            if kind == 'mut' and fn.endswith('_free') and first_handle:
                rules.append((name, 'consumed'))
            elif takes_over(name, body):
                rules.append((name, f'doc: that the function takes `{name}` over (frees or keeps it): the caller must not use or free it after'))
            else:
                rules.append((name, 'handle'))
            first_handle = False
            continue
        if kind == 'const':
            rules.append((name, 'input'))
        elif target == 'c_void' and not name.startswith('out'):
            rules.append((name, f'doc: how long `{name}` must stay valid and who frees it (the engine passes it back to a callback)'))
        else:
            rules.append((name, 'output'))
    if pointee(ret):
        rules.append(('return', 'doc: who owns the returned pointer: an owned object names the function that frees it, a borrowed one how long it stays valid'))
    return rules


FREE = re.compile(r'\bponko_\w+_free\b')


def problems_of(fn, file, params, ret, body, prose, opaque, names):
    text = ownership_text(prose)
    problems = []
    for what, rule in classify(fn, params, ret, body, opaque):
        if not rule.startswith('doc:'):
            continue
        need = rule[5:]
        if not text:
            problems.append(f'{file}: {fn}: say {need}.')
            continue
        if what == 'return':
            if not re.search(r'\breturn', text, re.IGNORECASE):
                problems.append(f'{file}: {fn}: its Ownership paragraph must say who owns the returned pointer.')
                continue
            owned = re.search(r'Box::into_raw|owned_world\(', body)
            frees = [f for f in FREE.findall(text) if f in names]
            if owned and not frees:
                problems.append(f'{file}: {fn}: returns an owned object; its Ownership paragraph must name the existing ponko_*_free that frees it.')
            if not owned and not frees and not re.search(r'borrow|until|static|lives as long|valid', text, re.IGNORECASE):
                problems.append(f'{file}: {fn}: say how long the returned pointer stays valid (or name the ponko_*_free that frees it).')
        elif not re.search(rf'`{re.escape(what)}`', text):
            problems.append(f'{file}: {fn}: its Ownership paragraph must name `{what}`: {need}.')
    for f in FREE.findall(text):
        if f not in names:
            problems.append(f'{file}: {fn}: its Ownership paragraph names {f}, which does not exist.')
    return problems


TEMPLATE = '''
Write the paragraph in the function's doc comment in crates/ponko_ffi/src, for example:
    /// Ownership: the returned list is owned by the caller; free it with ponko_list_free.
    /// Ownership: the returned string is borrowed until the next call on this thread.
    /// Ownership: `user` stays the caller's and must stay valid until the game is freed.
    /// Ownership: `builder` is taken over and freed, also on failure; don't use it after.
    /// Ownership: `out_clip` receives an owned clip; free it with ponko_clip_free.
The rules that need no paragraph are at the top of include/ponko.h ("Pointers and who owns them").'''


def check():
    opaque = opaque_types()
    found = exports()
    names = {f[0] for f in found}
    problems = []
    for fn, file, params, ret, body, prose in found:
        problems += problems_of(fn, file, params, ret, body, prose, opaque, names)
    for p in problems:
        print(p)
    if problems:
        print(f'\n{len(problems)} problem(s).' + TEMPLATE)
        return 1
    documented = sum(1 for f in found if ownership_text(f[5]))
    print(f'{len(found)} C functions: every pointer follows a rule of ponko.h or is documented '
          f'({documented} Ownership paragraphs).')
    return 0


def listing():
    opaque = opaque_types()
    for fn, file, params, ret, body, prose in exports():
        for what, rule in classify(fn, params, ret, body, opaque):
            print(f'{fn}  {what}: {rule}')


def selftest():
    opaque = {'PonkoCtx', 'PonkoWorld', 'PonkoClip'}
    cases = [
        ('ponko_world_free', [('world', '*mut PonkoWorld')], '', '', [('world', 'consumed')]),
        ('ponko_ctx_name', [('ctx', '*mut PonkoCtx'), ('name', '*const c_char'), ('buffer', '*mut c_char'),
                            ('len', '*mut usize')], '', '',
         [('ctx', 'handle'), ('name', 'input'), ('buffer', 'output'), ('len', 'output')]),
        ('ponko_ctx_run', [('ctx', '*const PonkoCtx'), ('user', '*mut c_void'), ('out_data', '*mut c_void')], '', '',
         [('ctx', 'handle'), ('user', 'doc'), ('out_data', 'output')]),
        ('ponko_clip_new', [('out', '*mut *mut PonkoClip')], 'PonkoStatus', '', [('out', 'doc')]),
        ('ponko_ctx_names', [('names', '*const *const c_char')], '', '', [('names', 'input')]),
        ('ponko_clip_merge', [('a', '*mut PonkoClip')], '', 'drop(Box::from_raw(a));', [('a', 'doc')]),
        ('ponko_world_new', [], '*mut PonkoWorld', '', [('return', 'doc')]),
    ]
    failed = False
    for fn, params, ret, body, want in cases:
        got = [(w, r.split(':')[0]) for w, r in classify(fn, params, ret, body, opaque)]
        if got != want:
            print(f'selftest failed for {fn}: got {got}, want {want}')
            failed = True
    owned = problems_of('ponko_world_new', 'x.rs', [], '*mut PonkoWorld', 'Box::into_raw(w)',
                        ['Ownership: the returned world is borrowed.'], opaque, {'ponko_world_new'})
    if not owned:
        print('selftest failed: an owned return without its free function passed')
        failed = True
    good = problems_of('ponko_world_new', 'x.rs', [], '*mut PonkoWorld', 'Box::into_raw(w)',
                       ['Ownership: the returned world is the caller\'s; free it with ponko_world_free.'],
                       opaque, {'ponko_world_new', 'ponko_world_free'})
    if good:
        print(f'selftest failed: a documented owned return was refused: {good}')
        failed = True
    print('selftest failed' if failed else 'selftest passed')
    return int(failed)


def main():
    args = sys.argv[1:]
    if args == ['--selftest']:
        return selftest()
    if args == ['--list']:
        listing()
        return 0
    if args:
        print(__doc__)
        return 2
    return check()


if __name__ == '__main__':
    sys.exit(main())
