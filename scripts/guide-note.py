#!/usr/bin/env python3
"""mdBook preprocessor: a note under every chapter title saying which release the guide documents.

The guide on the website (https://ponko.dev/guide/) is built from `main`, and a
crates.io release can lag behind it, so every page opens with a note that names
the version, says whether `main` has moved past it, and gives the dependency
line that compiles the examples. Nothing in the note is typed by hand: the
version is `[package] version` of the root `Cargo.toml` and the unreleased
changes are the fragments left in `changelog.d/`. A release bump (version
raised, fragments folded into the CHANGELOG) therefore updates every page, and
a guide built from a release tag says "documents exactly 0.2.0".

    guide-note.py preprocess   the preprocessor (see book/book.toml)
    guide-note.py note         print the note as it stands now
    guide-note.py selftest     check the note's placement and wording with made-up input
"""
import json
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# The project's home is the one line in PROJECT_URL (see scripts/project-url.py).
REPO_URL = (ROOT / "PROJECT_URL").read_text(encoding="utf-8").strip()
CHANGELOG_URL = f"{REPO_URL}/blob/main/CHANGELOG.md"  # BLOB of project-url.py


def engine_version(root=ROOT):
    """The version the root `Cargo.toml` says (`0.2.0`, `0.3.0-rc.1`)."""
    return tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]


def unreleased_changes(root=ROOT):
    """How many changelog fragments are waiting for the next release."""
    return len([p for p in (root / "changelog.d").glob("*.md") if p.name != "README.md"])


def make_note(version, fragments):
    """The note's text for a guide of `version` with `fragments` unreleased changes."""
    if fragments:
        where = (
            f"This guide follows the `main` branch: Ponko {version} plus "
            f"{fragments} change{'s' if fragments != 1 else ''} not released yet "
            "(`changelog.d/` lists them), so a page can describe APIs that the "
            "latest crates.io release does not have."
        )
    else:
        where = f"This guide documents Ponko {version}, as released."
    return (
        f"**Which version?** {where} "
        f'Its examples compile against `ponko = {{ git = "{REPO_URL}" }}`; '
        f"[the changelog]({CHANGELOG_URL}) says what each release has."
    )


def current_note(root=ROOT):
    return make_note(engine_version(root), unreleased_changes(root))


def add_note(content, note):
    """`content` with the note, as a quote, right under the first `# ` title."""
    quote = "\n".join(f"> {line}" if line else ">" for line in note.split("\n"))
    lines = content.split("\n")
    in_fence = False
    for i, line in enumerate(lines):
        if line.lstrip().startswith(("```", "~~~")):
            in_fence = not in_fence
        elif not in_fence and line.startswith("# "):
            return "\n".join(lines[: i + 1] + ["", quote] + lines[i + 1 :])
    return quote + "\n\n" + content


def transform(book, note):
    """Add the note to every chapter with a file, in place; returns the book."""

    def walk(entries):
        for entry in entries:
            chapter = entry.get("Chapter") if isinstance(entry, dict) else None
            if chapter is None:
                continue
            if chapter.get("path"):
                chapter["content"] = add_note(chapter["content"], note)
            walk(chapter.get("sub_items", []))

    walk(book.get("items", book.get("sections", [])))
    return book


def preprocess():
    if len(sys.argv) > 2 and sys.argv[2] == "supports":
        return 0  # every renderer is fine
    # mdBook speaks UTF-8 JSON; the locale's encoding (cp1252 on Windows) would corrupt it.
    sys.stdin.reconfigure(encoding="utf-8")
    sys.stdout.reconfigure(encoding="utf-8")
    _context, book = json.load(sys.stdin)
    json.dump(transform(book, current_note()), sys.stdout, ensure_ascii=False)
    return 0


def selftest():
    ahead = make_note("0.2.0", 3)
    assert "Ponko 0.2.0 plus 3 changes not released yet" in ahead and 'git = "' in ahead
    assert "plus 1 change not" in make_note("0.2.0", 1), "singular"
    exact = make_note("0.2.0", 0)
    assert "documents Ponko 0.2.0, as released" in exact and "not released yet" not in exact
    assert f"({REPO_URL}/blob/main/CHANGELOG.md)" in exact, "the link is a GitHub-style blob link"
    # The real tree: a version Cargo accepts, and a count that is a number.
    real = engine_version()
    assert real[0].isdigit() and real.count(".") >= 2, real
    assert isinstance(unreleased_changes(), int)
    assert real in current_note()
    page = "# Title\n\nText\n\n```sh\n# not a title\n```\n"
    assert add_note(page, "note").startswith("# Title\n\n> note\n\nText")
    fenced = "```sh\n# not a title\n```\n\n# Real\n"
    assert add_note(fenced, "note").endswith("# Real\n\n> note\n")
    assert add_note("no title", "note").startswith("> note\n\nno title")
    book = {
        "items": [
            {"Chapter": {"path": "a.md", "content": "# A\n", "sub_items": [
                {"Chapter": {"path": "sub/b.md", "content": "# B\n", "sub_items": []}},
                {"Chapter": {"path": None, "content": "# Draft\n", "sub_items": []}},
            ]}},
            "Separator",
        ]
    }
    transform(book, "Which version? note")
    top = book["items"][0]["Chapter"]
    assert "Which version?" in top["content"] and "Which version?" in top["sub_items"][0]["Chapter"]["content"]
    assert top["sub_items"][1]["Chapter"]["content"] == "# Draft\n", "draft chapters are left alone"
    print("guide-note.py: self-test passed")
    return 0


def main():
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "preprocess":
        return preprocess()
    if command == "note":
        print(current_note())
        return 0
    if command == "selftest":
        return selftest()
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main())
