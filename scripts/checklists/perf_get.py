#!/usr/bin/env python3
"""Ask a running game what its renderer opened and how much GPU memory it holds.

    PONKO_CONTROL=1 cargo run --release --features control-env --example shard_grove ...   (in another terminal)
    python3 scripts/checklists/perf_get.py [--pid PID] [--wait SECONDS]

The game's control endpoint (book/src/control.md) answers `perf.get`; its
`render_inspector` has the adapter (name, backend, driver, GPU tier), the
graphics quality level and the GPU memory by class. This prints those, which
the benchmark run of `lowend_bench.py` does not. It talks to this machine only.
Run it a few seconds after the game's first frame; a game that has not drawn
yet reports `null` for what it cannot know.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import sys
import time


def instances_dir():
    """Where games register themselves (the engine's rule)."""
    if os.environ.get("PONKO_INSTANCES_DIR"):
        return Path(os.environ["PONKO_INSTANCES_DIR"])
    if os.name == "nt":
        base = os.environ.get("LOCALAPPDATA")
        return Path(base) / "ponko" / "instances" if base else None
    return Path.home() / ".ponko" / "instances"


def alive(pid):
    try:
        os.kill(pid, 0)
    except PermissionError:
        return True
    except (OSError, ProcessLookupError, SystemError):
        return False
    return True


def pick(directory, pid=None):
    """The newest registered instance whose process is still running."""
    found = []
    for path in directory.glob("*.json") if directory and directory.is_dir() else []:
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        if (pid is None or data.get("pid") == pid) and isinstance(data.get("pid"), int) and alive(data["pid"]):
            found.append(data)
    return max(found, key=lambda d: d.get("started", 0), default=None)


def request(instance, method, params=None, timeout=20):
    """One request to the control endpoint; the `result`, or raises."""
    message = {"id": 1, "token": instance["token"], "method": method, "params": params or {}}
    with socket.create_connection(("127.0.0.1", instance["port"]), timeout=timeout) as sock:
        sock.sendall((json.dumps(message) + "\n").encode())
        reply = b""
        while not reply.endswith(b"\n"):
            chunk = sock.recv(65536)
            if not chunk:
                break
            reply += chunk
    answer = json.loads(reply.decode())
    if "error" in answer:
        raise RuntimeError(answer["error"])
    return answer["result"]


def summary(result):
    """The parts of a `perf.get` answer a low-end record needs."""
    inspector = result.get("render_inspector") or {}
    return {
        "adapter": inspector.get("adapter"),
        "quality": inspector.get("quality"),
        "gpu_memory_bytes": inspector.get("gpu_memory_bytes"),
        "unavailable": inspector.get("unavailable"),
        "fps": result.get("fps"),
        "frame_ms": result.get("frame_ms"),
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--pid", type=int, help="the game's process id (default: the newest running one)")
    parser.add_argument("--wait", type=float, default=10, help="seconds to wait for a game to register (default 10)")
    args = parser.parse_args(argv)
    directory = instances_dir()
    deadline = time.monotonic() + args.wait
    instance = pick(directory, args.pid)
    while instance is None and time.monotonic() < deadline:
        time.sleep(0.5)
        instance = pick(directory, args.pid)
    if instance is None:
        print(f"no running game with the control endpoint found in {directory}", file=sys.stderr)
        return 2
    try:
        result = request(instance, "perf.get")
    except (OSError, ValueError, RuntimeError) as error:
        print(f"perf.get failed: {error}", file=sys.stderr)
        return 1
    print(f"game: {instance.get('name')} (pid {instance['pid']})")
    print(json.dumps(summary(result), indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
