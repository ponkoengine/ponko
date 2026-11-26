#!/usr/bin/env python3
"""Fold changelog.d/ fragments into CHANGELOG.md, and write the migration chapter.

    python3 scripts/changelog.py           # fold the fragments in and delete them
                                           # (--version X.Y.Z when any breaks)
    python3 scripts/changelog.py --dry-run # print what would be written, change nothing
    python3 scripts/changelog.py --check   # validate the fragments
    python3 scripts/changelog.py --breaking FILE...
                                           # which of the fragments announce a
                                           # breaking change (semver-checks.py
                                           # asks this, see CONTRIBUTING.md)
    python3 scripts/changelog.py --selftest
                                           # check the rules with made-up input

A fragment is changelog.d/<pr-or-topic>.md holding one or more `### Heading`
sections with `- ` bullets (see changelog.d/README.md). Bullets are added at
the end of the Unreleased subsection with the same heading; a missing
subsection is created in Keep a Changelog order, with `Breaking changes`
first. Fragments fold in file-name order.

A `### Breaking changes` section (`### Breaking` is accepted too) holds
entries, not plain bullets. Each says what changed, what to write instead and
shows the code before and after; `--check` refuses an entry without them:

    ### Breaking changes
    - `Foo::bar` takes a `Baz` now.
      Instead: wrap the argument, `Baz::new(x)`.
      Before:
      ```rust
      foo.bar(x);
      ```
      After:
      ```rust
      foo.bar(Baz::new(x));
      ```

Folding turns each entry into a short bullet in CHANGELOG.md (what changed
and what to write instead, which also becomes the GitHub release's notes) and
writes the same entries, with the snippets, to book/src/migrating-to-<X.Y>.md,
which it adds to book/src/SUMMARY.md. That is why `--version` is needed
when a fragment has one: the chapter is named after the release.
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
BREAKING = 'Breaking changes'
ALIASES = {'Breaking': BREAKING}
ORDER = [BREAKING, 'Added', 'Changed', 'Deprecated', 'Removed', 'Fixed', 'Security']
HEADING = re.compile(r'^### (.+?)\s*$')
FENCE = re.compile(r'^\s*(`{3,})\s*([^`\s]*)\s*$')
VERSION = re.compile(r'^(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?$')
GUIDE = 'https://ponko.dev/guide'
START_MARK = '<!-- changelog.py: entries start -->'
END_MARK = '<!-- changelog.py: entries end -->'


class FragmentError(Exception):
    """A fragment (or the generated files) breaks a rule; the message says which."""


def fragment_files(root=ROOT):
    folder = root / 'changelog.d'
    if not folder.is_dir():
        return []
    return sorted(p for p in folder.glob('*.md') if p.name != 'README.md')


# Parsing

def parse_text(name, text):
    """A fragment's text as [(heading, [lines])]; raises FragmentError."""
    sections = []
    fence = None  # the opening backticks while inside a code block
    for number, line in enumerate(text.splitlines(), 1):
        where = f'{name}:{number}'
        if fence:
            sections[-1][1].append(line.rstrip())
            closing = FENCE.match(line)
            if closing and closing.group(1) == fence and not closing.group(2):
                fence = None
            continue
        match = HEADING.match(line)
        if match:
            heading = match.group(1)
            sections.append((ALIASES.get(heading, heading), []))
        elif not line.strip():
            continue
        elif not sections:
            raise FragmentError(f'{where}: text before the first `### Heading`')
        elif line.startswith('#'):
            raise FragmentError(f'{where}: use `### Heading` sections only')
        elif not sections[-1][1] and not line.startswith('- '):
            raise FragmentError(f'{where}: a section starts with a `- ` bullet')
        else:
            sections[-1][1].append(line.rstrip())
            opening = FENCE.match(line)
            if opening:
                fence = opening.group(1)
    if fence:
        raise FragmentError(f'{name}: a code block is never closed')
    if not sections:
        raise FragmentError(f'{name}: no `### Heading` section')
    for heading, lines in sections:
        if not lines:
            raise FragmentError(f'{name}: section `{heading}` has no bullets')
    return sections


def split_entries(lines):
    """The lines of a section as one list per `- ` bullet, the first being the bullet."""
    entries = []
    for line in lines:
        if line.startswith('- '):
            entries.append([line])
        else:
            entries[-1].append(line)
    return entries


def parse_entry(name, lines):
    """One `### Breaking changes` entry as a dict; raises FragmentError.

    Keys: headline (what changed), details (more lines of it), instead (what
    to write instead), before and after ((language, code lines) of each snippet).
    """
    headline = lines[0][2:].strip()
    where = f'{name}: breaking change "{headline[:50]}"'
    if not headline:
        raise FragmentError(f'{name}: a breaking change needs a first line saying what changed')
    body = []
    for line in lines[1:]:
        if line.startswith('  '):
            body.append(line[2:])
        elif not line.strip():
            body.append('')
        else:
            raise FragmentError(f'{where}: indent the lines after the first by two spaces')
    entry = {'headline': headline, 'details': [], 'instead': [], 'before': None, 'after': None}
    mode = 'details'
    i = 0
    while i < len(body):
        line = body[i]
        label = line.strip()
        if line.startswith('Instead:'):
            if mode != 'details':
                raise FragmentError(f'{where}: `Instead:` comes once, before the snippets')
            mode = 'instead'
            rest = line[len('Instead:'):].strip()
            entry['instead'] = [rest] if rest else []
        elif label in ('Before:', 'After:'):
            key = label[:-1].lower()
            if entry[key] is not None:
                raise FragmentError(f'{where}: `{label}` appears twice')
            if key == 'after' and entry['before'] is None:
                raise FragmentError(f'{where}: `Before:` comes first, then `After:`')
            i += 1
            opening = FENCE.match(body[i]) if i < len(body) else None
            if not opening:
                raise FragmentError(f'{where}: `{label}` must be followed by a fenced code block')
            code = []
            i += 1
            while i < len(body):
                closing = FENCE.match(body[i])
                if closing and closing.group(1) == opening.group(1) and not closing.group(2):
                    break
                code.append(body[i])
                i += 1
            else:
                raise FragmentError(f'{where}: the `{label}` code block is never closed')
            if not any(l.strip() for l in code):
                raise FragmentError(f'{where}: the `{label}` code block is empty')
            entry[key] = (opening.group(2), code)
            mode = 'snippets'
        elif FENCE.match(line):
            raise FragmentError(f'{where}: put `Before:` or `After:` on the line above a code block')
        elif not label:
            pass
        elif mode == 'details':
            entry['details'].append(line)
        elif mode == 'instead':
            entry['instead'].append(line)
        else:
            raise FragmentError(f'{where}: text after the snippets; put it before `Instead:`')
        i += 1
    if not any(l.strip() for l in entry['instead']):
        raise FragmentError(f'{where}: add `Instead:` with what to write instead')
    if entry['before'] is None or entry['after'] is None:
        raise FragmentError(f'{where}: add a `Before:` and an `After:` code block')
    return entry


def breaking_entries(name, sections):
    """Every entry of the fragment's Breaking changes sections, validated."""
    found = []
    for heading, lines in sections:
        if heading == BREAKING:
            found += [parse_entry(name, e) for e in split_entries(lines)]
    return found


def parse(path, root=ROOT):
    """A fragment as [(heading, [lines])] with its breaking entries validated,
    or exits with a message."""
    name = path.name if root is None else path.relative_to(root).as_posix()
    try:
        sections = parse_text(name, path.read_text(encoding='utf-8'))
        breaking_entries(name, sections)
    except FragmentError as error:
        sys.exit(str(error))
    return sections


def has_breaking(path):
    """Whether the fragment has a `### Breaking changes` section with bullets.

    Only the shape is read; `--check` is what validates the entries."""
    try:
        return any(heading == BREAKING for heading, _ in parse_text(path.name, path.read_text(encoding='utf-8')))
    except FragmentError as error:
        sys.exit(str(error))


# Versions

def version_parts(version):
    found = VERSION.match(version)
    if not found:
        raise FragmentError(f'`{version}` is not a version like 0.2.0')
    return tuple(int(n) for n in found.groups())


def may_break(version):
    """Whether a release may carry breaking changes: 0.y.0, or x.0.0 (STABILITY.md)."""
    major, minor, patch = version_parts(version)
    return patch == 0 and (major == 0 or minor == 0)


def chapter_stem(version):
    """The `X.Y` of migrating-to-X.Y.md for a 0.x release, `X` from 1.0 on."""
    major, minor, _ = version_parts(version)
    return f'0.{minor}' if major == 0 else str(major)


def chapter_title(version):
    major, minor, _ = version_parts(version)
    if major == 0 and minor > 0:
        return f'Migrating from 0.{minor - 1} to 0.{minor}'
    return f'Migrating to {chapter_stem(version)}'


def chapter_name(version):
    return f'migrating-to-{chapter_stem(version)}.md'


# Writing

def changelog_lines(entry):
    """An entry as CHANGELOG.md lines: what changed and what to write instead."""
    lines = [f'- {entry["headline"]}', *[f'  {l}' for l in entry['details']]]
    instead = entry['instead']
    lines.append(f'  Instead: {instead[0]}')
    lines += [f'  {l}' for l in instead[1:]]
    return lines


def pointer_line(version):
    return (f'Each entry has its code, before and after, in the '
            f'[migration guide]({GUIDE}/migrating-to-{chapter_stem(version)}.html).')


def chapter_entry(entry):
    """An entry as a section of the migration chapter."""
    out = ['', f'## {entry["headline"].rstrip(".")}', '']
    if entry['details']:
        out += [*entry['details'], '']
    out += ['**Instead:** ' + ' '.join(l.strip() for l in entry['instead']), '']
    for label, key in (('Before', 'before'), ('After', 'after')):
        lang, code = entry[key]
        out += [f'**{label}:**', '', f'```{lang}', *code, '```', '']
    return out[:-1]


def chapter_text(version, entries):
    return '\n'.join([
        f'# {chapter_title(version)}',
        '',
        f'Ponko {chapter_stem(version)} breaks the API in the ways listed here. Each entry says what changed, '
        'what to write instead, and shows the code before and after. The compiler names most of '
        'them; look the message up below.',
        '',
        'The entries come from the release\'s changelog entries, so the '
        '[CHANGELOG](../../CHANGELOG.md) lists the same '
        'changes in short form next to the rest of the release. '
        '[STABILITY.md](../../STABILITY.md) says what '
        'minor releases may break and how a replaced item is deprecated first where that is cheap.',
        '',
        START_MARK,
        *[l for entry in entries for l in chapter_entry(entry)],
        '',
        END_MARK,
        '',
    ])


def add_to_chapter(existing, entries):
    """An existing generated chapter with more entries before its end marker."""
    if START_MARK not in existing or END_MARK not in existing:
        raise FragmentError('the migration chapter exists but was not generated by this script '
                            f'(no `{END_MARK}` marker); add the new entries by hand')
    head, tail = existing.split(END_MARK, 1)
    added = '\n'.join(l for entry in entries for l in chapter_entry(entry))
    return head.rstrip('\n') + '\n' + added + '\n\n' + END_MARK + tail


def summary_with(summary, version):
    """SUMMARY.md with a line for the chapter after the last migration chapter."""
    name = chapter_name(version)
    if f'({name})' in summary:
        return summary
    line = f'- [{chapter_title(version)}]({name})'
    lines = summary.splitlines()
    last = max((i for i, l in enumerate(lines) if '(migrating-to-' in l), default=None)
    if last is None:
        raise FragmentError('book/src/SUMMARY.md has no migration chapter to put the new one after')
    lines.insert(last + 1, line)
    return '\n'.join(lines) + '\n'


# Folding

def unreleased_bounds(lines):
    start = next((i for i, l in enumerate(lines) if l.startswith('## [Unreleased]')), None)
    if start is None:
        raise FragmentError('CHANGELOG.md has no `## [Unreleased]` section')
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith('## ')), len(lines))
    return start, end


def fold(body, heading, bullets):
    """Add bullets to the subsection `heading` of body (the Unreleased lines)."""
    heads = [i for i, l in enumerate(body) if HEADING.match(l)]
    named = [i for i in heads if HEADING.match(body[i]).group(1) == heading]
    if named:
        start = named[0]
        end = next((i for i in heads if i > start), len(body))
        while end > start + 1 and not body[end - 1].strip():
            end -= 1
        body[end:end] = bullets
        return
    # New subsection: before the first standard heading that comes later in
    # ORDER, otherwise at the end.
    rank = ORDER.index(heading) if heading in ORDER else len(ORDER)
    at = len(body)
    for i in heads:
        name = HEADING.match(body[i]).group(1)
        if name in ORDER and ORDER.index(name) > rank:
            at = i
            break
    while at > 0 and not body[at - 1].strip():
        at -= 1
    body[at:at] = ['', f'### {heading}', *bullets]


def add_pointer(body, pointer):
    """The line pointing at the migration chapter, right under the Breaking changes heading, once."""
    heads = [i for i, l in enumerate(body) if HEADING.match(l)]
    start = next(i for i in heads if HEADING.match(body[i]).group(1) == BREAKING)
    end = next((i for i in heads if i > start), len(body))
    if any('/migrating-to-' in l for l in body[start:end]):
        return
    body[start + 1:start + 1] = ['', pointer, '']


def release(root, version, dry_run):
    """Fold the fragments into CHANGELOG.md and write the migration chapter.

    Returns the lines to print; with dry_run nothing is written or deleted.
    """
    files = fragment_files(root)
    parsed = []
    for path in files:
        name = path.relative_to(root).as_posix()
        sections = parse_text(name, path.read_text(encoding='utf-8'))
        parsed.append((path, sections, breaking_entries(name, sections)))
    entries = [e for _, _, found in parsed for e in found]
    if entries:
        if version is None:
            raise FragmentError('these fragments break the API: say which release this is with '
                                '`--version X.Y.Z` (the migration chapter is named after it)')
        if not may_break(version):
            raise FragmentError(f'{version} is a patch release and must not break the API '
                                '(STABILITY.md): release it as the next minor, or take the breaking '
                                'fragments out')
    changelog = root / 'CHANGELOG.md'
    lines = changelog.read_text(encoding='utf-8').splitlines()
    start, end = unreleased_bounds(lines)
    body = lines[start + 1:end]
    for _, sections, _ in parsed:
        for heading, bullets in sections:
            if heading == BREAKING:
                bullets = [l for entry in split_entries(bullets) for l in changelog_lines(
                    parse_entry('', entry))]
            fold(body, heading, bullets)
    if entries:
        add_pointer(body, pointer_line(version))
    while body and not body[-1].strip():
        body.pop()
    body.append('')
    out = [lines[start], *body]
    if entries:
        book = root / 'book' / 'src'
        chapter = book / chapter_name(version)
        summary = book / 'SUMMARY.md'
        text = (add_to_chapter(chapter.read_text(encoding='utf-8'), entries) if chapter.exists()
                else chapter_text(version, entries))
        new_summary = summary_with(summary.read_text(encoding='utf-8'), version)
        out += ['', f'--- book/src/{chapter.name}', text.rstrip('\n'), '',
                f'--- book/src/SUMMARY.md gets: - [{chapter_title(version)}]({chapter.name})']
        if not dry_run:
            chapter.write_text(text, encoding='utf-8', newline='\n')
            summary.write_text(new_summary, encoding='utf-8', newline='\n')
    if not dry_run:
        lines[start + 1:end] = body
        changelog.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')
        for path in files:
            path.unlink()
    return out if dry_run else [f'Folded {len(files)} fragment(s) into CHANGELOG.md'
                                + (f' and wrote book/src/{chapter_name(version)}.' if entries else '.')]


def main():
    args = sys.argv[1:]
    if '--selftest' in args:
        selftest()
        return
    if '--breaking' in args:
        # Prints each given fragment that has the section, one path per line.
        # A path that isn't a fragment (missing, README.md) is ignored.
        for path in map(Path, args[args.index('--breaking') + 1:]):
            if path.is_file() and path.suffix == '.md' and path.name != 'README.md' \
                    and has_breaking(path):
                print(path)
        return
    version = args[args.index('--version') + 1] if '--version' in args else None
    try:
        if '--check' in args:
            files = fragment_files()
            for path in files:
                parse(path)
            print(f'Changelog fragments passed ({len(files)} pending).')
            return
        if not fragment_files():
            print('No changelog fragments to fold.')
            return
        for line in release(ROOT, version, '--dry-run' in args):
            print(line)
    except FragmentError as error:
        sys.exit(str(error))


# The rules, checked with made-up input (`--selftest`).

GOOD = '''\
### Breaking changes
- `Foo::bar` takes a `Baz` now.
  It used to take the raw value.
  Instead: wrap the argument, `Baz::new(x)`.
  It keeps the old meaning.
  Before:
  ```rust
  foo.bar(x);

  foo.baz();
  ```
  After:
  ```rust
  foo.bar(Baz::new(x));
  ```
- `Qux` is gone.
  Instead: use `Quux`.
  Before:
  ```c
  qux();
  ```
  After:
  ```c
  quux();
  ```

### Added
- Something new.
'''


def selftest():
    def entries(text):
        return breaking_entries('f.md', parse_text('f.md', text))

    def refused(text, *words):
        try:
            entries(text)
        except FragmentError as error:
            assert all(w in str(error) for w in words), f'{error!s} should mention {words}'
            return
        raise AssertionError(f'accepted, but should be refused ({words}):\n{text}')

    # A complete entry parses, keeping blank lines inside code.
    found = entries(GOOD)
    assert [e['headline'] for e in found] == ['`Foo::bar` takes a `Baz` now.', '`Qux` is gone.']
    first = found[0]
    assert first['details'] == ['It used to take the raw value.']
    assert first['instead'] == ['wrap the argument, `Baz::new(x)`.', 'It keeps the old meaning.']
    assert first['before'] == ('rust', ['foo.bar(x);', '', 'foo.baz();'])
    assert first['after'] == ('rust', ['foo.bar(Baz::new(x));'])
    assert found[1]['before'][0] == 'c'
    # `### Breaking` is the same section; other sections need no entry format.
    assert [h for h, _ in parse_text('f.md', GOOD.replace('Breaking changes', 'Breaking'))][0] == BREAKING
    assert entries('### Added\n- plain bullet\n') == []

    # Each missing part is named.
    base = '''\
### Breaking changes
- `A` changed.
  Instead: use `B`.
  Before:
  ```rust
  a();
  ```
  After:
  ```rust
  b();
  ```
'''
    assert len(entries(base)) == 1
    refused(base.replace('  Instead: use `B`.\n', ''), 'Instead:')
    refused(base.replace('  Instead: use `B`.', '  Instead:'), 'Instead:')
    refused(base.replace('  Before:\n  ```rust\n  a();\n  ```\n', ''), 'Before:', 'After:')
    refused(base.replace('  After:\n  ```rust\n  b();\n  ```\n', ''), 'Before:', 'After:')
    refused(base.replace('a();', ''), 'Before:', 'empty')
    refused(base.replace('  Before:\n', '  Before:\n  text\n'), 'fenced code block')
    refused(base.replace('  Before:\n  ```rust', '  ```rust'), 'above a code block')
    refused(base.replace('Before:', 'After:', 1), 'Before:', 'first')
    refused(base + '  After:\n  ```rust\n  c();\n  ```\n', 'twice')
    refused(base + '  trailing text\n', 'after the snippets')
    refused(base.replace('  Instead: use `B`.', '  Instead: use `B`.\nnot indented'), 'indent')
    refused(base.replace('  ```rust\n  b();\n  ```', '  ```rust\n  b();'), 'never closed')
    refused('### Breaking changes\n- `A` changed.\n', 'Instead:')
    # An unclosed fence in any section is caught while reading.
    try:
        parse_text('f.md', '### Added\n- x\n  ```rust\n  a();\n')
        raise AssertionError('an unclosed code block is refused')
    except FragmentError as error:
        assert 'never closed' in str(error)
    # `#` lines inside a code block are code, not headings.
    kept = parse_text('f.md', '### Added\n- x\n  ```c\n#include <a.h>\n  ```\n')
    assert kept[0][1][2] == '#include <a.h>'

    # Versions: what may break and what the chapter is called.
    assert may_break('0.2.0') and may_break('0.2.0-rc.1') and may_break('1.0.0') and may_break('2.0.0')
    assert not may_break('0.2.1') and not may_break('1.1.0') and not may_break('1.0.1')
    assert chapter_name('0.2.0') == chapter_name('0.2.0-rc.1') == 'migrating-to-0.2.md'
    assert chapter_name('2.0.0') == 'migrating-to-2.md'
    assert chapter_title('0.2.0') == 'Migrating from 0.1 to 0.2'
    assert chapter_title('2.0.0') == 'Migrating to 2'
    try:
        version_parts('v0.2')
        raise AssertionError('a bad version is refused')
    except FragmentError:
        pass

    # The changelog form has what changed and what to write instead, no code.
    short = changelog_lines(first)
    assert short == ['- `Foo::bar` takes a `Baz` now.', '  It used to take the raw value.',
                     '  Instead: wrap the argument, `Baz::new(x)`.', '  It keeps the old meaning.']
    # The chapter form has everything.
    chapter = chapter_text('0.2.0', found)
    assert chapter.startswith('# Migrating from 0.1 to 0.2\n')
    assert '## `Foo::bar` takes a `Baz` now\n' in chapter and '## `Qux` is gone\n' in chapter
    assert '**Instead:** wrap the argument, `Baz::new(x)`. It keeps the old meaning.' in chapter
    assert '```rust\nfoo.bar(x);\n\nfoo.baz();\n```' in chapter and '```c\nquux();\n```' in chapter
    assert chapter.count(START_MARK) == chapter.count(END_MARK) == 1
    more = add_to_chapter(chapter, entries(base))
    assert more.index('## `A` changed') < more.index(END_MARK) and '## `Qux` is gone' in more
    try:
        add_to_chapter('# Hand written\n', found)
        raise AssertionError('a chapter that is not generated is not overwritten')
    except FragmentError:
        pass

    # SUMMARY.md: the line goes after the last migration chapter, once.
    book = '# Summary\n\n- [Naming](naming.md)\n- [Migrating from 0.0.1 to 0.1.0](migrating-to-0.1.md)\n- [Reflection](reflection.md)\n'
    added = summary_with(book, '0.2.0')
    assert added.splitlines()[4] == '- [Migrating from 0.1 to 0.2](migrating-to-0.2.md)'
    assert summary_with(added, '0.2.0') == added

    # Folding: the entries turn into short bullets, with the pointer once.
    body = ['', '### Added', '- earlier']
    fold(body, BREAKING, [l for e in found for l in changelog_lines(e)])
    add_pointer(body, pointer_line('0.2.0'))
    add_pointer(body, pointer_line('0.2.0'))
    text = '\n'.join(body)
    assert text.index('### Breaking changes') < text.index('### Added'), 'Breaking changes come first'
    assert text.count('migrating-to-0.2.html') == 1
    assert 'foo.bar' not in text, 'the changelog has no snippets'

    # The whole release in a throw-away tree.
    import tempfile
    with tempfile.TemporaryDirectory(prefix='changelog-') as tmp:
        tmp = Path(tmp)
        (tmp / 'changelog.d').mkdir()
        (tmp / 'book' / 'src').mkdir(parents=True)
        (tmp / 'book' / 'src' / 'SUMMARY.md').write_text(book, encoding='utf-8', newline='\n')
        (tmp / 'CHANGELOG.md').write_text('# Changelog\n\n## [Unreleased]\n\n## [0.1.0] - x\n- old\n', encoding='utf-8', newline='\n')
        (tmp / 'changelog.d' / '1-break.md').write_text(GOOD, encoding='utf-8', newline='\n')
        (tmp / 'changelog.d' / '2-fix.md').write_text('### Fixed\n- A fix.\n', encoding='utf-8', newline='\n')
        (tmp / 'changelog.d' / 'README.md').write_text('not a fragment', encoding='utf-8', newline='\n')
        try:
            release(tmp, None, True)
            raise AssertionError('a break needs --version')
        except FragmentError as error:
            assert '--version' in str(error)
        try:
            release(tmp, '0.2.1', True)
            raise AssertionError('a patch release may not break')
        except FragmentError as error:
            assert 'patch release' in str(error)
        shown = release(tmp, '0.2.0', True)
        assert any('migrating-to-0.2.md' in l for l in shown) and (tmp / 'changelog.d' / '1-break.md').exists(), \
            'a dry run changes nothing'
        assert not (tmp / 'book' / 'src' / 'migrating-to-0.2.md').exists()
        release(tmp, '0.2.0', False)
        log = (tmp / 'CHANGELOG.md').read_text(encoding='utf-8')
        assert log.index('### Breaking changes') < log.index('### Fixed') < log.index('## [0.1.0]')
        assert '- `Qux` is gone.\n  Instead: use `Quux`.' in log and 'quux();' not in log
        written = (tmp / 'book' / 'src' / 'migrating-to-0.2.md').read_text(encoding='utf-8')
        assert '```c\nquux();\n```' in written
        assert '(migrating-to-0.2.md)' in (tmp / 'book' / 'src' / 'SUMMARY.md').read_text(encoding='utf-8')
        assert [p.name for p in fragment_files(tmp)] == [], 'the fragments are deleted'
        # A fragment without breaks needs no version and writes no chapter.
        (tmp / 'changelog.d' / '3-add.md').write_text('### Added\n- More.\n', encoding='utf-8', newline='\n')
        release(tmp, None, False)
        assert '- More.' in (tmp / 'CHANGELOG.md').read_text(encoding='utf-8')
    # The fragments of this repository are valid.
    for path in fragment_files():
        parse(path)
    print('changelog.py: self-test passed')


if __name__ == '__main__':
    main()
