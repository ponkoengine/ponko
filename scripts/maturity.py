#!/usr/bin/env python3
"""Fail when a guide chapter or a top-level module doesn't say how proven it is.

Every chapter of the guide (book/src/SUMMARY.md) opens, right under its title,
with its maturity level and what the level rests on:

    > **Maturity: [tested](maturity.md).** Game: none. Tests: `tests/x.rs`.
    > Platforms: Linux, macOS and Windows, headless (...). Graphics: not
    > involved. Missing: a reference game that uses it.

The level is proven, tested or experimental (STABILITY.md, "Maturity levels",
defines them). Game, Platforms and Graphics are required for a level, Missing
is optional. The quote may span lines.

A page that describes no feature of its own (the introduction, a tutorial
index, a migration guide) says so and why in an HTML comment in the same
place, so the reader doesn't see a banner that has nothing to rate:

    <!-- Maturity: not a feature. A migration guide for one release. -->

The comment may span lines.

Every public top-level module of the engine (`pub mod` in src/lib.rs, except
the hidden ones and the prelude) says the same in its rustdoc, as the
paragraph after its first one. A module names the chapter it takes its level
from, and this script writes that paragraph from the chapter:

    //! **Maturity: tested**, from the guide chapter `physics3d.md`. Game: ...

A module that no chapter covers writes its own, with the same fields:

    //! **Maturity: tested**, no guide chapter. Game: none. Tests: ...

book/src/maturity.md, the list of every chapter and module with its level, is
generated from them; never edit it by hand.

    python3 scripts/maturity.py            # check chapters, modules and the page
    python3 scripts/maturity.py --write    # rewrite the page and the module paragraphs
    python3 scripts/maturity.py --list     # every chapter and module with its level
    python3 scripts/maturity.py --selftest # the parsing, with made-up input
"""
from pathlib import Path
import re
import sys
import textwrap

ROOT = Path(__file__).resolve().parent.parent
BOOK = ROOT / "book" / "src"
PAGE = "maturity.md"
LEVELS = ("proven", "tested", "experimental")
NOT_A_FEATURE = "not a feature"
FIELDS = ("Game:", "Platforms:", "Graphics:")
# Top-level modules that are not features of their own: the prelude
# re-exports, the deprecated `stream` and `physics3d` aliases and the hidden
# internals.
SKIP_MODULES = {"prelude", "stream", "physics3d", "bench_sharing", "__ffi", "__fuzz"}

CHAPTER_START = re.compile(r"^> \*\*Maturity: (?:\[([a-z ]+)\]\([^)]*\)|([a-z ]+))\.\*\*\s*(.*)$")
NOT_A_FEATURE_START = re.compile(r"^<!-- Maturity: not a feature\.\s*(.*)$")
MODULE_START = re.compile(
    r"^\*\*Maturity: ([a-z]+)\*\*, (?:from the guide chapter `([^`]+)`|no guide chapter)\.\s*(.*)$"
)


class Problem(Exception):
    pass


def parse_chapter(text):
    """(level, rests-on text) from a chapter's opening quote, or raise Problem."""
    lines = text.split("\n")
    in_fence = False
    title = None
    for i, line in enumerate(lines):
        if line.lstrip().startswith(("```", "~~~")):
            in_fence = not in_fence
        elif not in_fence and line.startswith("# "):
            title = i
            break
    if title is None:
        raise Problem("no `# ` title")
    rest = lines[title + 1 :]
    while rest and not rest[0].strip():
        rest.pop(0)
    if not rest:
        raise Problem("nothing under the title")
    comment = NOT_A_FEATURE_START.match(rest[0])
    if comment:
        return NOT_A_FEATURE, not_a_feature_reason(comment.group(1), rest[1:])
    match = CHAPTER_START.match(rest[0])
    if not match:
        raise Problem(
            "the first lines under the title must be `> **Maturity: <level>.** ...`, "
            "or `<!-- Maturity: not a feature. <why> -->` for a page that describes no feature"
        )
    level = match.group(1) or match.group(2)
    if level == NOT_A_FEATURE:
        raise Problem("write `not a feature` as a comment, `<!-- Maturity: not a feature. <why> -->`, not as a banner")
    words = [match.group(3)]
    for line in rest[1:]:
        if not line.startswith(">"):
            break
        words.append(line[1:].strip())
    body = " ".join(w for w in words if w)
    check_level(level, body, allow_not_a_feature=True)
    return level, body


def not_a_feature_reason(first, more):
    """The reason in a `<!-- Maturity: not a feature. ... -->` comment that starts with `first`."""
    words = []
    for line in [first] + more:
        text = line.strip()
        if "-->" in text:
            words.append(text[: text.index("-->")].strip())
            body = " ".join(w for w in words if w)
            if not body:
                raise Problem("`not a feature` needs a reason")
            return body
        words.append(text)
    raise Problem("the `<!-- Maturity: not a feature.` comment never ends with `-->`")


def check_level(level, body, allow_not_a_feature=False):
    if level == NOT_A_FEATURE:
        if not allow_not_a_feature:
            raise Problem("a module can't be `not a feature`; leave it out or give it a level")
        if not body:
            raise Problem("`not a feature` needs a reason")
        return
    if level not in LEVELS:
        raise Problem(f"unknown level {level!r}; use one of {', '.join(LEVELS)}")
    missing = [f for f in FIELDS if f not in body]
    if missing:
        raise Problem(f"the level needs {', '.join(missing)} (what it rests on)")
    if "[" in body or "](" in body:
        raise Problem("keep links out of what the level rests on: modules copy it into rustdoc")


def chapters():
    """Chapter paths relative to book/src, in SUMMARY.md's order, without the page itself."""
    summary = (BOOK / "SUMMARY.md").read_text(encoding="utf-8")
    seen = []
    for path in re.findall(r"\]\(([^)]+\.md)\)", summary):
        if path != PAGE and path not in seen:
            seen.append(path)
    return seen


def chapter_titles():
    summary = (BOOK / "SUMMARY.md").read_text(encoding="utf-8")
    return {path: title for title, path in re.findall(r"\[([^\]]+)\]\(([^)]+\.md)\)", summary)}


def modules():
    """(name, file, doc prefix) of every top-level public module."""
    lib = (ROOT / "src" / "lib.rs").read_text(encoding="utf-8")
    found = []
    for name, inline in re.findall(r"^\s*pub mod ([a-z0-9_]+)\s*(\{|;)", lib, re.M):
        if name in SKIP_MODULES:
            continue
        if inline == "{":
            found.append((name, ROOT / "src" / "lib.rs", "///"))
            continue
        for candidate in (ROOT / "src" / f"{name}.rs", ROOT / "src" / name / "mod.rs"):
            if candidate.exists():
                found.append((name, candidate, "//!"))
                break
        else:
            raise SystemExit(f"maturity.py: no file for module `{name}`")
    return found


def doc_paragraph(lines, prefix, start):
    """The doc paragraph that starts at `start`: its line indexes and its text."""
    end = start
    words = []
    while end < len(lines):
        stripped = lines[end].strip()
        if not stripped.startswith(prefix) or stripped == prefix:
            break
        words.append(stripped[len(prefix) :].strip())
        end += 1
    return start, end, " ".join(words)


def find_module_paragraph(lines, prefix, name):
    """(start, end, text) of a module's maturity paragraph, or None."""
    if prefix == "///":
        # An inline module in lib.rs: its maturity paragraph is in the doc
        # comment right above `pub mod name {`.
        mod_line = next(i for i, l in enumerate(lines) if re.match(rf"\s*pub mod {name}\s*\{{", l))
        i = mod_line - 1
        while i >= 0 and lines[i].strip().startswith(("///", "#[")):
            i -= 1
        candidates = range(i + 1, mod_line)
    else:
        candidates = range(len(lines))
    for i in candidates:
        stripped = lines[i].strip()
        if stripped.startswith(prefix + " **Maturity:"):
            return doc_paragraph(lines, prefix, i)
    return None


def module_text(level, chapter, body):
    source = f"from the guide chapter `{chapter}`" if chapter else "no guide chapter"
    return f"**Maturity: {level}**, {source}. {body}"


def wrap_doc(text, prefix, indent=""):
    width = 100 - len(indent) - len(prefix) - 1
    return [f"{indent}{prefix} {line}" for line in textwrap.wrap(text, width, break_on_hyphens=False, break_long_words=False)]


def check_modules(levels, write):
    """Problems with the modules' paragraphs; with `write`, fixes generated ones."""
    problems = []
    rows = []
    for name, path, prefix in modules():
        lines = path.read_text(encoding="utf-8").split("\n")
        found = find_module_paragraph(lines, prefix, name)
        where = f"{path.relative_to(ROOT).as_posix()} (module `{name}`)"
        if found is None:
            problems.append(f"{where}: no `{prefix} **Maturity: <level>**, ...` paragraph in its rustdoc")
            continue
        start, end, text = found
        match = MODULE_START.match(text)
        if not match:
            problems.append(f"{where}: the paragraph must start `**Maturity: <level>**, from the guide chapter `x.md`.` or `..., no guide chapter.`")
            continue
        level, chapter, body = match.groups()
        if chapter:
            if chapter not in levels:
                problems.append(f"{where}: `{chapter}` is not a chapter of the guide")
                continue
            level, body = levels[chapter]
            if level == NOT_A_FEATURE:
                problems.append(f"{where}: `{chapter}` is not a feature; name a feature's chapter")
                continue
        else:
            try:
                check_level(level, body)
            except Problem as e:
                problems.append(f"{where}: {e}")
                continue
        rows.append((name, level, chapter))
        wanted = module_text(level, chapter, body)
        if text != wanted or lines[start:end] != wrap_doc(wanted, prefix, re.match(r"\s*", lines[start]).group(0)):
            if write:
                indent = re.match(r"\s*", lines[start]).group(0)
                lines[start:end] = wrap_doc(wanted, prefix, indent)
                path.write_text("\n".join(lines), encoding="utf-8", newline="\n")
            else:
                problems.append(f"{where}: its maturity paragraph differs from `{chapter}`; run `python3 scripts/maturity.py --write`")
    return problems, rows


def criteria():
    """The level definitions, from STABILITY.md (they are written once, there)."""
    text = (ROOT / "STABILITY.md").read_text(encoding="utf-8")
    match = re.search(r"<!-- maturity-levels:start -->\n(.*?)<!-- maturity-levels:end -->", text, re.S)
    if not match:
        raise SystemExit("maturity.py: STABILITY.md has no maturity-levels:start/end block")
    return match.group(1).strip()


def cell(text):
    return text.replace("|", "\\|")


def field(body, name):
    """The text of one field (`Game:`, `Missing:`) of what a level rests on."""
    match = re.search(rf"{name}:\s*(.*?)(?:\s+(?:Game|Tests|Platforms|Graphics|Missing):|$)", body)
    return match.group(1).rstrip(".") if match else ""


def page(levels, titles, rows):
    count = {level: sum(1 for l, _ in levels.values() if l == level) for level in LEVELS}
    out = [
        "# Maturity levels",
        "",
        "<!-- Generated by `python3 scripts/maturity.py --write` from the chapters",
        "     and module docs; don't edit by hand. -->",
        "",
        "How proven each part of the engine is, from what the repository records:",
        "the tests in the list of checks (`scripts/checks.yml`), the reference games",
        "that use a feature and the platforms and graphics backends those runs cover.",
        "Every chapter of this guide opens with its level, and so does the API",
        "reference of every top-level module. The levels, and what each promises,",
        "are written in `STABILITY.md`; here they are again:",
        "",
        criteria(),
        "",
        f"Chapters: {count['proven']} proven, {count['tested']} tested, "
        f"{count['experimental']} experimental.",
        "",
        "## Chapters",
        "",
        "| Chapter | Level | Reference game | Gaps it names |",
        "|---|---|---|---|",
    ]
    for path, (level, body) in levels.items():
        if level == NOT_A_FEATURE:
            continue
        out.append(
            f"| [{cell(titles.get(path, path))}]({path}) | **{level}** | "
            f"{cell(field(body, 'Game'))} | {cell(field(body, 'Missing')) or '—'} |"
        )
    out += [
        "",
        "## Modules",
        "",
        "The level each top-level module of `ponko` gives in its rustdoc.",
        "",
        "| Module | Level | From |",
        "|---|---|---|",
    ]
    for name, level, chapter in sorted(rows):
        source = f"[{cell(titles.get(chapter, chapter))}]({chapter})" if chapter else "its own rustdoc"
        out.append(f"| `ponko::{name}` | **{level}** | {source} |")
    out += [
        "",
        "## Pages that are not a feature",
        "",
    ]
    for path, (level, body) in levels.items():
        if level == NOT_A_FEATURE:
            out.append(f"- [{titles.get(path, path)}]({path}): {body}")
    return "\n".join(out) + "\n"


def collect():
    problems = []
    levels = {}
    for path in chapters():
        try:
            levels[path] = parse_chapter((BOOK / path).read_text(encoding="utf-8"))
        except Problem as e:
            problems.append(f"book/src/{path}: {e}")
        except FileNotFoundError:
            problems.append(f"book/src/{path}: listed in SUMMARY.md but missing")
    return problems, levels


def run(write=False, listing=False):
    problems, levels = collect()
    if problems:
        print("maturity.py: chapters without a level:")
        for p in problems:
            print(f"  {p}")
        print("Every chapter opens with `> **Maturity: <level>.** Game: ... Platforms: ... Graphics: ...`;")
        print("STABILITY.md, \"Maturity levels\", says how to pick the level.")
        return 1
    module_problems, rows = check_modules(levels, write)
    if listing:
        for path, (level, _) in levels.items():
            print(f"{level:14} {path}")
        for name, level, chapter in sorted(rows):
            print(f"{level:14} ponko::{name}" + (f" ({chapter})" if chapter else ""))
    if "maturity.md" not in (BOOK / "SUMMARY.md").read_text(encoding="utf-8"):
        module_problems.append("book/src/SUMMARY.md: doesn't list maturity.md")
    generated = page(levels, chapter_titles(), rows)
    target = BOOK / PAGE
    current = target.read_text(encoding="utf-8") if target.exists() else None
    if current != generated:
        if write:
            target.write_text(generated, encoding="utf-8", newline="\n")
            print(f"maturity.py: wrote book/src/{PAGE}")
        else:
            module_problems.append(f"book/src/{PAGE} is out of date; run `python3 scripts/maturity.py --write`")
    if module_problems:
        print("maturity.py:")
        for p in module_problems:
            print(f"  {p}")
        return 1
    if not listing:
        print(f"maturity.py: {len(levels)} chapters and {len(rows)} modules have a level")
    return 0


def selftest():
    good = "# Title\n\n> **Maturity: [tested](maturity.md).** Game: none. Tests: x.\n> Platforms: Linux. Graphics: not involved.\n\nText\n"
    assert parse_chapter(good) == ("tested", "Game: none. Tests: x. Platforms: Linux. Graphics: not involved.")
    plain = "```sh\n# not a title\n```\n# Real\n> **Maturity: proven.** Game: a. Platforms: b. Graphics: c.\n"
    assert parse_chapter(plain)[0] == "proven"
    assert parse_chapter("# T\n\n<!-- Maturity: not a feature. An index. -->\n") == (NOT_A_FEATURE, "An index.")
    assert parse_chapter("# T\n\n<!-- Maturity: not a feature. An\n     index. -->\n\nText\n") == (NOT_A_FEATURE, "An index.")
    for bad, message in [
        ("# T\n\nText first.\n", "must be"),
        ("# T\n\n> **Maturity: solid.** Game: a. Platforms: b. Graphics: c.\n", "unknown level"),
        ("# T\n\n> **Maturity: tested.** Game: a. Platforms: b.\n", "Graphics:"),
        ("# T\n\n<!-- Maturity: not a feature. -->\n", "needs a reason"),
        ("# T\n\n<!-- Maturity: not a feature. An index.\n\nText\n", "never ends"),
        ("# T\n\n> **Maturity: not a feature.** An index.\n", "as a comment"),
        ("# T\n\n> **Maturity: tested.** Game: [a](b.md). Platforms: b. Graphics: c.\n", "links"),
        ("no title\n", "title"),
    ]:
        try:
            parse_chapter(bad)
        except Problem as e:
            assert message in str(e), (bad, e)
        else:
            raise AssertionError(f"accepted: {bad!r}")
    lines = ["//! Summary.", "//!", "//! **Maturity: tested**, no guide chapter. Game: none.", "//! Platforms: a.", "//!", "//! More."]
    start, end, text = find_module_paragraph(lines, "//!", "m")
    assert (start, end) == (2, 4) and text.endswith("Game: none. Platforms: a.")
    assert MODULE_START.match(text).group(1) == "tested"
    inline = ["/// Other.", "pub mod a;", "/// The crate.", "///", "/// **Maturity: proven**, from the guide chapter `ecs.md`. x", "#[doc(inline)]", "pub mod b {"]
    start, _, text = find_module_paragraph(inline, "///", "b")
    assert start == 4 and MODULE_START.match(text).group(2) == "ecs.md"
    assert field("Game: a b. Tests: c. Missing: d.", "Game") == "a b"
    assert field("Game: a. Graphics: c. Missing: d e.", "Missing") == "d e"
    wrapped = wrap_doc("word " * 40, "//!", "    ")
    assert all(len(l) <= 100 and l.startswith("    //! ") for l in wrapped)
    print("maturity.py selftest: ok")
    return 0


if __name__ == "__main__":
    args = sys.argv[1:]
    if args == ["--selftest"]:
        raise SystemExit(selftest())
    if args not in ([], ["--write"], ["--list"]):
        print(__doc__)
        raise SystemExit(2)
    raise SystemExit(run(write=args == ["--write"], listing=args == ["--list"]))
