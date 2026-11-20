#!/usr/bin/env python3
"""Tests for check-ffi-coverage.py's "Still Rust-only" name matching.

    python3 scripts/test_check_ffi_coverage.py
"""
from pathlib import Path
import importlib.util
import unittest

PATH = Path(__file__).resolve().parent / 'check-ffi-coverage.py'
SPEC = importlib.util.spec_from_file_location('check_ffi_coverage', PATH)
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)

HEADER = """
enum PonkoStatus ponko_ctx_two_bone_ik(struct PonkoCtx *ctx, uint64_t entity_bits);
enum PonkoStatus ponko_ctx_look_at_joint(struct PonkoCtx *ctx, uint64_t entity_bits);
enum PonkoStatus ponko_ctx_camera3d_look_at(struct PonkoCtx *ctx, struct PonkoVec3 target);
enum PonkoStatus ponko_ctx_skeleton2d_play(struct PonkoCtx *ctx, uint64_t entity_bits);
enum PonkoStatus ponko_ctx_load_lights(struct PonkoCtx *ctx, const char *path);
enum PonkoStatus ponko_ctx_storage_write_json(struct PonkoCtx *ctx, const char *slot);
"""


class CFunctionFor(unittest.TestCase):
    def test_another_types_function_of_the_same_name_is_not_a_binding(self):
        # The 3D skeleton's IK and the 3D camera's look-at are not the 2D
        # skeleton's, whose functions are named ponko_ctx_skeleton2d_*.
        self.assertIsNone(CHECK.c_function_for('Skeleton::two_bone_ik', HEADER))
        self.assertIsNone(CHECK.c_function_for('Skeleton::look_at', HEADER))

    def test_the_types_own_function_is_found(self):
        header = HEADER + 'enum PonkoStatus ponko_ctx_skeleton2d_look_at(struct PonkoCtx *ctx);\n'
        self.assertEqual(CHECK.c_function_for('Skeleton::look_at', header), 'ponko_ctx_skeleton2d_look_at')
        self.assertEqual(CHECK.c_function_for('Storage::write_json', HEADER), 'ponko_ctx_storage_write_json')

    def test_a_type_without_a_c_family_matches_any_function_of_the_name(self):
        self.assertEqual(CHECK.c_function_for('Graphics::load_lights', HEADER), 'ponko_ctx_load_lights')

    def test_one_word_names_never_match(self):
        self.assertIsNone(CHECK.c_function_for('Skeleton::play', HEADER))


if __name__ == '__main__':
    unittest.main()
