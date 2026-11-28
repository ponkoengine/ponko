#!/usr/bin/env python3
"""A new user's first run of a published release, from crates.io only.

    first-run.py VERSION [--dir DIR] [--templates 2d,3d,headless,server,plugin]
                         [--window] [--dry-run]
    first-run.py selftest

Run it on each OS after `cargo publish` (RELEASING.md, step 5.5), from
any folder outside the checkout: nothing here reads the repository, so it sees
what a user sees. It does what getting-started.md tells a new user to do:

1. `cargo install ponko_cli --version =VERSION --locked`, into DIR/cli (not
   your own ~/.cargo/bin), and `ponko --version` names VERSION.
2. `ponko new` each template, with the engine dependency set to exactly
   VERSION (a candidate, 0.2.0-rc.1, is never picked by `"0.2"`), then
   `cargo test` in each; the headless one also runs (`TICKS=60 cargo run`).
3. "Your first game": `cargo new`, `cargo add ponko@=VERSION`, the guide's
   first program in src/main.rs and a `cargo build`. With `--window` it also
   runs it (it opens a window; press Escape).

Every step prints its command; the first failure stops the run and the exit
code is 1. `--dry-run` prints the commands without running them. DIR defaults
to a new temporary folder, which is kept so a failure can be looked at; delete
it afterwards (each template is a full engine build).
"""

import argparse
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

TEMPLATES = ("2d", "3d", "headless", "server", "plugin")
SEMVER = re.compile(r"^\d+\.\d+\.\d+(-rc\.[1-9]\d*)?$")

# The guide's first program (book/src/getting-started.md, "Your first game"),
# with the texture it loads made by the script.
FIRST_GAME = """\
use ponko::prelude::*;

fn main() {
    Game::new("My Game", 800, 600).run(|ctx| {
        let crab = ctx.gfx.load_texture("crab.png");
        let mut pos = vec2(400.0, 300.0);

        move |ctx: &mut Ctx| {
            if ctx.input.key_down(Key::Right) { pos.x += 200.0 * ctx.time.dt(); }
            if ctx.input.key_down(Key::Left)  { pos.x -= 200.0 * ctx.time.dt(); }
            if ctx.input.key_pressed(Key::Escape) { ctx.quit(); }
            ctx.gfx.draw(crab, pos);
        }
    });
}
"""

def pixel_png():
    """A 1x1 opaque PNG, so the first program finds its texture."""
    def chunk(kind, body):
        return (struct.pack(">I", len(body)) + kind + body
                + struct.pack(">I", zlib.crc32(kind + body)))
    header = struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0)
    pixels = zlib.compress(b"\x00\xff\x80\x40\xff")
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", pixels) + chunk(b"IEND", b""))


def pin_engine(manifest, version):
    """The manifest with its `ponko` dependency set to exactly `version`.

    Handles both forms the templates use: `ponko = "X.Y"` and
    `ponko = { version = "X.Y", ... }`. Returns the new text and how many
    lines changed (a template without a `ponko` line is an error).
    """
    exact = f'"={version}"'
    text, n1 = re.subn(r'(?m)^(ponko\s*=\s*)"[^"]*"', rf"\g<1>{exact}", manifest)
    text, n2 = re.subn(
        r'(?m)^(ponko\s*=\s*\{[^}\n]*?\bversion\s*=\s*)"[^"]*"',
        rf"\g<1>{exact}",
        text,
    )
    return text, n1 + n2


class Runner:
    def __init__(self, dry_run):
        self.dry_run = dry_run

    def run(self, args, cwd, env=None, capture=False):
        shown = " ".join(args)
        prefix = " ".join(f"{k}={v}" for k, v in (env or {}).items())
        print(f"$ (cd {cwd} && {prefix + ' ' if prefix else ''}{shown})", flush=True)
        if self.dry_run:
            return ""
        full_env = dict(os.environ, **(env or {}))
        result = subprocess.run(
            args, cwd=cwd, env=full_env, text=True,
            stdout=subprocess.PIPE if capture else None,
        )
        if result.returncode != 0:
            raise SystemExit(f"first-run: failed ({result.returncode}): {shown}")
        return result.stdout or ""


def first_run(version, root, templates, window, dry_run):
    r = Runner(dry_run)
    if not dry_run:
        os.makedirs(root, exist_ok=True)
    cli_root = os.path.join(root, "cli")
    ponko = os.path.join(cli_root, "bin", "ponko.exe" if os.name == "nt" else "ponko")

    # 1. The command-line tool.
    r.run(["cargo", "install", "ponko_cli", "--version", f"={version}", "--locked",
           "--root", cli_root], cwd=root)
    said = r.run([ponko, "--version"], cwd=root, capture=True)
    if not dry_run and version not in said:
        raise SystemExit(f"first-run: `ponko --version` says {said.strip()!r}, not {version}")

    # 2. Each template, against exactly this release.
    for kind in templates:
        name = f"game-{kind}"
        project = os.path.join(root, name)
        r.run([ponko, "new", name, f"--{kind}"], cwd=root)
        manifest = os.path.join(project, "Cargo.toml")
        print(f"# pin ponko = \"={version}\" in {manifest}")
        if not dry_run:
            with open(manifest, encoding="utf-8") as f:
                text, changed = pin_engine(f.read(), version)
            if not changed:
                raise SystemExit(f"first-run: no `ponko` dependency in {manifest}")
            with open(manifest, "w", encoding="utf-8") as f:
                f.write(text)
        r.run(["cargo", "test"], cwd=project)
        if kind == "headless":
            r.run(["cargo", "run"], cwd=project, env={"TICKS": "60"})

    # 3. "Your first game", by hand.
    game = os.path.join(root, "my_game")
    r.run(["cargo", "new", "my_game"], cwd=root)
    r.run(["cargo", "add", f"ponko@={version}"], cwd=game)
    print(f"# write the guide's first program to {game}/src/main.rs and a crab.png")
    if not dry_run:
        with open(os.path.join(game, "src", "main.rs"), "w", encoding="utf-8") as f:
            f.write(FIRST_GAME)
        with open(os.path.join(game, "crab.png"), "wb") as f:
            f.write(pixel_png())
    r.run(["cargo", "build"], cwd=game)
    if window:
        r.run(["cargo", "run"], cwd=game)
    print(f"first-run: {version} works from crates.io ({', '.join(templates)} and the first game)."
          f" Delete {root} when done.")


def selftest():
    text, n = pin_engine('[dependencies]\nponko = { version = "0.2", features = ["3d"] }\n', "0.2.0-rc.1")
    assert n == 1 and 'ponko = { version = "=0.2.0-rc.1", features = ["3d"] }' in text, text
    text, n = pin_engine('[dependencies]\nponko = "0.2"\nponko_ecs = "0.2"\n', "0.2.0")
    assert n == 1 and 'ponko = "=0.2.0"' in text and 'ponko_ecs = "0.2"' in text, text
    text, n = pin_engine('[dependencies]\nserde = "1"\n', "0.2.0")
    assert n == 0
    text, n = pin_engine('ponko = { version = "0.2", default-features = false }\n', "0.2.0")
    assert n == 1 and '"=0.2.0", default-features = false' in text, text
    assert SEMVER.match("0.2.0") and SEMVER.match("0.2.0-rc.1")
    assert not SEMVER.match("0.2") and not SEMVER.match("0.2.0-rc.0") and not SEMVER.match("v0.2.0")
    png = pixel_png()
    i, kinds = 8, []
    while i < len(png):
        (n,) = struct.unpack(">I", png[i:i + 4])
        kind, body = png[i + 4:i + 8], png[i + 8:i + 8 + n]
        assert struct.unpack(">I", png[i + 8 + n:i + 12 + n])[0] == zlib.crc32(kind + body)
        kinds.append(kind)
        i += 12 + n
    assert kinds == [b"IHDR", b"IDAT", b"IEND"], kinds
    # The first program is the guide's, word for word.
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, "..", "book", "src", "getting-started.md"), encoding="utf-8") as f:
        assert FIRST_GAME in f.read(), "FIRST_GAME differs from getting-started.md's first program"
    print("first-run.py selftest: ok")


def main():
    if sys.argv[1:] == ["selftest"]:
        return selftest()
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("version", help="the published version, X.Y.Z or X.Y.Z-rc.N")
    p.add_argument("--dir", help="where to make the projects (default: a new temporary folder)")
    p.add_argument("--templates", default=",".join(TEMPLATES),
                   help=f"comma-separated, from {', '.join(TEMPLATES)}")
    p.add_argument("--window", action="store_true", help="also run the first game (opens a window)")
    p.add_argument("--dry-run", action="store_true", help="print the commands only")
    a = p.parse_args()
    version = a.version.removeprefix("v")
    if not SEMVER.match(version):
        p.error(f"not a release version: {a.version}")
    templates = [t for t in a.templates.split(",") if t]
    unknown = [t for t in templates if t not in TEMPLATES]
    if unknown:
        p.error(f"unknown template(s): {', '.join(unknown)}")
    if not shutil.which("cargo"):
        p.error("cargo is not on PATH")
    root = a.dir or (os.path.join(tempfile.gettempdir(), f"ponko-first-run-{version}")
                     if a.dry_run else tempfile.mkdtemp(prefix=f"ponko-first-run-{version}-"))
    if a.dir and not a.dry_run and os.path.isdir(a.dir) and os.listdir(a.dir):
        p.error(f"{a.dir} is not empty")
    first_run(version, os.path.abspath(root), templates, a.window, a.dry_run)


if __name__ == "__main__":
    main()
