"""Tests for evaluating the soak samples without a game or GPU."""

import importlib.util
from pathlib import Path
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


if __name__ == "__main__":
    unittest.main()
