"""The public subset of a local check report."""

import datetime
import html
import json
from pathlib import Path
import re
import tempfile

OS_FILES = {"macOS": "macos", "Windows": "windows", "Linux": "linux"}


def public_status(report):
    if report["schema_version"] != 1:
        raise ValueError("unsupported check report schema")
    system = report["os"]
    if system not in OS_FILES:
        raise ValueError(f"unsupported report OS: {system}")
    revision = report["revision"]
    if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", revision):
        raise ValueError("report must name a Git revision")
    when = datetime.datetime.fromisoformat(report["finished_at"])
    if when.utcoffset() is None:
        raise ValueError("report completion time must include a timezone")
    counts = {"passed": 0, "failed": 0, "skipped": 0}
    failed_steps = []
    for job in report["jobs"]:
        for step in job["steps"]:
            code = step["returncode"]
            if code is None:
                counts["skipped"] += 1
            elif type(code) is not int:
                raise ValueError("step returncode must be an integer or null")
            elif code == 0:
                counts["passed"] += 1
            else:
                counts["failed"] += 1
                failed_steps.append(f'{job["name"]} / {step["name"]}')
    code = 1 if counts["failed"] else (0 if counts["passed"] else 2)
    if report["exit_code"] != code:
        raise ValueError("report exit code does not match its step results")
    return {
        "schema_version": 1,
        "os": system,
        "revision": revision,
        "finished_at": when.astimezone(datetime.timezone.utc).isoformat(timespec="seconds"),
        "status": ("passing", "failing", "incomplete")[code],
        "counts": counts,
        "failed_steps": failed_steps,
    }


def markdown_text(value):
    single_line = " ".join(value.splitlines())
    return re.sub(r"([\\|`*_\[\]@])", r"\\\1", html.escape(single_line, quote=False))


def status_markdown(status):
    counts = status["counts"]
    lines = [
        "| OS | Finished (UTC) | Revision | Status | Passed | Failed | Skipped |",
        "| --- | --- | --- | --- | ---: | ---: | ---: |",
        f'| {status["os"]} | {status["finished_at"]} | `{status["revision"]}` | {status["status"]} | '
        f'{counts["passed"]} | {counts["failed"]} | {counts["skipped"]} |',
    ]
    if status["failed_steps"]:
        lines += ["", "Failed steps", ""]
        lines += [f"- {markdown_text(name)}" for name in status["failed_steps"]]
    return "\n".join(lines) + "\n"


def write_text_atomic(path, content):
    handle = tempfile.NamedTemporaryFile(mode="w", dir=path.parent, encoding="utf-8", delete=False)
    temporary = Path(handle.name)
    try:
        with handle:
            handle.write(content)
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def publish_status(report, directory):
    status = public_status(report)
    directory.mkdir(parents=True, exist_ok=True)
    slug = OS_FILES[status["os"]]
    write_text_atomic(directory / f"{slug}.json", json.dumps(status, indent=2) + "\n")
    write_text_atomic(directory / f"{slug}.md", status_markdown(status))
