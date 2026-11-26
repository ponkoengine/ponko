#!/usr/bin/env python3
"""Check the published crates for semver breaks (the `semver` job of checks.yml).

    semver-checks.py run [--base BRANCH] [--breaking]
    semver-checks.py selftest   # check the decisions with made-up input

`cargo semver-checks` compares the public API of each published crate with
the last release on crates.io. What a break means depends on the branch:

  release-*  A patch release must not break anything: every break fails.
  anything   Breaks since the last release pile up on `main` until the next
  else       minor release, so "there is a break" says nothing about this
             change. The crates.io comparison is therefore only the report.
             What gates is the break this change adds, found by comparing
             with the commit it is based on. It passes only with the
             `breaking` label and a changelog fragment that has a
             `### Breaking changes` section (CONTRIBUTING.md, "Semver checks").
             The other way round, a pull request with the `breaking` label
             must carry such a fragment, whether or not the tool found a
             break: the label promises a migration note, and the tool does
             not see every break (behaviour, defaults).

Which packages and features are checked comes from Cargo: every package
that can be published (not `publish = false`), with the features it builds on
docs.rs (`[package.metadata.docs.rs]`), so a new crate is covered without
touching this file. Crates the tool can't read (proc-macro crates, binaries)
are listed in the report.

`run` checks the current checkout as a change for the branch BRANCH (default
`main`; a `release-*` branch, or a checkout on one, is release mode): the
change is compared with `git merge-base origin/BRANCH HEAD`, so fetch first.
`--breaking` says the pull request carries (or will carry) the `breaking`
label; without it a break this change adds fails, and with it a
`### Breaking changes` fragment is required. Nothing is read from a server.
"""
from fnmatch import fnmatch
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
LABEL = 'breaking'
HEADING = 'Breaking changes'
RELEASE_BRANCHES = 'release-*'
LIB_KINDS = {'lib', 'rlib', 'dylib', 'cdylib', 'staticlib'}
# `cargo semver-checks` exits 100 when it found a break; any other failure
# is an error (it couldn't build, the crate isn't published, ...).
BREAK_EXIT = 100


# Decisions: plain functions of their input, checked by `selftest`.

def decide_mode(base_ref, ref_name):
    """'release' on a release-* branch (or a pull request into one), else 'main'."""
    return 'release' if fnmatch(base_ref or ref_name or '', RELEASE_BRANCHES) else 'main'


def feature_args(package):
    """The cargo-semver-checks flags for the features the package builds on docs.rs.

    Not the tool's default (every feature but `unstable*`): that would include
    `hotpatch` and `bench-internals`, which are not part of the API.
    """
    docs = ((package.get('metadata') or {}).get('docs') or {}).get('rs') or {}
    if docs.get('all-features'):
        return ['--all-features']
    args = ['--only-explicit-features' if docs.get('no-default-features') else '--default-features']
    if docs.get('features'):
        args += ['--features', ','.join(docs['features'])]
    return args


def skip_reason(package):
    """Why the tool can't read this package, or None."""
    kinds = {kind for target in package['targets'] for kind in target['kind']}
    if 'proc-macro' in kinds:
        return 'proc-macro crate (the tool reads no API from it)'
    if not kinds & LIB_KINDS:
        return 'no library target (binary only, no API)'
    return None


def verdict(mode, breaks, new_breaks, errors, labeled, fragment):
    """(ok, message lines) for the whole run.

    breaks: crates that break against the last release; new_breaks: of those,
    the ones this change breaks (None in release mode, where every break counts).
    """
    lines = []
    if errors:
        return False, [f'`cargo semver-checks` could not check {", ".join(errors)}: '
                       'a label cannot excuse that, fix the error above.']
    if not breaks:
        return True, ['No breaking change against the last release.']
    if mode == 'release':
        return False, [f'Breaking change in {", ".join(breaks)}: a patch release must not '
                       'break the API (STABILITY.md). Fix it on `main` first and carry over a '
                       'change that is compatible, or release it as the next minor.']
    if not new_breaks:
        return True, [f'{", ".join(breaks)} differ from the last release, but not because of '
                      'this change: earlier merged changes announced them. Nothing to do.']
    lines.append(f'This change breaks the API of {", ".join(new_breaks)}.')
    missing = []
    if not labeled:
        missing.append(f'the `{LABEL}` label on the pull request (add it, then run this again '
                       'with --breaking)')
    if not fragment:
        missing.append(f'a changelog fragment with a `### {HEADING}` section naming the replacement')
    if missing:
        return False, lines + ['Missing: ' + ' and '.join(missing) + '.',
                               'Or undo the break: CONTRIBUTING.md, "Semver checks".']
    return True, lines + [f'Allowed: `{LABEL}` label and a `### {HEADING}` fragment are there.']


def label_verdict(mode, labeled, fragment):
    """(ok, message lines) for the `breaking` label itself, whatever the tool found.

    The label promises a migration note: a changelog fragment with a
    `### Breaking changes` entry (before/after code, validated by
    `changelog.py --check`). A fragment without the label is only a notice:
    contributors can't add labels, and a reviewer decides.
    """
    if labeled and mode == 'release':
        return False, [f'The `{LABEL}` label is on a pull request into a release branch, '
                       'which must not break the API (STABILITY.md). Target `main` instead.']
    if labeled and not fragment:
        return False, [f'The `{LABEL}` label needs a migration note: add a changelog fragment with a '
                       f'`### {HEADING}` entry (what changed, what to write instead, a before/after '
                       'snippet; changelog.d/README.md), then run this again. Or remove the label '
                       'if nothing breaks.']
    if fragment and not labeled:
        return True, [f'This change has a `### {HEADING}` fragment but no `{LABEL}` label: a '
                      'maintainer should add it.']
    return True, []


# Side effects: cargo, git and gh.

def run(cmd, **kw):
    return subprocess.run(cmd, cwd=ROOT, text=True, encoding='utf-8', **kw)


def packages():
    out = run(['cargo', 'metadata', '--no-deps', '--format-version', '1'],
              capture_output=True, check=True).stdout
    # `publish = false` shows up as `publish: []`.
    return sorted((p for p in json.loads(out)['packages'] if p['publish'] != []),
                  key=lambda p: p['name'])


def group(title):
    print(f'--- {title}', flush=True)


def check(package, release_type, baseline, label):
    """Run the tool on one package: 'ok', 'break', 'unpublished' or 'error'."""
    cmd = ['cargo', 'semver-checks', 'check-release', '-p', package['name'],
           *feature_args(package), '--release-type', release_type, *baseline]
    group(f'{label}: {package["name"]} ({" ".join(cmd[5:])})')
    done = run(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    print(done.stdout, end='')
    package['output'] = done.stdout
    if done.returncode == 0:
        return 'ok'
    if done.returncode == BREAK_EXIT:
        return 'break'
    if 'not found in registry' in done.stdout:
        print(f'{package["name"]} is not on crates.io yet: nothing to compare with.')
        return 'unpublished'
    return 'error'


def current_branch():
    return run(['git', 'branch', '--show-current'], capture_output=True, check=True).stdout.strip()


def base_commit(target):
    """The commit the change is compared with: the one it is based on."""
    return run(['git', 'merge-base', f'origin/{target}', 'HEAD'],
               capture_output=True, check=True).stdout.strip()


def breaking_fragment(base):
    """Whether the change adds or edits a fragment with a Breaking changes section."""
    changed = run(['git', 'diff', '--name-only', '--diff-filter=AM', base, 'HEAD', '--', 'changelog.d'],
                  capture_output=True, check=True).stdout.split()
    if not changed:
        return False
    found = run([sys.executable, 'scripts/changelog.py', '--breaking', *changed],
                capture_output=True, check=True).stdout.split()
    return bool(found)


def failures(package):
    """The lint sections of a failed run, for the report."""
    match = re.search(r'--- failure.*?(?=\n\s*Summary)', package.get('output', ''), re.S)
    return match.group(0).strip() if match else package.get('output', '').strip()[-2000:]


def summary(mode, checked, skipped, results, lines):
    out = [f'## Semver checks ({mode} mode)', '', *lines, '',
           '| Crate | Against the last release | Added by this change |', '|---|---|---|']
    for p in checked:
        name = p['name']
        out.append(f'| `{name}` | {results[name].get("registry", "-")} | {results[name].get("new", "-")} |')
    for p in skipped:
        out.append(f'| `{p["name"]}` | not checked: {skip_reason(p)} | - |')
    for p in checked:
        for key in ('new', 'registry'):
            if results[p['name']].get(key) == 'break':
                out += ['', f'### `{p["name"]}`', '```', failures(p), '```']
                break
    print('\n'.join(out))


def main_run(args):
    branch = current_branch()
    target = branch if fnmatch(branch, RELEASE_BRANCHES) else 'main'
    if '--base' in args:
        index = args.index('--base')
        if index + 1 >= len(args):
            sys.exit('--base needs a branch name')
        target = args[index + 1]
    is_labeled = '--breaking' in args
    mode = decide_mode(target, branch)
    pkgs = packages()
    checked = [p for p in pkgs if not skip_reason(p)]
    skipped = [p for p in pkgs if skip_reason(p)]
    for p in skipped:
        print(f'{p["name"]}: not checked, {skip_reason(p)}')
    results = {p['name']: {} for p in checked}

    # `minor` is what 0.1.0 -> 0.1.x is for a 0.x crate (cargo's rule, which
    # the tool follows): additions pass, breaks fail. `patch` would also fail
    # additions, which STABILITY.md allows in a patch release. A main that
    # still says 0.1.0 is compared as the same.
    for p in checked:
        results[p['name']]['registry'] = check(p, 'minor', [], 'last release')
    breaks = [n for n, r in results.items() if r['registry'] == 'break']
    errors = [n for n, r in results.items() if r['registry'] == 'error']

    base = base_commit(target)
    new_breaks = None
    if mode == 'main' and breaks and not errors:
        print(f'Breaks since the last release: {", ".join(breaks)}. Comparing with {base[:10]} '
              'to see which this change adds.')
        for p in checked:
            if p['name'] in breaks:
                results[p['name']]['new'] = check(p, 'minor', ['--baseline-rev', base], 'before this change')
        errors += [n for n, r in results.items() if r.get('new') == 'error']
        new_breaks = [n for n, r in results.items() if r.get('new') == 'break']
    has_fragment = breaking_fragment(base)

    ok, lines = verdict(mode, breaks, new_breaks, errors, is_labeled, has_fragment)
    label_ok, label_lines = label_verdict(mode, is_labeled, has_fragment)
    ok, lines = ok and label_ok, lines + label_lines
    print()
    summary(mode, checked, skipped, results, lines)
    sys.exit(0 if ok else 1)


def selftest():
    assert decide_mode('release-0.1', 'merge') == 'release'
    assert decide_mode(None, 'release-0.2') == 'release'
    assert decide_mode('main', 'merge') == 'main'
    assert decide_mode(None, 'feature/release-notes') == 'main'

    assert feature_args({}) == ['--default-features']
    assert feature_args({'metadata': {'docs': {'rs': {'all-features': True}}}}) == ['--all-features']
    docs = {'metadata': {'docs': {'rs': {'features': ['a', 'b']}}}}
    assert feature_args(docs) == ['--default-features', '--features', 'a,b']
    bare = {'metadata': {'docs': {'rs': {'no-default-features': True}}}}
    assert feature_args(bare) == ['--only-explicit-features']

    assert skip_reason({'targets': [{'kind': ['lib']}]}) is None
    assert skip_reason({'targets': [{'kind': ['rlib', 'staticlib', 'cdylib']}]}) is None
    assert 'proc-macro' in skip_reason({'targets': [{'kind': ['proc-macro']}]})
    assert 'binary' in skip_reason({'targets': [{'kind': ['bin']}]})

    ok = lambda *a: verdict(*a)[0]
    # No break: always fine. A tool error is never excused.
    assert ok('main', [], None, [], False, False)
    assert ok('release', [], None, [], False, False)
    assert not ok('main', [], None, ['ponko'], True, True)
    # A release branch fails on any break, label and fragment or not.
    assert not ok('release', ['ponko'], None, [], True, True)
    # Main: breaks that earlier changes made are not this change's problem.
    assert ok('main', ['ponko'], [], [], False, False)
    # This change's break needs the label AND the fragment.
    assert not ok('main', ['ponko'], ['ponko'], [], False, False)
    assert not ok('main', ['ponko'], ['ponko'], [], True, False)
    assert not ok('main', ['ponko'], ['ponko'], [], False, True)
    assert ok('main', ['ponko'], ['ponko'], [], True, True)
    assert 'label' in ' '.join(verdict('main', ['ponko'], ['ponko'], [], False, True)[1])
    assert 'fragment' in ' '.join(verdict('main', ['ponko'], ['ponko'], [], True, False)[1])
    # The label without a migration note fails, with or without a break found.
    assert not label_verdict('main', True, False)[0]
    assert 'migration note' in ' '.join(label_verdict('main', True, False)[1])
    assert label_verdict('main', True, True) == (True, [])
    assert label_verdict('main', False, False) == (True, [])
    assert label_verdict('main', False, True)[0] and 'label' in ' '.join(label_verdict('main', False, True)[1])
    assert not label_verdict('release', True, True)[0], 'no break goes into a patch release'
    assert label_verdict('release', False, False) == (True, [])
    print('semver-checks.py: self-test passed')


if __name__ == '__main__':
    command = sys.argv[1] if len(sys.argv) > 1 else ''
    if command == 'run':
        main_run(sys.argv[2:])
    elif command == 'selftest':
        selftest()
    else:
        sys.exit(__doc__)
