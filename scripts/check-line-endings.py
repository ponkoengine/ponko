"""Check that tracked text files in the working tree use LF."""

import os
from pathlib import Path
import subprocess


def main():
    root = Path(__file__).resolve().parent.parent
    entries = subprocess.check_output(
        ["git", "ls-files", "--eol", "-z"], cwd=root
    )
    failures = []
    for entry in entries.split(b"\0"):
        if not entry:
            continue
        metadata, path = entry.split(b"\t", 1)
        _, worktree_eol, attributes = metadata.split(maxsplit=2)
        if attributes.split()[0] == b"attr/-text":
            continue
        if worktree_eol in (b"w/crlf", b"w/mixed"):
            failures.append(os.fsdecode(path))

    if failures:
        print("Tracked text files with CRLF line endings:")
        for path in failures:
            print(f"  {path!r}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
