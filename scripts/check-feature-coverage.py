#!/usr/bin/env python3
"""Fail when a cargo feature of the workspace has no check line in checks.yml.

The list of checks is scripts/checks.yml, which maintainers run locally. A
feature added without a line there can break without any listed command
noticing, so every feature of every workspace crate needs, in checks.yml,

  - a clippy line that turns it on (`cargo clippy ... --features f`),
  - a test line that turns it on (`cargo test` or `cargo miri test`), and
  - for a crate docs.rs builds with it (its `[package.metadata.docs.rs]`
    lists the feature, or sets all-features), a `cargo doc` line.

A line "turns it on" when its `--features` (or `-F`) names the feature, as
`feature` or `crate/feature`, when `--all-features` is given for the package,
when the feature is a default one and `--no-default-features` is not given, or
when a feature that is on lists it (`image-formats` turns on `image-bmp`).
A line's package is its `-p` (all of them); with `--workspace` every member;
with neither, the root package. Matrix entries (`scope`, `args`) are read
as the commands they make. Nothing is run: checks.yml is read as text.

What is allowed to lack a line goes in scripts/feature-checks.txt, one
`crate/feature kind reason` per line, `kind` being clippy, test or doc. The
check also fails when such an entry is not needed any more (the line exists
now) or names a feature that is gone, so the list only shrinks.

The same goes for the browser smoke runners: a program in web/smoke or
web/gallery (a first line of `#!/usr/bin/env node`) must be named in checks.yml,
or be listed as `runner <path> <reason>` in the same file.

    python3 scripts/check-feature-coverage.py            # check
    python3 scripts/check-feature-coverage.py --list     # every feature and its checks
    python3 scripts/check-feature-coverage.py --selftest # the parsing, with made-up input
"""
from pathlib import Path
import re
import shlex
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent
CI = ROOT / 'scripts' / 'checks.yml'
EXEMPT = ROOT / 'scripts' / 'feature-checks.txt'
KINDS = ('clippy', 'test', 'doc')
ROOT_PACKAGE = 'ponko'


class Crate:
    def __init__(self, manifest):
        package = manifest['package']
        self.name = package['name']
        self.features = manifest.get('features', {})
        docs = package.get('metadata', {}).get('docs', {}).get('rs', {})
        self.docs_all = bool(docs.get('all-features'))
        self.docs_features = set(docs.get('features', []))
        # Dependencies on other crates of the workspace: name -> (features, defaults on).
        # Cargo unifies them across a build, so a feature one member asks of
        # another is on in every build that includes both.
        self.dependencies = {}
        tables = [manifest.get(kind, {}) for kind in ('dependencies', 'dev-dependencies', 'build-dependencies')]
        for target in manifest.get('target', {}).values():
            tables += [target.get(kind, {}) for kind in ('dependencies', 'dev-dependencies')]
        for table in tables:
            for name, spec in table.items():
                if isinstance(spec, dict) and 'path' in spec:
                    features, defaults = self.dependencies.get(name, ([], False))
                    self.dependencies[name] = (features + list(spec.get('features', [])),
                                               defaults or spec.get('default-features', True))

    def documented(self, feature):
        return self.docs_all or feature in self.docs_features


def load_crates(root):
    top = tomllib.loads((root / 'Cargo.toml').read_text())
    crates = {}
    for member in top['workspace']['members']:
        manifest = tomllib.loads((root / member / 'Cargo.toml').read_text())
        crate = Crate(manifest)
        crates[crate.name] = crate
    return crates


# ---- checks.yml as commands ----------------------------------------------------

JOB = re.compile(r'^  [A-Za-z0-9_-]+:\s*$')
FIELD = re.compile(r'^(\s*)(- )?([A-Za-z_]+):\s*(.*?)\s*$')


def matrix_entries(lines):
    """The `include:` entries of one job's matrix, as dicts of scalar fields."""
    entries, current, inside = [], None, False
    for line in lines:
        if re.match(r'^\s+include:\s*$', line):
            inside = True
            continue
        if not inside:
            continue
        m = FIELD.match(line)
        if not m or line.strip().startswith('#'):
            continue
        indent, dash, key, value = m.groups()
        if len(indent) < 10 and not dash:
            break  # left the include list
        if dash and key == 'name':
            current = {}
            entries.append(current)
        if current is not None and key in ('name', 'scope', 'args', 'shard'):
            current[key] = value.strip('"\'')
    return entries


def substitute(command, entry):
    def matrix(m):
        expression = m.group(1)
        key, _, default = expression.partition('||')
        value = entry.get(key.strip().removeprefix('matrix.'))
        if value is None:
            value = default.strip().strip("'") if default else ''
        return value

    command = re.sub(r'\$\{\{\s*([^}]*matrix\.[^}]*?)\s*\}\}', matrix, command)
    return command.replace('$ARGS', entry.get('args', ''))


def commands(text):
    """Every `cargo ...` command of checks.yml, matrix entries expanded."""
    jobs, current = [], []
    for line in text.splitlines():
        if JOB.match(line) and current:
            jobs.append(current)
            current = []
        current.append(line)
    jobs.append(current)
    found = []
    for job in jobs:
        entries = matrix_entries(job) or [{}]
        raw = []
        for number, line in enumerate(job):
            stripped = line.strip()
            if stripped.startswith('#'):
                continue
            body = re.sub(r'^(- )?(run|name):\s*', '', stripped) if stripped.startswith(('- run:', 'run:')) else stripped
            raw.append(body)
        joined = []
        for body in raw:  # backslash continuations make one command
            if joined and joined[-1].endswith('\\'):
                joined[-1] = joined[-1][:-1].rstrip() + ' ' + body
            else:
                joined.append(body)
        for body in joined:
            m = re.match(r'^(?:[A-Z_]+=\S+\s+)*(?:dbus-run-session -- )?(cargo\b.*)$', body)
            if not m:
                continue
            for entry in entries:
                found.append(substitute(m.group(1), entry))
    return found


def parse(command):
    """(kind, packages, workspace, features, all_features, no_default) of a cargo command."""
    try:
        words = shlex.split(command, comments=False)
    except ValueError:
        return None
    if not words or words[0] != 'cargo':
        return None
    rest = [w for w in words[1:] if not w.startswith('+')]
    if not rest:
        return None
    if rest[0] == 'miri' and len(rest) > 1:
        verb = rest[1]
    else:
        verb = rest[0]
    kind = {'clippy': 'clippy', 'test': 'test', 'doc': 'doc'}.get(verb)
    if kind is None:
        return None
    packages, features = [], []
    workspace = all_features = no_default = False
    i = 0
    while i < len(rest):
        word = rest[i]
        value = rest[i + 1] if i + 1 < len(rest) else ''
        if word in ('-p', '--package'):
            packages.append(value)
            i += 1
        elif word.startswith('--package='):
            packages.append(word.split('=', 1)[1])
        elif word in ('--features', '-F'):
            features += [f for f in re.split(r'[,\s]+', value) if f]
            i += 1
        elif word.startswith('--features='):
            features += [f for f in re.split(r'[,\s]+', word.split('=', 1)[1]) if f]
        elif word == '--workspace' or word == '--all':
            workspace = True
        elif word == '--all-features':
            all_features = True
        elif word == '--no-default-features':
            no_default = True
        i += 1
    return kind, packages, workspace, features, all_features, no_default


def close(crate, names):
    """`names` and every feature of `crate` they list; also the other crates' features they ask for."""
    on, asked, todo = set(), [], list(names)
    while todo:
        name = todo.pop()
        if name in on or name not in crate.features:
            continue
        on.add(name)
        for item in crate.features[name]:
            if item.startswith('dep:'):
                continue
            if '/' in item:
                owner, _, local = item.partition('/')
                asked.append((owner.rstrip('?'), local))
            else:
                todo.append(item)
    return on, asked


def coverage(crates, text):
    """{(crate, feature): set of kinds with a line}: the features each line's own packages are built with."""
    covered = {(c, f): set() for c in crates for f in crates[c].features}
    for command in commands(text):
        parsed = parse(command)
        if parsed is None:
            continue
        kind, packages, workspace, features, all_features, no_default = parsed
        selected = [p for p in (list(crates) if workspace else (packages or [ROOT_PACKAGE])) if p in crates]
        # What each crate is asked for. A selected package gets its `--features`
        # and (unless --no-default-features) its defaults; a dependency gets what its
        # dependents ask of it. Resolved to a fixpoint, as Cargo unifies them.
        wanted = {name: set() for name in crates}
        active = set(selected)
        for name in selected:
            if not no_default:
                wanted[name].add('default')
            if all_features:
                wanted[name] |= set(crates[name].features)
        for feature in features:
            owner, _, local = feature.rpartition('/')
            owner = owner.rstrip('?')
            if owner:
                if owner in crates:
                    wanted[owner].add(local)
                    active.add(owner)
            else:
                for name in selected:
                    if local in crates[name].features:
                        wanted[name].add(local)
        on = {name: set() for name in crates}
        changed = True
        while changed:
            changed = False
            for name in sorted(active):
                closed, asked = close(crates[name], wanted[name])
                if not closed <= on[name]:
                    on[name] |= closed
                    changed = True
                for owner, local in asked:
                    if owner in crates and (local not in wanted[owner] or owner not in active):
                        wanted[owner].add(local)
                        active.add(owner)
                        changed = True
                for dep, (dep_features, defaults) in crates[name].dependencies.items():
                    if dep not in crates:
                        continue
                    extra = set(dep_features) | ({'default'} if defaults else set())
                    if not extra <= wanted[dep] or dep not in active:
                        wanted[dep] |= extra
                        active.add(dep)
                        changed = True
        for name in selected:
            for feature in on[name]:
                covered[(name, feature)].add(kind)
    return covered


# ---- the exemption list ------------------------------------------------------

def load_exemptions(path):
    entries, problems = {}, []
    if not path.exists():
        return entries, problems
    for number, line in enumerate(path.read_text().splitlines(), 1):
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        parts = line.split(None, 2)
        if parts and parts[0] == 'runner':
            parts = line.split(None, 2)
            if len(parts) < 3:
                problems.append(f'{path.name}:{number}: write `runner <path> <reason>`')
            else:
                entries[('runner', parts[1])] = parts[2]
            continue
        if len(parts) < 3 or parts[1] not in KINDS or '/' not in parts[0]:
            problems.append(f'{path.name}:{number}: write `crate/feature kind reason` (kind: {", ".join(KINDS)})')
            continue
        key = (parts[0], parts[1])
        if key in entries:
            problems.append(f'{path.name}:{number}: {parts[0]} {parts[1]} is listed twice')
        entries[key] = parts[2]
    return entries, problems


def check(crates, text, exemptions):
    covered = coverage(crates, text)
    problems, used = [], set()
    for (name, feature), kinds in sorted(covered.items()):
        needed = ['clippy', 'test'] + (['doc'] if crates[name].documented(feature) else [])
        for kind in needed:
            entry = (f'{name}/{feature}', kind)
            if kind in kinds:
                if entry in exemptions:
                    problems.append(f'{EXEMPT.name}: {entry[0]} {kind} has a check line now: remove the entry')
                continue
            if entry in exemptions:
                used.add(entry)
                continue
            problems.append(f'{name}/{feature}: no {kind} line in checks.yml (add one, or give a reason in {EXEMPT.name})')
    known = {f'{c}/{f}' for c, f in covered}
    for (feature, kind), reason in exemptions.items():
        if feature == 'runner':
            continue
        if feature not in known:
            problems.append(f'{EXEMPT.name}: {feature} is not a feature any more: remove the entry')
    return problems


def runners(root):
    """The browser runners: programs in web/smoke and web/gallery that start with a node shebang."""
    found = []
    for folder in ('web/smoke', 'web/gallery'):
        for path in sorted((root / folder).glob('*.mjs')):
            if path.read_text().startswith('#!/usr/bin/env node'):
                found.append(f'{folder}/{path.name}')
    return found


def check_runners(found, text, exemptions):
    problems = []
    for runner in found:
        named = runner in text
        entry = ('runner', runner)
        if not named and entry not in exemptions:
            problems.append(f'{runner}: not run in checks.yml (add a step, or `runner {runner} <reason>` to {EXEMPT.name})')
        if named and entry in exemptions:
            problems.append(f'{EXEMPT.name}: {runner} is run in checks.yml now: remove the entry')
    for kind, name in exemptions:
        if kind == 'runner' and name not in found:
            problems.append(f'{EXEMPT.name}: {name} is not a runner any more: remove the entry')
    return problems


def table(crates, text):
    covered = coverage(crates, text)
    rows = []
    for (name, feature), kinds in sorted(covered.items()):
        marks = ' '.join(f'{k}:{"yes" if k in kinds else "-"}' for k in KINDS)
        rows.append(f'{name}/{feature:<28} {marks}' + ('  (docs.rs)' if crates[name].documented(feature) else ''))
    return '\n'.join(rows)


# ---- self-test ---------------------------------------------------------------

SELFTEST_MANIFESTS = {
    'ponko': {
        'package': {'name': 'ponko', 'metadata': {'docs': {'rs': {'features': ['video']}}}},
        'features': {'default': ['audio'], 'audio': [], 'video': ['dep:rav1d'], 'lobby': [],
                     'image-formats': ['image-bmp'], 'image-bmp': ['image/bmp']},
    },
    'ponko_ffi': {
        'package': {'name': 'ponko_ffi', 'metadata': {'docs': {'rs': {'all-features': True}}}},
        'features': {'webrtc': ['ponko/lobby'], 'steam': []},
    },
}

SELFTEST_CI = """
jobs:
  clippy:
    strategy:
      matrix:
        include:
          - name: formats
            scope: -p ponko
            args: --features image-formats
          - name: default
            args: ""
    steps:
      - run: cargo clippy ${{ matrix.scope || '--workspace' }} --all-targets ${{ matrix.args }}
  test:
    steps:
      # cargo test -p ponko --features lobby   (a comment is not a line)
      - run: cargo test --workspace
      - run: |
          cargo test -p ponko --no-default-features --features video \\
            --test video
          cargo +nightly miri test -p ponko_ffi --features webrtc
  docs:
    steps:
      - run: cargo doc -p ponko --no-deps --features video
"""


def selftest():
    crates = {name: Crate(m) for name, m in SELFTEST_MANIFESTS.items()}
    covered = coverage(crates, SELFTEST_CI)
    expect = {
        ('ponko', 'audio'): {'clippy', 'test', 'doc'},  # a default feature of every default build
        ('ponko', 'image-formats'): {'clippy'},
        ('ponko', 'image-bmp'): {'clippy'},  # turned on by image-formats
        ('ponko', 'video'): {'test', 'doc'},
        ('ponko', 'lobby'): set(),  # only in a comment
        ('ponko_ffi', 'webrtc'): {'test'},
        ('ponko_ffi', 'steam'): set(),
    }
    failures = []
    for key, kinds in expect.items():
        if covered[key] != kinds:
            failures.append(f'{key}: {sorted(covered[key])}, expected {sorted(kinds)}')
    # `webrtc` lists ponko's lobby: a ponko_ffi build with it turns it on in ponko.
    if 'test' in covered[('ponko', 'lobby')]:
        failures.append('a feature of another crate was taken for ponko/lobby')
    problems = check(crates, SELFTEST_CI, {})
    wanted = [
        'ponko/lobby: no clippy line',
        'ponko/lobby: no test line',
        'ponko/video: no clippy line',
        'ponko_ffi/steam: no doc line',
        'ponko_ffi/webrtc: no clippy line',
    ]
    for want in wanted:
        if not any(p.startswith(want) for p in problems):
            failures.append(f'expected a problem starting `{want}`')
    if any(p.startswith('ponko/audio') for p in problems):
        failures.append('ponko/audio was reported although the default builds cover it')
    gone = check(crates, SELFTEST_CI, {('ponko/vanished', 'test'): 'x', ('ponko/video', 'test'): 'x'})
    if not any('ponko/vanished' in p and 'not a feature' in p for p in gone):
        failures.append('an exemption of a missing feature was accepted')
    if not any('ponko/video test has a check line now' in p for p in gone):
        failures.append('an exemption that is not needed any more was accepted')
    # A dependency's request counts in a build of both crates: the lobby
    # server asks for the client's feature, so a workspace build has it on.
    crates['ponko_lobby'] = Crate({'package': {'name': 'ponko_lobby'}, 'features': {},
                                   'dependencies': {'ponko': {'path': '../..', 'default-features': False, 'features': ['lobby']}}})
    unified = coverage(crates, 'jobs:\n  t:\n    steps:\n      - run: cargo test --workspace\n  c:\n    steps:\n      - run: cargo test -p ponko_lobby\n')
    if unified[('ponko', 'lobby')] != {'test'}:
        failures.append(f'a workspace build did not take the dependency edge: {sorted(unified[("ponko", "lobby")])}')
    # `-p ponko_lobby` alone builds ponko with the feature but runs no test of ponko.
    alone = coverage(crates, 'jobs:\n  c:\n    steps:\n      - run: cargo test -p ponko_lobby\n')
    if alone[('ponko', 'lobby')]:
        failures.append('a build of another package was counted as checking ponko/lobby')
    found = ['web/smoke/a.mjs', 'web/smoke/b.mjs']
    runner_problems = check_runners(found, 'node web/smoke/a.mjs --x', {('runner', 'web/smoke/gone.mjs'): 'x'})
    if len(runner_problems) != 2 or 'b.mjs' not in runner_problems[0] or 'gone.mjs' not in runner_problems[1]:
        failures.append(f'runner check: {runner_problems}')
    for failure in failures:
        print(f'selftest: {failure}', file=sys.stderr)
    return 1 if failures else 0


def main(argv):
    if '--selftest' in argv:
        status = selftest()
        if not status:
            print('check-feature-coverage selftest: ok')
        return status
    crates = load_crates(ROOT)
    text = CI.read_text()
    if '--list' in argv:
        print(table(crates, text))
        return 0
    exemptions, problems = load_exemptions(EXEMPT)
    problems += check(crates, text, exemptions)
    problems += check_runners(runners(ROOT), text, exemptions)
    if problems:
        print('Missing check lines (scripts/checks.yml is the list of checks):', file=sys.stderr)
        for problem in problems:
            print(f'  {problem}', file=sys.stderr)
        return 1
    total = sum(len(c.features) for c in crates.values())
    print(f'every one of the {total} features of {len(crates)} crates has its check lines')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
