"""Self-tests for the local check runner, using only synthetic jobs."""

import argparse
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "run_checks", Path(__file__).with_name("run-checks.py")
)
runner = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = runner
spec.loader.exec_module(runner)

INVENTORY = """
env:
  CHECK_VALUE: "example"
jobs:
  portable:
    runs-on: ${{ matrix.os }}
    local-tags: [quick, gpu]
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
        tier: [baseline, desktop]
    steps:
      - name: Continue the local job
        if: ${{ !cancelled() }}
        run: >-
          python3 -c "import os; print(os.environ['CHECK_VALUE'])"
          # command comment
      - name: Write a value for the next step
        run: |
          echo "CHECK_VALUE=${{ matrix.tier }}" >> "$GITHUB_ENV"
      - name: A known failure
        run: |
          python3 -c "import os, sys; print(os.environ['CHECK_VALUE']); sys.exit(7)"
          echo "should not be run"
      - name: Safe steps continue after failures
        if: ${{ !cancelled() }}
        run: python -c "print('render test ran [example] on Fake GPU (Metal, Other, driver 1.0)')"
      - name: Windows-only and windowed
        if: runner.os == 'Windows'
        local-tags: [windowed]
        run: exit 99
      - name: Hosted setup
        local-skip: Install locally first
        run: exit 99
  lint:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        include:
          - name: first
            args: --features example
          - name: second
            scope: -p example
    steps:
      - run: cargo clippy ${{ matrix.scope || '--workspace' }} ${{ matrix.args }}
  tool:
    runs-on: macos-latest
    local-requires: [ponko-deliberately-missing-tool]
    steps:
      - run: exit 99
"""


def options(**changes):
    return argparse.Namespace(**dict(
        {"job": [], "tag": [], "quick": False, "include_windowed": False},
        **changes,
    ))


class CheckRunnerTests(unittest.TestCase):
    def setUp(self):
        self.inventory = runner.read_inventory(INVENTORY)

    def plan(self, system, **changes):
        return runner.plan_checks(self.inventory, system, options(**changes))

    def test_each_platform_selects_its_matrix_lane(self):
        for system in ("Linux", "macOS", "Windows"):
            with self.subTest(system=system):
                plan = self.plan(system, quick=True)
                matches = [row for row in plan if row["job"] == "portable" and not row["reason"]]
                self.assertEqual(len(matches), 2)
                self.assertEqual({row["matrix"]["tier"] for row in matches}, {"baseline", "desktop"})
                self.assertTrue(all(row["reason"] for row in plan if row["job"] != "portable"))

    def test_include_rows_and_matrix_defaults_are_expanded(self):
        matches = [row for row in self.plan("Linux", job=["lint"]) if not row["reason"]]
        self.assertEqual(
            [row["steps"][0]["command"].rstrip() for row in matches],
            ["cargo clippy --workspace --features example", "cargo clippy -p example"],
        )

    def test_skip_reasons_cover_os_filters_tools_and_windows(self):
        plan = self.plan("macOS", job=["tool"])
        jobs = {row["job"]: row["reason"] for row in reversed(plan)}
        self.assertTrue(any(row["job"] == "portable" and "filter" in row["reason"] for row in plan))
        self.assertIn("macOS", jobs["lint"])
        self.assertIn("ponko-deliberately-missing-tool", jobs["tool"])
        windows = next(row for row in self.plan("Windows", tag=["gpu"]) if not row["reason"])
        self.assertIn("--include-windowed", windows["steps"][4]["reason"])
        explicit = next(row for row in self.plan("Windows", include_windowed=True) if not row["reason"])
        self.assertIsNone(explicit["steps"][4]["reason"])
        self.assertEqual(explicit["steps"][5]["reason"], "Install locally first")

    def test_hosted_expression_is_reported_without_execution(self):
        inventory = runner.read_inventory(INVENTORY.replace(
            "local-skip: Install locally first", "if: steps.setup.outputs.ready == 'true'"
        ))
        row = next(row for row in runner.plan_checks(inventory, "Linux", options()) if not row["reason"])
        self.assertIn("hosted condition", row["steps"][5]["reason"])

    @unittest.skipUnless(shutil.which("bash"), "requires Bash for synthetic subprocesses")
    def test_report_preserves_failure_and_records_real_subprocess_results(self):
        with tempfile.TemporaryDirectory() as tmp:
            plan = self.plan("Linux", job=["portable"])
            report = runner.run_checks(plan, Path(tmp), "Linux")
            runner.write_report(report, Path(tmp))
            report = json.loads((Path(tmp) / "results.json").read_text(encoding="utf-8"))
            self.assertEqual(report["exit_code"], 1)
            row = next(row for row in report["jobs"] if not row["reason"])
            steps = row["steps"]
            self.assertEqual([s["returncode"] for s in steps], [0, 0, 7, 0, None, None])
            self.assertEqual(steps[2]["failure_output"].strip(), "baseline")
            self.assertGreater(steps[2]["duration_seconds"], 0)
            self.assertIn("Fake GPU (Metal, Other, driver 1.0)", report["gpu_adapters"])
            summary = (Path(tmp) / "summary.txt").read_text(encoding="utf-8")
            self.assertIn("rc=7", summary)
            self.assertIn("SKIP", summary)

    def test_all_skips_are_not_a_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            report = runner.run_checks(self.plan("macOS", job=["tool"]), Path(tmp), "macOS")
        self.assertEqual(report["exit_code"], 2)


if __name__ == "__main__":
    unittest.main()
