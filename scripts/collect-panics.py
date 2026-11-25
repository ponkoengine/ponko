#!/usr/bin/env python3
"""Audit panic candidates in engine/ECS, including private callees.

Conservative lexical inventory, not a call-graph proof. --all includes tests.
The reviewed decisions in panic-audit.json mark internal invariants explicitly.
--check rejects new/unreviewed sites; line numbers may move without invalidating
a review. Run after edits and review each new site rather than blessing it:
give it a message that names what the caller did and the fix, turn it into an
`Error`, or record it as an internal invariant:

    python3 scripts/collect-panics.py --record src/x.rs:12 invariant "set just above"

Classifications: invariant (internal, a caller can't reach it), caller-contract
(the message names the misuse and the fix), forwarded-error (re-raises an
`Error`'s own message), non-panic-method (a method merely named like one).
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent
LITERAL = re.compile(
    r"""//[^\n]*|/\*.*?\*/|r(?P<hash>\#*)".*?"(?P=hash)|'(?:\\.|[^'\\])'|"(?:\\.|[^"\\])*" """.rstrip(),
    re.S,
)
SITE = re.compile(r"\b(?:panic|assert|assert_eq|assert_ne|debug_assert|debug_assert_eq|debug_assert_ne)\s*!|\.\s*(?:unwrap|expect)\s*\(")


def blank(text):
    return "".join("\n" if c == "\n" else " " for c in text)


def code_only(text):
    return LITERAL.sub(lambda m: blank(m.group()), text)


def closing(code, opening):
    depth = 0
    for i in range(opening, len(code)):
        if code[i] in "({[":
            depth += 1
        elif code[i] in ")}]":
            depth -= 1
            if depth == 0:
                return i + 1
    return len(code)


def production(text):
    code = code_only(text)
    for match in reversed(list(re.finditer(r"#\[cfg\((?:test|all\([^\n]*\btest\b[^\n]*)\)\]", text))):
        start = match.end()
        following = text[start:].lstrip()
        if re.match(r"\w+\s*:", following):
            end = text.find("\n", start + len(text[start:]) - len(following))
        else:
            opening = code.find("{", start)
            semi = code.find(";", start)
            end = semi + 1 if semi >= 0 and (opening < 0 or semi < opening) else closing(code, opening)
        text = text[:match.start()] + blank(text[match.start():end]) + text[end:]
    return text


def collect(include_tests=False):
    for base in ("src", "crates/ponko_ecs/src"):
        for path in sorted((ROOT / base).rglob("*.rs")):
            text = path.read_text(encoding="utf-8")
            if not include_tests:
                if re.search(r"(?:_test|_tests|/tests|_conformance|_bench|/fuzz|/scripted|render_test_support|regression_stage)\.rs$", path.as_posix()):
                    continue
                text = production(text)
            code = code_only(text)
            for match in SITE.finditer(code):
                opening = match.end() - 1 if match.group().endswith("(") else re.search(r"[({\[]", code[match.end():]).start() + match.end()
                end = closing(code, opening)
                construction = " ".join(text[match.start():end].split())
                name = path.relative_to(ROOT).as_posix()
                prefix = code[:match.start()]
                functions = list(re.finditer(r"\bfn\s+(\w+)", prefix))
                function = functions[-1].group(1) if functions else "module"
                context = " ".join(text[max(0, match.start()-100):match.start()].split())
                key = name + ":" + hashlib.sha256((function + context + construction).encode()).hexdigest()[:16]
                line = text.count("\n", 0, match.start()) + 1
                yield key, f"{name}:{line}", construction


CLASSIFICATIONS = ("invariant", "caller-contract", "forwarded-error", "non-panic-method")


def record(audit, decisions):
    """Writes reviewed decisions for `path:line` sites; drops entries for sites that are gone."""
    found = list(collect())
    # One line can hold several sites; recording names the line, so it covers all of them.
    sites = {}
    for key, site, construction in found:
        sites.setdefault(site, []).append((key, construction))
    for site, classification, reason in decisions:
        if classification not in CLASSIFICATIONS:
            raise SystemExit(f"{classification!r} is not one of {', '.join(CLASSIFICATIONS)}")
        if site not in sites:
            raise SystemExit(f"no panic site at {site}; run without --record to list them")
        for key, construction in sites[site]:
            audit[key] = {"classification": classification, "construction": construction, "reason": reason, "site": site}
    live = {key for key, _, _ in found}
    return {key: audit[key] for key in sorted(audit) if key in live}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--all", action="store_true", help="include test code")
    parser.add_argument("--check", action="store_true", help="fail when a site has no reviewed decision")
    parser.add_argument("--record", nargs=3, action="append", metavar=("SITE", "CLASS", "REASON"),
                        help="record a reviewed decision for the site at path:line (repeatable)")
    args = parser.parse_args()
    audit_path = ROOT / "scripts/panic-audit.json"
    audit = json.loads(audit_path.read_text(encoding="utf-8")) if audit_path.exists() else {}
    if args.record:
        audit = record(audit, args.record)
        audit_path.write_text(json.dumps(audit, indent=2, sort_keys=True) + "\n")
        return
    unknown = sorted({d["classification"] for d in audit.values()} - set(CLASSIFICATIONS))
    if unknown:
        raise SystemExit(f"panic-audit.json has unknown classifications: {', '.join(unknown)}")
    missing = 0
    print("site\tclassification\treason\tconstruction")
    for key, site, construction in collect(args.all):
        decision = audit.get(key, {"classification": "REVIEW", "reason": "New or changed construction"})
        missing += decision["classification"] == "REVIEW"
        print(f'{site}\t{decision["classification"]}\t{decision["reason"]}\t{construction}')
    if args.check and missing:
        raise SystemExit(f"{missing} panic sites need review")


if __name__ == "__main__":
    main()
