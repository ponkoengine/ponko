#!/usr/bin/env python3
"""Checks the render tests' reference images, and gathers the evidence a
reviewer needs when a change touches them.

The references live in tests/fixtures/render: a small PNG and its metadata
(<name>.json) per image, written only by a test run with
PONKO_UPDATE_REFERENCES=1 (see src/render/render_test_support.rs and the
book's "GPU regression tests").

    check-reference-images.py
        Every image has metadata and the other way round, the metadata is
        complete, its checksum is the PNG's, and the images are small.

    check-reference-images.py --changed BASE [--evidence DIR] [--summary FILE]
        Also lists the references that differ from the git revision BASE
        (added, changed or removed). Each must come with changed metadata
        and a reason. With --evidence, writes the before and after images
        and a page showing them side by side to DIR; with --summary,
        appends a Markdown table to FILE (for the pull request's
        description). Prints `changed=<n>` last.

    check-reference-images.py --self-test
        Runs the script's own checks on made-up folders.

Uses only the standard library and git.
"""

import argparse
import html
import json
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = "tests/fixtures/render"
REQUIRED = [
    "name",
    "scene",
    "width",
    "height",
    "format",
    "tolerance",
    "adapter",
    "backend",
    "driver",
    "checksum",
    "reason",
]
# References are small pictures of controlled scenes, not screenshots.
MAX_SIDE = 256
MAX_BYTES = 64 * 1024
# Tolerances looser than this compare nothing: a review must see the image.
MAX_CHANNEL = 32
MAX_OUTLIERS = 0.15
# An edge may sit this many pixels apart (the tolerance's `edge`): further,
# and a moved object passes as the same picture.
MAX_EDGE = 2


def fnv1a64(data: bytes) -> str:
    """The checksum render_test_support.rs records (`checksum`)."""
    h = 0xCBF29CE484222325
    for byte in data:
        h = ((h ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"{h:016x}"


def png_size(data: bytes):
    """(width, height) from a PNG's header, or None if it isn't one."""
    if len(data) < 24 or data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        return None
    return struct.unpack(">II", data[16:24])


def check_folder(folder: Path):
    """Problems with the references in `folder`, as messages."""
    problems = []
    if not folder.is_dir():
        return [f"{folder}: no such folder"]
    for path in sorted(folder.iterdir()):
        if path.suffix not in (".png", ".json"):
            problems.append(f"{path.name}: only reference images and their metadata belong here")
        elif not path.with_suffix(".json" if path.suffix == ".png" else ".png").exists():
            other = "metadata (.json)" if path.suffix == ".png" else "image (.png)"
            problems.append(f"{path.name}: its {other} is missing")
    for meta_path in sorted(folder.glob("*.json")):
        png_path = meta_path.with_suffix(".png")
        if not png_path.exists():
            continue
        name = meta_path.stem
        try:
            meta = json.loads(meta_path.read_text(encoding="utf-8"))
        except (ValueError, UnicodeDecodeError) as e:
            problems.append(f"{meta_path.name}: not JSON ({e})")
            continue
        missing = [key for key in REQUIRED if key not in meta]
        if missing:
            problems.append(f"{meta_path.name}: missing {', '.join(missing)}")
            continue
        data = png_path.read_bytes()
        size = png_size(data)
        if meta["name"] != name:
            problems.append(f"{meta_path.name}: its name is {meta['name']!r}")
        if size is None:
            problems.append(f"{png_path.name}: not a PNG")
        elif list(size) != [meta["width"], meta["height"]]:
            problems.append(
                f"{name}: the image is {size[0]}x{size[1]}, the metadata says {meta['width']}x{meta['height']}"
            )
        elif max(size) > MAX_SIDE:
            problems.append(f"{name}: {size[0]}x{size[1]} is larger than {MAX_SIDE} a side")
        if len(data) > MAX_BYTES:
            problems.append(f"{name}: {len(data)} bytes is more than {MAX_BYTES}")
        if fnv1a64(data) != meta["checksum"]:
            problems.append(
                f"{name}: the image is not the one its metadata was made with (checksum {fnv1a64(data)}, "
                f"metadata says {meta['checksum']}): regenerate it with PONKO_UPDATE_REFERENCES=1"
            )
        if not str(meta["reason"]).strip():
            problems.append(f"{name}: no reason on record (PONKO_REFERENCE_REASON)")
        if len(str(meta["scene"])) < 40:
            problems.append(f"{name}: the scene isn't described")
        if not str(meta["adapter"]).strip() or not str(meta["backend"]).strip():
            problems.append(f"{name}: the adapter it was made on isn't recorded")
        tolerance = meta["tolerance"]
        if not isinstance(tolerance, dict) or "channel" not in tolerance or "outliers" not in tolerance:
            problems.append(f"{name}: no tolerance")
        elif tolerance["channel"] > MAX_CHANNEL or tolerance["outliers"] > MAX_OUTLIERS:
            problems.append(
                f"{name}: a tolerance of {tolerance['channel']} levels on {tolerance['outliers']:.0%} of the "
                f"pixels compares nothing (at most {MAX_CHANNEL} and {MAX_OUTLIERS:.0%}): assert what must "
                "hold semantically instead"
            )
        elif not 0 <= tolerance.get("edge", 0) <= MAX_EDGE:
            problems.append(
                f"{name}: a tolerance that lets edges sit {tolerance['edge']} pixels apart compares nothing "
                f"(at most {MAX_EDGE})"
            )
    return problems


def git(root: Path, *args, binary=False):
    """git's output in `root`, or None if it failed (a file missing at a revision)."""
    result = subprocess.run(["git", "-C", str(root), *args], capture_output=True)
    if result.returncode != 0:
        return None
    return result.stdout if binary else result.stdout.decode("utf-8", "replace")


def changed_references(root: Path, base: str):
    """The references that differ from `base`: [(name, before, after)], each
    side a dict with `png` (bytes) and `meta` (dict), or None when the
    reference doesn't exist on that side. Raises on a base git doesn't know."""
    listing = git(root, "ls-tree", "-r", "--name-only", base, "--", FIXTURES)
    if listing is None:
        raise SystemExit(f"git doesn't know the revision {base}")
    before_names = {Path(line).stem for line in listing.splitlines() if line.endswith((".png", ".json"))}
    folder = root / FIXTURES
    after_names = {p.stem for p in folder.iterdir() if p.suffix in (".png", ".json")} if folder.is_dir() else set()
    changes = []
    for name in sorted(before_names | after_names):
        old_png = git(root, "show", f"{base}:{FIXTURES}/{name}.png", binary=True)
        old_meta = git(root, "show", f"{base}:{FIXTURES}/{name}.json")
        new_png = (folder / f"{name}.png").read_bytes() if (folder / f"{name}.png").exists() else None
        new_meta = (
            (folder / f"{name}.json").read_text(encoding="utf-8") if (folder / f"{name}.json").exists() else None
        )

        def side(png, meta):
            if png is None and meta is None:
                return None
            try:
                parsed = json.loads(meta) if meta else {}
            except ValueError:
                parsed = {}
            return {"png": png, "meta": parsed}

        def text(meta):
            # A Windows checkout may hold the metadata with CRLF line ends.
            return None if meta is None else meta.replace("\r\n", "\n")

        if old_png != new_png or text(old_meta) != text(new_meta):
            changes.append((name, side(old_png, old_meta), side(new_png, new_meta)))
    return changes


def change_problems(changes):
    """What a reviewer can't accept about `changes`, as messages."""
    problems = []
    for name, before, after in changes:
        if after is None or before is None:
            continue
        image_changed = before["png"] != after["png"]
        if image_changed and before["meta"].get("checksum") == after["meta"].get("checksum"):
            problems.append(f"{name}: the image changed without its metadata: regenerate it with the tests")
        if image_changed and after["meta"].get("reason") == before["meta"].get("reason"):
            problems.append(
                f"{name}: the image changed and the reason on record is still the old one: "
                "PONKO_REFERENCE_REASON says why this picture changes"
            )
    return problems


def describe(side):
    if side is None:
        return "none"
    meta = side["meta"]
    tolerance = meta.get("tolerance", {})
    return (
        f"{meta.get('width', '?')}x{meta.get('height', '?')}, {meta.get('adapter', '?')} "
        f"({meta.get('backend', '?')}), tolerance {tolerance.get('channel', '?')} levels on "
        f"{tolerance.get('outliers', 0):.2%} of pixels, edges up to {tolerance.get('edge', 0)} pixel(s) apart"
    )


def kind(before, after):
    if before is None:
        return "added"
    if after is None:
        return "removed"
    if before["png"] == after["png"]:
        return "metadata changed"
    return "image changed"


PAGE = """<!doctype html>
<meta charset="utf-8">
<title>Reference image changes</title>
<style>
  body {{ font: 14px/1.4 system-ui, sans-serif; margin: 24px; background: #1b1b1f; color: #e6e6e6; }}
  h2 {{ margin: 32px 0 4px; }}
  .row {{ display: flex; gap: 16px; flex-wrap: wrap; }}
  figure {{ margin: 0; }}
  figcaption {{ color: #aaa; }}
  img, .difference {{ width: 384px; height: 384px; image-rendering: pixelated; background: #000; display: block; }}
  .difference {{ position: relative; }}
  .difference img {{ position: absolute; inset: 0; }}
  .difference img + img {{ mix-blend-mode: difference; }}
  dl {{ display: grid; grid-template-columns: max-content 1fr; gap: 2px 12px; max-width: 1200px; }}
  dt {{ color: #aaa; }}
  dd {{ margin: 0; }}
</style>
<h1>Reference image changes against {base}</h1>
<p>Each reference that differs: the image before, the image after, and their difference (black where they are
the same). A reviewer confirms that every change is the one the pull request means to make.</p>
{sections}
"""


def write_evidence(changes, folder: Path, base: str):
    """The before/after images of `changes` and an index.html showing them."""
    folder.mkdir(parents=True, exist_ok=True)
    sections = []
    for name, before, after in changes:
        figures = []
        for label, side in (("before", before), ("after", after)):
            if side is None or side["png"] is None:
                figures.append(f"<figure><div class=difference></div><figcaption>{label}: none</figcaption></figure>")
                continue
            (folder / f"{name}.{label}.png").write_bytes(side["png"])
            figures.append(
                f'<figure><img src="{name}.{label}.png" alt="{name} {label}">'
                f"<figcaption>{label}</figcaption></figure>"
            )
        if before and after and before["png"] and after["png"]:
            figures.append(
                f'<figure><div class=difference><img src="{name}.before.png" alt="">'
                f'<img src="{name}.after.png" alt=""></div><figcaption>difference</figcaption></figure>'
            )
        meta = (after or before)["meta"]
        facts = [
            ("change", kind(before, after)),
            ("why", meta.get("reason", "")),
            ("scene", meta.get("scene", "")),
            ("before", describe(before)),
            ("after", describe(after)),
        ]
        rows = "".join(f"<dt>{html.escape(k)}</dt><dd>{html.escape(str(v))}</dd>" for k, v in facts)
        sections.append(f"<h2>{html.escape(name)}</h2><dl>{rows}</dl><div class=row>{''.join(figures)}</div>")
    (folder / "index.html").write_text(
        PAGE.format(base=html.escape(base), sections="\n".join(sections)), encoding="utf-8"
    )


def summary(changes, problems):
    lines = ["## Reference images", ""]
    if not changes:
        return "\n".join(lines + ["No reference image changes.", ""])
    lines += ["| Reference | Change | Why | Before | After |", "|---|---|---|---|---|"]
    for name, before, after in changes:
        reason = (after or before)["meta"].get("reason", "").replace("|", "\\|")
        lines.append(f"| `{name}` | {kind(before, after)} | {reason} | {describe(before)} | {describe(after)} |")
    lines += [
        "",
        "The before and after images are in the evidence folder (open `index.html`); attach them to the pull request.",
        "",
    ]
    lines += [f"- :x: {problem}" for problem in problems]
    return "\n".join(lines) + "\n"


def run(root: Path, base, evidence, summary_file):
    problems = check_folder(root / FIXTURES)
    changes = []
    if base is not None:
        changes = changed_references(root, base)
        problems += change_problems(changes)
        for name, before, after in changes:
            reason = (after or before)["meta"].get("reason", "")
            print(f"{name}: {kind(before, after)} ({describe(before)} -> {describe(after)}): {reason}")
        if evidence is not None and changes:
            write_evidence(changes, Path(evidence), base)
            print(f"evidence: {Path(evidence) / 'index.html'}")
        if summary_file is not None:
            with open(summary_file, "a", encoding="utf-8") as out:
                out.write(summary(changes, problems))
    for problem in problems:
        print(f"error: {problem}", file=sys.stderr)
    if base is not None:
        print(f"changed={len(changes)}")
    return 1 if problems else 0


def self_test():
    """The checks above, on folders made here."""
    png = (
        b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR" + struct.pack(">II", 4, 4) + b"\x08\x06\x00\x00\x00" + b"\x00" * 16
    )

    def meta(data, **changes):
        base = {
            "name": "sample",
            "scene": "A grey square on black, drawn with nothing lit and no antialiasing.",
            "width": 4,
            "height": 4,
            "format": "rgba8-srgb",
            "tolerance": {"channel": 8, "outliers": 0.02},
            "adapter": "Test adapter",
            "backend": "None",
            "driver": "unknown",
            "checksum": fnv1a64(data),
            "reason": "First picture.",
        }
        base.update(changes)
        return json.dumps(base, indent=2) + "\n"

    assert fnv1a64(b"") == "cbf29ce484222325" and fnv1a64(b"foobar") == "85944171f73967e8"
    assert png_size(png) == (4, 4) and png_size(b"not a png") is None

    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        folder = root / FIXTURES
        folder.mkdir(parents=True)

        def write(data=png, **changes):
            (folder / "sample.png").write_bytes(data)
            (folder / "sample.json").write_text(meta(png, **changes), encoding="utf-8")

        def expect(text):
            problems = check_folder(folder)
            assert any(text in p for p in problems), f"expected {text!r} in {problems}"

        write()
        assert check_folder(folder) == [], check_folder(folder)
        write(data=png + b"\x00")
        expect("not the one its metadata was made with")
        write(reason=" ")
        expect("no reason on record")
        write(width=8)
        expect("the metadata says 8x4")
        write(tolerance={"channel": 200, "outliers": 0.02})
        expect("compares nothing")
        write(tolerance={"channel": 8, "outliers": 0.02, "edge": 5})
        expect("edges sit 5 pixels apart")
        write(tolerance={"channel": 8, "outliers": 0.02, "edge": 1})
        assert check_folder(folder) == []
        write(scene="A square.")
        expect("isn't described")
        write()
        (folder / "orphan.png").write_bytes(png)
        expect("orphan.png: its metadata (.json) is missing")
        (folder / "orphan.png").unlink()
        (folder / "notes.txt").write_text("x")
        expect("only reference images")
        (folder / "notes.txt").unlink()
        (folder / "sample.json").write_text('{"name": "sample"}')
        expect("missing scene")

        # Changes against a git revision.
        def commit(message):
            for args in (["add", "-A"], ["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-qm", message]):
                subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True)

        subprocess.run(["git", "-C", str(root), "init", "-q"], check=True, capture_output=True)
        write()
        commit("first")
        assert changed_references(root, "HEAD") == []
        # A new image with the old reason: not acceptable.
        other = png + b"\x01"
        (folder / "sample.png").write_bytes(other)
        (folder / "sample.json").write_text(meta(other), encoding="utf-8")
        changes = changed_references(root, "HEAD")
        assert [(n, kind(b, a)) for n, b, a in changes] == [("sample", "image changed")], changes
        assert any("reason on record is still the old one" in p for p in change_problems(changes))
        # With a new reason: fine, and the evidence is written.
        (folder / "sample.json").write_text(meta(other, reason="Brighter light."), encoding="utf-8")
        changes = changed_references(root, "HEAD")
        assert change_problems(changes) == []
        write_evidence(changes, root / "evidence", "HEAD")
        page = (root / "evidence" / "index.html").read_text(encoding="utf-8")
        assert "Brighter light." in page and "sample.before.png" in page and "sample.after.png" in page
        assert (root / "evidence" / "sample.before.png").read_bytes() == png
        assert (root / "evidence" / "sample.after.png").read_bytes() == other
        assert "| `sample` | image changed | Brighter light." in summary(changes, [])
        # The image changed and the metadata did not.
        (folder / "sample.json").write_text(meta(png), encoding="utf-8")
        assert any("without its metadata" in p for p in change_problems(changed_references(root, "HEAD")))
        # Added and removed references are changes too.
        write()
        (folder / "new.png").write_bytes(png)
        (folder / "new.json").write_text(meta(png, name="new"), encoding="utf-8")
        (folder / "sample.png").unlink()
        (folder / "sample.json").unlink()
        kinds = {n: kind(b, a) for n, b, a in changed_references(root, "HEAD")}
        assert kinds == {"new": "added", "sample": "removed"}, kinds
        # The whole run: its exit code and last line.
        assert run(root, "HEAD", None, None) == 0
        shutil.rmtree(root / "evidence")
    print("check-reference-images.py: self-test passed")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--changed", metavar="BASE", help="also list the references that differ from this revision")
    parser.add_argument("--evidence", metavar="DIR", help="write before/after images and index.html here")
    parser.add_argument("--summary", metavar="FILE", help="append a Markdown summary of the changes to this file")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    return run(ROOT, args.changed, args.evidence, args.summary)


if __name__ == "__main__":
    sys.exit(main())
