#!/usr/bin/env python3
"""Fetch well-known third-party glTF content into a local cache.

The render tests draw scenes they build themselves. This tool downloads
content someone else made in a real tool, at pinned revisions, so the
engine's loader and renderer can be checked against it:

- samples: every model of the Khronos glTF Sample Assets (one variant
  each: the binary .glb, else the .gltf with its files), with its reference
  screenshot, its metadata.json and its license. Sponza is one of them.
- bistro: Amazon Lumberyard Bistro, the large scene, as a glTF with BC7
  DDS textures (`MSFT_texture_dds`).

Nothing downloaded belongs in the repository: the default cache is
`.cache/known-content` at the repository's root (git-ignored), or the
folder `PONKO_KNOWN_CONTENT` names (share one cache between checkouts).
The tool writes `manifest.json` (what was fetched, from where, and each
asset's license) and `LICENSES.md` there. Files already in the cache are
not fetched again, so a second run only checks the cache.

Usage:
  python3 scripts/fetch-known-content.py                 # samples (~0.65 GB)
  python3 scripts/fetch-known-content.py --set all       # samples and the Bistro (~3.1 GB)
  python3 scripts/fetch-known-content.py --model Sponza --model Box
  python3 scripts/fetch-known-content.py --list          # what is in the cache
  python3 scripts/fetch-known-content.py --selftest

The load sweep (`tests/known_content.rs`), the drawing comparison and the
scene measurements (`src/render/known_content_render_test.rs`) read the
manifest; the guide's "Well-known content" chapter has the results.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import os
import posixpath
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# The Khronos glTF Sample Assets, pinned. Raise it on purpose (and redo the
# guide's table): a new revision adds and changes models.
SAMPLES_REPO = "KhronosGroup/glTF-Sample-Assets"
SAMPLES_REV = "e124e8d8d7330a5372bc2b4d00d39a4a9d7997ea"

# Amazon Lumberyard Bistro as glTF: NVIDIA's RTXDI conversion with BC7 DDS
# textures, lightly edited (a camera, a sun, point lights), pinned.
BISTRO_REPO = "zeux/niagara_bistro"
BISTRO_REV = "2bdb6a410f8ebd475d3737e7c8e038ed2b00b02e"

# Which variant of a sample is loaded: the plain ones first. A model that
# only comes in an extension's variant is fetched in the first listed.
VARIANTS = ("glTF-Binary", "glTF", "glTF-Embedded")

USER_AGENT = "ponko-fetch-known-content"


def raw_url(repo: str, rev: str, path: str) -> str:
    return f"https://raw.githubusercontent.com/{repo}/{rev}/{urllib.parse.quote(path)}"


def default_cache() -> Path:
    env = os.environ.get("PONKO_KNOWN_CONTENT")
    return Path(env) if env else ROOT / ".cache" / "known-content"


def download(url: str, dest: Path, retries: int = 4) -> int:
    """Fetches `url` into `dest` (written whole, then renamed): its size.
    A file already there is kept."""
    if dest.is_file():
        return dest.stat().st_size
    dest.parent.mkdir(parents=True, exist_ok=True)
    partial = dest.with_name(dest.name + ".part")
    for attempt in range(retries):
        try:
            request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(request, timeout=120) as response, open(partial, "wb") as out:
                while True:
                    chunk = response.read(1 << 20)
                    if not chunk:
                        break
                    out.write(chunk)
            partial.replace(dest)
            return dest.stat().st_size
        except urllib.error.HTTPError as e:
            if e.code == 404:
                raise FileNotFoundError(f"{url}: not found (404)") from e
            error = e
        except (urllib.error.URLError, TimeoutError, ConnectionError, OSError) as e:
            error = e
        time.sleep(2 * (attempt + 1))
    raise RuntimeError(f"{url}: {error}")


def fetch_text(url: str) -> str:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read().decode("utf-8")


def referenced_files(gltf: dict) -> list[str]:
    """The files a .gltf names (buffers and images), as paths relative to
    it, URI escapes decoded; inline `data:` URIs are left out."""
    out = []
    for key in ("buffers", "images"):
        for item in gltf.get(key, []):
            uri = item.get("uri")
            if uri and not uri.startswith("data:"):
                out.append(urllib.parse.unquote(uri))
    return sorted(set(out))


def safe_join(folder: str, relative: str) -> str:
    """`relative` (a URI from a downloaded file) under `folder`: never
    above it, never absolute."""
    joined = posixpath.normpath(posixpath.join(folder, relative))
    if relative.startswith("/") or ".." in joined.split("/") or not joined.startswith(folder.rstrip("/") + "/"):
        raise ValueError(f"{relative!r} points outside {folder!r}")
    return joined


def pick_variant(variants: dict) -> tuple[str, str]:
    for name in VARIANTS:
        if name in variants:
            return name, variants[name]
    name = sorted(variants)[0]
    return name, variants[name]


def license_entries(metadata: dict) -> list[dict]:
    out = []
    for legal in metadata.get("legal", []):
        out.append(
            {
                "license": legal.get("license", ""),
                "owner": legal.get("owner") or legal.get("artist", ""),
                "year": legal.get("year", ""),
                "what": legal.get("what", ""),
            }
        )
    return out


def fetch_sample(cache: Path, entry: dict) -> dict:
    """One sample model: its chosen variant with every file it names, its
    screenshot, metadata and license. The manifest's entry for it."""
    name = entry["name"]
    base = f"Models/{name}"
    variant, main = pick_variant(entry["variants"])
    folder = f"{base}/{variant}"
    local = cache / "samples" / name
    files = [f"{folder}/{main}", f"{base}/metadata.json", f"{base}/LICENSE.md"]
    if entry.get("screenshot"):
        files.append(f"{base}/{entry['screenshot']}")
    total = 0
    for path in files:
        total += download(raw_url(SAMPLES_REPO, SAMPLES_REV, path), local / path[len(base) + 1 :])
    if main.endswith(".gltf"):
        gltf = json.loads((local / variant / main).read_text(encoding="utf-8"))
        for relative in referenced_files(gltf):
            path = safe_join(folder, relative)
            total += download(raw_url(SAMPLES_REPO, SAMPLES_REV, path), local / path[len(base) + 1 :])
    metadata = json.loads((local / "metadata.json").read_text(encoding="utf-8"))
    return {
        "name": name,
        "set": "samples",
        "label": entry.get("label", name),
        "file": f"samples/{name}/{variant}/{main}",
        "variant": variant,
        "variants": sorted(entry["variants"]),
        "screenshot": f"samples/{name}/{entry['screenshot']}" if entry.get("screenshot") else None,
        "tags": entry.get("tags", []),
        "licenses": license_entries(metadata),
        "source": f"https://github.com/{SAMPLES_REPO}/tree/{SAMPLES_REV}/{urllib.parse.quote(base)}",
        "bytes": total,
    }


def fetch_bistro(cache: Path, jobs: int) -> dict:
    local = cache / "bistro"
    total = download(raw_url(BISTRO_REPO, BISTRO_REV, "bistro.gltf"), local / "bistro.gltf")
    total += download(raw_url(BISTRO_REPO, BISTRO_REV, "LICENSE"), local / "LICENSE")
    gltf = json.loads((local / "bistro.gltf").read_text(encoding="utf-8"))
    # The file names a PNG and a DDS file for each texture; the repository
    # holds the DDS ones only (BC7), read through MSFT_texture_dds.
    wanted = [f for f in referenced_files(gltf) if not f.lower().endswith(".png")]
    paths = [safe_join("bistro", f)[len("bistro/") :] for f in wanted]
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        sizes = list(
            pool.map(lambda p: download(raw_url(BISTRO_REPO, BISTRO_REV, p), local / p), paths)
        )
    total += sum(sizes)
    return {
        "name": "Bistro",
        "set": "bistro",
        "label": "Amazon Lumberyard Bistro",
        "file": "bistro/bistro.gltf",
        "variant": "glTF (DDS textures)",
        "variants": ["glTF"],
        "screenshot": None,
        "tags": ["scene"],
        "licenses": [
            {
                "license": "CC-BY-4.0",
                "owner": "Amazon Lumberyard",
                "year": "2017",
                "what": "The Bistro scene (models and textures), as first published",
            },
            {
                "license": "MIT",
                "owner": "NVIDIA GameWorks",
                "year": "2023",
                "what": "The glTF conversion (rtxdi-assets), per the repository's LICENSE",
            },
        ],
        "source": f"https://github.com/{BISTRO_REPO}/tree/{BISTRO_REV}",
        "bytes": total,
    }


def licenses_markdown(manifest: dict) -> str:
    lines = [
        "# Licenses of the fetched content",
        "",
        "Written by scripts/fetch-known-content.py. None of this content is part",
        "of the repository; each asset keeps its own license, listed here.",
        "",
        "| Asset | Set | License | Owner | Source |",
        "|---|---|---|---|---|",
    ]
    for asset in manifest["assets"]:
        for legal in asset["licenses"] or [{"license": "(none listed)", "owner": ""}]:
            lines.append(
                f"| {asset['name']} | {asset['set']} | {legal['license']} | {legal['owner']} | {asset['source']} |"
            )
    return "\n".join(lines) + "\n"


def write_manifest(cache: Path, assets: list[dict]) -> dict:
    path = cache / "manifest.json"
    previous = json.loads(path.read_text(encoding="utf-8")) if path.is_file() else {"assets": []}
    by_name = {(a["set"], a["name"]): a for a in previous.get("assets", [])}
    for asset in assets:
        by_name[(asset["set"], asset["name"])] = asset
    manifest = {
        "version": 1,
        "samples": {"repository": SAMPLES_REPO, "revision": SAMPLES_REV},
        "bistro": {"repository": BISTRO_REPO, "revision": BISTRO_REV},
        "assets": sorted(by_name.values(), key=lambda a: (a["set"], a["name"].lower())),
    }
    path.write_text(json.dumps(manifest, indent=1) + "\n", encoding="utf-8")
    (cache / "LICENSES.md").write_text(licenses_markdown(manifest), encoding="utf-8")
    return manifest


def selftest() -> None:
    gltf = {
        "buffers": [{"uri": "a%20b.bin"}, {"uri": "data:application/octet-stream;base64,AAAA"}],
        "images": [{"uri": "tex/c.png"}, {"bufferView": 0}],
    }
    assert referenced_files(gltf) == ["a b.bin", "tex/c.png"], referenced_files(gltf)
    assert safe_join("Models/X/glTF", "tex/c.png") == "Models/X/glTF/tex/c.png"
    for bad in ("../../secret", "/etc/passwd", "a/../../b"):
        try:
            safe_join("Models/X/glTF", bad)
        except ValueError:
            continue
        raise AssertionError(f"{bad} was allowed")
    assert pick_variant({"glTF": "a.gltf", "glTF-Binary": "a.glb"}) == ("glTF-Binary", "a.glb")
    assert pick_variant({"glTF-Draco": "a.gltf"}) == ("glTF-Draco", "a.gltf")
    assert license_entries({"legal": [{"license": "CC-BY-4.0", "artist": "A", "year": "1", "what": "w"}]}) == [
        {"license": "CC-BY-4.0", "owner": "A", "year": "1", "what": "w"}
    ]
    print("fetch-known-content selftest: ok")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--cache", type=Path, default=None, help="cache folder (default: PONKO_KNOWN_CONTENT or .cache/known-content)")
    parser.add_argument("--set", choices=("samples", "bistro", "all"), default="samples")
    parser.add_argument("--model", action="append", default=[], help="only this sample (repeatable)")
    parser.add_argument("--jobs", type=int, default=8, help="parallel downloads")
    parser.add_argument("--list", action="store_true", help="print what the cache holds")
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()
    if args.selftest:
        selftest()
        return 0
    cache = (args.cache or default_cache()).resolve()
    if args.list:
        path = cache / "manifest.json"
        if not path.is_file():
            print(f"{cache}: empty (run without --list to fetch)")
            return 0
        manifest = json.loads(path.read_text(encoding="utf-8"))
        for asset in manifest["assets"]:
            licenses = ", ".join(l["license"] for l in asset["licenses"])
            print(f"{asset['set']:8} {asset['name']:40} {asset['bytes'] / 1e6:8.1f} MB  {licenses}")
        return 0
    cache.mkdir(parents=True, exist_ok=True)
    fetched: list[dict] = []
    failures: list[str] = []
    started = time.monotonic()
    if args.set in ("samples", "all"):
        index = json.loads(fetch_text(raw_url(SAMPLES_REPO, SAMPLES_REV, "Models/model-index.json")))
        if args.model:
            unknown = set(args.model) - {e["name"] for e in index}
            if unknown:
                print(f"no such sample: {', '.join(sorted(unknown))}", file=sys.stderr)
                return 2
            index = [e for e in index if e["name"] in args.model]
        print(f"samples: {len(index)} models from {SAMPLES_REPO}@{SAMPLES_REV[:10]} into {cache}")
        with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
            futures = {pool.submit(fetch_sample, cache, e): e["name"] for e in index}
            for future in concurrent.futures.as_completed(futures):
                try:
                    fetched.append(future.result())
                except Exception as e:  # noqa: BLE001 - report each model's failure and go on
                    failures.append(f"{futures[future]}: {e}")
        # The license texts the samples' metadata points at.
        for license_id in sorted({l["license"] for a in fetched for l in a["licenses"] if l["license"]}):
            try:
                download(raw_url(SAMPLES_REPO, SAMPLES_REV, f"LICENSES/{license_id}.txt"), cache / "licenses" / f"{license_id}.txt")
            except FileNotFoundError:
                pass
    if args.set in ("bistro", "all"):
        print(f"bistro: {BISTRO_REPO}@{BISTRO_REV[:10]} into {cache / 'bistro'} (~2.4 GB)")
        try:
            fetched.append(fetch_bistro(cache, max(1, args.jobs)))
        except Exception as e:  # noqa: BLE001
            failures.append(f"Bistro: {e}")
    manifest = write_manifest(cache, fetched)
    total = sum(a["bytes"] for a in fetched)
    print(
        f"{len(fetched)} assets ({total / 1e6:.0f} MB) in {time.monotonic() - started:.0f} s; "
        f"the cache lists {len(manifest['assets'])}; licenses in {cache / 'LICENSES.md'}"
    )
    for failure in failures:
        print(f"failed: {failure}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
