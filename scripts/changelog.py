#!/usr/bin/env python3
"""Fold changelog.d/ fragments into CHANGELOG.md, and write the migration chapter.

    python3 scripts/changelog.py           # fold the fragments in and delete them
                                           # (--version X.Y.Z when any breaks)
    python3 scripts/changelog.py --dry-run # print what would be written, change nothing
    python3 scripts/changelog.py --check   # validate the fragments
    python3 scripts/changelog.py --check --compile
                                           # and compile their `After:` code
                                           # (scripts/check-book-snippets.py)
    python3 scripts/changelog.py --areas   # the area each fragment is told under
    python3 scripts/changelog.py --notes X.Y.Z [--dry-run]
                                           # write the generated part of the
                                           # release page, book/src/release-X.Y.md
    python3 scripts/changelog.py --breaking FILE...
                                           # which of the fragments announce a
                                           # breaking change (semver-checks.py
                                           # asks this, see CONTRIBUTING.md)
    python3 scripts/changelog.py --selftest
                                           # check the rules with made-up input

A fragment is changelog.d/<pr-or-topic>.md holding one or more `### Heading`
sections with `- ` bullets (see changelog.d/README.md). Bullets are added at
the end of the Unreleased subsection with the same heading, under the bold
label of the fragment's area (AREAS: guessed from the file name and text,
or set with `<!-- area: Rendering -->` as the fragment's first line); a
missing subsection is created in Keep a Changelog order, with `Breaking
changes` first, and a missing area label in AREAS order. Fragments fold in
file-name order within an area.

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
when a fragment has one: the chapter is named after the release. The
chapter groups the entries by area too, and its `After:` code compiles
(`--check --compile` before the fold, scripts/check-book-snippets.py after).

The release page, book/src/release-X.Y.md ("What's new in X.Y"), has a story
written by hand and a part `--notes X.Y.Z` generates from the fragments (the
number of changes per area, the breaking changes linked to the chapter).
The fold copies the story to the top of the CHANGELOG section, which is the
GitHub release's notes, and lists the page in SUMMARY.md.
"""
from pathlib import Path
import re
import subprocess
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
# A bullet that says it breaks belongs in a Breaking changes section, as an entry.
BREAKING_BULLET = re.compile(r'^- \*\*Breaking[:*]', re.I)
# `<!-- area: Rendering -->` on a fragment's first line files it under that area.
AREA_LINE = re.compile(r'^<!--\s*area:\s*(.*?)\s*-->\s*$')

# The areas the release's changes are grouped by, in the order they are told
# (CHANGELOG.md, the migration chapter and the release page). A fragment goes
# to the area whose words its file name has most, then its text; the words
# are matched whole and ignore case. `<!-- area: ... -->` overrides the guess.
AREAS = [
    ('API and crate layout', r'one path|prelude|front doors|shortcuts|third party types|small apis|'
     r'error messages|loader convention|fallible startup|experimental items|unused api|naming|'
     r'typed handles|handle kinds|stability'),
    ('ECS and systems', r'ecs|worlds?|entit(?:y|ies)|archetypes?|quer(?:y|ies)|components?|systems?|'
     r'schedul\w*|commands|hierarch\w*|hooks?|plugins?|game states?|despawn\w*|change detection|'
     r'change tracking|resources?|events?|batch moves'),
    ('Rendering', r'render\w*|shadows?|gi|ssr|lights?|lighting|materials?|textures?|shaders?|sky|post|'
     r'gpu|taa|hdr|probes?|transparen\w*|stencil|meshes|mesh|sprites?|particles?|cameras?|depth|'
     r'exposure|upscal\w*|bloom|normal maps?|reflections?|occlusion|msaa|fog|decals?|instances?|oit|'
     r'passes|culling|wgpu|dx12|reverse z|viewports?|alpha masks?'),
    ('Animation', r'anim\w*|skeletons?|humanoid|clips?|retarget\w*|motion|joints?|morph\w*|bones?|'
     r'skinn\w*'),
    ('Assets, scenes and saves', r'assets?|gltf|cook\w*|packs?|image formats?|aseprite|ldtk|tiled|'
     r'tilemaps?|loaders?|loading|storage|saves?|prefabs?|scenes?|levels?|streaming|video|importer|'
     r'unity|fbx|tilesets?|compressed|hot reload|image limits'),
    ('UI, text and input', r'ui|text|ime|locali\w*|rich text|screen readers?|accessibility|gamepads?|'
     r'touch|controllers?|joysticks?|dualsense|keys?|input|actions?|widgets?|fonts?|glyphs?|'
     r'safe areas?|file dialogs?|window service|cursor|pressed this frame'),
    ('Audio', r'audio|sounds?|mixer|music|buses|reverb|voices?'),
    ('Physics, navigation and AI', r'physics\w*|colliders?|collisions?|nav\w*|crowds?|boids|'
     r'behaviou?r trees?|brains?|ai|raycasts?|water|buoyancy|heightfield|terrain|wave field'),
    ('Networking', r'net|networking|replication|rollback|webrtc|lobby|relay|steam|prediction|servers?|'
     r'multiplayer|sockets?|handshake|turn|lockstep|peers?|protocol|coturn'),
    ('Web, mobile and platforms', r'web|webgpu|webgl2?|wasm|browsers?|ios|android|xr|threads|macos|'
     r'linux|windows|package command|more c libraries'),
    ('Tools: editor, inspector and agents', r'editor|inspector|debug draw\w*|console|mcp|agents?|'
     r'agent suite|scenarios?|crash reports?|profil\w*|validation bundles?|control|mods?|cli|'
     r'backtraces?|authoring|inspection|record sheets'),
    ('C and C++', r'ponko_ffi|ffi|c\+\+|cpp|c abi|abi|bindings?|ponko\.h|ponko\.hpp|native|descriptors'),
    ('Guide and examples', r'tutorials?|cookbook|guide|docs|documentation|examples?|gallery|bevy|'
     r'rustdoc|ember hop|shard grove|coin dash|legions|cc0 art|maturity levels|stale statements'),
    ('Development and releases', r'checks?|fuzz\w*|semver|ci|bench\w*|scoreboard|releases?|leak|'
     r'miri|sanitizers?|unsafe|tests?|coverage|snapshots?|dependenc\w*|line endings|community|'
     r'security|audit|msrv|known content|nightly|quiet|numbers|yanked|health|trusted publishing|'
     r'migration notes|long run|flaky'),
]
OTHER = 'Other changes'
AREA_NAMES = [name for name, _ in AREAS] + [OTHER]
_AREA_WORDS = [(name, re.compile(r'(?<![\w])(?:' + words + r')(?![\w])', re.I)) for name, words in AREAS]


class FragmentError(Exception):
    """A fragment (or the generated files) breaks a rule; the message says which."""


def fragment_files(root=ROOT):
    folder = root / 'changelog.d'
    if not folder.is_dir():
        return []
    return sorted(p for p in folder.glob('*.md') if p.name != 'README.md')


def guess_area(name, text):
    """The area a fragment belongs to, from its file name first and its text second."""
    topic = re.sub(r'^(?:gl)?[\d-]+', '', name.removesuffix('.md')).replace('-', ' ').replace('_', ' ')
    best, score = OTHER, 0
    for area, words in _AREA_WORDS:
        found = 1000 * len(words.findall(topic)) + len(words.findall(text))
        if found > score:
            best, score = area, found
    return best


def fragment_area(name, text):
    """The area of a fragment: its `<!-- area: ... -->` line, or the guess."""
    first = text.split('\n', 1)[0]
    chosen = AREA_LINE.match(first)
    if chosen:
        if chosen.group(1) not in AREA_NAMES:
            raise FragmentError(f'{name}:1: unknown area `{chosen.group(1)}`; use one of: '
                                + ', '.join(AREA_NAMES))
        return chosen.group(1)
    return guess_area(name, text)


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
        elif number == 1 and AREA_LINE.match(line):
            continue
        elif not sections:
            raise FragmentError(f'{where}: text before the first `### Heading`')
        elif line.startswith('#'):
            raise FragmentError(f'{where}: use `### Heading` sections only')
        elif not sections[-1][1] and not line.startswith('- '):
            raise FragmentError(f'{where}: a section starts with a `- ` bullet')
        elif BREAKING_BULLET.match(line) and sections[-1][0] != BREAKING:
            raise FragmentError(f'{where}: a breaking change goes in `### Breaking changes` as an entry '
                                'with `Instead:`, `Before:` and `After:` (changelog.d/README.md)')
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
        text = path.read_text(encoding='utf-8')
        sections = parse_text(name, text)
        breaking_entries(name, sections)
        fragment_area(name, text)
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

def first_sentence(text):
    """`text` split after its first sentence: (sentence, rest).

    A sentence ends at `.`, `!` or `?` followed by a space or the end, outside
    inline code, so `a.b` in backticks and a wrapped first line don't cut it."""
    code = False
    for i, ch in enumerate(text):
        if ch == '`':
            code = not code
        elif not code and ch in '.!?' and (i + 1 == len(text) or text[i + 1] == ' '):
            if text[max(0, i - 3):i + 1].lower() in ('e.g.', 'i.e.') or text[max(0, i - 2):i + 1] == 'vs.':
                continue
            return text[:i + 1], text[i + 1:].strip()
    return text, ''


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


TITLE_MAX = 100
CLAUSE_BREAKS = (': ', '; ', ', so ')


def split_title(text):
    """An entry's what-changed text as (heading, rest) for the migration chapter.

    The heading is the first sentence; one longer than TITLE_MAX characters is
    cut at its first `: `, `; ` or `, so ` outside inline code, and the rest of
    it opens the paragraph."""
    sentence, rest = first_sentence(text)
    if len(sentence) > TITLE_MAX:
        code = False
        for i, ch in enumerate(sentence):
            if ch == '`':
                code = not code
                continue
            brk = next((b for b in CLAUSE_BREAKS if not code and i >= 10 and sentence.startswith(b, i)), None)
            if brk:
                tail = sentence[i + len(brk):]
                tail = ('So ' + tail) if brk == ', so ' else tail[:1].upper() + tail[1:]
                return sentence[:i], f'{tail} {rest}'.strip()
    return sentence.rstrip('.'), rest


def title_of(text):
    """The migration chapter's heading for an entry whose what-changed text is `text`."""
    return split_title(text)[0]


def entry_text(entry):
    return ' '.join([entry['headline'], *(l.strip() for l in entry['details'])])


def entry_title(entry):
    """The migration chapter's heading of an entry (split_title)."""
    return title_of(entry_text(entry))


def anchor(title):
    """The id mdBook gives a heading (its `normalize_id`): `## `A::b` is gone` -> `ab-is-gone`."""
    return ''.join(c for c in title.lower().replace(' ', '-') if c.isalnum() or c in '-_')


def chapter_entry(entry):
    """An entry as a `###` section of the migration chapter."""
    title, rest = split_title(entry_text(entry))
    out = ['', f'### {title}', '']
    if rest:
        out += [rest, '']
    out += ['**Instead:** ' + ' '.join(l.strip() for l in entry['instead']), '']
    for label, key in (('Before', 'before'), ('After', 'after')):
        lang, code = entry[key]
        out += [f'**{label}:**', '', f'```{lang}', *code, '```', '']
    return out[:-1]


def area_rank(area):
    return AREA_NAMES.index(area) if area in AREA_NAMES else len(AREA_NAMES)


def chapter_body(groups):
    """The chapter's entries, `## Area` then its entries, areas in AREA_NAMES order."""
    out = []
    for area, entries in sorted(groups, key=lambda g: area_rank(g[0])):
        out += ['', f'## {area}']
        for entry in entries:
            out += chapter_entry(entry)
    return out


def chapter_text(version, groups):
    """The migration chapter; `groups` is [(area, [entry])]."""
    areas = sorted((g for g in groups if g[1]), key=lambda g: area_rank(g[0]))
    contents = [f'- [{area}](#{anchor(area)}): {len(entries)} change{"s" if len(entries) > 1 else ""}'
                for area, entries in areas]
    return '\n'.join([
        f'# {chapter_title(version)}',
        '',
        f'Ponko {chapter_stem(version)} breaks the API in the ways listed here, grouped by area. Each '
        'entry says what changed, what to write instead, and shows the code before and after. The '
        'compiler names most of them; look the message up below. The code after each change is '
        'compiled by the guide\'s checks, so it builds as shown.',
        '',
        'The entries come from the release\'s changelog entries, so the '
        '[CHANGELOG](../../CHANGELOG.md) lists the same '
        'changes in short form next to the rest of the release. '
        '[STABILITY.md](../../STABILITY.md) says what '
        'minor releases may break and how a replaced item is deprecated first where that is cheap.',
        '',
        *contents,
        '',
        START_MARK,
        *chapter_body(areas),
        '',
        END_MARK,
        '',
    ])


def add_to_chapter(existing, groups):
    """An existing generated chapter with more entries, each under its area's heading."""
    if START_MARK not in existing or END_MARK not in existing:
        raise FragmentError('the migration chapter exists but was not generated by this script '
                            f'(no `{END_MARK}` marker); add the new entries by hand')
    head, rest = existing.split(START_MARK, 1)
    body, tail = rest.split(END_MARK, 1)
    lines = body.rstrip('\n').split('\n')
    for area, entries in groups:
        added = [l for entry in entries for l in chapter_entry(entry)]
        heads = [i for i, l in enumerate(lines) if l.startswith('## ')]
        mine = [i for i in heads if lines[i][3:] == area]
        if mine:
            end = next((i for i in heads if i > mine[0]), len(lines))
            while end > mine[0] + 1 and not lines[end - 1].strip():
                end -= 1
            lines[end:end] = added
        else:
            at = next((i for i in heads if area_rank(lines[i][3:]) > area_rank(area)), len(lines))
            while at > 0 and not lines[at - 1].strip():
                at -= 1
            lines[at:at] = ['', f'## {area}', *added]
    return head + START_MARK + '\n'.join(lines) + '\n\n' + END_MARK + tail


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


# The release page: book/src/release-X.Y.md, "What's new in X.Y". Its story
# (between STORY marks) is written by hand from the dry run; the rest is
# generated by `--notes X.Y.Z` from the fragments, so the cut only re-runs it.
# The fold copies the story to the top of the CHANGELOG section (the GitHub
# release's notes) and lists the page in SUMMARY.md.
STORY_START = '<!-- release story: written by hand, copied to CHANGELOG.md by the fold -->'
STORY_END = '<!-- release story end -->'
NOTES_START = '<!-- changelog.py --notes: generated from changelog.d/, do not edit by hand -->'
NOTES_END = '<!-- changelog.py --notes: end -->'


def release_page_name(version):
    return f'release-{chapter_stem(version)}.md'


def release_page_title(version):
    return f"What's new in {chapter_stem(version)}"


def story_of(page):
    """The hand-written story of a release page, or None if the page has none."""
    if STORY_START not in page or STORY_END not in page:
        return None
    return page.split(STORY_START, 1)[1].split(STORY_END, 1)[0].strip('\n')


def guide_links(text):
    """Relative chapter links (`(ecs.md#x)`) as links into the guide on the website, for CHANGELOG.md."""
    return re.sub(r'\]\((?!https?:|#)([\w./-]+?)\.md(#[\w-]+)?\)',
                  lambda m: f']({GUIDE}/{m.group(1)}.html{m.group(2) or ""})', text)


def notes_text(version, parsed):
    """The generated part of the release page: what changed where, and the breaking changes by area.

    parsed: [(area, sections, entries)] per fragment."""
    kinds = ['Added', 'Changed', 'Fixed', 'Removed', 'Deprecated', 'Security']
    counts, breaks = {}, {}
    for area, sections, entries in parsed:
        row = counts.setdefault(area, dict.fromkeys(kinds + [BREAKING], 0))
        for heading, lines in sections:
            key = heading if heading in row else 'Changed'
            row[key] += len(split_entries(lines))
        breaks.setdefault(area, []).extend(entries)
    shown = [k for k in [BREAKING] + kinds if any(r[k] for r in counts.values())]
    header = ['Area', *[('Breaking' if k == BREAKING else k) for k in shown]]
    table = ['| ' + ' | '.join(header) + ' |', '|' + '---|' * len(header)]
    for area in sorted(counts, key=area_rank):
        table.append('| ' + ' | '.join([area, *[str(counts[area][k] or '') for k in shown]]) + ' |')
    total = {k: sum(r[k] for r in counts.values()) for k in shown}
    table.append('| ' + ' | '.join(['**All**', *[f'**{total[k]}**' for k in shown]]) + ' |')
    chapter = chapter_name(version)
    out = [NOTES_START, '', '## Every change, by area', '',
           f'The [CHANGELOG](../../CHANGELOG.md) lists each of these {sum(total.values())} changes '
           'under its area; the numbers count its entries.', '', *table]
    if any(breaks.values()):
        out += ['', '## Breaking changes at a glance', '',
                f'Each links to its entry, with the code before and after, in '
                f'[{chapter_title(version)}]({chapter}).']
        for area in sorted(breaks, key=area_rank):
            if breaks[area]:
                out += ['', f'**{area}**', '']
                out += [f'- [{entry_title(e)}]({chapter}#{anchor(entry_title(e))})' for e in breaks[area]]
    return '\n'.join(out + ['', NOTES_END])


def release_page(version, existing, generated):
    """The release page with its generated part replaced (or a new page with an empty story)."""
    if existing is None:
        existing = '\n'.join([
            f'# {release_page_title(version)}', '', STORY_START,
            f'Ponko {chapter_stem(version)} is ... (one paragraph: what the release is about, for a '
            'game developer), then **Highlights**: one bullet per area that matters, each linking '
            'its guide chapter.', STORY_END, '', NOTES_START, NOTES_END, ''])
    if NOTES_START not in existing or NOTES_END not in existing:
        raise FragmentError(f'book/src/{release_page_name(version)} has no generated part '
                            f'(`{NOTES_START}` ... `{NOTES_END}`)')
    head, rest = existing.split(NOTES_START, 1)
    tail = rest.split(NOTES_END, 1)[1]
    return head + generated + tail


def summary_with_page(summary, version):
    """SUMMARY.md with the release page listed right before the migration chapter."""
    name = release_page_name(version)
    if f'({name})' in summary:
        return summary
    lines = summary.splitlines()
    at = next((i for i, l in enumerate(lines) if f'({chapter_name(version)})' in l), None)
    if at is None:
        at = max((i + 1 for i, l in enumerate(lines) if '(migrating-to-' in l), default=None)
    if at is None:
        raise FragmentError('book/src/SUMMARY.md has no migration chapter to put the release page next to')
    lines.insert(at, f'- [{release_page_title(version)}]({name})')
    return '\n'.join(lines) + '\n'


# Folding

def unreleased_bounds(lines):
    start = next((i for i, l in enumerate(lines) if l.startswith('## [Unreleased]')), None)
    if start is None:
        raise FragmentError('CHANGELOG.md has no `## [Unreleased]` section')
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith('## ')), len(lines))
    return start, end


AREA_LABEL = re.compile(r'^\*\*(.+)\*\*$')


def fold(body, heading, bullets, area=None):
    """Add bullets to the subsection `heading` of body (the Unreleased lines).

    With an area they go under its `**Area**` label in that subsection
    (created in AREA_NAMES order if missing)."""
    heads = [i for i, l in enumerate(body) if HEADING.match(l)]
    named = [i for i in heads if HEADING.match(body[i]).group(1) == heading]
    if not named:
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
        body[at:at] = ['', f'### {heading}']
        heads = [i for i, l in enumerate(body) if HEADING.match(l)]
        named = [at + 1]
    start = named[0]
    end = next((i for i in heads if i > start), len(body))
    if area is None:
        while end > start + 1 and not body[end - 1].strip():
            end -= 1
        body[end:end] = bullets
        return
    labels = [i for i in range(start + 1, end) if AREA_LABEL.match(body[i])]
    mine = [i for i in labels if AREA_LABEL.match(body[i]).group(1) == area]
    if mine:
        stop = next((i for i in labels if i > mine[0]), end)
        while stop > mine[0] + 1 and not body[stop - 1].strip():
            stop -= 1
        body[stop:stop] = bullets
        return
    at = next((i for i in labels if area_rank(AREA_LABEL.match(body[i]).group(1)) > area_rank(area)), end)
    while at > start + 1 and not body[at - 1].strip():
        at -= 1
    body[at:at] = ['', f'**{area}**', *bullets]


def add_pointer(body, pointer):
    """The line pointing at the migration chapter, right under the Breaking changes heading, once."""
    heads = [i for i, l in enumerate(body) if HEADING.match(l)]
    start = next(i for i in heads if HEADING.match(body[i]).group(1) == BREAKING)
    end = next((i for i in heads if i > start), len(body))
    if any('/migrating-to-' in l for l in body[start:end]):
        return
    body[start + 1:start + 1] = ['', pointer]


def add_story(body, story):
    """The release page's story at the top of the section, once (a later fold finds it there)."""
    text = guide_links(story).strip('\n').split('\n')
    if text and text[0] in body:
        return
    while body and not body[0].strip():
        body.pop(0)
    body[0:0] = ['', *text, '']


def parse_all(root):
    """Every fragment as (path, area, sections, entries); raises FragmentError."""
    parsed = []
    for path in fragment_files(root):
        name = path.relative_to(root).as_posix()
        text = path.read_text(encoding='utf-8')
        sections = parse_text(name, text)
        parsed.append((path, fragment_area(name, text), sections, breaking_entries(name, sections)))
    return parsed


def grouped(parsed):
    """[(area, [entry])] of the fragments' breaking entries, areas in AREA_NAMES order."""
    groups = {}
    for _, area, _, entries in parsed:
        if entries:
            groups.setdefault(area, []).extend(entries)
    return sorted(groups.items(), key=lambda g: area_rank(g[0]))


def release(root, version, dry_run):
    """Fold the fragments into CHANGELOG.md and write the migration chapter.

    Returns the lines to print; with dry_run nothing is written or deleted.
    """
    files = fragment_files(root)
    parsed = parse_all(root)
    entries = [e for *_, found in parsed for e in found]
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
    for _, area, sections, _ in parsed:
        for heading, bullets in sections:
            if heading == BREAKING:
                bullets = [l for entry in split_entries(bullets) for l in changelog_lines(
                    parse_entry('', entry))]
            fold(body, heading, bullets, area)
    if entries:
        add_pointer(body, pointer_line(version))
    book = root / 'book' / 'src'
    summary = book / 'SUMMARY.md'
    page = book / release_page_name(version) if version else None
    story = story_of(page.read_text(encoding='utf-8')) if page and page.exists() else None
    if story:
        add_story(body, story)
    while body and not body[-1].strip():
        body.pop()
    body.append('')
    out = [lines[start], *body]
    new_summary = summary.read_text(encoding='utf-8') if summary.exists() else None
    if entries:
        chapter = book / chapter_name(version)
        text = (add_to_chapter(chapter.read_text(encoding='utf-8'), grouped(parsed)) if chapter.exists()
                else chapter_text(version, grouped(parsed)))
        new_summary = summary_with(new_summary, version)
        out += ['', f'--- book/src/{chapter.name}', text.rstrip('\n'), '',
                f'--- book/src/SUMMARY.md gets: - [{chapter_title(version)}]({chapter.name})']
        if not dry_run:
            chapter.write_text(text, encoding='utf-8', newline='\n')
    if story:
        new_summary = summary_with_page(new_summary, version)
        out += [f'--- book/src/SUMMARY.md gets: - [{release_page_title(version)}]({page.name})']
    if not dry_run:
        if new_summary is not None:
            summary.write_text(new_summary, encoding='utf-8', newline='\n')
        lines[start + 1:end] = body
        changelog.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')
        for path in files:
            path.unlink()
    return out if dry_run else [f'Folded {len(files)} fragment(s) into CHANGELOG.md'
                                + (f' and wrote book/src/{chapter_name(version)}.' if entries else '.')]


def notes(root, version, dry_run):
    """Write the generated part of book/src/release-X.Y.md from the fragments; returns the page."""
    parsed = parse_all(root)
    page = root / 'book' / 'src' / release_page_name(version)
    existing = page.read_text(encoding='utf-8') if page.exists() else None
    text = release_page(version, existing, notes_text(version, [(a, s, e) for _, a, s, e in parsed]))
    if not dry_run:
        page.write_text(text, encoding='utf-8', newline='\n')
    return text


def compile_snippets():
    """Compile every `After:` block of the fragments (scripts/check-book-snippets.py does it)."""
    return subprocess.run([sys.executable, str(ROOT / 'scripts' / 'check-book-snippets.py'),
                           '--changelog-only']).returncode


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
            if '--compile' in args:
                sys.exit(compile_snippets())
            return
        if '--areas' in args:
            for path, area, _, _ in parse_all(ROOT):
                print(f'{area:38} {path.name}')
            return
        if '--notes' in args:
            version = args[args.index('--notes') + 1]
            version_parts(version)
            text = notes(ROOT, version, '--dry-run' in args)
            print(text if '--dry-run' in args else f'Wrote book/src/{release_page_name(version)}.')
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
    # The chapter form has everything, grouped by area.
    chapter = chapter_text('0.2.0', [('Rendering', found[1:]), ('ECS and systems', found[:1])])
    assert chapter.startswith('# Migrating from 0.1 to 0.2\n')
    assert '### `Foo::bar` takes a `Baz` now\n' in chapter and '### `Qux` is gone\n' in chapter
    assert chapter.index('## ECS and systems') < chapter.index('`Foo::bar` takes') \
        < chapter.index('## Rendering') < chapter.index('`Qux` is gone'), 'areas in AREAS order'
    assert '- [ECS and systems](#ecs-and-systems): 1 change' in chapter
    assert '**Instead:** wrap the argument, `Baz::new(x)`. It keeps the old meaning.' in chapter
    assert '```rust\nfoo.bar(x);\n\nfoo.baz();\n```' in chapter and '```c\nquux();\n```' in chapter
    assert chapter.count(START_MARK) == chapter.count(END_MARK) == 1
    more = add_to_chapter(chapter, [('Rendering', entries(base)), ('Audio', entries(base))])
    assert more.index('`Qux` is gone') < more.index('### `A` changed') < more.index('## Audio') \
        < more.index(END_MARK), 'a later fold adds under the area, or a new area in order'
    try:
        add_to_chapter('# Hand written\n', [('Rendering', found)])
        raise AssertionError('a chapter that is not generated is not overwritten')
    except FragmentError:
        pass
    # The heading is the first sentence, even when the first line wraps.
    wrapped = entries(base.replace('- `A` changed.', '- `A` has two new values,\n  `B` and `C.d`. More words.'))[0]
    assert entry_title(wrapped) == '`A` has two new values, `B` and `C.d`'
    assert '\nMore words.\n' in '\n'.join(chapter_entry(wrapped))
    assert first_sentence('Use e.g. this. Then') == ('Use e.g. this.', 'Then')
    long = '`ponko::x` is gone: everything lives in `ponko::y`, under the names `y` gave it already, ' \
           'for every type there was. More.'
    assert split_title(long) == ('`ponko::x` is gone', 'Everything lives in `ponko::y`, under the names '
                                 '`y` gave it already, for every type there was. More.')
    assert split_title('`A` has new values, `B: C` and more of them, so a `match` over every value '
                       'no longer compiles anywhere.')[0] == '`A` has new values, `B: C` and more of them'
    assert title_of('Short: kept whole.') == 'Short: kept whole'

    assert anchor('`A::b` is gone, really') == 'ab-is-gone-really'

    # Areas: the `<!-- area: -->` line, else the file name, else the text.
    assert fragment_area('x.md', '<!-- area: Audio -->\n### Added\n- a\n') == 'Audio'
    try:
        fragment_area('x.md', '<!-- area: Sounds -->\n### Added\n- a\n')
        raise AssertionError('an unknown area is refused')
    except FragmentError as error:
        assert 'unknown area' in str(error)
    assert guess_area('gl12-shadow-cache.md', '### Fixed\n- The physics step.\n') == 'Rendering'
    assert guess_area('fix.md', '### Fixed\n- The physics step and the collider.\n') == 'Physics, navigation and AI'
    assert guess_area('fix.md', '### Fixed\n- Something.\n') == OTHER
    assert parse_text('f.md', '<!-- area: Audio -->\n### Added\n- a\n') == [('Added', ['- a'])]
    try:
        parse_text('f.md', '### Changed\n- **Breaking:** `A` is gone.\n')
        raise AssertionError('a breaking bullet outside Breaking changes is refused')
    except FragmentError as error:
        assert '### Breaking changes' in str(error)

    # SUMMARY.md: the line goes after the last migration chapter, once.
    book = '# Summary\n\n- [Naming](naming.md)\n- [Migrating from 0.0.1 to 0.1.0](migrating-to-0.1.md)\n- [Reflection](reflection.md)\n'
    added = summary_with(book, '0.2.0')
    assert added.splitlines()[4] == '- [Migrating from 0.1 to 0.2](migrating-to-0.2.md)'
    assert summary_with(added, '0.2.0') == added

    # Folding: the entries turn into short bullets, with the pointer once,
    # each under its area's label.
    body = ['', '### Added', '', '**Rendering**', '- earlier']
    fold(body, BREAKING, [l for e in found for l in changelog_lines(e)], 'Audio')
    fold(body, 'Added', ['- an ECS thing'], 'ECS and systems')
    fold(body, 'Added', ['- later'], 'Rendering')
    fold(body, 'Added', ['- plain'])
    add_pointer(body, pointer_line('0.2.0'))
    add_pointer(body, pointer_line('0.2.0'))
    text = '\n'.join(body)
    assert text.index('### Breaking changes') < text.index('**Audio**') < text.index('### Added'), 'Breaking changes come first'
    assert text.index('### Breaking changes') < text.index('migrating-to-0.2.html') < text.index('**Audio**')
    assert text.index('**ECS and systems**\n- an ECS thing') < text.index('**Rendering**\n- earlier\n- later\n- plain')
    assert text.count('migrating-to-0.2.html') == 1
    assert 'foo.bar' not in text, 'the changelog has no snippets'
    # The story goes on top once, with chapter links into the guide.
    top = ['', '### Added', '- x']
    add_story(top, 'Ponko 0.2 is here.\n\n- [ECS](ecs.md#queries), [site](https://x.dev/a.md)')
    add_story(top, 'Ponko 0.2 is here.')
    assert top[:4] == ['', 'Ponko 0.2 is here.', '', f'- [ECS]({GUIDE}/ecs.html#queries), [site](https://x.dev/a.md)']
    assert top.count('Ponko 0.2 is here.') == 1

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
        # The release page: a new one has an empty story; --notes fills its generated part.
        page = notes(tmp, '0.2.0', False)
        assert page.startswith("# What's new in 0.2\n") and STORY_START in page
        assert '## Breaking changes at a glance' in page and '(migrating-to-0.2.md#qux-is-gone)' in page
        assert '| **All** |' in page
        story_page = tmp / 'book' / 'src' / 'release-0.2.md'
        story_page.write_text(page.replace(story_of(page), 'Ponko 0.2 is about [ECS](ecs.md).'), encoding='utf-8')
        again = notes(tmp, '0.2.0', False)
        assert 'Ponko 0.2 is about [ECS](ecs.md).' in again, 'a re-run keeps the story'
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
        summary = (tmp / 'book' / 'src' / 'SUMMARY.md').read_text(encoding='utf-8')
        assert summary.index("(release-0.2.md)") < summary.index('(migrating-to-0.2.md)')
        assert f'Ponko 0.2 is about [ECS]({GUIDE}/ecs.html).' in log.split('### Breaking changes')[0], \
            'the story heads the section'
        assert '## ' in written and '### `Qux` is gone' in written
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
