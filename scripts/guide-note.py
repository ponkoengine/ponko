#!/usr/bin/env python3
"""mdBook preprocessor: a note under every chapter title saying the guide follows `main`.

The guide on the website (https://ponko.dev/guide/) is built from `main`, and a
crates.io release can lag behind it, so every page opens with a note saying so
and naming the dependency line that compiles its examples.

    guide-note.py preprocess   the preprocessor (see book/book.toml)
    guide-note.py selftest     check the note's placement with made-up input
"""
import json
import sys
from pathlib import Path

# The project's home is the one line in PROJECT_URL (see scripts/project-url.py).
REPO_URL = (Path(__file__).resolve().parent.parent / "PROJECT_URL").read_text(encoding='utf-8').strip()
NOTE = (
    "**Which version?** This guide follows the `main` branch, so it can "
    "describe APIs that the latest crates.io release does not have yet. "
    f'Its examples compile against `ponko = {{ git = "{REPO_URL}" }}`; the '
    f"[changelog]({REPO_URL}/-/blob/main/CHANGELOG.md) says what each release has."
)


def add_note(content, note=NOTE):
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


def transform(book):
    """Add the note to every chapter with a file, in place; returns the book."""

    def walk(entries):
        for entry in entries:
            chapter = entry.get("Chapter") if isinstance(entry, dict) else None
            if chapter is None:
                continue
            if chapter.get("path"):
                chapter["content"] = add_note(chapter["content"])
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
    json.dump(transform(book), sys.stdout, ensure_ascii=False)
    return 0


def selftest():
    assert "follows the `main` branch" in NOTE and 'git = "' in NOTE
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
    transform(book)
    top = book["items"][0]["Chapter"]
    assert "Which version?" in top["content"] and "Which version?" in top["sub_items"][0]["Chapter"]["content"]
    assert top["sub_items"][1]["Chapter"]["content"] == "# Draft\n", "draft chapters are left alone"
    print("guide-note.py: self-test passed")
    return 0


def main():
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "preprocess":
        return preprocess()
    if command == "selftest":
        return selftest()
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main())
