"""Tests of the Blender add-on's logic, without Blender:

    python3 -m unittest discover -s tools/blender

`fixtures/ponko-schema.json` is what `ponko schema` writes for a game that
reflects `Health { hp: f32 }` (described) and `Barrel { full: bool }`.
"""

import json
import pathlib
import sys
import unittest

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from ponko_properties import object_problems  # noqa: E402  (imports without Blender)
from ponko_properties.schema import Schema, check, example, suggestions  # noqa: E402

SCHEMA = HERE / "fixtures" / "ponko-schema.json"


class FakeObject(dict):
    """A Blender object as far as the add-on reads it: custom properties."""


class SchemaFile(unittest.TestCase):
    def setUp(self):
        self.schema = Schema.load(SCHEMA)

    def test_the_menu_offers_the_convention_and_the_reflected_components(self):
        keys = self.schema.keys()
        for key in ("ponko:collider", "ponko:body", "ponko:prefab", "ponko:lod", "ponko:Health", "ponko:Barrel"):
            self.assertIn(key, keys)
        self.assertLess(keys.index("ponko:prefab"), keys.index("ponko:Health"), "convention first")
        collider = self.schema.properties["ponko:collider"]
        self.assertEqual(collider.kind, "choice")
        self.assertEqual(collider.choices, ["box", "sphere", "capsule", "hull", "trimesh"])
        self.assertTrue(collider.description)

    def test_new_properties_start_with_values_that_pass(self):
        for key, prop in self.schema.properties.items():
            self.assertIsNone(self.schema.check_property(key, prop.default()), key)

    def test_a_component_starts_as_json_text_with_its_fields(self):
        health = self.schema.properties["ponko:Health"]
        self.assertEqual(health.kind, "component")
        self.assertEqual(json.loads(health.default()), {"hp": 0})

    def test_misspelt_names_suggest_the_right_one(self):
        self.assertIn('did you mean "ponko:collider"', self.schema.check_property("ponko:colider", "box"))
        self.assertIn('did you mean "ponko:Health"', self.schema.check_property("ponko:Helth", "{}"))
        self.assertIn("reflect it", self.schema.check_property("ponko:Velocity", "{}"))

    def test_wrong_values_say_what_the_property_takes(self):
        error = self.schema.check_property("ponko:collider", "cube")
        self.assertIn("it takes one of box, sphere, capsule, hull, trimesh", error)
        self.assertIn("ponko:body", self.schema.check_property("ponko:body", "flying"))
        self.assertIsNotNone(self.schema.check_property("ponko:layers", 40), "layers are 0-31")
        self.assertIsNotNone(self.schema.check_property("ponko:lod", [0.5, 2.0]), "screen heights are 0-1")
        self.assertIsNotNone(self.schema.check_property("ponko:prefab", ""))

    def test_values_blender_writes_pass(self):
        ok = {
            "ponko:collider": "box",
            "ponko:sensor": True,
            "ponko:hidden": 1,  # older Blender versions store booleans as 0 and 1
            "ponko:layers": "[1, 3]",  # JSON in a String property
            "ponko:collides_with": [0, 1],
            "ponko:spawn": "player",
            "ponko:lod": (0.3, 0.08, 0.02),  # a Float Array
            "ponko:Health": '{"hp": 5}',
            "ponko:Barrel": {"full": True},
            "ponko": {"collider": "sphere"},
            "door": "front",  # the game's own: not checked
        }
        self.assertEqual(object_problems(self.schema, FakeObject(ok)), [])

    def test_component_values_are_checked_against_their_fields(self):
        error = self.schema.check_property("ponko:Health", '{"hp": "lots"}')
        self.assertIsNotNone(error)
        self.assertIn('"hp"', error)
        self.assertIsNotNone(self.schema.check_property("ponko:Health", '{"hp": 1, "armor": 2}'))

    def test_the_ponko_group_is_checked_like_prefixed_keys(self):
        error = self.schema.check_property("ponko", '{"colider": "box"}')
        self.assertIn('did you mean "ponko:collider"', error)

    def test_every_problem_of_an_object_is_listed_in_key_order(self):
        obj = FakeObject({"ponko:collider": "cube", "ponko:body": "flying", "ponko:sensor": True})
        problems = object_problems(self.schema, obj)
        self.assertEqual(len(problems), 2)
        self.assertTrue(problems[0].startswith('"ponko:body"'))


class Logic(unittest.TestCase):
    def test_suggestions_find_typos_and_case_slips_but_not_strangers(self):
        known = ["collider", "collides_with", "Health", "prefab"]
        self.assertEqual(suggestions("colider", known), ["collider"])
        self.assertEqual(suggestions("health", known), ["Health"])
        self.assertEqual(suggestions("perfab", known), ["prefab"])
        self.assertEqual(suggestions("velocity", known), [])

    def test_the_schema_subset_checks_types_ranges_and_lists(self):
        vec3 = {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3}
        self.assertIsNone(check(vec3, [1, 2.5, 3]))
        self.assertIsNotNone(check(vec3, [1, 2]))
        self.assertIsNotNone(check({"type": "integer", "minimum": 0}, -1))
        self.assertIsNotNone(check({"type": "integer"}, True), "a bool is no integer")
        self.assertIsNone(check({"type": "integer"}, 2.0))
        self.assertIsNone(check({"anyOf": [{"type": "string"}, {"const": True}]}, True))
        self.assertIsNone(check({"not": {"pattern": "^ponko:"}}, "door"))
        self.assertIsNotNone(check({"not": {"pattern": "^ponko:"}}, "ponko:door"))

    def test_examples_fit_their_schemas(self):
        schema = {
            "type": "object",
            "properties": {"pos": {"type": "array", "prefixItems": [{"type": "number"}] * 2, "minItems": 2}},
            "required": ["pos"],
        }
        self.assertEqual(example(schema), {"pos": [0, 0]})
        self.assertIsNone(check(schema, example(schema)))

    def test_a_file_that_is_not_a_schema_is_refused(self):
        with self.assertRaises(ValueError):
            Schema({"type": "object"})


if __name__ == "__main__":
    unittest.main()
