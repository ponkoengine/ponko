"""Exercise nightly publication and issue reconciliation without GitHub."""

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

from nightly_report import public_status, publish_status


def load_script(filename):
    spec = importlib.util.spec_from_file_location(filename, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


issue = load_script("nightly-issue.py")

FAKE_GH = """
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
state = json.loads(path.read_text())
args = sys.argv[2:]
state['calls'].append([args, sys.stdin.read() if '--body-file' in args else None])
action = args[1]
if action == 'list':
    print(json.dumps(state['open']))
elif action == 'create':
    state['open'].append({'number': 12, 'title': args[args.index('--title') + 1]})
elif action == 'close':
    state['open'] = [item for item in state['open'] if item['number'] != int(args[2])]
elif action != 'edit':
    raise AssertionError(args)
path.write_text(json.dumps(state))
"""


def report(code, system="macOS"):
    return {
        "schema_version": 1, "os": system, "revision": "a" * 40,
        "finished_at": "2026-11-25T01:30:00+00:00", "exit_code": code,
        "jobs": [{
            "name": "Integration | smoke", "job": "smoke",
            "steps": [
                {"name": "Check HTML <details>", "returncode": 7 if code == 1 else (None if code == 2 else 0),
                 "failure_output": "local secret", "command": "secret command"},
                {"name": "Skipped", "returncode": None},
            ],
        }],
    }


class NightlyChecks(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.path = Path(temp.name)
        script = self.path / "fake_gh.py"
        script.write_text(FAKE_GH, encoding="utf-8")
        self.state = self.path / "gh.json"
        self.write_state([{"number": 2, "title": "Nightly checks failing on macOS extras"}])
        self.gh = [sys.executable, str(script), str(self.state)]

    def write_state(self, opened):
        self.state.write_text(json.dumps({"open": opened, "calls": []}), encoding="utf-8")

    def read_state(self):
        return json.loads(self.state.read_text(encoding="utf-8"))

    def test_public_files_are_per_os_and_replace_the_previous_result(self):
        for system, slug in (("macOS", "macos"), ("Windows", "windows"), ("Linux", "linux")):
            publish_status(report(1, system), self.path)
            data = (self.path / f"{slug}.json").read_text(encoding="utf-8")
            markdown = (self.path / f"{slug}.md").read_text(encoding="utf-8")
            status = json.loads(data)
            self.assertEqual(status["counts"], {"passed": 0, "failed": 1, "skipped": 1})
            self.assertEqual(status["failed_steps"], ["Integration | smoke / Check HTML <details>"])
            self.assertIn("2026-11-25T01:30:00", markdown)
            self.assertIn("Check HTML &lt;details&gt;", markdown)
            self.assertNotIn("local secret", data + markdown)
            self.assertNotIn("secret command", data + markdown)
        publish_status(report(0, "Windows"), self.path)
        self.assertEqual(json.loads((self.path / "windows.json").read_text())["failed_steps"], [])
        self.assertEqual(json.loads((self.path / "linux.json").read_text())["counts"]["failed"], 1)

    def test_first_failure_creates_repeated_failure_edits_and_pass_closes_one_issue(self):
        statuses = [1, 1, 0]
        for code in statuses:
            issue.sync_issue(public_status(report(code)), apply=True, gh=self.gh)
        state = self.read_state()
        self.assertEqual([args[1] for args, _ in state["calls"]],
                         ["list", "create", "list", "edit", "list", "close"])
        self.assertEqual(state["calls"][3][0][2], "12")
        self.assertEqual(state["calls"][5][0][2], "12")
        self.assertIn("Check HTML", state["calls"][3][1])
        self.assertNotIn("local secret", state["calls"][1][1])
        self.assertEqual(state["open"], [{"number": 2, "title": "Nightly checks failing on macOS extras"}])

    def test_dry_run_only_reads_and_incomplete_run_never_closes(self):
        opened = [{"number": 12, "title": "Nightly checks failing on macOS"}]
        self.write_state(opened)
        self.assertIn("close #12", issue.sync_issue(public_status(report(0)), gh=self.gh))
        self.assertIn("incomplete", issue.sync_issue(public_status(report(2)), apply=True, gh=self.gh))
        state = self.read_state()
        self.assertEqual([args[1] for args, _ in state["calls"]], ["list"])
        self.assertEqual(state["open"], opened)

    def test_duplicate_exact_matches_are_left_for_maintainer_review(self):
        self.write_state([{"number": number, "title": "Nightly checks failing on macOS"} for number in (2, 12)])
        with self.assertRaisesRegex(ValueError, "multiple open issues"):
            issue.sync_issue(public_status(report(1)), apply=True, gh=self.gh)
        self.assertEqual([args[1] for args, _ in self.read_state()["calls"]], ["list"])

    def test_invalid_reports_cannot_drive_issue_updates(self):
        for override in ({"os": "../macos"}, {"exit_code": 0}, {"revision": "unknown"}):
            with self.subTest(override=override), self.assertRaises(ValueError):
                public_status({**report(1), **override})


if __name__ == "__main__":
    unittest.main()
