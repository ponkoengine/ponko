#!/usr/bin/env python3
"""Drive a stdio MCP server from a shell, one tool call per command.

An MCP server over stdio lives as long as its client keeps stdin open, but a
shell (or a coding agent's shell tool) runs one command at a time. This
script keeps the server running behind a Unix socket, so each tool call is a
separate command:

    python3 scripts/mcp-session.py start S -- target/debug/examples/agent_suite \\
        mcp tune-speed --dir /tmp/suite/tune-speed --agent my-agent &
    python3 scripts/mcp-session.py instructions S
    python3 scripts/mcp-session.py tools S              # names and descriptions
    python3 scripts/mcp-session.py tools S get_resource # one tool's input schema
    python3 scripts/mcp-session.py call S get_resource '{"resource": "Settings"}'
    python3 scripts/mcp-session.py stop S

`S` is the socket path. `start` runs in the foreground until `stop` (start
it in the background); it sends `initialize` and `notifications/initialized`
and prints the server's instructions. `call` prints the tool's text result
and exits 1 when the result is an error. Standard library only; it is how
the agent acceptance suite is tried with an agent that has a shell but no
MCP client ("The agent acceptance suite" in book/src/agents.md).
"""

import contextlib
import json
import os
import socket
import subprocess
import sys


@contextlib.contextmanager
def short_path(path):
    """A socket path is limited to about 100 bytes: use it from its folder."""
    here = os.getcwd()
    os.chdir(os.path.dirname(os.path.abspath(path)))
    try:
        yield os.path.basename(path)
    finally:
        os.chdir(here)


def serve(path, command):
    child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
    next_id = [0]

    def request(method, params):
        next_id[0] += 1
        message = {"jsonrpc": "2.0", "id": next_id[0], "method": method, "params": params}
        child.stdin.write(json.dumps(message) + "\n")
        child.stdin.flush()
        while True:
            line = child.stdout.readline()
            if not line:
                return {"error": {"message": "the server exited"}}
            reply = json.loads(line)
            if reply.get("id") == next_id[0]:
                return reply

    init = request(
        "initialize",
        {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "mcp-session", "version": "1"},
        },
    )
    child.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
    child.stdin.flush()
    instructions = init.get("result", {}).get("instructions", "")
    print(instructions, flush=True)

    path = os.path.abspath(path)
    if os.path.exists(path):
        os.unlink(path)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    with short_path(path) as name:
        server.bind(name)
    server.listen(1)
    try:
        while True:
            conn, _ = server.accept()
            with conn:
                data = b""
                while not data.endswith(b"\n"):
                    chunk = conn.recv(65536)
                    if not chunk:
                        break
                    data += chunk
                ask = json.loads(data)
                if ask["method"] == "stop":
                    conn.sendall(b"{}\n")
                    break
                if ask["method"] == "instructions":
                    reply = {"result": {"instructions": instructions}}
                else:
                    reply = request(ask["method"], ask.get("params", {}))
                conn.sendall((json.dumps(reply) + "\n").encode())
    finally:
        server.close()
        os.unlink(path)
        child.stdin.close()
        child.wait(timeout=60)


def ask(path, method, params=None):
    conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    with short_path(path) as name:
        conn.connect(name)
    conn.sendall((json.dumps({"method": method, "params": params or {}}) + "\n").encode())
    data = b""
    while not data.endswith(b"\n"):
        chunk = conn.recv(1 << 20)
        if not chunk:
            break
        data += chunk
    conn.close()
    return json.loads(data)


def main(argv):
    if len(argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    verb, path, rest = argv[0], argv[1], argv[2:]
    if verb == "start":
        if not rest or rest[0] != "--" or len(rest) < 2:
            print("usage: mcp-session.py start SOCKET -- COMMAND...", file=sys.stderr)
            return 2
        serve(path, rest[1:])
        return 0
    if verb == "stop":
        ask(path, "stop")
        return 0
    if verb == "instructions":
        print(ask(path, "instructions")["result"]["instructions"])
        return 0
    if verb == "tools":
        reply = ask(path, "tools/list")
        tools = reply.get("result", {}).get("tools", [])
        if rest:
            for tool in tools:
                if tool["name"] == rest[0]:
                    print(tool.get("description", ""))
                    print(json.dumps(tool.get("inputSchema", {}), indent=2))
                    return 0
            print(f"no tool `{rest[0]}`", file=sys.stderr)
            return 1
        for tool in tools:
            first = (tool.get("description") or "").strip().split("\n")[0]
            print(f"{tool['name']}: {first}")
        return 0
    if verb == "call":
        if not rest:
            print("usage: mcp-session.py call SOCKET TOOL [JSON]", file=sys.stderr)
            return 2
        arguments = json.loads(rest[1]) if len(rest) > 1 else {}
        reply = ask(path, "tools/call", {"name": rest[0], "arguments": arguments})
        if "error" in reply:
            print(json.dumps(reply["error"]), file=sys.stderr)
            return 1
        result = reply["result"]
        for item in result.get("content", []):
            print(item.get("text", json.dumps(item)))
        return 1 if result.get("isError") else 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
