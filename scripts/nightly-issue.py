#!/usr/bin/env python3
"""Reconcile one open nightly failure issue per OS from a local checks report."""

import argparse
import json
from pathlib import Path
import subprocess
import sys

from nightly_report import public_status, status_markdown

REPOSITORY = "ponkoengine/ponko"
ISSUE_LIMIT = 1000


def gh_issue(gh, args, body=None):
    return subprocess.run(
        [*gh, "issue", *args, "--repo", REPOSITORY],
        input=body, stdout=subprocess.PIPE, encoding="utf-8", check=True,
    ).stdout


def sync_issue(status, *, apply=False, gh=("gh",)):
    if status["status"] == "incomplete":
        return "No issue change: incomplete run (zero checks ran)."
    title = f'Nightly checks failing on {status["os"]}'
    # Read repository issues directly so a just-created issue is visible even
    # before GitHub's search index catches up.
    opened = json.loads(gh_issue(
        gh, ["list", "--state", "open", "--limit", str(ISSUE_LIMIT), "--json", "number,title"],
    ))
    if len(opened) >= ISSUE_LIMIT:
        raise ValueError("open issue list reached its limit; cannot establish a unique match")
    numbers = [str(item["number"]) for item in opened if item["title"] == title]
    if len(numbers) > 1:
        raise ValueError(f'multiple open issues titled "{title}": {", ".join(numbers)}')
    number = numbers[0] if numbers else None
    if status["status"] == "passing":
        if number is None:
            return f'No issue change: "{title}" is already clear.'
        action = f'close #{number} ("{title}")'
        args = ["close", number, "--comment", "Nightly checks recovered.\n\n" + status_markdown(status)]
        body = None
    else:
        body = status_markdown(status)
        if number is None:
            action = f'create "{title}"'
            args = ["create", "--title", title, "--body-file", "-"]
        else:
            action = f'edit #{number} ("{title}")'
            args = ["edit", number, "--body-file", "-"]
    if apply:
        output = gh_issue(gh, args, body).strip()
        return f"Applied: {action}" + (f"\n{output}" if output else "")
    return f"Dry run: would {action}."


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path, help="results.json from run-checks.py")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true", help="look up the issue and print the action (default)")
    mode.add_argument("--apply", action="store_true", help="create, update, or close the matching issue")
    args = parser.parse_args()
    try:
        status = public_status(json.loads(args.report.read_text(encoding="utf-8")))
        print(sync_issue(status, apply=args.apply))
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print(f"nightly-issue: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
