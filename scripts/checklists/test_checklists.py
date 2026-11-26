#!/usr/bin/env python3
"""Tests for record_run.py and lowend_bench.py.

    python3 scripts/checklists/test_checklists.py
"""
from pathlib import Path
import importlib.util
import json
import tempfile
import unittest

HERE = Path(__file__).resolve().parent


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RECORD = load("record_run")
BENCH = load("lowend_bench")
PERF = load("perf_get")

LOG = """\
render test ran [render::a::one] on Mesa Intel(R) UHD Graphics (Vulkan, IntegratedGpu, driver 25.0)
render test ran [render::a::two] on Mesa Intel(R) UHD Graphics (Vulkan, IntegratedGpu, driver 25.0)
render test SKIPPED [render::a::three]: needs ScreenSpaceReflections, which this adapter lacks
render test SKIPPED [render::a::four]: needs ScreenSpaceReflections, which this adapter lacks
test result: ok. 3 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out; finished in 4.2s
"""


class RenderCounts(unittest.TestCase):
    def test_ran_skipped_and_cargo_totals_are_counted(self):
        counts = RECORD.count_render(LOG)
        self.assertEqual((counts["ran"], counts["skipped"]), (2, 2))
        self.assertEqual((counts["passed"], counts["failed"], counts["ignored"]), (3, 1, 2))
        self.assertEqual(list(counts["reasons"].values()), [2])
        self.assertIn("2 ran on a GPU and 2 skipped", RECORD.render_record("vulkan.log", counts))

    def test_a_test_printed_twice_counts_once(self):
        self.assertEqual(RECORD.count_render(LOG + LOG)["ran"], 2)


class ChecksRecord(unittest.TestCase):
    def report(self, codes):
        steps = [{"name": f"step {i}", "returncode": code, "gpu_skips": ["no mesh shaders"] if i == 0 else []}
                 for i, code in enumerate(codes)]
        return {
            "finished_at": "2026-11-26T10:00:00+00:00", "revision": "a" * 40, "dirty": False,
            "os": "Linux", "os_version": "6.8", "architecture": "x86_64", "toolchains": {"stable": "rustc 1.99\n"},
            "gpu_adapters": ["Mesa (Vulkan)"], "exit_code": 1 if any(c for c in codes if c) else 0,
            "jobs": [{"name": "test", "steps": steps}],
        }

    def test_failures_and_skips_are_listed(self):
        text = RECORD.checks_record(self.report([1, 0, None]), "Ryzen 5, 16 GB")
        self.assertIn("**Result: FAIL** (1 passed, 1 failed, 1 skipped)", text)
        self.assertIn("- test / step 0", text)
        self.assertIn("- no mesh shaders", text)
        self.assertIn("Ryzen 5, 16 GB", text)

    def test_a_clean_run_says_pass(self):
        text = RECORD.checks_record(self.report([0, 0]), "")
        self.assertIn("**Result: PASS**", text)
        self.assertIn("________", text)


class BenchRows(unittest.TestCase):
    def test_a_row_comes_from_the_benchmark_json(self):
        data = {"frames": 600, "fps": 61.234, "frame_ms": {"avg": 16.3, "p50": 16.2, "p95": 17.456, "p99": 20.0, "max": 31.119}}
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "g.json"
            path.write_text(json.dumps(data), encoding="utf-8")
            row = BENCH.row_from_json(path, 3.14159)
        self.assertEqual((row["fps"], row["p95"], row["max"], row["load"]), (61.2, 17.46, 31.12, 3.1))
        table = BENCH.format_table([("coin_dash", row), ("legions", None)])
        self.assertIn("| coin_dash | 600 | 61.2 |", table)
        self.assertIn("legions | did not finish", table)

    def test_dry_run_prints_commands_without_running_anything(self):
        import contextlib
        import io
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer):
            self.assertEqual(BENCH.main(["--out", "x", "--game", "ember_hop", "--tier", "baseline", "--dry-run"]), 0)
        text = buffer.getvalue()
        self.assertIn("cargo build --release --example ember_hop --features physics", text)
        self.assertIn("PONKO_BENCH=600", text)
        self.assertIn("PONKO_GPU_TIER=baseline", text)


class PerfGet(unittest.TestCase):
    def test_the_newest_running_instance_is_picked(self):
        import os
        with tempfile.TemporaryDirectory() as folder:
            for name, started, pid in (("old", 1, os.getpid()), ("new", 2, os.getpid()), ("dead", 3, 2**22 + 12345)):
                (Path(folder) / f"{name}.json").write_text(
                    json.dumps({"name": name, "started": started, "pid": pid, "port": 1, "token": "t"}), encoding="utf-8")
            (Path(folder) / "broken.json").write_text("{", encoding="utf-8")
            self.assertEqual(PERF.pick(Path(folder))["name"], "new")
            self.assertIsNone(PERF.pick(Path(folder), pid=1))

    def test_the_summary_keeps_what_the_record_needs(self):
        answer = {"fps": 59.5, "render_inspector": {"adapter": {"name": "GPU", "tier": "baseline"}, "quality": {"level": "low"}, "gpu_memory_bytes": {"total": 5}}}
        got = PERF.summary(answer)
        self.assertEqual((got["adapter"]["tier"], got["quality"]["level"], got["fps"]), ("baseline", "low", 59.5))
        self.assertIsNone(PERF.summary({})["adapter"])


if __name__ == "__main__":
    unittest.main()
