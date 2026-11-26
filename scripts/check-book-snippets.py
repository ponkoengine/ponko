#!/usr/bin/env python3
"""Compile the guide's inline Rust snippets, so a rename can't silently break them.

Every ```rust block in book/src and in the top-level README.md (the crates.io
page) is extracted into one scratch crate (one module per block, one file per
chapter) and checked with a single `cargo check`.

How a block is compiled (the same idea as rustdoc, kept deliberately small):

- Lines starting with `# ` (or a lone `#`) are mdBook's hidden lines: the book
  hides them, the checker keeps them (minus the `# `). Use them for context the
  reader doesn't need: `# struct Health(f32);`, `# let steer = Vec2::ZERO;`.
  `##` at the start of a line stands for a literal `#`.
- `use ponko::prelude::*;` is in scope, and the body sits in
  `fn snippet(ctx: &mut Ctx, world: &mut World) -> Result<(), Box<dyn Error>>`,
  so snippets can use `ctx`, `world` and `?` without setup. A block that
  defines its own `fn main` is used as it is, without the wrapper.
- `no_run` and `should_panic` blocks are compiled too (nothing is ever run).
- A block that is only an `{{#include ...}}` is skipped: the included file
  is compiled by `cargo build --examples` / `cargo test`.
- `ignore` is the escape hatch (at most MAX_IGNORED blocks, so use it rarely):
  the block must say why in a comment line, `// ignore: <reason>` (put it on
  a hidden line, `# // ignore: needs a GPU`, to keep it out of the book).
- Warnings such as unused variables are allowed; deprecated items are not.
  The checker also rejects the root glob `use ponko::*;` (games start from
  `use ponko::prelude::*;` and module imports), in snippets and in inline
  code in the prose (except the naming and migration chapters, which
  explain it). The blocks of `migrating-to-*.md` are not compiled: they show
  code from before the change.

Every `use ponko::...;` line in a Rust block (and in an `{{#include}}`d
region) must name a path that exists in the API snapshot (`api/ponko/*.txt`,
see scripts/api-snapshots.py), so a guide can't tell readers to import a path
that is not there (`use ponko::d2::LevelObject;` when the type lives in
`ponko::tilemap`). A glob (`use ponko::d3::*;`) needs its module to exist.

Generated files keep the markdown's line numbers, so a compiler error at
`src/physics.rs:40` is `book/src/physics.md:40`.

Usage: scripts/check-book-snippets.py [--features a,b] [--no-compile] [--keep] [--selftest]
"""

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BOOK = ROOT / "book" / "src"
MAX_IGNORED = 3
# Everything except the mutually exclusive or tooling-only features.
DEFAULT_FEATURES = "3d,physics,physics3d,navmesh,steam,video,lobby,webrtc,mods-wasm"

FENCE = re.compile(r"^(\s*)(`{3,}|~{3,})\s*([^`]*)$")
IGNORE_REASON = re.compile(r"//\s*ignore:\s*\S")
LOCAL_ATTRS = ("no_run", "should_panic", "ignore", "compile_fail")


def extract_blocks(path):
    """Yield (open_line, close_line, info_words, code_lines, label_line) per fenced block.

    Lines are 1-based; `label_line` is the last non-blank line above the fence,
    stripped (`After:` in a changelog fragment, `**After:**` in a generated
    migration chapter)."""
    lines = path.read_text(encoding='utf-8').split("\n")
    i = 0
    while i < len(lines):
        m = FENCE.match(lines[i])
        if not m:
            i += 1
            continue
        indent, fence, info = m.groups()
        close = re.compile(r"^\s*" + fence[0] + "{" + str(len(fence)) + r",}\s*$")
        j = i + 1
        while j < len(lines) and not close.match(lines[j]):
            j += 1
        if j >= len(lines):
            sys.exit(f"{path}:{i + 1}: unterminated code fence")
        attrs = [a for a in re.split(r"[,\s]+", info.strip()) if a]
        body = [l[len(indent):] if l.startswith(indent) else l.lstrip() for l in lines[i + 1 : j]]
        above = next((l.strip() for l in reversed(lines[:i]) if l.strip()), "")
        yield i + 1, j + 1, attrs, body, above
        i = j + 1


def extract(path):
    """Yield (open_line, close_line, attrs, code_lines) per Rust block (1-based lines)."""
    for start, end, attrs, body, _ in extract_blocks(path):
        if attrs and attrs[0] == "rust":
            yield start, end, attrs[1:], body


def unhide(line):
    """mdBook hidden lines: `# code` -> `code`, `#` -> ``, `## x` -> `# x`."""
    if line == "#":
        return ""
    if line.startswith("# "):
        return line[2:]
    if line.startswith("##"):
        return line[1:]
    return line


NAMING = BOOK / "naming.md"
# Chapters whose prose names the old spellings to explain them.
EXPLAINS_OLD_NAMES = {NAMING, BOOK / "migrating-to-0.1.md"}
# The README is the crates.io page: its snippets are checked like a chapter's.
README = ROOT / "README.md"


def chapters():
    """The guide's chapters: book/src/*.md, and the pages of a folder of chapters (the cookbook)."""
    return [*sorted(BOOK.glob("*.md")), *sorted(BOOK.glob("*/*.md"))]


def chapter_stem(md):
    """A name for the chapter's generated file: `physics`, or `cookbook-physics` for a page in a folder."""
    if md.parent.name == "changelog.d":
        return "changelog-" + md.stem  # a changelog fragment
    if BOOK not in md.parents:
        return md.stem  # README.md
    return "-".join(md.relative_to(BOOK).with_suffix("").parts)


def label(md):
    """A markdown file's path as errors name it (`book/src/x.md`, `README.md`)."""
    return md.relative_to(ROOT).as_posix()


ROOT_PATH = re.compile(r"(?<![\w:])(?:::)?ponko::(\w+|\*|\{)")


def root_path_names(code):
    """Top-level names a piece of code reaches through the crate root (`*` for the root glob)."""
    found = []
    for m in ROOT_PATH.finditer(code):
        head = m.group(1)
        if head != "{":
            found.append(head)
            continue
        depth, i, start = 1, m.end(), m.end()
        while i < len(code) and depth:
            depth += {"{": 1, "}": -1}.get(code[i], 0)
            i += 1
        inner, depth, part = code[start : i - 1], 0, ""
        parts = []
        for ch in inner + ",":
            if ch == "," and depth == 0:
                parts.append(part.strip())
                part = ""
                continue
            depth += {"{": 1, "}": -1}.get(ch, 0)
            part += ch
        found.extend(re.match(r"(\w+|\*)", p).group(1) for p in parts if re.match(r"(\w+|\*)", p))
    return found


def old_spellings(code):
    """The root glob, if `code` uses it."""
    return [
        "use ponko::*; (use ponko::prelude::*; and module imports)" for name in root_path_names(code) if name == "*"
    ]


def check_prose(md):
    """Inline code in the prose doesn't use the root glob."""
    problems = []
    in_fence = None
    for n, line in enumerate(md.read_text(encoding='utf-8').split("\n"), 1):
        m = FENCE.match(line)
        if m:
            in_fence = None if in_fence else m.group(2)[0]
            continue
        if in_fence:
            continue
        for span in re.findall(r"`([^`]+)`", line):
            for bad in old_spellings(span):
                problems.append(f"{label(md)}:{n}: `{span}` uses {bad}")
    return problems


API_DIR = ROOT / "api" / "ponko"
USE_STMT = re.compile(r"\b(?:pub\s+)?use\s+(?:::)?ponko::([^;]*);")
ITEM_PATH = re.compile(r"\bponko(?:::\w+)*")
INCLUDE = re.compile(r"\{\{#(?:rustdoc_)?include\s+([^}\s:]+)((?::[\w.-]*)*)\s*\}\}")
ANCHOR_LINE = re.compile(r"\bANCHOR(?:_END)?:\s*\S+")


def load_api_paths():
    """(names, prefixes): every item path in the API snapshot, and the paths below it that the snapshot can't list.

    Those are the contents of a glob re-export and of a re-exported third-party
    crate (`pub use ponko::reflect::json`, which is `serde_json`): the snapshot
    names the re-export, not what is in the other crate.
    """
    names, prefixes = set(), []
    for listing in sorted(API_DIR.glob("*.txt")):
        for line in listing.read_text(encoding='utf-8').split("\n"):
            if not line or line.startswith("impl "):
                continue
            m = ITEM_PATH.search(line)
            if not m:
                continue
            if "<<" in line:
                prefixes.append(m.group(0) + "::")
            elif line.startswith("pub use "):
                names.add(m.group(0))
                prefixes.append(m.group(0) + "::")
            else:
                names.add(m.group(0))
    return names, prefixes


def expand_use_tree(tree):
    """`{a, b::{c, d}}` after `ponko::` -> ['a', 'b::c', 'b::d'] (`as` renames and `self` dropped)."""
    tree = tree.strip()
    brace = tree.find("{")
    if brace < 0:
        path = re.sub(r"\s+as\s+\w+$", "", tree.strip())
        return [path.strip()]
    prefix, depth, part, parts = tree[:brace], 0, "", []
    inner = tree[brace + 1 : tree.rindex("}")]
    for ch in inner + ",":
        if ch == "," and depth == 0:
            if part.strip():
                parts.append(part.strip())
            part = ""
            continue
        depth += {"{": 1, "}": -1}.get(ch, 0)
        part += ch
    out = []
    for p in parts:
        for sub in expand_use_tree(p):
            out.append(prefix + sub if sub != "self" else prefix.rstrip(":"))
    return out


def check_use_paths(code, where, api):
    """Problems for the `use ponko::...;` statements in `code`; `where` is `file:line` of its first line."""
    names, prefixes = api
    problems = []
    path, _, first = where.rpartition(":")
    for m in USE_STMT.finditer(code):
        line = int(first) + code.count("\n", 0, m.start())
        for rel in expand_use_tree(m.group(1)):
            full = "ponko" + ("::" + rel if rel else "")
            module = full[:-3] if full.endswith("::*") else full
            if module in names or any((module + "::").startswith(p) for p in prefixes):
                continue
            problems.append(f"{path}:{line}: `use {full};` names a path that is not in the API snapshot (api/ponko/*.txt)")
    return problems


def included_text(md, line):
    """The text a `{{#include path:anchor}}` line pulls in, or None if it isn't a readable include."""
    m = INCLUDE.search(line)
    if not m:
        return None
    target = (md.parent / m.group(1)).resolve()
    if not target.is_file():
        return None
    lines = target.read_text(encoding='utf-8').split("\n")
    spec = [x for x in m.group(2).split(":")[1:]]
    if len(spec) == 1 and not re.fullmatch(r"\d*", spec[0]):
        name, region, on = spec[0], [], False
        for l in lines:
            if re.search(rf"\bANCHOR_END:\s*{re.escape(name)}\s*$", l):
                on = False
            elif re.search(rf"\bANCHOR:\s*{re.escape(name)}\s*$", l):
                on = True
            elif on and not ANCHOR_LINE.search(l):
                region.append(l)
        return "\n".join(region)
    if spec:
        start = int(spec[0]) if spec[0] else 1
        end = int(spec[1]) if len(spec) > 1 and spec[1] else len(lines)
        return "\n".join(lines[start - 1 : end])
    return "\n".join(lines)


CHANGELOG_DIR = ROOT / "changelog.d"
# What changelog.py writes into a migration chapter it generated, and the
# labels above the code a breaking entry's reader ends up with.
GENERATED_CHAPTER = "<!-- changelog.py: entries start -->"
AFTER_LABELS = ("After:", "**After:**")
BREAKING_HEADINGS = ("### Breaking changes", "### Breaking")
# Languages an `After:` block may be written in, and how each is checked.
AFTER_LANGUAGES = ("rust", "c", "cpp", "wgsl", "toml")
# The engine's shader bindings a WGSL `After:` block may read (the material
# and screen-effect prelude), declared after the block as the engine does.
WGSL_PRELUDE = ROOT / "src" / "render" / "mesh.wgsl"
WGSL_BINDING = re.compile(r"^@group\(\d+\)\s*@binding\(\d+\)\s*var\s+\w+\s*:\s*(?:texture_|sampler)[^;]*;")
RUST_WRAPPER = "#![allow(warnings)] #![deny(deprecated)]"


def fragments():
    """The changelog fragments waiting for the release (changelog.d/*.md)."""
    return sorted(p for p in CHANGELOG_DIR.glob("*.md") if p.name != "README.md")


def after_blocks(md):
    """The `After:` blocks a reader of the migration chapter copies: (start, end, lang, attrs, body).

    In a changelog fragment those of its `### Breaking changes` sections; in a
    migration chapter that changelog.py generated, every `**After:**` block.
    A hand-written chapter (migrating-to-0.1.md) has none."""
    text = md.read_text(encoding='utf-8')
    fragment = md.parent == CHANGELOG_DIR
    if not fragment and GENERATED_CHAPTER not in text:
        return []
    lines = text.split("\n")
    found = []
    for start, end, attrs, body, above in extract_blocks(md):
        if above not in AFTER_LABELS:
            continue
        if fragment:
            heading = next((l.strip() for l in reversed(lines[: start - 1]) if l.startswith("### ")), "")
            if heading not in BREAKING_HEADINGS:
                continue
        lang = attrs[0] if attrs else ""
        found.append((start, end, lang, attrs[1:], body))
    return found


def rust_module(start, body, out):
    """Append one Rust block to `out` as a module whose code keeps the markdown's line numbers."""
    code = [unhide(l) for l in body]
    has_main = any(re.search(r"\bfn\s+main\s*\(", l) for l in code)
    if has_main:
        header = f"mod b{start} {{ {RUST_WRAPPER} use ponko::prelude::*;"
        footer = "}"
    else:
        header = (
            f"mod b{start} {{ {RUST_WRAPPER} use ponko::prelude::*; "
            "fn snippet(ctx: &mut Ctx, world: &mut World) -> Result<(), Box<dyn std::error::Error>> {"
        )
        footer = ";Ok(()) } }"
    # The header sits on the opening fence's line, the footer on the closing one,
    # so every code line keeps its markdown line number.
    out.extend([""] * (start - 1 - len(out)))
    out.append(header)
    out.extend(code)
    out.append(footer)


def native_source(lang, md, start, body):
    """A C or C++ `After:` block as a translation unit; `#line` keeps the markdown's file and line.

    The block is the body of a function given the context (`PonkoCtx *ctx` in C,
    `ponko::ctx& ctx` in C++), the way a game's callback gets it."""
    if lang == "c":
        head = ['#include "ponko.h"', "#include <stddef.h>", "#include <stdint.h>", "void snippet(PonkoCtx *ctx) {"]
    else:
        head = ['#include "ponko.hpp"', "#include <vector>", "#include <string>", "void snippet(ponko::ctx& ctx) {"]
    return "\n".join([*head, f'#line {start + 1} "{label(md)}"', *body, "}", ""])


def wgsl_source(start, body):
    """A WGSL `After:` block at its markdown line, then the engine's texture and sampler bindings."""
    prelude = [l.strip() for l in WGSL_PRELUDE.read_text(encoding='utf-8').split("\n") if WGSL_BINDING.match(l.strip())]
    return "\n".join([*[""] * start, *body, "", "// The engine's bindings (src/render/mesh.wgsl):", *prelude, ""])


def generate(out_src, features, changelog_only=False):
    """Write src/<chapter>.rs for every chapter with blocks.

    Returns (checked, skipped, ignored, problems, others): `others` holds the
    `After:` blocks in other languages as (lang, file label, source text)."""
    modules = []
    checked = skipped = 0
    ignored = []
    problems = []
    others = []
    api = load_api_paths()
    sources = [] if changelog_only else [*chapters(), README]
    for md in [*sources, *fragments()]:
        # A migration chapter shows the old code, which no longer compiles
        # (changelog.py writes them from the `Before:` and `After:` snippets):
        # only its `After:` blocks are compiled, as in the fragments they came from.
        after_only = md.name.startswith("migrating-to-") or md.parent == CHANGELOG_DIR
        if md not in EXPLAINS_OLD_NAMES and not after_only:
            problems.extend(check_prose(md))
        out = []  # generated lines; len(out) is the line number reached
        if after_only:
            for start, end, lang, attrs, body in after_blocks(md):
                rel = f"{label(md)}:{start}"
                if lang not in AFTER_LANGUAGES:
                    problems.append(f"{rel}: an `After:` block in `{lang or 'no language'}` can't be checked; "
                                    f"write it in one of {', '.join(AFTER_LANGUAGES)}")
                    continue
                if attrs:
                    problems.append(f"{rel}: an `After:` block takes no attributes (`{' '.join(attrs)}`): "
                                    "it is what the reader writes, so it must compile")
                    continue
                if lang == "rust":
                    shown = "\n".join(unhide(l) for l in body)
                    problems.extend(check_use_paths(shown, f"{label(md)}:{start + 1}", api))
                    for bad in sorted(set(old_spellings(shown))):
                        problems.append(f"{rel}: uses {bad}; see the naming chapter")
                    rust_module(start, body, out)
                    checked += 1
                    modules.append((chapter_stem(md), md.name))
                elif lang == "toml":
                    import tomllib
                    try:
                        tomllib.loads("\n".join(body))
                    except tomllib.TOMLDecodeError as error:
                        problems.append(f"{rel}: the TOML doesn't parse: {error}")
                    checked += 1
                elif lang == "wgsl":
                    others.append((lang, rel, wgsl_source(start, body)))
                    checked += 1
                else:
                    others.append((lang, rel, native_source(lang, md, start, body)))
                    checked += 1
            if out:
                (out_src / f"{chapter_stem(md)}.rs").write_text("\n".join(out) + "\n", encoding='utf-8', newline='\n')
            continue
        for start, end, attrs, body in extract(md):
            rel = f"{label(md)}:{start}"
            # The `use ponko::` lines, as written or as pulled in from an example's source.
            shown = "\n".join(unhide(l) for l in body)
            problems.extend(check_use_paths(shown, f"{label(md)}:{start + 1}", api))
            for offset, l in enumerate(body):
                pulled = included_text(md, l)
                if pulled:
                    problems.extend(check_use_paths(pulled, f"{label(md)}:{start + 1 + offset}", api))
            if "ignore" in attrs:
                ignored.append(rel)
                if not any(IGNORE_REASON.search(l) for l in body):
                    problems.append(f"{rel}: `ignore` block without a `// ignore: <reason>` comment")
                continue
            if "compile_fail" in attrs:
                problems.append(f"{rel}: `compile_fail` blocks aren't supported; remove the attribute or use `ignore`")
                continue
            if any(l.lstrip().startswith("{{#") for l in body):
                skipped += 1
                continue
            code = [unhide(l) for l in body]
            for bad in sorted(set(old_spellings("\n".join(code)))):
                problems.append(f"{rel}: uses {bad}; see the naming chapter")
            rust_module(start, body, out)
            checked += 1
            modules.append((chapter_stem(md), md.name))
        if out:
            (out_src / f"{chapter_stem(md)}.rs").write_text("\n".join(out) + "\n", encoding='utf-8', newline='\n')
    if len(ignored) > MAX_IGNORED:
        problems.append(f"{len(ignored)} `ignore` blocks ({', '.join(ignored)}); at most {MAX_IGNORED} are allowed")
    stems = sorted({stem for stem, _ in modules})
    lib = ["// Generated by scripts/check-book-snippets.py; do not edit."]
    for stem in stems:
        ident = re.sub(r"\W", "_", stem).lower()
        lib.append(f'#[path = "{stem}.rs"] mod c_{ident};')
    (out_src / "lib.rs").write_text("\n".join(lib) + "\n", encoding='utf-8', newline='\n')
    return checked, skipped, ignored, problems, others


def selftest():
    """The `use ponko::` check, with made-up input against the real snapshot."""
    api = load_api_paths()
    good = "use ponko::prelude::*;\nuse ponko::d3::*;\nuse ponko::render::{MaterialDesc, Mesh, MeshData};\nuse ponko::tilemap::LevelObject;"
    assert check_use_paths(good, "x.md:1", api) == []
    for bad in ("use ponko::d2::LevelObject;", "use ponko::d3::Model;", "use ponko::d9::*;", "use ponko::ui::{UiLayout, Nope};"):
        found = check_use_paths(bad, "x.md:7", api)
        assert len(found) == 1 and found[0].startswith("x.md:7:"), (bad, found)
    assert expand_use_tree("a::{b, c::{d, self}}") == ["a::b", "a::c::d", "a::c"]
    assert check_use_paths("use ponko::reflect::json::json;", "x.md:1", api) == [], "re-exported crates are not listed"
    print("check-book-snippets.py: self-test passed")
    return 0


WGSL_BUILD_RS = """// Generated by scripts/check-book-snippets.py; do not edit.
// Parses and validates each WGSL `After:` block with naga, the shader
// compiler wgpu uses; a failure fails `cargo check`.
fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("wgsl");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut failed = Vec::new();
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    for path in paths {
        let source = std::fs::read_to_string(&path).unwrap();
        let (name, source) = source.split_once('\\n').unwrap();
        let name = name.trim_start_matches("// ");
        match naga::front::wgsl::parse_str(source) {
            Err(e) => failed.push(e.emit_to_string_with_path(source, name)),
            Ok(module) => {
                let mut validator = naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::all(),
                );
                if let Err(e) = validator.validate(&module) {
                    failed.push(e.emit_to_string_with_path(source, name));
                }
            }
        }
    }
    if !failed.is_empty() {
        panic!("WGSL `After:` blocks that don't compile:\\n{}", failed.join("\\n"));
    }
}
"""


def workspace_version(name):
    """The version requirement the workspace's Cargo.toml gives a dependency (`naga`)."""
    m = re.search(rf'^{name}\s*=\s*\{{[^}}\n]*version\s*=\s*"([^"]+)"', (ROOT / "Cargo.toml").read_text(encoding='utf-8'), re.M)
    return m.group(1) if m else "*"


def compile_native(crate, others):
    """Check the C and C++ `After:` blocks against include/ with the system compiler. Returns error text."""
    folder = crate / "native"
    folder.mkdir(exist_ok=True)
    for old in folder.iterdir():
        old.unlink()
    errors = []
    for n, (lang, rel, source) in enumerate(o for o in others if o[0] in ("c", "cpp")):
        ext, compiler, std = (".c", os.environ.get("CC", "cc"), "-std=c11") if lang == "c" else \
            (".cpp", os.environ.get("CXX", "c++"), "-std=c++17")
        path = folder / f"after{n}{ext}"
        path.write_text(source, encoding='utf-8', newline='\n')
        cmd = [compiler, std, "-fsyntax-only", "-I", str(ROOT / "include"), str(path)]
        try:
            result = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, encoding='utf-8')
        except FileNotFoundError:
            errors.append(f"{rel}: no {lang} compiler (`{compiler}`; set {'CC' if lang == 'c' else 'CXX'})")
            continue
        if result.returncode != 0:
            errors.append(result.stderr.rstrip())
    return "\n".join(errors)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--features", default=DEFAULT_FEATURES, help="ponko features to enable (default: %(default)s)")
    ap.add_argument("--no-compile", action="store_true", help="only extract and validate the blocks")
    ap.add_argument("--keep", action="store_true", help="keep the scratch crate's files (always kept under target/)")
    ap.add_argument("--changelog-only", action="store_true",
                    help="only the `After:` blocks of the changelog fragments (what `changelog.py --check --compile` runs)")
    ap.add_argument("--selftest", action="store_true", help="check the `use ponko::` path check with made-up input")
    args = ap.parse_args()
    if args.selftest:
        return selftest()

    target = Path(os.environ.get("CARGO_TARGET_DIR") or ROOT / "target")
    crate = target / "book-snippets"
    src = crate / "src"
    src.mkdir(parents=True, exist_ok=True)
    for old in src.glob("*.rs"):
        old.unlink()

    checked, skipped, ignored, problems, others = generate(src, args.features, args.changelog_only)
    features = ", ".join(f'"{f}"' for f in args.features.split(",") if f)
    generated = "\n".join(p.read_text(encoding='utf-8') for p in src.glob("*.rs"))
    # The other crates of the workspace a block names (a breaking entry about
    # the C library's Rust enums, the image comparison crate) are added only
    # then, so the guide's check builds what it always did.
    extra = "".join(
        f'{name} = {{ path = "{ROOT.as_posix()}/crates/{name}" }}\n'
        for name in ("ponko_ffi", "ponko_image_diff", "ponko_ecs")
        if re.search(rf"\b{name}\b", generated)
    )
    wgsl = [o for o in others if o[0] == "wgsl"]
    build = ""
    shaders = crate / "wgsl"
    shaders.mkdir(exist_ok=True)
    for old in shaders.iterdir():
        old.unlink()
    if wgsl:
        build = f'\n[build-dependencies]\nnaga = {{ version = "{workspace_version("naga")}", features = ["wgsl-in"] }}\n'
        for n, (_, rel, source) in enumerate(wgsl):
            (shaders / f"after{n}.wgsl").write_text(f"// {rel.rpartition(':')[0]}\n{source}", encoding='utf-8', newline='\n')
        (crate / "build.rs").write_text(WGSL_BUILD_RS, encoding='utf-8', newline='\n')
    elif (crate / "build.rs").exists():
        (crate / "build.rs").unlink()
    (crate / "Cargo.toml").write_text(
        f"""# Generated by scripts/check-book-snippets.py; do not edit.
[package]
name = "book_snippets"
version = "0.0.0"
edition = "2024"
publish = false

[lib]
path = "src/lib.rs"

[dependencies]
ponko = {{ path = "{ROOT.as_posix()}", features = [{features}] }}
ponko_unity = {{ path = "{ROOT.as_posix()}/crates/ponko_unity" }}
{extra}serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
{build}
[workspace]
""", encoding='utf-8', newline='\n'
    )
    # The same dependency versions as the workspace, so the build cache is shared.
    lock = ROOT / "Cargo.lock"
    if lock.exists():
        (crate / "Cargo.lock").write_text(lock.read_text(encoding='utf-8'), encoding='utf-8', newline='\n')

    what = "changelog `After:` blocks" if args.changelog_only else "guide snippets"
    print(f"{what}: {checked} to compile, {skipped} included from real files, {len(ignored)} ignored")
    for p in problems:
        print(f"error: {p}", file=sys.stderr)
    if problems:
        return 1
    if args.no_compile:
        return 0

    native = compile_native(crate, others)
    cmd = ["cargo", "check", "--manifest-path", str(crate / "Cargo.toml"), "--target-dir", str(target), "--color", "never"]
    result = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, encoding='utf-8')
    if result.returncode == 0 and not native:
        sys.stderr.write(result.stderr.splitlines()[-1] + "\n" if result.stderr else "")
        print(f"all {what} compile")
        return 0
    if result.returncode != 0:
        # Point errors at the markdown: src/physics.rs:40 is book/src/physics.md:40,
        # src/changelog-gl10-x.rs:12 is changelog.d/gl10-x.md:12.
        names = {chapter_stem(md): label(md) for md in [*chapters(), README, *fragments()]}
        text = re.sub(r"(--> )(?:\S*book-snippets/)?src/([\w.-]+)\.rs:",
                      lambda m: f"{m.group(1)}{names.get(m.group(2), 'book/src/' + m.group(2) + '.md')}:", result.stderr)
        sys.stderr.write(text)
    if native:
        sys.stderr.write(native + "\n")
    print(f"\nerror: some {what} don't compile (the paths above are the markdown files and lines)", file=sys.stderr)
    print("Fix the snippet, or add hidden context lines (`# let x = ...;`); see book/src/contributing.md.", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
