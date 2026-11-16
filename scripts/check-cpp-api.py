#!/usr/bin/env python3
"""Fail when the C++ wrapper falls behind the C API or loses its docs.

Two checks on include/ponko.hpp:

1. Coverage. Every C function declared in include/ponko.h is called from
   ponko.hpp (outside comments), or is on the C-only list,
   scripts/cpp-c-only.txt, with a one-line reason. The check fails when
     - a C function has no wrapper and is not on the list (wrap it),
     - a listed function is wrapped now or no longer exists (shrink the list),
     - an entry has no reason or appears twice,
     - an entry is missing from the guide's C-only table (book/src/c-cpp.md).

2. Docs. Every public declaration of ponko.hpp (types, aliases, enums,
   functions, members of public classes) has a `///` comment on the lines
   above it or a `///<` after it, or sits in a documented member group
   (`/// what they are` then `///@{` ... `///@}`). Code in `namespace
   detail`, private members, function bodies, forward declarations,
   destructors and macros are skipped.

3. Book snippets. Every ```c and ```cpp block in book/src holds only
   `{{#include}}` lines of files in examples/cpp (guide.cpp and guide.c hold the
   guide's own), by an anchor that exists there, so CI's native build
   compiles and runs every C and C++ example the guide shows.

    python3 scripts/check-cpp-api.py          # check
    python3 scripts/check-cpp-api.py --list   # print C functions without a wrapper
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
HEADER = ROOT / 'include' / 'ponko.h'
WRAPPER = ROOT / 'include' / 'ponko.hpp'
C_ONLY = ROOT / 'scripts' / 'cpp-c-only.txt'
GUIDE = ROOT / 'book' / 'src' / 'c-cpp.md'
BOOK = ROOT / 'book' / 'src'
NATIVE = ROOT / 'examples' / 'cpp'

COMMENT = re.compile(r'//[^\n]*|/\*.*?\*/', re.DOTALL)
STRING = re.compile(r'"(?:\\.|[^"\\\n])*"')


def c_functions():
    return set(re.findall(r'\b(ponko_[a-z0-9_]+)\s*\(', COMMENT.sub(' ', HEADER.read_text())))


def wrapped(text):
    code = STRING.sub('""', COMMENT.sub(' ', text))
    return set(re.findall(r'\b(ponko_[a-z0-9_]+)\b', code))


def c_only_list():
    entries, problems = {}, []
    for number, line in enumerate(C_ONLY.read_text().splitlines(), 1):
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        name, _, reason = line.partition('#')
        name = name.strip()
        if not reason.strip():
            problems.append(f'{C_ONLY.name}:{number}: {name} has no reason after "#"')
        if name in entries:
            problems.append(f'{C_ONLY.name}:{number}: {name} is listed twice')
        entries[name] = reason.strip()
    return entries, problems


def coverage_problems(text):
    functions = c_functions()
    missing = functions - wrapped(text)
    listed, problems = c_only_list()
    guide = GUIDE.read_text()
    for name in sorted(missing - listed.keys()):
        problems.append(f'{name}: ponko.hpp does not wrap it and it is not on the C-only list. Wrap it in '
                        f'include/ponko.hpp (with a doc comment and a test in examples/cpp/tests.cpp), or, if '
                        f'C++ has a better way, add it to scripts/{C_ONLY.name} with the reason.')
    for name in sorted(listed.keys() - functions):
        problems.append(f'{name}: on the C-only list but not in ponko.h any more; remove the entry.')
    for name in sorted((listed.keys() & functions) - missing):
        problems.append(f'{name}: ponko.hpp wraps it now; remove it from the C-only list.')
    for name in sorted(listed.keys()):
        if f'`{name}`' not in guide:
            problems.append(f'{name}: on the C-only list but not in the C-only table of book/src/c-cpp.md.')
    return problems, len(functions), len(missing)


DECLARATION = re.compile(
    r'^(template<.*>\s*)?((inline|constexpr|static|explicit|virtual|friend|\[\[nodiscard\]\])\s+)*'
    r'(struct|class|enum|using|union|[A-Za-z_][\w:<>,\*& ]*?[\s\*&]+(operator\s*\S+?|[A-Za-z_]\w*)\s*\(|'
    r'[A-Za-z_]\w*\s*\(|constexpr\s)')
NOT_DECLARATION = re.compile(r'^(if|for|while|switch|return|else|do|case|throw|static_assert|using namespace|'
                             r'detail::|std::|::|public:|private:|protected:|default:)\b')


def undocumented(text):
    """Line numbers and text of public declarations without a doc comment."""
    lines = text.split('\n')
    found = []
    # One frame per open brace: 'ns' (namespace), 'detail', 'public' / 'private'
    # (class or struct bodies), 'body' (functions, enums, initializers).
    stack = []
    group = 0  # open documented ///@{ groups
    in_macro = False
    pending = ''  # a declaration whose opening brace is on a later line
    for number, line in enumerate(lines, 1):
        stripped = line.strip()
        if in_macro or stripped.startswith('#'):
            in_macro = line.rstrip().endswith('\\')
            continue
        if stripped == '///@{':
            group += 1
            continue
        if stripped == '///@}':
            group -= 1
            continue
        code = STRING.sub('""', COMMENT.sub(' ', line))
        hidden = any(frame in ('detail', 'private', 'body') for frame in stack)
        if not hidden and stripped and not stripped.startswith('//') and not pending:
            is_decl = (DECLARATION.match(stripped) and not NOT_DECLARATION.match(stripped)
                       and not re.match(r'^(class|struct)\s+\w+\s*;', stripped)  # forward declaration
                       and not re.match(r'^~', stripped)                        # destructor
                       and not stripped.startswith(('{', '}', ':', ')')))
            if is_decl and not group:
                above = number - 2
                while above >= 0 and re.match(r'^\s*(template<[^{;]*>|\[\[.*\]\])\s*$', lines[above]):
                    above -= 1
                documented = '///' in line or (above >= 0 and lines[above].strip().startswith('///'))
                if not documented:
                    found.append((number, stripped))
        # Track braces.
        for at, char in enumerate(code):
            if char == '{':
                before = (pending + ' ' + code[:at]).strip()
                pending = ''
                if re.search(r'\bnamespace\s+detail\s*$', before):
                    stack.append('detail')
                elif re.search(r'\bnamespace(\s+\w+)?\s*$', before):
                    stack.append('ns')
                elif re.search(r'\bclass\s+\w+[^()]*$', before) and not re.search(r'\benum\s+class\b', before):
                    stack.append('private')
                elif re.search(r'\b(struct|union)\s+\w*[^()]*$', before):
                    stack.append('public')
                else:
                    stack.append('body')
            elif char == '}':
                if stack:
                    stack.pop()
        # A class head whose brace comes on a later line.
        if re.match(r'^\s*(template<.*>\s*)?(class|struct)\s+\w+[^;{]*$', code) and '(' not in code:
            pending = code
        if stack and stack[-1] in ('public', 'private'):
            if re.match(r'^\s*public\s*:', line):
                stack[-1] = 'public'
            elif re.match(r'^\s*(private|protected)\s*:', line):
                stack[-1] = 'private'
    return found


def book_problems():
    problems = []
    block = re.compile(r'^```(c|cpp)\b[^\n]*\n(.*?)^```', re.MULTILINE | re.DOTALL)
    include = re.compile(r'\{\{#include ([^}:]+)(?::([\w-]+))?\}\}')
    for chapter in [*sorted(BOOK.glob('*.md')), *sorted(BOOK.glob('*/*.md'))]:
        if chapter.name.startswith('migrating-to-'):
            continue  # shows code from before a change, which is not compiled
        text = chapter.read_text()
        for match in block.finditer(text):
            line = text.count('\n', 0, match.start()) + 1
            where = f'{chapter.relative_to(ROOT).as_posix()}:{line}'
            lines = [line.strip() for line in match.group(2).splitlines() if line.strip()]
            if not lines or not all(include.fullmatch(line) for line in lines):
                problems.append(f'{where}: a ```{match.group(1)} block that is not compiled. Put the code in '
                                f'examples/cpp/guide.cpp (or guide.c for C) between ANCHOR lines and include '
                                f'it: {{{{#include ../../examples/cpp/guide.cpp:name}}}}.')
                continue
            for line in lines:
                found = include.fullmatch(line)
                source = (chapter.parent / found.group(1)).resolve()
                if source.parent != NATIVE or not source.is_file():
                    problems.append(f'{where}: includes {found.group(1)}, which is not a file in examples/cpp.')
                    continue
                anchor = found.group(2)
                if anchor and not re.search(rf'ANCHOR:\s*{re.escape(anchor)}\s*$', source.read_text(),
                                            re.MULTILINE):
                    problems.append(f'{where}: anchor `{anchor}` is not in {source.relative_to(ROOT)}.')
    return problems


def main():
    text = WRAPPER.read_text()
    if '--list' in sys.argv:
        missing = sorted(c_functions() - wrapped(text))
        print('\n'.join(missing))
        print(f'{len(missing)} of {len(c_functions())} C functions have no C++ wrapper', file=sys.stderr)
        return 0
    problems, total, missing = coverage_problems(text)
    problems += book_problems()
    for number, declaration in undocumented(text):
        problems.append(f'include/ponko.hpp:{number}: no doc comment (`///` above it): {declaration[:90]}')
    if problems:
        print('\n'.join(problems))
        print(f'{len(problems)} problem(s); see the top of scripts/check-cpp-api.py.', file=sys.stderr)
        return 1
    print(f'{total - missing} of {total} C functions are wrapped in ponko.hpp; {missing} are C only. '
          f'Every public C++ declaration has a doc comment, and every C and C++ block of the guide is compiled.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
