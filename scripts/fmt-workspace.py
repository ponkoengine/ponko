#!/usr/bin/env python3
"""Format every workspace target with rustfmt, in batches.

`cargo fmt --all` passes the root file of every target in the workspace to
one `rustfmt` command. The workspace has over 500 targets, so on Windows that
command line comes close to the 32,767-character limit, and in a checkout
whose path is a few characters longer it passes it: "The filename or
extension is too long (os error 206)". This script runs the same rustfmt
calls with at most a few thousand characters of paths each.

    python3 scripts/fmt-workspace.py --check   # what `cargo fmt --all --check` checks
    python3 scripts/fmt-workspace.py           # format in place

Any other arguments are passed to rustfmt.
"""
import json
import subprocess
import sys
from collections import defaultdict

# Far below Windows' limit, so a long checkout path never matters.
BATCH_CHARS = 8000


def targets():
    """Each workspace target's root file, grouped by the edition it uses."""
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        check=True, capture_output=True, encoding="utf-8",
    ).stdout
    by_edition = defaultdict(list)
    for package in json.loads(out)["packages"]:
        for target in package["targets"]:
            edition = target.get("edition") or package["edition"]
            path = target["src_path"]
            if path not in by_edition[edition]:
                by_edition[edition].append(path)
    return by_edition


def batches(paths, limit=BATCH_CHARS):
    """`paths` split into lists whose joined length stays within `limit`."""
    batch, size = [], 0
    for path in paths:
        if batch and size + len(path) + 1 > limit:
            yield batch
            batch, size = [], 0
        batch.append(path)
        size += len(path) + 1
    if batch:
        yield batch


def main(args):
    failed = False
    for edition, paths in sorted(targets().items()):
        for batch in batches(paths):
            command = ["rustfmt", "--edition", edition, *args, *batch]
            failed |= subprocess.run(command).returncode != 0
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
