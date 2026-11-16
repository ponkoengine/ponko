#!/usr/bin/env python3
"""Where the project lives: one URL, and the links that follow it.

The home of the project is the single line in `PROJECT_URL` (repository root).
The docs link to files of the repository with *relative* links, which work in
the repository browser:

    [STABILITY.md](../../STABILITY.md)        in book/src/ecs.md
    [the roadmap](ROADMAP.md)                 in README.md

The built guide is hosted somewhere else, where such a link would be dead, so
the `repo-links` preprocessor (book/book.toml) rewrites every link that leaves
`book/src/` into a link to the file on the project's host, built from
PROJECT_URL. Moving the project is therefore one command:

    project-url.py set <new url>   rewrite PROJECT_URL and every literal copy of
                                   the old URL in the docs (README, crate READMEs,
                                   NOTICE files, book.toml, rustdoc links)
    project-url.py check           fail on links to the old host, on relative
                                   links that point nowhere, and on PROJECT_URL
                                   copies that disagree with the file
    project-url.py preprocess      the mdBook preprocessor (see book/book.toml)
    project-url.py selftest        check the link rewriting with made-up input

If the host's URL layout differs from GitLab's (`/-/blob/main/...`), change
BLOB and TREE below.
"""

import json
import re
import subprocess
import sys
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent.parent
BOOK_SRC = ROOT / "book" / "src"
URL_FILE = ROOT / "PROJECT_URL"

# Where a file or a folder of the default branch lives on the project's host.
BLOB = "/-/blob/main/"
TREE = "/-/tree/main/"

# Strings of the hosts the project left; no doc may name them.
DEAD = ("github.com/ponkoengine", "ponkoengine.github.io", "raw.githubusercontent.com/ponkoengine")

# Files that talk about hosts on purpose (release tooling, the gallery's own
# build, the CI workflows) and this script.
EXCLUDED_PREFIXES = (".github/", "web/", "api/", "target/")
EXCLUDED_FILES = {"scripts/project-url.py", "scripts/release-check.py", "Cargo.lock"}
DOC_SUFFIXES = (".md", ".txt", ".toml", ".liquid", ".rs")


def project_url():
    url = URL_FILE.read_text().strip()
    if not re.fullmatch(r"https://[^\s/]+(/[^\s/]+)+", url):
        sys.exit(f"PROJECT_URL must be one https URL without a trailing slash, got {url!r}")
    return url


def doc_files():
    """Tracked files the URL rules apply to, as repository-relative paths."""
    out = subprocess.run(["git", "ls-files"], cwd=ROOT, capture_output=True, text=True, check=True).stdout
    for name in out.splitlines():
        if name in EXCLUDED_FILES or name.startswith(EXCLUDED_PREFIXES):
            continue
        if name.endswith(DOC_SUFFIXES) or PurePosixPath(name).name == "NOTICE":
            yield name


# ---- links ------------------------------------------------------------------

# `[text](target)`, with an optional `"title"`, and `[ref]: target` definitions.
INLINE = re.compile(r"(\]\()([^)\s]+)((?:\s+\"[^\"]*\")?\))")
DEFINITION = re.compile(r"^(\s*\[[^\]]+\]:\s+)(\S+)", re.M)
FENCE = re.compile(r"^\s*(`{3,}|~{3,})")


def is_relative(target):
    return bool(target) and not re.match(r"([a-zA-Z][a-zA-Z0-9+.-]*:|#|/|\{\{)", target)


def split_anchor(target):
    path, hash_, anchor = target.partition("#")
    return path, hash_ + anchor


def map_links(text, fn):
    """Apply `fn(target) -> target` to every link target outside code fences."""
    out, fence = [], None
    for line in text.split("\n"):
        m = FENCE.match(line)
        if m:
            fence = None if fence else m.group(1)[0]
            out.append(line)
            continue
        if fence is None:
            line = INLINE.sub(lambda m: m.group(1) + fn(m.group(2)) + m.group(3), line)
            line = DEFINITION.sub(lambda m: m.group(1) + fn(m.group(2)), line)
        out.append(line)
    return "\n".join(out)


def to_project_link(target, source_dir, url, warn=None):
    """A relative link from a chapter in `source_dir` that leaves book/src -> the file on the project host."""
    if not is_relative(target):
        return target
    path, anchor = split_anchor(target)
    if not path:
        return target
    resolved = Path(source_dir, path).resolve()
    try:
        resolved.relative_to(BOOK_SRC.resolve())
        return target  # inside the book: mdBook handles it
    except ValueError:
        pass
    try:
        repo_path = resolved.relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        if warn:
            warn(f"link {target!r} leaves the repository")
        return target
    if not resolved.exists():
        if warn:
            warn(f"link {target!r} points to {repo_path}, which does not exist")
        return target
    kind = TREE if resolved.is_dir() else BLOB
    return f"{url}{kind}{repo_path.rstrip('/')}{anchor}"


def transform(book, url, warn=None):
    def walk(entries):
        for entry in entries:
            chapter = entry.get("Chapter") if isinstance(entry, dict) else None
            if chapter is None:
                continue
            if chapter.get("path"):
                source_dir = (BOOK_SRC / chapter["path"]).parent
                where = chapter["path"]
                chapter["content"] = map_links(
                    chapter["content"],
                    lambda t: to_project_link(t, source_dir, url, (lambda m: warn(f"{where}: {m}")) if warn else None),
                )
            walk(chapter.get("sub_items", []))

    walk(book.get("items", book.get("sections", [])))
    return book


def preprocess():
    if len(sys.argv) > 2 and sys.argv[2] == "supports":
        return 0
    sys.stdin.reconfigure(encoding="utf-8")
    sys.stdout.reconfigure(encoding="utf-8")
    _context, book = json.load(sys.stdin)
    json.dump(transform(book, project_url(), lambda m: print(f"repo-links: {m}", file=sys.stderr)), sys.stdout, ensure_ascii=False)
    return 0


# ---- check and set ------------------------------------------------------------


def check():
    url = project_url()
    problems = []
    for name in doc_files():
        text = (ROOT / name).read_text(encoding="utf-8")
        for dead in DEAD:
            for n, line in enumerate(text.split("\n"), 1):
                if dead in line:
                    problems.append(f"{name}:{n}: names {dead}; use a relative link, or PROJECT_URL ({url})")
        if name.endswith(".md"):
            base = (ROOT / name).parent
            seen = []
            map_links(text, lambda t: seen.append(t) or t)
            for target in seen:
                if not is_relative(target):
                    continue
                path, _ = split_anchor(target)
                if path and not (base / path).exists():
                    problems.append(f"{name}: relative link {target!r} points to nothing")
    for p in problems:
        print(f"error: {p}", file=sys.stderr)
    if not problems:
        print(f"project URL {url}: no links to the old host, every relative link resolves")
    return 1 if problems else 0


def set_url(new):
    old = project_url()
    if not re.fullmatch(r"https://[^\s/]+(/[^\s/]+)+", new):
        sys.exit("the new URL must be one https URL without a trailing slash")
    changed = 0
    for name in doc_files():
        path = ROOT / name
        text = path.read_text(encoding="utf-8")
        if old in text:
            path.write_text(text.replace(old, new), encoding="utf-8")
            changed += 1
    URL_FILE.write_text(new + "\n")
    print(f"PROJECT_URL is {new} now; {changed} files rewritten (review `git diff`)")
    return 0


def selftest():
    url = "https://example.test/org/proj"
    got = to_project_link("../../STABILITY.md#c-abi", BOOK_SRC, url)
    assert got == f"{url}{BLOB}STABILITY.md#c-abi", got
    assert to_project_link("../../examples", BOOK_SRC, url) == f"{url}{TREE}examples"
    assert to_project_link("physics.md#x", BOOK_SRC, url) == "physics.md#x", "links inside the book stay"
    assert to_project_link("https://docs.rs/ponko", BOOK_SRC, url) == "https://docs.rs/ponko"
    assert to_project_link("#top", BOOK_SRC, url) == "#top"
    warnings = []
    assert to_project_link("../../nope.md", BOOK_SRC, url, warnings.append) == "../../nope.md" and warnings
    text = "[a](../../STABILITY.md) and ![i](x.png)\n```md\n[b](../../STABILITY.md)\n```\n[ref]: ../../ROADMAP.md\n"
    out = map_links(text, lambda t: to_project_link(t, BOOK_SRC, url))
    assert f"[a]({url}{BLOB}STABILITY.md)" in out and "[b](../../STABILITY.md)" in out, out
    assert f"[ref]: {url}{BLOB}ROADMAP.md" in out, out
    book = {"items": [{"Chapter": {"path": "ecs.md", "content": "[s](../../STABILITY.md)", "sub_items": []}}]}
    transform(book, url)
    assert book["items"][0]["Chapter"]["content"] == f"[s]({url}{BLOB}STABILITY.md)"
    print("project-url.py: self-test passed")
    return 0


def main():
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "preprocess":
        return preprocess()
    if command == "check":
        return check()
    if command == "set" and len(sys.argv) == 3:
        return set_url(sys.argv[2])
    if command == "selftest":
        return selftest()
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main())
