"""Tests for evaluating the soak samples without a game or GPU."""

import argparse
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("soak", Path(__file__).with_name("soak.py"))
soak = importlib.util.module_from_spec(spec)
spec.loader.exec_module(soak)


class SoakTests(unittest.TestCase):
    def test_only_post_warmup_growth_counts(self):
        samples = [
            {"seconds": t, "rss_bytes": rss}
            for t, rss in [(0, 0), (10, 100), (20, 100), (40, 100)]
        ]
        result = soak.trend(samples, "rss_bytes", 10, 1)
        self.assertEqual(result, {"status": "pass", "samples": 3, "slope_per_hour": 0, "bound_per_hour": 1})

    def test_positive_slope_uses_elapsed_time_and_exceeds_bound(self):
        samples = [{"seconds": t, "rss_bytes": 2 * t} for t in [0, 10, 30]]
        self.assertEqual(soak.trend(samples, "rss_bytes", 0, 7199)["status"], "fail")
        self.assertAlmostEqual(soak.trend(samples, "rss_bytes", 0, 7200)["slope_per_hour"], 7200)

    def test_unreported_and_intermittently_missing_metrics_are_distinct(self):
        samples = [{"seconds": t, "wgpu_bytes": None} for t in [0, 10, 20]]
        self.assertEqual(soak.trend(samples, "wgpu_bytes", 0, 1)["status"], "unavailable")
        samples[0]["wgpu_bytes"] = 0
        self.assertEqual(soak.trend(samples, "wgpu_bytes", 0, 1)["status"], "insufficient")

    def test_samples_must_cover_whole_resize_cycles(self):
        self.assertIsNotNone(soak.option_error(600, 10, 120))
        self.assertIsNone(soak.option_error(600, 20, 120))
        self.assertIsNone(soak.option_error(14400, 60, 600))
        self.assertIsNotNone(soak.option_error(60, 40, 0))

    def test_frame_time_may_change_once_by_half_its_median(self):
        def frame_result(after):
            samples = [{"seconds": t, "frame_ms_p50": 10.0 if t < 300 else after} for t in range(0, 601, 20)]
            args = argparse.Namespace(warmup=120, seconds=600, sample=20, revision="test", headless=True, windowed=False)
            with tempfile.TemporaryDirectory() as out, contextlib.redirect_stdout(io.StringIO()):
                soak.evaluate(Path(out), "coin_dash", samples, args, 0)
                return json.loads((Path(out) / "report.json").read_text())["metrics"]["frame_ms_p50"]
        self.assertEqual(frame_result(13.0)["status"], "pass")
        self.assertEqual(frame_result(30.0)["status"], "fail")


if __name__ == "__main__":
    unittest.main()
