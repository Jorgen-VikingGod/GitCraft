#!/usr/bin/env python3
"""Measure the API and the bare git command behind it, to compare before and after the port.

    python tests/parity/baseline.py                         # synthetic repo, Python server
    python tests/parity/baseline.py --repo /path/to/repo    # a real repository (read-only requests)
    python tests/parity/baseline.py --server "<cmd>"        # another server

"git" is the main git command behind the request, run directly. "added" is everything
else: process handling, parsing, graph layout, serialisation, and any further git
commands the request runs (/refs runs five, /status several), so it is an upper bound
on what the server language costs. UI rendering is not measured here. Memory is Linux only.
"""

from __future__ import annotations

import argparse
import json
import shlex
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from parity import BACKEND, TOKEN, Server  # noqa: E402
from scenarios import GLOBAL_CONFIG, isolated_env  # noqa: E402

LOG_FORMAT = "--format=%x1e%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%D%x1f%s"
RUNS = 5


def synthetic(path: Path, env: dict[str, str], commits: int, branches: int, files: int) -> None:
    """Fast-import history: a main line where every 10th commit merges a side branch, plus a dirty big file."""
    subprocess.run(["git", "init", "-q", "-b", "main", str(path)], env=env, check=True)
    big = "".join(f"line {i}\n" for i in range(15_000))
    out: list[str] = []
    who = "Bench <bench@example.com>"

    def blob(text: str) -> str:
        return f"data {len(text.encode())}\n{text}\n"

    mark = 0
    head = 0
    pending = 0
    for i in range(commits):
        mark += 1
        side = i % 10 == 8
        branch = f"refs/heads/side{i % branches}" if side else "refs/heads/main"
        out.append(f"commit {branch}\nmark :{mark}\ncommitter {who} {1_600_000_000 + i * 60} +0000\n{blob(f'commit number {i}').rstrip()}\n")
        if head:
            out.append(f"from :{head}\n")
        if pending:
            out.append(f"merge :{pending}\n")
            pending = 0
        if i == 0:
            out.extend(f"M 100644 inline src/f{n % 50}/file{n}.txt\n{blob(f'file {n}')}" for n in range(files))
            out.append(f"M 100644 inline big.txt\n{blob(big)}")
        out.append(f"M 100644 inline src/f{i % 50}/file{i % files}.txt\n{blob(f'rev {i}')}\n")
        if side:
            pending = mark
        else:
            head = mark
    subprocess.run(["git", "fast-import", "--quiet"], cwd=path, env=env, input="".join(out).encode(), check=True)
    subprocess.run(["git", "reset", "-q", "--hard", "main"], cwd=path, env=env, check=True)
    (path / "big.txt").write_text("".join(f"changed {i}\n" for i in range(15_000)))


def timed(run) -> float:
    samples = []
    for _ in range(RUNS):
        start = time.perf_counter()
        run()
        samples.append((time.perf_counter() - start) * 1000)
    return statistics.median(samples)


def rss_mb(pid: int) -> float | None:
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                return int(line.split()[1]) / 1024
    except OSError:
        pass
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", help="existing repository; default builds a synthetic one")
    parser.add_argument("--server", help="server command; default is the Python server")
    parser.add_argument("--commits", type=int, default=100_000)
    parser.add_argument("--json", help="also write the results to this file")
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="gitcraft-baseline-") as temp:
        base = Path(temp).resolve()
        home = base / "home"
        home.mkdir()
        (home / ".gitconfig").write_text(GLOBAL_CONFIG)
        env = isolated_env(home)
        if args.repo:
            repo = Path(args.repo).resolve()
        else:
            repo = base / "repo"
            synthetic(repo, env, args.commits, branches=20, files=5_000)
        count = subprocess.run(["git", "rev-list", "--all", "--count"], cwd=repo, env=env, capture_output=True, text=True, check=True).stdout.strip()

        if args.server:
            command, cwd = shlex.split(args.server), None
        else:
            python = BACKEND / ".venv" / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
            command, cwd = [str(python), "-m", "gitcraft.app"], BACKEND
        server_env = {**env, "GITCRAFT_TOKEN": TOKEN, "GITCRAFT_CONFIG_DIR": str(base / "config")}

        startups = []
        for _ in range(RUNS):
            start = time.perf_counter()
            server = Server(command, cwd, server_env)
            startups.append((time.perf_counter() - start) * 1000)
            server.stop()

        server = Server(command, cwd, server_env)
        idle = rss_mb(server.process.pid)
        try:
            opened = server.call("POST", "/repos/open", {}, {"path": str(repo)})
            if opened["status"] != 201:
                raise RuntimeError(opened)
            top = opened["body"]["path"]

            def git(*git_args: str):
                return lambda: subprocess.run(["git", "-c", "core.quotepath=false", *git_args], cwd=repo, env=env, capture_output=True, check=False)

            def api(route: str, **params):
                def call():
                    response = server.call("GET", route, {"repo": top, **params}, None)
                    if response["status"] != 200:
                        raise RuntimeError(f"{route}: {response['status']} {str(response['body'])[:200]}")
                    return response

                return call

            log = [LOG_FORMAT, "--topo-order", "--decorate=full", "--all"]
            benches = [
                ("log, 2000 commits", api("/log", limit=2000), git("log", *log, "-n", "2001")),
                ("log, 20000 commits", api("/log", limit=20_000), git("log", *log, "-n", "20001")),
                ("log, 100000 commits", api("/log", limit=100_000), git("log", *log, "-n", "100001")),
                ("status", api("/status"), git("status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all")),
                ("refs", api("/refs"), git("for-each-ref")),
                ("diff, 15000 changed lines", api("/diff/working", path="big.txt", staged=False), git("diff", "--", "big.txt")),
                ("blame, big file", api("/blame", path="big.txt"), git("blame", "--porcelain", "--", "big.txt")),
            ]
            rows = [
                (name, timed(api_call), timed(git_call), len(json.dumps(api_call()["body"])) // 1024)
                for name, api_call, git_call in benches
            ]
            loaded = rss_mb(server.process.pid)
        finally:
            server.stop()

    print(f"repository: {'synthetic' if not args.repo else repo}, {count} commits; median of {RUNS} runs")
    print(f"server start until ready: {statistics.median(startups):.0f} ms")
    if idle is not None:
        print(f"server memory: {idle:.0f} MB idle, {loaded:.0f} MB after the runs")
    print(f"{'request':<28}{'api ms':>10}{'git ms':>10}{'added ms':>10}{'added %':>9}{'body kB':>9}")
    for name, api_ms, git_ms, size in rows:
        added = api_ms - git_ms
        print(f"{name:<28}{api_ms:>10.0f}{git_ms:>10.0f}{added:>10.0f}{added / api_ms * 100:>8.0f}%{size:>9}")
    if args.json:
        Path(args.json).write_text(
            json.dumps(
                {
                    "commits": count,
                    "startupMs": statistics.median(startups),
                    "idleMb": idle,
                    "loadedMb": loaded,
                    "requests": [{"name": n, "apiMs": a, "gitMs": g, "bodyKb": s} for n, a, g, s in rows],
                },
                indent=1,
            )
            + "\n"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
