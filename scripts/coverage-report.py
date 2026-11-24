#!/usr/bin/env python3
"""Summarize a workspace `cargo llvm-cov --json` export as Markdown."""

import argparse
import importlib
import json
from collections import defaultdict
from functools import cache
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


@cache
def source_path(filename, root):
    path = (root / filename).resolve()
    if path.is_relative_to(root):
        return path.relative_to(root).as_posix()
    return None


def source_module(filename):
    if filename is None or not filename.endswith(".rs"):
        return None
    parts = filename.split("/")
    if parts[0] == "src":
        crate = "ponko"
        source = parts[1:]
    elif len(parts) >= 4 and parts[0] == "crates" and parts[2] == "src":
        crate = parts[1]
        source = parts[3:]
    else:
        return None
    if source[-1].endswith("_test.rs"):
        return None
    if source[0] in ("lib.rs", "main.rs"):
        return crate
    return f"{crate}::{Path(source[0]).stem}"


def module_lines(data, root):
    modules = defaultdict(lambda: (0, 0))
    for file in data["files"]:
        module = source_module(source_path(file["filename"], root))
        if module is None:
            continue
        lines = file["summary"]["lines"]
        if lines["count"]:
            covered, count = modules[module]
            modules[module] = covered + lines["covered"], count + lines["count"]
    return dict(modules)


def function_regions(data, root):
    functions = defaultdict(list)
    for function in data["functions"]:
        # LLVM exports the code region covering the whole function first.
        # Later regions can represent expansions, branches or gaps.
        region = next((region for region in function["regions"] if region[7] == 0), None)
        if region is None:
            continue
        start_line, start_col, end_line, end_col, count, file_index, _, _ = region
        filename = source_path(function["filenames"][file_index], root)
        if source_module(filename) is not None:
            functions[filename].append(((start_line, start_col), (end_line, end_col), count))
    return functions


def function_status(functions, filename, start, end):
    matched = False
    for region_start, region_end, count in functions.get(filename, ()):
        if start <= region_start <= region_end <= end:
            matched = True
            if count:
                return "covered"
    return "unexecuted" if matched else "unmapped"


def public_functions(doc):
    names = importlib.import_module("check-naming").public_paths(doc)
    index = doc["index"]
    root_name = index[str(doc["root"])]["name"]

    for key, paths in names.items():
        item = index.get(key)
        if item is None:
            continue
        # Prefer the defining public path to a prelude re-export.
        path = min(paths, key=lambda p: (p.startswith("prelude::"), p.count("::"), p))
        path = f"{root_name}::{path}"
        inner = item["inner"]
        if "function" in inner and inner["function"]["has_body"]:
            yield path, item["span"]
        for kind in ("struct", "enum", "union"):
            if kind not in inner:
                continue
            for impl_id in inner[kind]["impls"]:
                impl = index[str(impl_id)]["inner"]["impl"]
                if impl["trait"] is not None:
                    continue
                for method_id in impl["items"]:
                    method = index[str(method_id)]
                    if method["visibility"] == "public" and "function" in method["inner"]:
                        yield f"{path}::{method['name']}", method["span"]
        if "trait" in inner:
            for method_id in inner["trait"]["items"]:
                method = index[str(method_id)]
                function = method["inner"].get("function")
                if function is not None and function["has_body"]:
                    yield f"{path}::{method['name']}", method["span"]


def render_modules(modules):
    yield "| Module | Lines executed | Lines mapped | Coverage |"
    yield "|---|---:|---:|---:|"
    for name, (covered, total) in sorted(modules.items()):
        yield f"| `{name}` | {covered:,} | {total:,} | {covered / total:.1%} |"
    covered = sum(lines[0] for lines in modules.values())
    total = sum(lines[1] for lines in modules.values())
    if not total:
        raise ValueError("coverage export contains no mapped workspace source")
    yield f"| **Total** | **{covered:,}** | **{total:,}** | **{covered / total:.1%}** |"


def render_functions(data, documents, root):
    functions = function_regions(data, root)
    rows = set()
    for doc in documents:
        for name, span in public_functions(doc):
            if span is None:
                continue
            filename = source_path(span["filename"], root)
            if source_module(filename) is None:
                continue
            state = function_status(functions, filename, tuple(span["begin"]), tuple(span["end"]))
            if state != "covered":
                rows.add((state, name, filename, span["begin"][0]))

    yield "| Public function | Source | Coverage status |"
    yield "|---|---|---|"
    for state, name, filename, line in sorted(rows, key=lambda row: (row[1], row[2], row[3])):
        yield f"| `{name}` | `{filename}:{line}` | {state} |"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("export", type=Path, help="LLVM coverage JSON export")
    parser.add_argument("--rustdoc", type=Path, nargs="+", help="emit the API table using these rustdoc JSON files")
    args = parser.parse_args()
    export = json.loads(args.export.read_text(encoding="utf-8"))
    if export.get("type") != "llvm.coverage.json.export" or len(export.get("data", ())) != 1:
        parser.error("expected one merged LLVM coverage export")
    data = export["data"][0]
    if args.rustdoc:
        documents = (json.loads(file.read_text(encoding="utf-8")) for file in args.rustdoc)
        print("\n".join(render_functions(data, documents, ROOT)))
    else:
        print("\n".join(render_modules(module_lines(data, ROOT))))


if __name__ == "__main__":
    main()
