#!/usr/bin/env python3
"""Every fuzz target is in the written fuzz list.

The list is fuzz/targets.txt: one target per line with the seconds the
weekly job runs it. A target is a file in fuzz/fuzz_targets/; it also needs
its [[bin]] in fuzz/Cargo.toml (or `cargo fuzz` can't build it) and its row
in the table of fuzz/README.md. This fails, naming the missing place, when
a target exists in one of them and not in the others, when a line of the
list is malformed or repeated, or when a target runs for no time.

    python3 scripts/check-fuzz-list.py
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FUZZ = ROOT / 'fuzz'


def listed(text):
    """The list's targets and seconds, and what is wrong with its lines."""
    targets, problems = {}, []
    for number, line in enumerate(text.splitlines(), 1):
        line = line.split('#', 1)[0].strip()
        if not line:
            continue
        parts = line.split()
        if len(parts) != 2 or not re.fullmatch(r'[a-z][a-z0-9_]*', parts[0]) or not parts[1].isdigit():
            problems.append(f'fuzz/targets.txt:{number}: expected "<target> <seconds>", got "{line}"')
            continue
        name, seconds = parts[0], int(parts[1])
        if name in targets:
            problems.append(f'fuzz/targets.txt:{number}: {name} is listed twice')
        elif seconds == 0:
            problems.append(f'fuzz/targets.txt:{number}: {name} runs for 0 seconds')
        targets[name] = seconds
    return targets, problems


def check(fuzz):
    """The problems of the fuzz folder `fuzz`, as lines."""
    list_file = fuzz / 'targets.txt'
    if not list_file.is_file():
        return ['fuzz/targets.txt is missing']
    in_list, problems = listed(list_file.read_text(encoding='utf-8'))
    places = {
        'file in fuzz/fuzz_targets/': {p.stem for p in (fuzz / 'fuzz_targets').glob('*.rs')},
        '[[bin]] in fuzz/Cargo.toml': set(
            re.findall(r'\[\[bin\]\]\s*name\s*=\s*"([^"]+)"', (fuzz / 'Cargo.toml').read_text(encoding='utf-8'))
        ),
        'row in the table of fuzz/README.md': set(
            re.findall(r'^\|\s*`([a-z0-9_]+)`\s*\|', (fuzz / 'README.md').read_text(encoding='utf-8'), re.M)
        ),
    }
    for place, names in places.items():
        for name in sorted(names - set(in_list)):
            problems.append(f'fuzz target {name} has a {place} but no line in fuzz/targets.txt')
        for name in sorted(set(in_list) - names):
            problems.append(f'fuzz target {name} is in fuzz/targets.txt but has no {place}')
    return problems


def main():
    problems = check(FUZZ)
    for problem in problems:
        print(problem)
    if problems:
        print('fix the fuzz list: fuzz/targets.txt, fuzz/README.md "Fuzz targets"')
        return 1
    print('check-fuzz-list: every fuzz target is in the list')
    return 0


if __name__ == '__main__':
    sys.exit(main())
