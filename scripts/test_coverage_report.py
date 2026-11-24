#!/usr/bin/env python3
"""Regression tests for the coverage report's source and function matching."""

import importlib
import unittest
from pathlib import Path


report = importlib.import_module("coverage-report")
ROOT = Path(__file__).resolve().parent.parent


def file(path, covered, count):
    return {"filename": str(ROOT / path), "summary": {"lines": {"covered": covered, "count": count}}}


class ModuleCoverageTests(unittest.TestCase):
    def test_module_counts_are_weighted_by_lines_and_keep_the_crate_prefix(self):
        data = {
            "files": [
                file("src/render/mod.rs", 9, 10),
                file("src/render/pipeline.rs", 20, 90),
                file("crates/ponko_ecs/src/world.rs", 4, 10),
                file("tests/render_smoke.rs", 10, 10),
                file("src/render/fog_render_test.rs", 4, 5),
                file("/mock/registry/sibling/src/lib.rs", 10, 10),
            ],
        }
        self.assertEqual(
            report.module_lines(data, ROOT),
            {"ponko::render": (29, 100), "ponko_ecs::world": (4, 10)},
        )


class FunctionCoverageTests(unittest.TestCase):
    def test_one_executed_instantiation_marks_the_public_body_covered(self):
        filename = "src/sound.rs"
        data = {"functions": [
            {"filenames": [filename], "regions": [[10, 5, 12, 6, 0, 0, 0, 0]]},
            {"filenames": [filename], "regions": [[10, 5, 12, 6, 2, 0, 0, 0]]},
            {"filenames": [filename], "regions": [[14, 8, 14, 34, 0, 0, 0, 0]]},
        ]}
        functions = report.function_regions(data, ROOT)
        self.assertEqual(report.function_status(functions, filename, (10, 1), (12, 6)), "covered")
        self.assertEqual(report.function_status(functions, filename, (14, 1), (14, 35)), "unexecuted")
        self.assertEqual(report.function_status(functions, filename, (16, 1), (18, 6)), "unmapped")

    def test_public_reexport_and_inherent_method_use_the_public_type_path(self):
        span = {"filename": "src/model.rs", "begin": [1, 1], "end": [3, 2]}
        doc = {"root": 0, "index": {
            "0": {"name": "example", "inner": {"module": {"items": [1, 2]}}},
            "1": {"name": None, "inner": {"use": {"is_glob": False, "id": 3, "name": "Model"}}},
            "2": {"name": "create", "span": span, "inner": {"function": {"has_body": True}}},
            "3": {"name": "InternalModel", "inner": {"struct": {"impls": [4]}}},
            "4": {"inner": {"impl": {"trait": None, "items": [5, 6]}}},
            "5": {"name": "new", "visibility": "public", "span": span, "inner": {"function": {}}},
            "6": {"name": "reset", "visibility": "crate", "span": span, "inner": {"function": {}}},
        }}
        self.assertEqual(dict(report.public_functions(doc)), {
            "example::Model::new": span,
            "example::create": span,
        })


if __name__ == "__main__":
    unittest.main()
