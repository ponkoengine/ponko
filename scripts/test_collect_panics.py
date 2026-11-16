#!/usr/bin/env python3
"""Regression tests for the panic audit scanner (no Rust build required)."""
import contextlib
import importlib.util
import io
from pathlib import Path
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("collect_panics", Path(__file__).with_name("collect-panics.py"))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class CollectorTests(unittest.TestCase):
    def test_literals_comments_and_raw_strings_do_not_create_sites(self):
        text = """// panic!("comment");
const S: &str = r#"unwrap() " panic!("raw")"#;
let c = '"';
/* assert!(false); */
fn real() { panic!("fix it"); }
"""
        sites = list(audit.SITE.finditer(audit.code_only(text)))
        self.assertEqual(len(sites), 1)
        self.assertEqual(sites[0].group(), "panic!")

    def test_test_module_removal_preserves_production_after_it(self):
        text = '#[cfg(test)] mod tests { fn test() { assert!(false); } }\nfn real() { panic!("fix"); }'
        self.assertEqual(len(list(audit.SITE.finditer(audit.code_only(audit.production(text))))), 1)

    def test_test_field_does_not_hide_production_methods(self):
        text = 'struct S { #[cfg(test)]\n field: usize,\n}\nimpl S { fn f() { assert!(false); } }'
        self.assertEqual(len(list(audit.SITE.finditer(audit.code_only(audit.production(text))))), 1)

    def test_semicolon_in_message_does_not_truncate_construction(self):
        text = 'panic!("bad; fix it")'
        code = audit.code_only(text)
        self.assertEqual(audit.closing(code, code.index("(")), len(text))


class CheckTests(unittest.TestCase):
    def run_main(self, root, *args):
        old_root, old_argv = audit.ROOT, sys.argv
        audit.ROOT, sys.argv = root, ["collect-panics.py", *args]
        try:
            with contextlib.redirect_stdout(io.StringIO()) as out:
                audit.main()
            return out.getvalue()
        finally:
            audit.ROOT, sys.argv = old_root, old_argv

    def test_check_fails_on_a_new_site_until_it_is_recorded(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scripts").mkdir()
            (root / "crates/ponko_ecs/src").mkdir(parents=True)
            (root / "src").mkdir()
            (root / "src/lib.rs").write_text('pub fn f(x: Option<u8>) -> u8 {\n    x.expect("set above")\n}\n')
            with self.assertRaises(SystemExit) as failed:
                self.run_main(root, "--check")
            self.assertIn("1 panic sites need review", str(failed.exception))
            self.run_main(root, "--record", "src/lib.rs:2", "invariant", "Internal invariant: set above.")
            self.assertIn("src/lib.rs:2\tinvariant\tInternal invariant", self.run_main(root, "--check"))

    def test_record_rejects_unknown_classifications(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "crates/ponko_ecs/src").mkdir(parents=True)
            (root / "scripts").mkdir()
            (root / "src/lib.rs").write_text("fn f() { panic!(); }\n")
            with self.assertRaises(SystemExit):
                self.run_main(root, "--record", "src/lib.rs:1", "fine", "looks fine")


if __name__ == "__main__":
    unittest.main()
