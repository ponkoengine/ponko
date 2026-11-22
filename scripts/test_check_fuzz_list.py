#!/usr/bin/env python3
"""Tests for check-fuzz-list.py.

    python3 scripts/test_check_fuzz_list.py
"""
from pathlib import Path
import importlib.util
import tempfile
import unittest

PATH = Path(__file__).resolve().parent / 'check-fuzz-list.py'
SPEC = importlib.util.spec_from_file_location('check_fuzz_list', PATH)
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


def fuzz_folder(root, files, bins, readme_rows, listed):
    """A fuzz folder with these targets in each place."""
    fuzz = Path(root)
    (fuzz / 'fuzz_targets').mkdir()
    for name in files:
        (fuzz / 'fuzz_targets' / f'{name}.rs').write_text('')
    (fuzz / 'Cargo.toml').write_text(
        ''.join(f'[[bin]]\nname = "{name}"\npath = "fuzz_targets/{name}.rs"\n' for name in bins)
    )
    (fuzz / 'README.md').write_text(
        '| Target | What it reads |\n|---|---|\n' + ''.join(f'| `{name}` | something |\n' for name in readme_rows)
    )
    (fuzz / 'targets.txt').write_text('# the list\n' + ''.join(f'{line}\n' for line in listed))
    return fuzz


class FuzzList(unittest.TestCase):
    def problems(self, **places):
        with tempfile.TemporaryDirectory() as root:
            defaults = dict(files=['a', 'b'], bins=['a', 'b'], readme_rows=['a', 'b'], listed=['a 60', 'b 60'])
            return CHECK.check(fuzz_folder(root, **{**defaults, **places}))

    def test_a_list_that_names_every_target_passes(self):
        self.assertEqual(self.problems(), [])

    def test_a_target_file_the_list_does_not_name_fails(self):
        problems = self.problems(files=['a', 'b', 'c'])
        self.assertEqual(len(problems), 1)
        self.assertIn('fuzz target c has a file in fuzz/fuzz_targets/ but no line', problems[0])

    def test_a_target_that_is_not_built_or_not_in_the_readme_fails(self):
        self.assertTrue(any('b is in fuzz/targets.txt but has no [[bin]]' in p for p in self.problems(bins=['a'])))
        self.assertTrue(any('b is in fuzz/targets.txt but has no row in the table' in p for p in self.problems(readme_rows=['a'])))

    def test_a_target_in_the_list_without_a_file_fails(self):
        problems = self.problems(listed=['a 60', 'b 60', 'gone 60'])
        self.assertTrue(any('gone is in fuzz/targets.txt but has no file' in p for p in problems))

    def test_malformed_repeated_and_zero_second_lines_fail(self):
        self.assertTrue(any('expected "<target> <seconds>"' in p for p in self.problems(listed=['a 60', 'b'])))
        self.assertTrue(any('b is listed twice' in p for p in self.problems(listed=['a 60', 'b 60', 'b 90'])))
        self.assertTrue(any('runs for 0 seconds' in p for p in self.problems(listed=['a 60', 'b 0'])))

    def test_the_trees_own_list_passes(self):
        self.assertEqual(CHECK.check(CHECK.FUZZ), [])


if __name__ == '__main__':
    unittest.main()
