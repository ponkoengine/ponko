#!/usr/bin/env python3
"""Builds the C library archives of a release, and smoke-builds the C++ examples against one.

    python3 scripts/c-library.py targets
    python3 scripts/c-library.py build [--target TRIPLE] [--version vX.Y.Z] [--out DIR]
                                       [--zig] [--glibc 2.28] [--sysroot DIR]
                                       [--no-build] [--dry-run]
    python3 scripts/c-library.py sysroot --out DIR PACKAGE.deb...
    python3 scripts/c-library.py smoke ARCHIVE [--link-only] [--cmake-arg ARG]... [--keep]
                                       [--zig [--glibc 2.28]] [--docker [--image IMAGE]]
    python3 scripts/c-library.py selftest

`build` makes one archive, `ponko-vX.Y.Z-<os>-<arch>.tar.gz` (`.zip` for
Windows), in `--out` (default `target/c-library`), holding one folder of the
same name:

    include/ponko.h, include/ponko.hpp
    lib/        the shared library and the static library of `ponko_ffi`
                (libponko_ffi.so + .a, libponko_ffi.dylib + .a, or
                ponko_ffi.dll + its import library ponko_ffi.dll.lib +
                ponko_ffi.lib)
    LICENSE-MIT, LICENSE-APACHE, NOTICE
    THIRD-PARTY-NOTICES.txt   every library linked into ponko_ffi on that
                              target (`ponko notices --target`)

It runs `cargo build --release --locked -p ponko_ffi --target TRIPLE` (with
`ponko_ffi`'s default features), checks that the shared library really is for
that target's CPU, and packs the folder. `--target` defaults to this machine.
The six release targets are in `targets`. Cross-compiling Linux needs a C
linker for the target: `--zig` builds with `cargo zigbuild` (zig as the
linker, glibc `--glibc`, default 2.28), and `--sysroot` points pkg-config at
the target's ALSA and udev libraries (make one with `sysroot`).
`--dry-run` prints the commands and the archive layout without running
anything.

`sysroot` unpacks Debian/Ubuntu packages (`.deb`) into a folder, enough for a
cross build to link against: for linux-arm64 the arm64 packages of
libasound2, libasound2-dev, libudev1 and libudev-dev.

`smoke` unpacks an archive into a fresh folder (a path with a space), builds
`examples/cpp` against its `include/` and `lib/` with CMake, as the C/C++
chapter of the guide tells users to, and runs the C and C++ test programs with
ctest. An Intel macOS archive on an Apple Silicon Mac builds for x86_64 and
runs under Rosetta. `--link-only` stops after the build: for an archive whose
programs can't run here (Windows arm64 on an x64 machine builds with
`-A ARM64`; other cross builds pass a toolchain with `--cmake-arg`).
`--zig` compiles the programs for a Linux archive's CPU with zig (link only
on another CPU). `--docker` runs the whole smoke of a Linux archive inside a
container instead, on a machine that has Docker (Docker Desktop on a Mac
runs arm64 Linux natively on Apple silicon): the image, Rocky Linux 8 by
default, has glibc 2.28, the oldest the archives support, so the test also
proves that floor. It needs the network (to install CMake and a compiler).

Exit codes: 0 done, 1 a step failed, 3 bad arguments. RELEASING.md,
"The C library archives", has the procedure, per machine.
"""

import argparse
import os
import platform
import shlex
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATE = "ponko_ffi"
TITLE = "the Ponko C library"
HEADERS = ("ponko.h", "ponko.hpp")
LICENSE_FILES = ("LICENSE-MIT", "LICENSE-APACHE", "NOTICE")
NOTICES = "THIRD-PARTY-NOTICES.txt"
DEFAULT_GLIBC = "2.28"
# The container `smoke --docker` runs in: its glibc is the floor of the Linux
# archives, so a library that needs a newer one fails there.
DOCKER_IMAGE = "rockylinux:8"

# Target triple -> archive name, in release order.
TARGETS = {
    "x86_64-unknown-linux-gnu": "linux-x86_64",
    "aarch64-unknown-linux-gnu": "linux-arm64",
    "aarch64-apple-darwin": "macos-arm64",
    "x86_64-apple-darwin": "macos-x86_64",
    "x86_64-pc-windows-msvc": "windows-x86_64",
    "aarch64-pc-windows-msvc": "windows-arm64",
}

# Where the archive's libraries come from in target/<triple>/release; the
# first one is the shared library, checked for the target's CPU.
LIBRARIES = {
    "linux": ("libponko_ffi.so", "libponko_ffi.a"),
    "macos": ("libponko_ffi.dylib", "libponko_ffi.a"),
    "windows": ("ponko_ffi.dll", "ponko_ffi.dll.lib", "ponko_ffi.lib"),
}


class Failure(Exception):
    """A step failed (exit code 1)."""


class Usage(Exception):
    """Bad arguments (exit code 3)."""


def target_os(triple):
    if "-linux-" in triple:
        return "linux"
    if "-apple-darwin" in triple:
        return "macos"
    if "-windows-" in triple:
        return "windows"
    raise Usage(f"not a release target: {triple} (see `targets`)")


def target_arch(triple):
    return triple.split("-", 1)[0]


def archive_name(version, triple):
    if triple not in TARGETS:
        raise Usage(f"not a release target: {triple}; the release targets are: {', '.join(TARGETS)}")
    ext = "zip" if target_os(triple) == "windows" else "tar.gz"
    return f"ponko-{version}-{TARGETS[triple]}", ext


def root_version():
    """The workspace version from the root Cargo.toml, as a tag (vX.Y.Z)."""
    in_package = False
    for line in (ROOT / "Cargo.toml").read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line.startswith("["):
            in_package = line in ("[package]", "[workspace.package]")
        elif in_package and line.startswith("version") and "=" in line:
            return "v" + line.split("=", 1)[1].strip().strip('"')
    raise Failure("no version in the root Cargo.toml")


def host_triple():
    out = subprocess.run(["rustc", "-vV"], capture_output=True, text=True, check=True).stdout
    for line in out.splitlines():
        if line.startswith("host:"):
            return line.split(":", 1)[1].strip()
    raise Failure("rustc -vV names no host")


def multiarch(triple):
    """Debian's library folder name for a Linux triple (aarch64-linux-gnu)."""
    return triple.replace("-unknown-", "-")


def build_command(triple, zig=False, glibc=DEFAULT_GLIBC):
    if zig:
        if target_os(triple) != "linux":
            raise Usage("--zig is for the Linux targets; build macOS and Windows with their own toolchains")
        return ["cargo", "zigbuild", "--release", "--locked", "-p", CRATE, "--target", f"{triple}.{glibc}"]
    return ["cargo", "build", "--release", "--locked", "-p", CRATE, "--target", triple]


def build_env(triple, sysroot=None):
    """Environment additions for the build: pkg-config looks in the sysroot."""
    if sysroot is None:
        return {}
    sysroot = Path(sysroot).resolve()
    libdirs = [
        sysroot / "usr/lib" / multiarch(triple) / "pkgconfig",
        sysroot / "usr/lib/pkgconfig",
        sysroot / "usr/share/pkgconfig",
    ]
    return {
        "PKG_CONFIG_ALLOW_CROSS": "1",
        "PKG_CONFIG_SYSROOT_DIR": str(sysroot),
        "PKG_CONFIG_LIBDIR": os.pathsep.join(str(d) for d in libdirs),
    }


def notices_command(triple, out):
    return [
        "cargo", "run", "--quiet", "-p", "ponko_cli", "--", "notices", "-p", CRATE,
        "--target", triple, "--title", TITLE, "--out", str(out),
    ]


# ---- which CPU a library is for ---------------------------------------------

ELF_MACHINES = {62: "x86_64", 183: "aarch64"}
MACHO_CPUS = {0x01000007: "x86_64", 0x0100000C: "aarch64"}
PE_MACHINES = {0x8664: "x86_64", 0xAA64: "aarch64"}


def binary_arches(data):
    """The CPUs a shared library is built for, from its header (ELF, Mach-O or PE)."""
    if data[:4] == b"\x7fELF":
        endian = "<" if data[5] == 1 else ">"
        return [ELF_MACHINES.get(struct.unpack_from(endian + "H", data, 18)[0], "unknown")]
    magic = struct.unpack_from("<I", data, 0)[0]
    if magic in (0xFEEDFACF, 0xFEEDFACE):
        return [MACHO_CPUS.get(struct.unpack_from("<i", data, 4)[0] & 0xFFFFFFFF, "unknown")]
    if data[:4] in (b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca"):  # universal (fat) Mach-O
        count = struct.unpack_from(">I", data, 4)[0]
        return [MACHO_CPUS.get(struct.unpack_from(">I", data, 8 + 20 * i)[0], "unknown") for i in range(count)]
    if data[:2] == b"MZ":
        pe = struct.unpack_from("<I", data, 0x3C)[0]
        if data[pe : pe + 4] == b"PE\0\0":
            return [PE_MACHINES.get(struct.unpack_from("<H", data, pe + 4)[0], "unknown")]
    return ["unknown"]


def check_arch(path, triple):
    with open(path, "rb") as f:
        head = f.read(4096)
    found = binary_arches(head)
    if found != [target_arch(triple)]:
        raise Failure(f"{path} is built for {', '.join(found)}, not {target_arch(triple)} ({triple})")


# ---- build ------------------------------------------------------------------

# A cdylib's install name is the path it was linked at (inside target/), so a
# program linked against it would look for it there. The archive's library
# says @rpath instead: programs find it through their rpath, such as
# @executable_path when it ships next to them.
INSTALL_NAME = "@rpath/libponko_ffi.dylib"


def install_name(dylib):
    out = subprocess.run(["otool", "-D", str(dylib)], capture_output=True, text=True, check=True).stdout
    return out.splitlines()[-1].strip()


def set_install_name(dylib):
    run(["install_name_tool", "-id", INSTALL_NAME, str(dylib)])
    # Changing the header invalidates the linker's ad-hoc signature, which
    # Apple Silicon requires: sign it again (ad hoc, as the linker did).
    run(["codesign", "--force", "--sign", "-", str(dylib)])
    if install_name(dylib) != INSTALL_NAME:
        raise Failure(f"{dylib}: install name is {install_name(dylib)}, not {INSTALL_NAME}")


def layout(triple):
    """The files of an archive, relative to its folder."""
    files = [f"include/{h}" for h in HEADERS]
    files += [f"lib/{lib}" for lib in LIBRARIES[target_os(triple)]]
    files += list(LICENSE_FILES) + [NOTICES]
    return files


def run(cmd, env=None):
    print("+", shlex.join(cmd), flush=True)
    full = dict(os.environ, **(env or {}))
    if subprocess.run(cmd, cwd=ROOT, env=full).returncode != 0:
        raise Failure(f"failed: {shlex.join(cmd)}")


def pack(folder, archive):
    """Packs `folder` (with the folder itself as the top entry) into `archive`."""
    archive.unlink(missing_ok=True)
    paths = sorted(p for p in folder.rglob("*") if p.is_file())
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as z:
            for p in paths:
                z.write(p, f"{folder.name}/{p.relative_to(folder).as_posix()}")
    else:
        def normal(info):
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            info.mode = 0o755 if info.isdir() else 0o644
            return info

        with tarfile.open(archive, "w:gz") as t:
            t.add(folder, folder.name, filter=normal)


def cmd_build(args):
    triple = args.target or host_triple()
    version = args.version or root_version()
    name, ext = archive_name(version, triple)
    if args.sysroot and target_os(triple) != "linux":
        raise Usage("--sysroot is for the Linux targets")
    out = Path(args.out).resolve() if args.out else ROOT / "target" / "c-library"
    folder, archive = out / name, out / f"{name}.{ext}"
    release = ROOT / "target" / triple / "release"
    build = build_command(triple, args.zig, args.glibc)
    env = build_env(triple, args.sysroot)
    if args.dry_run:
        for k, v in env.items():
            print(f"{k}={v}")
        if not args.no_build:
            print("+", shlex.join(build))
        print("+", shlex.join(notices_command(triple, folder / NOTICES)))
        print(f"{archive}:")
        for f in layout(triple):
            print(f"  {name}/{f}")
        return 0
    if not args.no_build:
        run(build, env)
    libs = LIBRARIES[target_os(triple)]
    missing = [lib for lib in libs if not (release / lib).is_file()]
    if missing:
        raise Failure(f"not built: {', '.join(missing)} in {release} (leave out --no-build?)")
    check_arch(release / libs[0], triple)
    if folder.exists():
        shutil.rmtree(folder)
    (folder / "include").mkdir(parents=True)
    (folder / "lib").mkdir()
    for h in HEADERS:
        shutil.copy2(ROOT / "include" / h, folder / "include" / h)
    for f in LICENSE_FILES:
        shutil.copy2(ROOT / f, folder / f)
    for lib in libs:
        shutil.copy2(release / lib, folder / "lib" / lib)
    if target_os(triple) == "macos":
        set_install_name(folder / "lib" / libs[0])
    run(notices_command(triple, folder / NOTICES))
    have = sorted(p.relative_to(folder).as_posix() for p in folder.rglob("*") if p.is_file())
    if have != sorted(layout(triple)):
        raise Failure(f"{folder} holds {have}, expected {sorted(layout(triple))}")
    pack(folder, archive)
    print(f"wrote {archive} ({archive.stat().st_size / 1e6:.1f} MB)")
    return 0


# ---- sysroot ----------------------------------------------------------------


def deb_members(data):
    """The members of an `ar` archive (a .deb), as (name, bytes)."""
    if data[:8] != b"!<arch>\n":
        raise Failure("not a .deb (ar) archive")
    pos, members = 8, []
    while pos + 60 <= len(data):
        header = data[pos : pos + 60]
        name = header[:16].decode().strip().rstrip("/")
        size = int(header[48:58].decode().strip())
        members.append((name, data[pos + 60 : pos + 60 + size]))
        pos += 60 + size + (size & 1)
    return members


def cmd_sysroot(args):
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    for deb in args.debs:
        members = dict(deb_members(Path(deb).read_bytes()))
        names = [n for n in members if n.startswith("data.tar")]
        if not names:
            raise Failure(f"{deb}: no data.tar member")
        with tempfile.TemporaryDirectory() as tmp:
            data = Path(tmp) / names[0]
            data.write_bytes(members[names[0]])
            # zstd-compressed data (newer Ubuntu) needs the tar command.
            if names[0].endswith(".zst"):
                if subprocess.run(["tar", "-xf", str(data), "-C", str(out)]).returncode:
                    raise Failure(f"{deb}: can't unpack {names[0]} (tar with zstd needed)")
            else:
                with tarfile.open(data) as t:
                    t.extractall(out, filter="data")
        print(f"unpacked {deb}")
    # The -dev packages link libfoo.so to an absolute /usr/lib path; point
    # those links into the sysroot.
    for link in out.rglob("*.so"):
        if link.is_symlink() and os.readlink(link).startswith("/"):
            target = os.readlink(link)
            link.unlink()
            link.symlink_to(os.path.relpath(out / target.lstrip("/"), link.parent))
    print(f"sysroot: {out}")
    return 0


# ---- smoke ------------------------------------------------------------------


def archive_triple(path):
    stem = path.name.removesuffix(".tar.gz").removesuffix(".zip")
    for triple, name in TARGETS.items():
        if stem.endswith("-" + name):
            return triple
    raise Usage(f"{path.name} is not a C library archive (ponko-vX.Y.Z-<os>-<arch>)")


def host_os():
    return {"Darwin": "macos", "Linux": "linux", "Windows": "windows"}.get(platform.system(), "unknown")


def host_arch():
    machine = platform.machine().lower()
    if host_os() == "windows":
        # An x64 Python on a Windows on Arm machine says AMD64; the machine's
        # own CPU is in these variables.
        machine = (os.environ.get("PROCESSOR_ARCHITEW6432") or os.environ.get("PROCESSOR_ARCHITECTURE") or machine).lower()
    return {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)


def smoke_plan(triple, here_os, here_arch, rosetta, link_only):
    """(extra cmake arguments, whether to run the programs) for an archive, or Usage."""
    os_, arch = target_os(triple), target_arch(triple)
    if os_ != here_os:
        if link_only:
            return [], False
        raise Usage(f"a {TARGETS[triple]} archive builds on {os_}; here is {here_os} (cross builds: --link-only with a --cmake-arg toolchain)")
    if os_ == "windows" and arch == "aarch64":
        # Visual Studio's default platform follows the CMake process, which
        # may be an emulated x64 one on Windows on Arm: name the platform.
        return ["-A", "ARM64"], arch == here_arch and not link_only
    if arch == here_arch:
        return [], not link_only
    if os_ == "macos":
        cmake = [f"-DCMAKE_OSX_ARCHITECTURES={arch.replace('aarch64', 'arm64')}"]
        runs = arch == "x86_64" and rosetta and not link_only
        return cmake, runs
    if os_ == "windows":
        return ["-A", "ARM64" if arch == "aarch64" else "x64"], False
    if link_only:
        return [], False
    raise Usage(f"{TARGETS[triple]} programs can't run on {here_arch}: --link-only with a --cmake-arg toolchain")


# What runs inside the container: the archive is mounted at /archive and the
# source tree (examples/cpp and the assets) at /src, both read-only. The work
# folder has a space in its name, like the one the plain smoke makes.
DOCKER_SCRIPT = r"""set -eu
# The system gcc 8 has std::filesystem in a separate library: use a newer one.
dnf install -y -q cmake make gcc-toolset-12-gcc-c++ alsa-lib systemd-libs > /dev/null
. /opt/rh/gcc-toolset-12/enable
ldd --version | head -n 1
c++ --version | head -n 1
work="/work/ponko c library"
mkdir -p "$work"
tar -xzf "/archive/$ARCHIVE" -C "$work"
dir="$work/${ARCHIVE%.tar.gz}"
cmake -S /src/examples/cpp -B "$work/build" -DCMAKE_BUILD_TYPE=Release \
    "-DPONKO_INCLUDE_DIR=$dir/include" "-DPONKO_LIBRARY_DIR=$dir/lib"
cmake --build "$work/build" --config Release --parallel
ctest --test-dir "$work/build" -C Release --output-on-failure
"""


def docker_command(archive, triple, image):
    """The `docker run` command that smoke-tests a Linux archive in a container."""
    if target_os(triple) != "linux":
        raise Usage("--docker is for the Linux archives (the container is Linux)")
    platform_ = "linux/" + ("arm64" if target_arch(triple) == "aarch64" else "amd64")
    return [
        "docker", "run", "--rm", "--platform", platform_,
        "-v", f"{ROOT}:/src:ro", "-v", f"{archive.parent}:/archive:ro",
        "-e", f"ARCHIVE={archive.name}", image, "sh", "-c", DOCKER_SCRIPT,
    ]


def zig_toolchain(work, triple, glibc):
    """CMake arguments that build for a Linux target with zig as the C and C++ compiler."""
    if target_os(triple) != "linux":
        raise Usage("--zig is for the Linux archives")
    zig = shutil.which("zig")
    if not zig:
        raise Usage("--zig needs zig on PATH")
    target = f"{target_arch(triple)}-linux-gnu.{glibc}"
    wrappers = {}
    for lang, tool in (("C", "cc"), ("CXX", "c++")):
        path = work / f"zig-{tool}"
        path.write_text(f'#!/bin/sh\nexec {shlex.quote(zig)} {tool} -target {target} "$@"\n')
        path.chmod(0o755)
        wrappers[lang] = path
    args = [
        "-DCMAKE_SYSTEM_NAME=Linux",
        f"-DCMAKE_SYSTEM_PROCESSOR={target_arch(triple)}",
        f"-DCMAKE_C_COMPILER={wrappers['C']}",
        f"-DCMAKE_CXX_COMPILER={wrappers['CXX']}",
    ]
    return args


def cmd_smoke(args):
    archive = Path(args.archive).resolve()
    triple = archive_triple(archive)
    rosetta = host_os() == "macos" and subprocess.run(["arch", "-x86_64", "true"], capture_output=True).returncode == 0
    cross = args.zig and (target_os(triple), target_arch(triple)) != (host_os(), host_arch())
    if args.docker:
        # The programs build and run in the container, whatever this machine is.
        cmake_args, runs = [], True
    else:
        cmake_args, runs = smoke_plan(triple, host_os(), host_arch(), rosetta, args.link_only or cross)
    work = Path(tempfile.mkdtemp(prefix="ponko c library "))
    try:
        if archive.name.endswith(".zip"):
            with zipfile.ZipFile(archive) as z:
                z.extractall(work)
        else:
            with tarfile.open(archive) as t:
                t.extractall(work, filter="data")
        folder = work / archive.name.removesuffix(".tar.gz").removesuffix(".zip")
        have = sorted(p.relative_to(folder).as_posix() for p in folder.rglob("*") if p.is_file())
        if have != sorted(layout(triple)):
            raise Failure(f"{archive.name} holds {have}, expected {sorted(layout(triple))}")
        check_arch(folder / "lib" / LIBRARIES[target_os(triple)][0], triple)
        if target_os(triple) == "macos" and host_os() == "macos":
            name = install_name(folder / "lib" / LIBRARIES["macos"][0])
            if name != INSTALL_NAME:
                raise Failure(f"{archive.name}: the library's install name is {name}, not {INSTALL_NAME}")
        if args.docker:
            if args.zig or args.link_only or args.cmake_arg:
                raise Usage("--docker runs the whole smoke in a container: not with --zig, --link-only or --cmake-arg")
            run(docker_command(archive, triple, args.image))
            print(f"smoke: {archive.name} built and ran examples/cpp in {args.image}")
            return 0
        if args.zig:
            cmake_args += zig_toolchain(work, triple, args.glibc)
        build = work / "build"
        run(["cmake", "-S", str(ROOT / "examples/cpp"), "-B", str(build), "-DCMAKE_BUILD_TYPE=Release",
             f"-DPONKO_INCLUDE_DIR={folder / 'include'}", f"-DPONKO_LIBRARY_DIR={folder / 'lib'}",
             *cmake_args, *args.cmake_arg])
        run(["cmake", "--build", str(build), "--config", "Release", "--parallel"])
        if runs:
            run(["ctest", "--test-dir", str(build), "-C", "Release", "--output-on-failure"])
            print(f"smoke: {archive.name} built and ran examples/cpp")
        else:
            print(f"smoke: {archive.name} built (link only: its programs can't run here)")
    finally:
        if args.keep:
            print(f"kept {work}")
        else:
            shutil.rmtree(work, ignore_errors=True)
    return 0


# ---- selftest ---------------------------------------------------------------


def selftest(_args):
    def expect(cond, what):
        if not cond:
            raise Failure("selftest: " + what)

    expect(len(TARGETS) == 6 and len(set(TARGETS.values())) == 6, "six distinct archives")
    expect(archive_name("v0.2.0", "x86_64-apple-darwin") == ("ponko-v0.2.0-macos-x86_64", "tar.gz"), "macOS name")
    expect(archive_name("v0.2.0", "aarch64-pc-windows-msvc") == ("ponko-v0.2.0-windows-arm64", "zip"), "Windows zip")
    try:
        archive_name("v0.2.0", "riscv64gc-unknown-linux-gnu")
        expect(False, "unknown target refused")
    except Usage:
        pass
    expect("lib/ponko_ffi.dll.lib" in layout("aarch64-pc-windows-msvc"), "import library packed")
    expect(layout("aarch64-unknown-linux-gnu")[2:4] == ["lib/libponko_ffi.so", "lib/libponko_ffi.a"], "Linux libs")
    expect(build_command("aarch64-unknown-linux-gnu", zig=True)[-1] == "aarch64-unknown-linux-gnu.2.28", "glibc suffix")
    expect(build_command("x86_64-apple-darwin")[:2] == ["cargo", "build"], "plain cargo build")
    try:
        build_command("x86_64-apple-darwin", zig=True)
        expect(False, "--zig refused for macOS")
    except Usage:
        pass
    env = build_env("aarch64-unknown-linux-gnu", "/s")
    expect(Path(env["PKG_CONFIG_LIBDIR"].split(os.pathsep)[0]).as_posix().endswith("usr/lib/aarch64-linux-gnu/pkgconfig"), "pkg-config dir")
    # Headers of shared libraries, made up.
    elf = bytearray(64)
    elf[:6] = b"\x7fELF\x02\x01"
    struct.pack_into("<H", elf, 18, 183)
    expect(binary_arches(bytes(elf)) == ["aarch64"], "ELF arm64")
    macho = struct.pack("<Ii", 0xFEEDFACF, 0x01000007) + bytes(24)
    expect(binary_arches(macho) == ["x86_64"], "Mach-O x86_64")
    fat = struct.pack(">II", 0xCAFEBABE, 2) + struct.pack(">I", 0x01000007) + bytes(16) + struct.pack(">I", 0x0100000C) + bytes(16)
    expect(binary_arches(fat) == ["x86_64", "aarch64"], "universal Mach-O")
    pe = bytearray(0x100)
    pe[:2] = b"MZ"
    struct.pack_into("<I", pe, 0x3C, 0x80)
    pe[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<H", pe, 0x84, 0xAA64)
    expect(binary_arches(bytes(pe)) == ["aarch64"], "PE arm64")
    # A made-up .deb (ar) archive.
    member = b"hello"
    ar = b"!<arch>\n" + b"data.tar.xz/".ljust(16) + b"0".ljust(12) + b"0".ljust(6) * 2 + b"644".ljust(8) + str(len(member)).encode().ljust(10) + b"`\n" + member + b"\n"
    expect(deb_members(ar) == [("data.tar.xz", member)], "ar members")
    # Which archives run here, which only link.
    expect(archive_triple(Path("ponko-v0.2.0-macos-x86_64.tar.gz")) == "x86_64-apple-darwin", "archive triple")
    expect(smoke_plan("x86_64-apple-darwin", "macos", "aarch64", True, False) == (["-DCMAKE_OSX_ARCHITECTURES=x86_64"], True), "Rosetta runs")
    expect(smoke_plan("x86_64-apple-darwin", "macos", "aarch64", False, False)[1] is False, "no Rosetta: link only")
    expect(smoke_plan("aarch64-pc-windows-msvc", "windows", "x86_64", False, False) == (["-A", "ARM64"], False), "Windows ARM64 links")
    expect(smoke_plan("aarch64-pc-windows-msvc", "windows", "aarch64", False, False) == (["-A", "ARM64"], True), "Windows on Arm runs")
    expect(smoke_plan("aarch64-pc-windows-msvc", "windows", "aarch64", False, True) == (["-A", "ARM64"], False), "Windows on Arm, link only")
    expect(smoke_plan("x86_64-pc-windows-msvc", "windows", "x86_64", False, False) == ([], True), "Windows x64 runs")
    expect(smoke_plan("aarch64-unknown-linux-gnu", "linux", "aarch64", False, False) == ([], True), "native runs")
    for bad in [("aarch64-unknown-linux-gnu", "linux", "x86_64"), ("aarch64-unknown-linux-gnu", "macos", "aarch64")]:
        try:
            smoke_plan(*bad, False, False)
            expect(False, f"{bad} refused without --link-only")
        except Usage:
            pass
    expect(smoke_plan("aarch64-unknown-linux-gnu", "macos", "aarch64", False, True) == ([], False), "cross link only")
    # The container command: right platform, read-only mounts, Linux only.
    cmd = docker_command(Path("/a/ponko-v0.2.0-linux-arm64.tar.gz"), "aarch64-unknown-linux-gnu", DOCKER_IMAGE)
    expect(cmd[:5] == ["docker", "run", "--rm", "--platform", "linux/arm64"], "arm64 container")
    expect(cmd[cmd.index("-v") + 1].endswith(":/src:ro") and f"{Path('/a')}:/archive:ro" in cmd, "read-only mounts")
    expect("ARCHIVE=ponko-v0.2.0-linux-arm64.tar.gz" in cmd and DOCKER_IMAGE in cmd, "archive and image named")
    expect(docker_command(Path("/a/x.tar.gz"), "x86_64-unknown-linux-gnu", "i")[4] == "linux/amd64", "amd64 container")
    try:
        docker_command(Path("/a/x.zip"), "aarch64-pc-windows-msvc", DOCKER_IMAGE)
        expect(False, "--docker refused for Windows")
    except Usage:
        pass
    # A packed archive keeps its folder on top.
    with tempfile.TemporaryDirectory() as tmp:
        folder = Path(tmp) / "ponko-v0.0.0-linux-arm64"
        (folder / "lib").mkdir(parents=True)
        (folder / "lib" / "x").write_text("x")
        for ext in ("tar.gz", "zip"):
            archive = Path(tmp) / f"{folder.name}.{ext}"
            pack(folder, archive)
            if ext == "zip":
                names = zipfile.ZipFile(archive).namelist()
            else:
                names = [m.name for m in tarfile.open(archive).getmembers() if m.isfile()]
            expect(names == ["ponko-v0.0.0-linux-arm64/lib/x"], f"{ext} layout: {names}")
    print("selftest passed")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("targets", help="the release targets and their archive names")
    b = sub.add_parser("build", help="build ponko_ffi for a target and pack its archive")
    b.add_argument("--target", help="target triple (default: this machine)")
    b.add_argument("--version", help="the release tag (default: v + the root Cargo.toml version)")
    b.add_argument("--out", help="where the folder and archive go (default: target/c-library)")
    b.add_argument("--zig", action="store_true", help="Linux targets: build with cargo zigbuild")
    b.add_argument("--glibc", default=DEFAULT_GLIBC, help=f"with --zig: the oldest glibc to run on (default {DEFAULT_GLIBC})")
    b.add_argument("--sysroot", help="Linux cross builds: the target's ALSA and udev libraries (see `sysroot`)")
    b.add_argument("--no-build", action="store_true", help="pack what target/<triple>/release holds")
    b.add_argument("--dry-run", action="store_true", help="print the commands and the layout only")
    s = sub.add_parser("sysroot", help="unpack .deb packages into a cross-build sysroot")
    s.add_argument("--out", required=True)
    s.add_argument("debs", nargs="+")
    m = sub.add_parser("smoke", help="build examples/cpp against an archive and run its tests")
    m.add_argument("archive")
    m.add_argument("--link-only", action="store_true", help="build, don't run")
    m.add_argument("--cmake-arg", action="append", default=[], help="extra cmake configure argument (repeatable)")
    m.add_argument("--keep", action="store_true", help="keep the unpacked archive and build")
    m.add_argument("--zig", action="store_true", help="Linux archives: compile with zig cc for the archive's target (implies --link-only off that machine)")
    m.add_argument("--glibc", default=DEFAULT_GLIBC, help=f"with --zig: the glibc to build for (default {DEFAULT_GLIBC})")
    m.add_argument("--docker", action="store_true", help="Linux archives: build and run the tests inside a container (needs Docker)")
    m.add_argument("--image", default=DOCKER_IMAGE, help=f"with --docker: the image (default {DOCKER_IMAGE}, glibc 2.28)")
    sub.add_parser("selftest", help="check the script's decisions with made-up inputs")
    args = parser.parse_args()
    try:
        if args.cmd == "targets":
            for triple, name in TARGETS.items():
                print(f"{triple:28} {name:16} {archive_name('vX.Y.Z', triple)[1]}")
            return 0
        return {"build": cmd_build, "sysroot": cmd_sysroot, "smoke": cmd_smoke, "selftest": selftest}[args.cmd](args)
    except Usage as e:
        print(f"error: {e}", file=sys.stderr)
        return 3
    except Failure as e:
        print(f"error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
