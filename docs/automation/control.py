#!/usr/bin/env python3
"""Drives a running GitCraft through its control server (craft-automation).

As a module:

    from control import GitCraft
    with GitCraft("target/release/gitcraft", repo, config_dir) as app:
        app.wait_loaded()
        app.call("ui.open", {"what": "settings"})
        app.call("ui.screenshot", {"path": "settings.png"})

From the command line, a steps file runs one request after the other and prints each reply:

    python3 docs/automation/control.py target/release/gitcraft <repo> steps.json

    steps.json: [{"method": "ui.menu.invoke", "params": {"path": ["View", "Local changes"]}},
                 {"method": "ui.screenshot", "params": {"path": "changes.png"}}]

Pictures with a `path` land in `--out` (default target/automation). The app runs with a settings
folder of its own (`--config`, default target/automation/config), so it never touches yours.
The methods are listed in crates/gitcraft-ui/src/control.rs.
"""

import argparse
import json
import os
import re
import secrets
import socket
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


class GitCraft:
    """One GitCraft window started with a control port, and a connection to it."""

    def __init__(self, binary, repo, config, out=None, env=None):
        self.token = secrets.token_hex(32)
        os.makedirs(config, exist_ok=True)
        full = dict(os.environ)
        full.update(env or {})
        full.update({
            "GITCRAFT_CONFIG_DIR": config,
            "GITCRAFT_CONTROL_PORT": "0",
            "GITCRAFT_CONTROL_TOKEN": self.token,
        })
        if out:
            full["GITCRAFT_CONTROL_DIR"] = out
        self.process = subprocess.Popen([binary, repo], env=full, stderr=subprocess.PIPE,
                                        text=True)
        port = None
        for line in self.process.stderr:
            found = re.search(r"control server listening on [^:]+:(\d+)", line)
            if found:
                port = int(found.group(1))
                break
        if port is None:
            raise RuntimeError("GitCraft didn't start its control server")
        self.socket = socket.create_connection(("127.0.0.1", port))
        self.stream = self.socket.makefile("rw", encoding="utf-8")
        self.next_id = 0
        self.call("auth", {"token": self.token})

    def call(self, method, params=None):
        """Sends one request and returns its result; a failed request raises."""
        self.next_id += 1
        request = {"id": self.next_id, "method": method, "params": params or {}}
        self.stream.write(json.dumps(request) + "\n")
        self.stream.flush()
        reply = json.loads(self.stream.readline())
        if not reply.get("ok"):
            raise RuntimeError(f"{method}: {reply.get('error')}")
        return reply.get("result")

    def wait_loaded(self, timeout=60):
        """Waits until a repository is open and has loaded, and returns the window's state."""
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            state = self.call("app.state")
            if state["repository"] and not state["loading"] and not state["busy"]:
                return state
            time.sleep(0.1)
        raise TimeoutError("the repository didn't finish loading")

    def close(self):
        try:
            self.call("app.quit")
        except (OSError, RuntimeError, ValueError):
            pass
        try:
            self.process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            self.process.kill()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("binary", help="the gitcraft program")
    parser.add_argument("repo", help="the repository to open")
    parser.add_argument("steps", help="a JSON list of {method, params}")
    parser.add_argument("--out", default=os.path.join(ROOT, "target", "automation"))
    parser.add_argument("--config", default=os.path.join(ROOT, "target", "automation", "config"))
    args = parser.parse_args()
    with open(args.steps, encoding="utf-8") as file:
        steps = json.load(file)
    with GitCraft(args.binary, args.repo, args.config, out=args.out) as app:
        app.wait_loaded()
        for step in steps:
            try:
                result = app.call(step["method"], step.get("params"))
            except RuntimeError as error:
                sys.exit(str(error))
            print(json.dumps({"method": step["method"], "result": result}))


if __name__ == "__main__":
    main()
