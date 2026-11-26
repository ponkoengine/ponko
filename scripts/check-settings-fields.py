#!/usr/bin/env python3
"""Fail when a field of a Rust settings struct has no C counterpart.

A C game sets a Rust struct's fields through its C struct (and the `...Ex`
descriptor beside it, STABILITY.md "Versioned descriptors"). When someone adds
a field to a Rust settings struct, C can't reach it until the C struct gets it
too, which for an ABI 2 struct means a versioned descriptor. This check makes
that visible: every public field of every struct listed in
scripts/settings-fields.txt has

  - a field of the same name in one of its C structs, or
  - a line in settings-fields.txt that names its counterpart (a C field with
    another name, or a C function that sets it) with the reason, or
  - `-` there, and a line on scripts/rust-only.txt (`<Name>::<field>`, with
    its row in the "Still Rust-only" table of book/src/c-cpp.md, which
    check-ffi-coverage.py keeps), or `rust-only:<entry>` for a field that an
    existing entry covers (`rust-only:SkyShader`).

The check also fails when a line is stale: the Rust field or the C field or
function it names no longer exists, it is no longer needed (the field has a
counterpart of the same name now), its Rust-only line is missing, or a
Rust-only line names a field of a listed struct that has a counterpart.

    python3 scripts/check-settings-fields.py
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
CONFIG = ROOT / 'scripts' / 'settings-fields.txt'
RUST_ONLY = ROOT / 'scripts' / 'rust-only.txt'
HEADER = ROOT / 'include' / 'ponko.h'
FFI_SRC = ROOT / 'crates' / 'ponko_ffi' / 'src'


def strip_comments(line):
    return line.split('//', 1)[0]


def struct_body(text, name):
    """The lines inside `pub struct <name> { ... }`, or None."""
    match = re.search(rf'^pub struct {name}\s*\{{', text, re.M)
    if not match:
        return None
    depth, body = 1, []
    for line in text[match.end():].splitlines(True):
        code = strip_comments(line)
        depth += code.count('{') - code.count('}')
        if depth <= 0:
            break
        body.append(code)
    return body


def fields(body):
    """Public fields at the top level of a struct body."""
    names, depth = [], 0
    for line in body:
        if depth == 0:
            found = re.match(r'\s*pub\s+([a-z_][a-z0-9_]*)\s*:', line)
            if found:
                names.append(found.group(1))
        depth += sum(line.count(c) for c in '{(<[') - sum(line.count(c) for c in '})>]')
    return names


def ffi_text():
    return '\n'.join(f.read_text(encoding='utf-8') for f in sorted(FFI_SRC.glob('*.rs')))


def read_config():
    """(structs, mappings, problems): structs are (public, rust, file, [c structs]) and
    mappings {(public, field): (target, line number)}."""
    structs, mappings, problems = [], {}, []
    for number, line in enumerate(CONFIG.read_text(encoding='utf-8').splitlines(), 1):
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        fields_part, _, reason = line.partition('#')
        parts = fields_part.split()
        if parts and parts[0] == 'struct':
            if len(parts) != 5:
                problems.append(f'{CONFIG.name}:{number}: expected "struct <Name> <Rust struct> <file> <C structs>"')
                continue
            structs.append((parts[1], parts[2], parts[3], parts[4].split(',')))
            continue
        if len(parts) != 2 or '.' not in parts[0]:
            problems.append(f'{CONFIG.name}:{number}: expected "<Name>.<field>  <C field | C function | ->  # reason"')
            continue
        if not reason.strip():
            problems.append(f'{CONFIG.name}:{number}: {parts[0]} has no reason after "#"')
        key = tuple(parts[0].split('.', 1))
        if key in mappings:
            problems.append(f'{CONFIG.name}:{number}: {parts[0]} is listed twice')
        mappings[key] = (parts[1], number)
    return structs, mappings, problems


def rust_only_names():
    names = set()
    for line in RUST_ONLY.read_text(encoding='utf-8').splitlines():
        if line.strip() and not line.lstrip().startswith('#'):
            names.add(line.partition('#')[0].split()[0])
    return names


def problems_found():
    structs, mappings, problems = read_config()
    ffi = ffi_text()
    header = HEADER.read_text(encoding='utf-8')
    rust_only = rust_only_names()
    listed = {name for name, *_ in structs}
    for public, rust, path, c_names in structs:
        file = ROOT / path
        if not file.exists():
            problems.append(f'{public}: {path} does not exist.')
            continue
        body = struct_body(file.read_text(encoding='utf-8'), rust)
        if body is None:
            problems.append(f'{public}: no `pub struct {rust}` in {path}.')
            continue
        rust_fields = fields(body)
        c_fields = {}
        for c in c_names:
            c_body = struct_body(ffi, c)
            if c_body is None:
                problems.append(f'{public}: no `pub struct {c}` in crates/ponko_ffi/src.')
            else:
                c_fields[c] = set(fields(c_body))
        every_c_field = set().union(*c_fields.values()) if c_fields else set()
        for field in rust_fields:
            mapped = mappings.get((public, field))
            only = f'{public}::{field}' in rust_only
            if mapped is None:
                if field not in every_c_field:
                    problems.append(
                        f'{public}.{field}: none of {", ".join(c_names)} has a field `{field}`. Add it to the '
                        f'versioned descriptor, or add a line to scripts/{CONFIG.name} (its C counterpart, or `-` '
                        f'with a line `{public}::{field}` on scripts/{RUST_ONLY.name}).')
                if only:
                    problems.append(f'{public}.{field}: on scripts/{RUST_ONLY.name} but C has a field of that name now; '
                                    f'remove the entry and its table row.')
                continue
            target, number = mapped
            if target == '-' or target.startswith('rust-only:'):
                entry = target.removeprefix('rust-only:') if target != '-' else f'{public}::{field}'
                if entry not in rust_only:
                    problems.append(f'{CONFIG.name}:{number}: {public}.{field} is Rust-only but `{entry}` is not '
                                    f'on scripts/{RUST_ONLY.name}.')
            elif target.startswith('ponko_'):
                if not re.search(rf'\b{target}\(', header):
                    problems.append(f'{CONFIG.name}:{number}: {target} is not declared in include/ponko.h.')
                if only:
                    problems.append(f'{public}.{field}: set by {target}, so it is not Rust-only; remove it from '
                                    f'scripts/{RUST_ONLY.name}.')
            else:
                if target not in every_c_field:
                    problems.append(f'{CONFIG.name}:{number}: {target} is not a field of {", ".join(c_names)}.')
                if target == field:
                    problems.append(f'{CONFIG.name}:{number}: {public}.{field} has a C field of the same name; '
                                    f'remove the line.')
                if only:
                    problems.append(f'{public}.{field}: has the C field {target}, so it is not Rust-only; remove it '
                                    f'from scripts/{RUST_ONLY.name}.')
        for (name, field), (target, number) in mappings.items():
            if name == public and field not in rust_fields:
                problems.append(f'{CONFIG.name}:{number}: {public} has no public field `{field}` in {path}; '
                                f'remove the line.')
    for (name, field), (target, number) in mappings.items():
        if name not in listed:
            problems.append(f'{CONFIG.name}:{number}: {name} is not a struct listed in {CONFIG.name}.')
    return problems


def main():
    problems = problems_found()
    if problems:
        print('\n'.join(problems))
        return 1
    structs = read_config()[0]
    print(f'{len(structs)} settings structs: every public field has a C counterpart or a reason.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
