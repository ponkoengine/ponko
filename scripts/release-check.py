#!/usr/bin/env python3
"""Refuse a release whose tag, crate versions and CHANGELOG section disagree.

    release-check.py check TAG      TAG is vX.Y.Z or vX.Y.Z-rc.N; the maintainer
                                    runs this before building or publishing
                                    anything
    release-check.py notes TAG FILE
                                    write that version's CHANGELOG section,
                                    the GitHub release's notes, to FILE (a
                                    release's notes end with a link to the
                                    web gallery of the examples)
    release-check.py bump X.Y.Z [--dry-run]
                                    set every publishable crate, the requirements
                                    between them and the docs' and templates'
                                    `ponko = "X.Y"` lines to that version
    release-check.py published      every version of our crates on crates.io
                                    has a git tag
    release-check.py selftest       check the decisions with made-up input

`check` fails unless all of these hold:

- the tag is `vX.Y.Z`, or `vX.Y.Z-rc.N` (N from 1), a release candidate:
  crates.io and Cargo treat its version as a pre-release, so nobody gets it
  without writing it out, and the GitHub release is marked pre-release.
  Candidates are for minor releases (X.Y.0), not for patch releases;
- every publishable crate (no `publish = false`) is at the tag's version
  (X.Y.Z or X.Y.Z-rc.N), and so are the `version = "..."` requirements it
  puts on its sibling crates;
- CHANGELOG.md has a `## [X.Y.Z]` section with something in it, and still
  has its `## [Unreleased]` heading. A candidate's notes may instead be in
  `## [X.Y.Z-rc.N]` or, as long as the release is not named yet, in
  `## [Unreleased]` itself (the first of the three that exists is used);
- that section fits in a GitHub release body (at most 125,000 characters),
  since it becomes the release's notes;
- no changelog.d/ fragment is left (the release folds them in first);
- a release that lists `### Breaking changes` is one that may break (0.y.0,
  x.0.0: never a patch release) and has its migration chapter,
  book/src/migrating-to-X.Y.md (`scripts/changelog.py --version X.Y.Z`
  generates it with the fold), listed in book/src/SUMMARY.md and holding every
  breaking entry of the section. A candidate needs the same chapter, under
  the same name, as soon as its section lists a break (testers need it most);
- the dependency lines the docs and the project templates tell people to write
  (`ponko = "X.Y"` in getting-started.md, the two tutorials and
  crates/ponko_cli/templates) name the tag's minor, so a release that is
  published does not leave the guide pointing at the one before. The guide's
  "which version" note needs no such step: it reads the root Cargo.toml and
  changelog.d/ itself (scripts/guide-note.py);
- every publishable crate under crates/ carries copies of the root
  LICENSE-MIT, LICENSE-APACHE and NOTICE (crates.io gets each crate's own
  folder), identical to the root ones; `selftest` checks this too;
- with `--on-branch`: the tagged commit is an ancestor of origin/main or of
  origin/release-X.Y (the patch branch of the tag's minor: v0.1.1 on
  release-0.1, never on release-0.2), i.e. it was merged through a pull
  request (it needs the full history). Whether the checks passed on it is
  the maintainer's job: run scripts/checks.yml on the tagged commit first.

`published` asks the crates.io API (one read request per crate) and compares
with `git tag`; it needs no token.

See "Releasing" in CONTRIBUTING.md for the whole procedure.
"""
import json
import re
import subprocess
import sys
import tempfile
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(Path(__file__).resolve().parent))
import changelog  # noqa: E402  (the version rules and the migration chapter's name)

TAG = re.compile(r"^v(\d+\.\d+\.\d+(?:-rc\.[1-9]\d*)?)$")
TAG_HELP = "a release tag like v1.2.3 or v1.2.3-rc.1"
# GitHub refuses a release whose body is longer than this many characters.
MAX_NOTES = 125_000


def crates(root=ROOT):
    """(name, version, manifest dict) of every publishable crate of the workspace."""
    found = []
    for manifest in [root / "Cargo.toml", *sorted(root.glob("crates/*/Cargo.toml"))]:
        data = tomllib.loads(manifest.read_text(encoding='utf-8'))
        package = data.get("package")
        if package is None or package.get("publish") is False:
            continue
        found.append((package["name"], package["version"], data))
    return found


LICENSE_FILES = ("LICENSE-MIT", "LICENSE-APACHE", "NOTICE")


def license_problems(root=ROOT):
    """Publishable sub-crates whose license files are missing or differ from the root's."""
    out = []
    for manifest in sorted(root.glob("crates/*/Cargo.toml")):
        package = tomllib.loads(manifest.read_text(encoding='utf-8')).get("package")
        if package is None or package.get("publish") is False:
            continue
        for name in LICENSE_FILES:
            copy = manifest.parent / name
            if not copy.is_file():
                out.append(f"{package['name']} has no {name} (copy the root one into {manifest.parent.name}/)")
            elif copy.read_bytes() != (root / name).read_bytes():
                out.append(f"{manifest.parent.name}/{name} differs from the root {name}: copy it again")
    return out


def sibling_requirements(data, names):
    """(dependency, version requirement) of every dependency on a sibling crate."""
    out = []
    for table in ("dependencies", "dev-dependencies", "build-dependencies"):
        for name, spec in data.get(table, {}).items():
            if name in names and isinstance(spec, dict) and "version" in spec:
                out.append((name, spec["version"]))
    return out


def changelog_section(text, version):
    """The lines of `## [version]` in a changelog, or None if there is no section."""
    lines = text.splitlines()
    start = next((i for i, l in enumerate(lines) if l.startswith(f"## [{version}]")), None)
    if start is None:
        return None
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith("## [")), len(lines))
    return [l for l in lines[start + 1 : end] if not re.match(r"^\[[^\]]*\]: ", l)]


def is_candidate(version):
    """Whether `version` (X.Y.Z-rc.N) is a release candidate."""
    return "-" in version


def notes_section(text, version):
    """(heading, lines) of the CHANGELOG section that holds the notes of `version`, or None.

    A release has its own `## [version]` section. A candidate may also use
    the final version's section or `## [Unreleased]` (the changes are folded
    in but the release is not named yet); the first that exists wins.
    """
    names = [version]
    if is_candidate(version):
        names += [version.split("-")[0], "Unreleased"]
    for name in names:
        section = changelog_section(text, name)
        if section is not None:
            return name, section
    return None


def candidate_preamble(version):
    """The line a candidate's GitHub release starts with."""
    return (f"Release candidate of {version.split('-')[0]}, for testing. Cargo does not select it "
            f"unless the version is written out (`ponko = \"{version}\"`); the release follows "
            "unchanged if no regression turns up.\n\n")


def project_url(root=ROOT):
    """The project's URL from PROJECT_URL (https, host, owner, repository)."""
    url = (root / "PROJECT_URL").read_text(encoding="utf-8").strip()
    if not re.fullmatch(r"https://[^\s/]+(/[^\s/]+)+", url):
        sys.exit(f"PROJECT_URL must be one https URL without a trailing slash, got {url!r}")
    return url


def pages_url(url):
    """Where GitHub Pages serves a repository's gh-pages branch: https://<owner>.github.io/<repository>/."""
    m = re.fullmatch(r"https://github\.com/([^/]+)/([^/]+)", url)
    if not m:
        sys.exit(f"no GitHub Pages address for {url}: PROJECT_URL is not https://github.com/<owner>/<repository>")
    return f"https://{m.group(1).lower()}.github.io/{m.group(2)}/"


# The web gallery of the examples (web/gallery/README.md), which the
# maintainer publishes for a release that is the newest version. Its address
# follows PROJECT_URL (the same rule as web/gallery/lib.mjs).
GALLERY_URL = pages_url(project_url())


def gallery_line(version):
    """The line a release's notes end with: where to play its examples. None for a candidate (no gallery)."""
    if is_candidate(version):
        return None
    return (f"\n**Play the examples in a browser:** {GALLERY_URL} (the web gallery of the newest "
            f"release; each page links the example's source).\n")


def section_link(changelog_text, heading, version):
    """Where a changelog section is read in full: CHANGELOG.md at the tag, at the section's anchor."""
    line = next((l for l in changelog_text.splitlines() if l.startswith(f"## [{heading}]")), f"## [{heading}]")
    slug = re.sub(r"[^\w\- ]", "", line[3:].strip().lower()).replace(" ", "-")
    return f"{project_url()}/blob/v{version}/CHANGELOG.md#{slug}"


def release_notes(section, link=None):
    """A changelog section's lines as the text of a GitHub release body.

    A section longer than a release body holds (MAX_NOTES) keeps what comes
    before its first subsection (the release's story and highlights) and its
    Breaking changes, and links the rest, which is read in CHANGELOG.md at
    `link` and on the guide's release page. Without a link it is kept whole."""
    full = "\n".join(section).strip() + "\n"
    if len(full) <= MAX_NOTES or link is None:
        return full
    heads = [i for i, l in enumerate(section) if l.startswith("### ")]
    breaking = [i for i in heads if section[i][4:].strip() == changelog.BREAKING]
    cut = next((i for i in heads if i > breaking[0]), len(section)) if breaking else \
        (heads[0] if heads else len(section))
    left = sum(1 for l in section[cut:] if l.startswith("- "))
    kept = "\n".join(section[:cut]).strip()
    return (kept + f"\n\n### Everything else\n\nThe other {left:,} changes, grouped by area under "
            f"Added, Changed, Deprecated, Removed, Fixed and Security, are in [CHANGELOG.md]({link}): "
            "the whole list is longer than a GitHub release holds.\n")


def breaking_headlines(section):
    """The bullets of the `### Breaking changes` subsection of a changelog section."""
    found, inside = [], False
    for line in section:
        if line.startswith("### "):
            inside = line[4:].strip() == changelog.BREAKING
        elif inside and line.startswith("- "):
            found.append(line[2:].strip())
    return found


def breaking_texts(section):
    """What each bullet of `### Breaking changes` says changed: its lines up to `Instead:`, joined."""
    found, inside = [], False
    for line in section:
        if line.startswith("### "):
            inside = line[4:].strip() == changelog.BREAKING
        elif inside and line.startswith("- "):
            found.append([line[2:].strip()])
        elif inside and found and line.startswith("  ") and len(found[-1]) and found[-1][-1] is not None:
            if line.strip().startswith("Instead:"):
                found[-1].append(None)
            else:
                found[-1].append(line.strip())
    return [" ".join(l for l in parts if l is not None) for parts in found]


def migration_problems(version, section, chapter, summary):
    """What is wrong with the migration chapter of `version`, as messages.

    section: the lines of its changelog section; chapter: the text of
    book/src/migrating-to-X.Y.md or None if there is none; summary: the text
    of book/src/SUMMARY.md.
    """
    headlines = breaking_headlines(section)
    if not headlines:
        return []
    if not changelog.may_break(version):
        return [f"CHANGELOG.md's `## [{version}]` section lists breaking changes, but {version} "
                "is a patch release and must not break the API (STABILITY.md): release it as the "
                "next minor"]
    name = changelog.chapter_name(version)
    if chapter is None:
        return [f"{version} has breaking changes but book/src/{name} does not exist: run "
                f"`python3 scripts/changelog.py --version {version}` with the fragments in place, "
                "which writes it from the same entries"]
    out = []
    if f"({name})" not in summary:
        out.append(f"book/src/SUMMARY.md has no line for {name}")
    # An entry's heading in the chapter is its first sentence (cut short when
    # long, changelog.split_title), so either that or its first line is there.
    missing = [h for h, text in zip(headlines, breaking_texts(section))
               if h.rstrip(".") not in chapter and changelog.title_of(text) not in chapter]
    if missing:
        shown = "; ".join(f"`{h[:50]}`" for h in missing[:3]) + (" ..." if len(missing) > 3 else "")
        out.append(f"book/src/{name} lacks {len(missing)} breaking change(s) of the CHANGELOG: {shown}")
    return out


# Where the docs and templates write the engine's own version requirement.
PIN_FILES = ("README.md", "book/src/*.md", "book/src/*/*.md",
             "crates/ponko_cli/templates/*/Cargo.toml.liquid")
# The migration chapters and release pages quote old versions on purpose.
PIN_SKIP = re.compile(r"(?:^|/)(?:migrating-to-|release-)[\w.-]*\.md$")
PIN = re.compile(r'^\s*ponko\s*=\s*(?:\{\s*version\s*=\s*)?"(\d+\.\d+)(?:\.\d+)?(?:-rc\.\d+)?"', re.M)


def pin_problems(version, texts):
    """Messages for each `ponko = "X.Y"` line in `texts` ({name: text}) that is not `version`'s minor."""
    minor = ".".join(version.split("-")[0].split(".")[:2])
    out = []
    for name, text in sorted(texts.items()):
        for found in PIN.finditer(text):
            if found.group(1) != minor:
                out.append(f'{name} tells people to depend on ponko "{found.group(1)}", not "{minor}"')
    return out


def pin_texts(root=ROOT):
    """{name: text} of the files that carry the engine's version requirement."""
    texts = {}
    for pattern in PIN_FILES:
        for path in sorted(root.glob(pattern)):
            name = path.relative_to(root).as_posix()
            if not PIN_SKIP.search(name):
                texts[name] = path.read_text(encoding='utf-8')
    return texts


MANIFESTS = ("Cargo.toml", "crates/*/Cargo.toml")
SIBLING = r'(?m)^(\s*(?:{names})\s*=\s*\{{[^}}\n]*?\bversion\s*=\s*")[^"]+(")'


def bumped(version, files, names):
    """{name: new text} for the files that change when the release is `version`.

    files: {name: text} of the workspace manifests (MANIFESTS) and of the
    pins (pin_texts()); names: the publishable crates. A publishable crate's
    `[package] version` becomes `version`, every one-line requirement on a
    sibling (`ponko_ecs = { path = ..., version = "..." }`, in any manifest)
    too, and each `ponko = "X.Y"` pin the version's minor."""
    minor = ".".join(version.split("-")[0].split(".")[:2])
    sibling = re.compile(SIBLING.format(names="|".join(sorted(names))))
    out = {}
    for name, text in files.items():
        new = text
        if name.endswith("Cargo.toml"):
            package = tomllib.loads(text).get("package")
            if package and package.get("publish") is not False and package.get("name") in names:
                new = re.sub(r'(?ms)^(\[package\][^\[]*?^version\s*=\s*")[^"]+(")',
                             lambda m: m.group(1) + version + m.group(2), new, count=1)
            new = sibling.sub(lambda m: m.group(1) + version + m.group(2), new)
        else:
            new = PIN.sub(lambda m: re.sub(r'"[^"]+"$', f'"{minor}"', m.group(0)), new)
        if new != text:
            out[name] = new
    return out


def bump(version, dry_run, root=ROOT):
    """Sets the crates, their sibling requirements and the docs' pins to `version` (bumped())."""
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-rc\.[1-9]\d*)?", version):
        sys.exit(f"`{version}` is not X.Y.Z or X.Y.Z-rc.N")
    files = {p.relative_to(root).as_posix(): p.read_text(encoding="utf-8")
             for pattern in MANIFESTS for p in sorted(root.glob(pattern))}
    files.update(pin_texts(root))
    changed = bumped(version, files, {name for name, _, _ in crates(root)})
    for name, text in sorted(changed.items()):
        print(("would change " if dry_run else "changed ") + name)
        if not dry_run:
            (root / name).write_text(text, encoding="utf-8", newline="\n")
    if not dry_run:
        print("Now refresh Cargo.lock: cargo update --workspace")
    return 0


def problems(tag, crate_list, changelog_text, fragments, book=None, pins=None):
    """What is wrong with releasing `tag`, as a list of messages (empty: fine).

    book: (the migration chapter's text or None, SUMMARY.md's text), or None
    to skip the migration check. pins: {name: text} of the files that carry
    the engine's version requirement (`pin_texts()`), or None to skip that check.
    """
    found = TAG.match(tag)
    if not found:
        return [f"`{tag}` is not {TAG_HELP}"]
    version = found.group(1)
    out = []
    if is_candidate(version) and not version.split("-")[0].endswith(".0"):
        out.append(f"`{tag}`: release candidates are for minor releases (X.Y.0); "
                   "a patch release is released directly")
    names = {name for name, _, _ in crate_list}
    for name, crate_version, data in crate_list:
        if crate_version != version:
            out.append(f"{name} is at {crate_version}, not {version}")
        for dep, req in sibling_requirements(data, names):
            if req.lstrip("=^~ ") != version:
                out.append(f"{name} requires {dep} {req}, not {version}")
    chosen = notes_section(changelog_text, version)
    if chosen is None:
        out.append(f"CHANGELOG.md has no `## [{version}]` section"
                   + (" (nor `## [Unreleased]`)" if is_candidate(version) else ""))
        section = None
    else:
        heading, section = chosen
        if not any(l.strip() for l in section):
            out.append(f"CHANGELOG.md's `## [{heading}]` section is empty")
        elif len(release_notes(section, section_link(changelog_text, heading, version))) > MAX_NOTES:
            out.append(
                f"the release notes from CHANGELOG.md's `## [{heading}]` section have "
                f"{len(release_notes(section, section_link(changelog_text, heading, version))):,} characters "
                f"(its story and Breaking changes; the rest is linked); a GitHub release body holds at "
                f"most {MAX_NOTES:,}. Shorten them (summarize, and keep long lists in the guide)"
            )
    if section and book is not None:
        out += migration_problems(version, section, *book)
    if not any(l.startswith("## [Unreleased]") for l in changelog_text.splitlines()):
        out.append("CHANGELOG.md lost its `## [Unreleased]` heading")
    if pins is not None:
        out += pin_problems(version, pins)
    if fragments:
        shown = ", ".join(sorted(fragments)[:3]) + (" ..." if len(fragments) > 3 else "")
        out.append(
            f"changelog.d/ still has {len(fragments)} fragment(s) (run scripts/changelog.py "
            f"first): {shown}"
        )
    return out


def untagged(published, tags):
    """The (crate, version) pairs on crates.io whose `vVERSION` tag does not exist."""
    return sorted((name, v) for name, versions in published.items() for v in versions if f"v{v}" not in tags)


def git(*args, repo=ROOT):
    return subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, encoding='utf-8')


def release_branch(version):
    """The patch branch of the minor `version` belongs to: release-0.1 for 0.1.1."""
    major, minor, _ = changelog.version_parts(version)
    return f"release-{major}.{minor}"


def branch_problems(tag, repo=ROOT):
    """Problems with `tag` not being a commit merged to origin/main or its release branch.

    Both are fetched first. The release branch is the tag's own minor's
    (release-0.1 for v0.1.1, release-0.2 for v0.2.0-rc.2); a tag is never
    accepted on another minor's branch.
    """
    found = TAG.match(tag)
    if not found:
        return [f"`{tag}` is not {TAG_HELP}"]
    if git("fetch", "--quiet", "origin", "main", repo=repo).returncode != 0:
        return ["could not fetch origin/main to check the tag against it"]
    if git("rev-parse", "--verify", "--quiet", f"{tag}^{{commit}}", repo=repo).returncode != 0:
        return [f"{tag} does not exist in this checkout"]
    if git("merge-base", "--is-ancestor", f"{tag}^{{commit}}", "origin/main", repo=repo).returncode == 0:
        return []
    branch = release_branch(found.group(1))
    # An explicit refspec: a plain `fetch origin NAME` would not update origin/NAME everywhere.
    fetched = git("fetch", "--quiet", "origin", f"+refs/heads/{branch}:refs/remotes/origin/{branch}", repo=repo)
    if fetched.returncode != 0:
        return [f"{tag} is not on origin/main, and there is no origin/{branch} branch it could be on: "
                f"tag a commit that was merged to main (or create {branch} from the minor's release)"]
    if git("merge-base", "--is-ancestor", f"{tag}^{{commit}}", f"origin/{branch}", repo=repo).returncode != 0:
        return [f"{tag} is on neither origin/main nor origin/{branch}: tag a commit that was merged to one of them"]
    return []


def check(tag, on_branch):
    fragments = [p.name for p in (ROOT / "changelog.d").glob("*.md") if p.name != "README.md"]
    found = TAG.match(tag)
    book = None
    if found:
        chapter = ROOT / "book" / "src" / changelog.chapter_name(found.group(1))
        book = (chapter.read_text(encoding='utf-8') if chapter.is_file() else None,
                (ROOT / "book" / "src" / "SUMMARY.md").read_text(encoding='utf-8'))
    found = problems(tag, crates(), (ROOT / "CHANGELOG.md").read_text(encoding='utf-8'), fragments, book, pin_texts()) + license_problems()
    if on_branch:
        found += branch_problems(tag)
    if found:
        print(f"Not releasing {tag}:")
        for message in found:
            print(f"  - {message}")
        return 1
    print(f"{tag}: crate versions, CHANGELOG section and fragments agree.")
    return 0


def notes(tag, out):
    """Writes the release notes of `tag` to `out` (the same checks as `check`)."""
    found = TAG.match(tag)
    if not found:
        sys.exit(f"`{tag}` is not {TAG_HELP}")
    version = found.group(1)
    chosen = notes_section((ROOT / "CHANGELOG.md").read_text(encoding='utf-8'), version)
    if chosen is None or not any(l.strip() for l in chosen[1]):
        sys.exit(f"CHANGELOG.md has no notes for {version}")
    changelog_text = (ROOT / "CHANGELOG.md").read_text(encoding='utf-8')
    text = (candidate_preamble(version) if is_candidate(version) else "") + \
        release_notes(chosen[1], section_link(changelog_text, chosen[0], version))
    text += gallery_line(version) or ""
    if len(text) > MAX_NOTES:
        sys.exit(f"the notes for {version} have {len(text):,} characters; GitHub allows {MAX_NOTES:,}")
    Path(out).write_text(text, encoding="utf-8", newline='\n')
    print(f"wrote {out} ({len(text):,} characters)")
    return 0


def fetch_published(names):
    out = {}
    for name in names:
        request = urllib.request.Request(
            f"https://crates.io/api/v1/crates/{name}", headers={"User-Agent": "ponko-release-check"}
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                data = json.load(response)
        except urllib.error.HTTPError as error:
            if error.code == 404:  # never published
                continue
            sys.exit(f"crates.io answered {error.code} for {name}; try again later")
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as error:
            sys.exit(f"could not ask crates.io about {name}: {error}")
        out[name] = [v["num"] for v in data["versions"]]
    return out


def published():
    published_versions = fetch_published([name for name, _, _ in crates()])
    tags = set(git("tag", "--list", "v*").stdout.split())
    missing = untagged(published_versions, tags)
    for name, version in missing:
        print(f"{name} {version} is on crates.io but v{version} is not a git tag")
    if missing:
        print("Tag the commit it was published from (see \"Releasing\" in CONTRIBUTING.md).")
        return 1
    print("Every published version has a git tag.")
    return 0


def on_branch_selftest():
    """branch_problems against a throw-away origin and clone (deleted afterwards)."""
    with tempfile.TemporaryDirectory(prefix="release-check-") as tmp:
        tmp = Path(tmp)

        def run(*args, cwd):
            subprocess.run(
                ["git", "-c", "user.name=t", "-c", "user.email=t@example.com",
                 "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false", *args],
                cwd=cwd, check=True, capture_output=True,
            )

        run("init", "--bare", "--initial-branch=main", "origin.git", cwd=tmp)
        work = tmp / "work"
        run("clone", "origin.git", "work", cwd=tmp)
        run("checkout", "-b", "main", cwd=work)
        run("commit", "--allow-empty", "-m", "merged", cwd=work)
        run("tag", "v0.1.0", cwd=work)
        run("push", "origin", "main", cwd=work)
        # The patch branch of 0.1, cut from the release, with a fix of its own.
        run("checkout", "-b", "release-0.1", "v0.1.0", cwd=work)
        run("commit", "--allow-empty", "-m", "fix on the patch branch", cwd=work)
        run("tag", "v0.1.1", cwd=work)
        run("tag", "v0.2.0", cwd=work)  # a tag of another minor on this branch
        run("push", "origin", "release-0.1", cwd=work)
        # Main moves on to the next minor.
        run("checkout", "main", cwd=work)
        run("commit", "--allow-empty", "-m", "breaking change on main", cwd=work)
        run("tag", "v0.2.0-rc.1", cwd=work)
        run("push", "origin", "main", cwd=work)
        run("checkout", "-b", "feature", cwd=work)
        run("commit", "--allow-empty", "-m", "never merged", cwd=work)
        run("tag", "v0.1.2", cwd=work)
        run("tag", "v0.2.0-rc.2", cwd=work)

        assert branch_problems("v0.1.0", work) == [], "a tag on a merged commit passes"
        assert branch_problems("v0.2.0-rc.1", work) == [], "a candidate on main passes"
        assert branch_problems("v0.1.1", work) == [], "a patch release on its release branch passes"
        found = branch_problems("v0.1.2", work)
        assert found and "neither origin/main nor origin/release-0.1" in found[0], (
            "a tag on an unmerged branch is refused even where its release branch exists"
        )
        found = branch_problems("v0.2.0-rc.2", work)
        assert found and "no origin/release-0.2 branch" in found[0], (
            "an unmerged candidate is refused, and the message names the branch it could be on"
        )
        found = branch_problems("v0.2.0", work)
        assert found and "no origin/release-0.2 branch" in found[0], (
            "release-0.1 is not the branch for a 0.2 tag"
        )
        assert "does not exist" in branch_problems("v9.9.9", work)[0]
        assert "is not a release tag" in branch_problems("0.1.1", work)[0]
        run("remote", "set-url", "origin", str(tmp / "missing.git"), cwd=work)
        assert "could not fetch" in branch_problems("v0.1.0", work)[0], "no main to compare with is refused"


def selftest():
    def toml(name, version, deps=""):
        return tomllib.loads(f'[package]\nname = "{name}"\nversion = "{version}"\n[dependencies]\n{deps}')

    sibling = 'ponko_ecs = { path = "crates/ponko_ecs", version = "0.0.2" }\nserde = "1"\n'
    ok = [("ponko", "0.0.2", toml("ponko", "0.0.2", sibling)), ("ponko_ecs", "0.0.2", toml("ponko_ecs", "0.0.2"))]
    log = "## [Unreleased]\n\n### Added\n- next\n\n## [0.0.2] - 2026-12-01\n\n### Added\n- thing\n\n## [0.0.1] - x\n- old\n"

    assert problems("v0.0.2", ok, log, []) == [], "a consistent release passes"
    assert problems("v0.0.2-rc.1", ok, log, [])[0].startswith("`v0.0.2-rc.1`: release candidates are for minor"), (
        "a patch release has no candidates"
    )
    assert problems("0.0.2", ok, log, []) == [f"`0.0.2` is not {TAG_HELP}"]
    for odd in ("v0.0.2-beta.1", "v0.0.2-rc", "v0.0.2-rc.0", "v0.0.2-rc.01", "v0.0.2-rc.1.2", "v0.0.2-desktop-dry-run",
                "v0.0.2+build", "v0.0", "v1.2.3.4"):
        assert problems(odd, ok, log, []) == [f"`{odd}` is not {TAG_HELP}"], f"{odd} is not a release tag"
    assert any("ponko is at 0.0.2, not 0.0.3" in p for p in problems("v0.0.3", ok, log, [])), (
        "a tag ahead of the crate versions is refused"
    )
    assert any("has no `## [0.0.3]` section" in p for p in problems("v0.0.3", ok, log, []))
    lagging = [ok[0], ("ponko_ecs", "0.0.1", toml("ponko_ecs", "0.0.1"))]
    assert any("ponko_ecs is at 0.0.1" in p for p in problems("v0.0.2", lagging, log, []))
    stale = [("ponko", "0.0.2", toml("ponko", "0.0.2", sibling.replace("0.0.2", "0.0.1"))), ok[1]]
    assert any("requires ponko_ecs 0.0.1" in p for p in problems("v0.0.2", stale, log, [])), (
        "a sibling requirement must move with the version"
    )
    empty = "## [Unreleased]\n\n## [0.0.2] - x\n\n[0.0.2]: https://example.com\n"
    assert any("section is empty" in p for p in problems("v0.0.2", ok, empty, [])), (
        "link lines are not release notes"
    )
    assert any("lost its" in p for p in problems("v0.0.2", ok, "## [0.0.2]\n- x\n", []))
    assert any("1 fragment(s)" in p for p in problems("v0.0.2", ok, log, ["12-x.md"]))
    assert changelog_section(log, "0.0.1") == ["- old"] or changelog_section(log, "0.0.1")[-1] == "- old"
    huge = log.replace("- thing\n", "- thing\n" + "- x\n" * (MAX_NOTES // 4 + 1))
    assert not any("release body" in p for p in problems("v0.0.2", ok, huge, [])), (
        "a long list of changes is linked, not put in the release body"
    )
    short = release_notes(changelog_section(huge, "0.0.2"), "https://example.com/CHANGELOG.md#002---x")
    assert "- x" not in short and "The other 31,252 changes" in short and "#002---x)" in short, short[:300]
    assert section_link("## [0.2.0] - 2026-12-03\n", "0.2.0", "0.2.0").endswith("/blob/v0.2.0/CHANGELOG.md#020---2026-12-03")
    huge_breaks = log.replace("### Added\n- thing\n", "### Breaking changes\n" + "- `x` is gone.\n  Instead: y.\n" * (MAX_NOTES // 20) + "\n### Added\n- thing\n")
    assert any("a GitHub release body holds at most" in p for p in problems("v0.0.2", ok, huge_breaks, [])), (
        "notes longer than a GitHub release body are refused before anything is built"
    )
    assert release_notes(changelog_section(log, "0.0.2")) == "### Added\n- thing\n"

    # A release with breaking changes needs its migration chapter.
    breaking_log = log.replace(
        "### Added\n- thing\n",
        "### Breaking changes\n\nSee the guide.\n\n- `A` is `B` now.\n  Instead: use `B`.\n- `C` is gone.\n\n### Added\n- thing\n",
    )
    section = changelog_section(breaking_log, "0.0.2")
    assert breaking_headlines(section) == ["`A` is `B` now.", "`C` is gone."]
    assert breaking_headlines(changelog_section(log, "0.0.2")) == []
    chapter = "# Migrating\n## `A` is `B` now\n## `C` is gone\n"
    summary = "- [Migrating](migrating-to-0.0.md)\n"
    assert migration_problems("0.0.2", section, chapter, summary) != [], "0.0.2 is a patch release"
    assert "patch release" in migration_problems("0.0.2", section, chapter, summary)[0]
    minor = section
    assert migration_problems("0.3.0", minor, None, "")[0].endswith("which writes it from the same entries"), (
        "a minor release with breaks and no chapter is refused"
    )
    assert "migrating-to-0.3.md does not exist" in migration_problems("0.3.0", minor, None, "")[0]
    good_summary = "- [Migrating from 0.2 to 0.3](migrating-to-0.3.md)\n"
    assert migration_problems("0.3.0", minor, chapter, good_summary) == []
    assert migration_problems("0.3.0-rc.1", minor, chapter, good_summary) == [], "a candidate needs it too, same chapter"
    assert any("no line for migrating-to-0.3.md" in p for p in migration_problems("0.3.0", minor, chapter, ""))
    assert any("lacks 1 breaking change" in p and "`C` is gone" in p
               for p in migration_problems("0.3.0", minor, chapter.replace("## `C` is gone\n", ""), good_summary))
    assert migration_problems("0.3.0", changelog_section(log, "0.0.2"), None, "") == [], "no breaks, no chapter needed"
    # Through problems(): the chapter is looked at only when a book is given.
    three = [("ponko", "0.3.0", toml("ponko", "0.3.0"))]
    log3 = breaking_log.replace("[0.0.2]", "[0.3.0]")
    assert problems("v0.3.0", three, log3, []) == [], "skipped without a book"
    assert any("does not exist" in p for p in problems("v0.3.0", three, log3, [], (None, "")))
    assert problems("v0.3.0", three, log3, [], (chapter, good_summary)) == []

    # Release candidates: crates at X.Y.0-rc.N, notes from the first of the
    # candidate's, the final version's or the Unreleased section.
    def rc_crates(version):
        dep = f'ponko_ecs = {{ path = "crates/ponko_ecs", version = "{version}" }}\n'
        return [("ponko", version, toml("ponko", version, dep)), ("ponko_ecs", version, toml("ponko_ecs", version))]

    rc = rc_crates("0.3.0-rc.1")
    unreleased = "## [Unreleased]\n\n### Added\n- the candidate's news\n\n## [0.2.0] - x\n- old\n"
    assert problems("v0.3.0-rc.1", rc, unreleased, []) == [], "a candidate may keep its notes under Unreleased"
    assert problems("v0.3.0", rc_crates("0.3.0"), unreleased, []) != [], "the release itself needs its own section"
    assert notes_section(unreleased, "0.3.0-rc.1") == ("Unreleased", ["", "### Added", "- the candidate's news", ""])
    assert notes_section(unreleased, "0.3.0") is None
    named = unreleased.replace("## [Unreleased]\n", "## [Unreleased]\n\n## [0.3.0-rc.1] - x\n- named\n")
    assert notes_section(named, "0.3.0-rc.1")[0] == "0.3.0-rc.1", "its own section comes first"
    final = unreleased.replace("## [0.2.0]", "## [0.3.0] - x\n- final\n\n## [0.2.0]")
    assert notes_section(final, "0.3.0-rc.2")[0] == "0.3.0", "then the final version's"
    assert problems("v0.3.0-rc.1", rc, "## [Unreleased]\n\n## [0.2.0] - x\n- old\n", []) == [
        "CHANGELOG.md's `## [Unreleased]` section is empty"
    ], "a candidate with nothing to tell is refused"
    assert any("no `## [0.3.0-rc.1]` section (nor `## [Unreleased]`)" in p
               for p in problems("v0.3.0-rc.1", rc, "## [0.2.0] - x\n- old\n", [])), (
        "a candidate with no section to take notes from is refused"
    )
    assert any("ponko is at 0.3.0, not 0.3.0-rc.1" in p for p in problems("v0.3.0-rc.1", rc_crates("0.3.0"), unreleased, [])), (
        "the crates carry the candidate's version, or they would publish as the release"
    )
    assert any("requires ponko_ecs 0.3.0, not 0.3.0-rc.1" in p for p in problems(
        "v0.3.0-rc.1", [("ponko", "0.3.0-rc.1", toml("ponko", "0.3.0-rc.1", 'ponko_ecs = { path = "x", version = "0.3.0" }\n')),
                        ("ponko_ecs", "0.3.0-rc.1", toml("ponko_ecs", "0.3.0-rc.1"))], unreleased, [])), (
        "sibling requirements move to the candidate too"
    )
    assert any("1 fragment(s)" in p for p in problems("v0.3.0-rc.1", rc, unreleased, ["1-x.md"])), (
        "fragments are folded in before a candidate, the way they are before a release"
    )
    # Breaking changes: a candidate's notes under Unreleased need the chapter too.
    breaking_unreleased = unreleased.replace("### Added", "### Breaking changes\n- `A` is `B` now.\n  Instead: use `B`.\n\n### Added")
    assert any("migrating-to-0.3.md does not exist" in p
               for p in problems("v0.3.0-rc.1", rc, breaking_unreleased, [], (None, ""))), (
        "a candidate with breaking entries needs the chapter"
    )
    assert problems("v0.3.0-rc.1", rc, breaking_unreleased, [], (chapter, good_summary)) == []
    assert problems("v0.3.0-rc.2", rc_crates("0.3.0-rc.2"), breaking_unreleased, [], (chapter, good_summary)) == [], (
        "every candidate of a minor shares its chapter"
    )
    assert release_branch("0.1.1") == "release-0.1" and release_branch("0.2.0-rc.1") == "release-0.2"
    assert release_branch("1.4.2") == "release-1.4"
    assert candidate_preamble("0.3.0-rc.1").startswith("Release candidate of 0.3.0, for testing.")
    assert GALLERY_URL in gallery_line("0.3.0"), "a release's notes link the web gallery"
    assert pages_url("https://github.com/Some-Org/proj") == "https://some-org.github.io/proj/"
    assert GALLERY_URL == pages_url((ROOT / "PROJECT_URL").read_text(encoding="utf-8").strip()), "the gallery follows PROJECT_URL"
    for bad in ("https://git.example.test/o/r", "https://github.com/o/r/extra"):
        try:
            pages_url(bad)
        except SystemExit as e:
            assert "no GitHub Pages address" in str(e), e
        else:
            raise AssertionError(f"{bad} has no Pages address")
    assert gallery_line("0.3.0-rc.1") is None, "a candidate publishes no gallery, so its notes don't link one"
    assert "ponko = \"0.3.0-rc.1\"" in candidate_preamble("0.3.0-rc.1")
    assert untagged({"ponko": ["0.0.1", "0.0.2"], "ponko_ecs": ["0.0.1"]}, {"v0.0.1"}) == [
        ("ponko", "0.0.2")
    ], "a published version needs its tag"
    assert untagged({"ponko": ["0.0.1"]}, {"v0.0.1", "v1.0.0"}) == []
    on_branch_selftest()
    with tempfile.TemporaryDirectory(prefix="release-check-") as tmp:
        tmp = Path(tmp)
        (tmp / "crates" / "a").mkdir(parents=True)
        (tmp / "crates" / "a" / "Cargo.toml").write_text('[package]\nname = "a"\nversion = "0.0.2"\n', encoding='utf-8', newline='\n')
        for name in LICENSE_FILES:
            (tmp / name).write_text(name, encoding='utf-8', newline='\n')
            (tmp / "crates" / "a" / name).write_text(name, encoding='utf-8', newline='\n')
        assert license_problems(tmp) == [], "identical copies pass"
        (tmp / "crates" / "a" / "NOTICE").write_text("old", encoding='utf-8', newline='\n')
        assert "a/NOTICE differs" in license_problems(tmp)[0], "a stale copy is refused"
        (tmp / "crates" / "a" / "NOTICE").unlink()
        assert "a has no NOTICE" in license_problems(tmp)[0], "a missing copy is refused"
    docs = {"a.md": 'ponko = "0.2"\n', "b.liquid": 'ponko = { version = "0.2", features = ["3d"] }\n',
            "c.md": '    ponko = { version = "0.1" }\nserde = "0.1"\nponko_ecs = "0.1"\n'}
    assert pin_problems("0.2.0", docs) == ['c.md tells people to depend on ponko "0.1", not "0.2"'], (
        "only the engine's own line counts, and a stale minor is named")
    assert pin_problems("0.2.0-rc.1", {"a.md": 'ponko = "0.2"\n'}) == [], "a candidate's pins are the minor too"
    assert pin_problems("0.2.3", {"a.md": 'ponko = "0.2.1"\n'}) == [], "a full version of the minor is fine"
    assert problems("v0.0.2", ok, log, [], None, {"a.md": 'ponko = "0.1"\n'}) == [
        'a.md tells people to depend on ponko "0.1", not "0.0"'], "problems() runs the pin check"
    assert len(pin_texts()) >= 5, "finds the real getting-started, tutorials and templates"
    assert "book/src/3d.md" in pin_texts() and not any("migrating-to-" in n for n in pin_texts()), \
        "every chapter's pins, but not the migration chapters' old code"
    # The bump: package versions, sibling requirements anywhere, the docs' pins.
    files = {
        "Cargo.toml": '[package]\nname = "ponko"\nversion = "0.1.0"\n\n[dependencies]\n'
                      'ponko_ecs = { path = "crates/ponko_ecs", version = "0.1.0", features = ["x"] }\n'
                      'serde = { version = "1.0.1" }\n',
        "crates/ponko_bench/Cargo.toml": '[package]\nname = "ponko_bench"\nversion = "0.1.0"\npublish = false\n'
                                         '[dependencies]\nponko = { path = "../..", version = "0.1.0" }\n',
        "book/src/3d.md": 'x\nponko = { version = "0.1", features = ["3d"] }\nponko = "0.1"\n',
    }
    out = bumped("0.2.0-rc.1", files, {"ponko", "ponko_ecs"})
    assert 'version = "0.2.0-rc.1"\n' in out["Cargo.toml"] and 'version = "0.2.0-rc.1", features' in out["Cargo.toml"]
    assert 'serde = { version = "1.0.1" }' in out["Cargo.toml"], "other crates keep their requirement"
    assert out["crates/ponko_bench/Cargo.toml"].count('"0.1.0"') == 1, "an unpublished crate keeps its own version"
    assert out["book/src/3d.md"] == 'x\nponko = { version = "0.2", features = ["3d"] }\nponko = "0.2"\n'
    assert pin_problems("0.2.0", {k: v for k, v in out.items() if k.endswith(".md")}) == []
    real = license_problems()
    assert real == [], f"the sub-crates' license files: {real}"
    names = {n for n, _, _ in crates()}
    assert "ponko" in names and "ponko_ecs" in names, "reads the real workspace"
    print("release-check.py: self-test passed")


def main():
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "check" and len(sys.argv) > 2:
        return check(sys.argv[2], "--on-branch" in sys.argv)
    if command == "notes" and len(sys.argv) > 3:
        return notes(sys.argv[2], sys.argv[3])
    if command == "bump" and len(sys.argv) > 2:
        return bump(sys.argv[2], "--dry-run" in sys.argv)
    if command == "published":
        return published()
    if command == "selftest":
        return selftest()
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main())
