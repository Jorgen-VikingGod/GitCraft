#!/usr/bin/env python3
"""Record or replay GitCraft API goldens against any server command.

    python tests/parity/parity.py capture                 # write goldens from the Python server
    python tests/parity/parity.py check                   # replay against the Python server
    python tests/parity/parity.py check --server "<cmd>"  # replay against another server

The server must accept `--no-browser --port 0`, read GITCRAFT_TOKEN and print
`running at <url>` on stdout. Standard library only, except for the WebSocket
check, which is skipped without `websockets`.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).parent))

from cases import CASES, STATIC_READS  # noqa: E402
from scenarios import GLOBAL_CONFIG, IDENTITY, SERVER_TIME, available, build, isolated_env  # noqa: E402

HERE = Path(__file__).parent
BACKEND = HERE.parent.parent / "backend"
GOLDENS = HERE / "goldens"
ACCEPTED = HERE / "accepted_differences.json"
TOKEN = "parity-token"
ROOT_MARK = "<ROOT>"
INLINE_LIMIT = 64 * 1024
# Git wording changes between versions, so these only compare on the capture's git version.
MESSAGE_KEYS = {"error", "message", "detail"}
MAX_COMMITS_SWEPT = 12
MAX_FILES_SWEPT = 6


class Server:
    def __init__(self, command: list[str], cwd: Path | None, env: dict[str, str]) -> None:
        self.process = subprocess.Popen(
            [*command, "--no-browser", "--port", "0"],
            cwd=cwd,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
        assert self.process.stdout
        match = None
        deadline = time.time() + 60
        while time.time() < deadline and not match:
            line = self.process.stdout.readline()
            if not line:
                raise RuntimeError("server exited before announcing its URL")
            match = re.search(r"running at (http://[^\s/]+)", line)
        if not match:
            raise RuntimeError("server did not announce its URL")
        self.base = match.group(1)

    def stop(self) -> None:
        self.process.terminate()
        try:
            self.process.wait(10)
        except subprocess.TimeoutExpired:
            self.process.kill()

    def call(self, method: str, path: str, params: dict[str, Any], body: Any, token: str | None = TOKEN) -> dict[str, Any]:
        query = urllib.parse.urlencode({k: str(v).lower() if isinstance(v, bool) else v for k, v in params.items()})
        url = f"{self.base}/api{path}" + (f"?{query}" if query else "")
        data = json.dumps(body).encode() if method == "POST" else None
        request = urllib.request.Request(url, data=data, method=method)
        request.add_header("content-type", "application/json")
        if token:
            request.add_header("x-gitcraft-token", token)
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                status, raw, kind = response.status, response.read(), response.headers.get("content-type", "")
        except urllib.error.HTTPError as error:
            status, raw, kind = error.code, error.read(), error.headers.get("content-type", "")
        kind = kind.split(";")[0].strip()
        if kind == "application/json":
            return {"status": status, "type": kind, "body": json.loads(raw) if raw else None}
        return {"status": status, "type": kind, "body": raw.decode("utf-8", errors="replace")}


def normalise(value: Any, root: str) -> Any:
    if isinstance(value, dict):
        # git blame stamps uncommitted lines with the current time.
        uncommitted = set(str(value.get("sha", "x"))) == {"0"}
        volatile = {"lastOpened", "time"} if uncommitted else {"lastOpened"}
        return {k: ("<TIME>" if k in volatile else normalise(v, root)) for k, v in value.items()}
    if isinstance(value, list):
        return [normalise(v, root) for v in value]
    if isinstance(value, str):
        text = value.replace(root, ROOT_MARK).replace(root.replace("\\", "/"), ROOT_MARK)
        text = text.replace(os.environ.get("GITCRAFT_PARITY_HOME", "\0"), "<HOME>")
        # Temp files git or the server may name in messages.
        text = re.sub(r"gitcraft-todo-[\w.-]+", "gitcraft-todo-<ID>", text)
        # git format-patch signs its output with the git version.
        return re.sub(r"\n-- \n\d+\.\d+[^\n]*\n", "\n-- \n<GIT>\n", text)
    return value


def denormalise(value: Any, root: str) -> Any:
    if isinstance(value, dict):
        return {k: denormalise(v, root) for k, v in value.items()}
    if isinstance(value, list):
        return [denormalise(v, root) for v in value]
    if isinstance(value, str):
        return value.replace(ROOT_MARK, root)
    return value


def compact(response: dict[str, Any]) -> dict[str, Any]:
    """Keep the golden files reviewable: very large bodies are stored as a hash."""
    text = json.dumps(response["body"], ensure_ascii=False, sort_keys=True)
    if len(text) <= INLINE_LIMIT:
        return response
    return {**response, "body": {"<large>": True, "size": len(text), "sha256": hashlib.sha256(text.encode()).hexdigest()}}


def git_state(repo: Path, env: dict[str, str]) -> dict[str, Any]:
    """Repository state read with git itself, independent of the server under test."""

    def run(*args: str) -> str:
        result = subprocess.run(["git", *args], cwd=repo, env=env, capture_output=True, check=False)
        return result.stdout.decode("utf-8", errors="replace")

    files = {}
    for path in sorted(p for p in repo.rglob("*") if p.is_file() and ".git" not in p.relative_to(repo).parts):
        files[path.relative_to(repo).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()[:16]
    return {
        "head": run("rev-parse", "HEAD").strip(),
        "symbolicHead": run("symbolic-ref", "-q", "HEAD").strip(),
        "index": run("ls-files", "-s", "-z").split("\0"),
        "status": run("status", "--porcelain=v1", "-z", "--untracked-files=all").split("\0"),
        "refs": run("for-each-ref", "--format=%(refname) %(objectname)").splitlines(),
        "stashes": run("stash", "list", "--format=%H %gs").splitlines(),
        "worktree": files,
    }


class Ctx:
    """Resolves repository-dependent values while capturing."""

    def __init__(self, repo: Path, env: dict[str, str], server: Server) -> None:
        self.repo, self.env, self.server = repo, env, server

    def rev(self, spec: str) -> str:
        result = subprocess.run(["git", "rev-parse", spec], cwd=self.repo, env=self.env, capture_output=True, check=True)
        return result.stdout.decode().strip()

    def format_patch(self, spec: str) -> str:
        result = subprocess.run(
            ["git", "format-patch", "-1", "--stdout", spec], cwd=self.repo, env=self.env, capture_output=True, check=True
        )
        return result.stdout.decode()

    def hunk_line(self, path: str, hunk: int, marker: str, staged: bool = False) -> int:
        params = {"repo": str(self.repo), "path": path, "staged": staged, "context": 3}
        diff = self.server.call("GET", "/diff/working", params, None)["body"]
        lines = diff["hunks"][hunk]["lines"]
        for index, line in enumerate(lines):
            if line["t"] == marker:
                return index
        raise RuntimeError(f"no {marker} line in {path} hunk {hunk}: {lines[:3]}")


def fresh_workspace(base: Path, scenario: str) -> tuple[Path, Path]:
    root = Path(tempfile.mkdtemp(dir=base)).resolve() / "w"
    env = isolated_env(base / "home")
    repo = build(scenario, root, env)
    return root, repo


def read_sweep(repo: Path, server: Server) -> list[dict[str, Any]]:
    """Every read route for one repository, derived from what the server reports."""
    p = {"repo": str(repo)}
    plan: list[dict[str, Any]] = [{**r, "params": {**r["params"], **p}} for r in STATIC_READS]
    add = lambda route, **params: plan.append({"method": "GET", "path": route, "params": {**p, **params}})  # noqa: E731

    log = server.call("GET", "/log", p, None)["body"] or {}
    commits = log.get("commits", [])
    status = server.call("GET", "/status", p, None)["body"] or {}
    for commit in commits[:MAX_COMMITS_SWEPT]:
        sha = commit["sha"]
        add("/commit", sha=sha)
        files = (server.call("GET", "/commit", {**p, "sha": sha}, None)["body"] or {}).get("files", [])
        for file in files[:MAX_FILES_SWEPT]:
            extra = {"orig_path": file["origPath"]} if file.get("origPath") else {}
            add("/diff/commit", sha=sha, path=file["path"], **extra)
            add("/diff/commit", sha=sha, path=file["path"], context=0, ignore_ws=True)
    if commits:
        add("/search", q=commits[0]["sha"][:7], mode="sha")
        add("/search", q=commits[0]["subject"][:4] or "x", mode="message")
        add("/patch", sha=commits[0]["sha"])
        add("/rebase/commits", base=commits[-1]["sha"])
        add("/compare", base=commits[-1]["sha"], target=commits[0]["sha"])
        add("/reflog", ref="HEAD", limit=5)
        for file in (server.call("GET", "/commit", {**p, "sha": commits[0]["sha"]}, None)["body"] or {}).get("files", [])[:MAX_FILES_SWEPT]:
            add("/diff/compare", base=commits[-1]["sha"], target=commits[0]["sha"], path=file["path"])
            add("/file-history", path=file["path"])
            add("/blame", path=file["path"])
            add("/blame", path=file["path"], rev=commits[0]["sha"])
    for group, staged in (("unstaged", False), ("staged", True), ("untracked", False)):
        for file in status.get(group, []):
            extra = {"orig_path": file["origPath"]} if file.get("origPath") else {}
            add("/diff/working", path=file["path"], staged=staged, **extra)
            add("/diff/working", path=file["path"], staged=staged, context=0, ignore_ws=True, **extra)
    for file in status.get("conflicts", []):
        add("/conflict", path=file["path"])
    for stash in (server.call("GET", "/refs", p, None)["body"] or {}).get("stashes", []):
        add("/stash/files", name=stash["name"])
        for file in (server.call("GET", "/stash/files", {**p, "name": stash["name"]}, None)["body"] or [])[:MAX_FILES_SWEPT]:
            add("/stash/diff", name=stash["name"], path=file["path"])
    return plan


def global_cases(root: Path) -> list[dict[str, Any]]:
    """Requests that need no scenario: auth, repo validation, the folder browser."""
    fs = root / "fs"
    (fs / "plain" / "child").mkdir(parents=True)
    (fs / ".hidden").mkdir()
    (fs / "a-file.txt").write_text("x")
    return [
        {"method": "GET", "path": "/health", "params": {}},
        {"method": "GET", "path": "/health", "params": {}, "token": None},
        {"method": "GET", "path": "/health", "params": {}, "token": "wrong"},
        {"method": "GET", "path": "/status", "params": {"repo": str(root / "never-opened")}},
        {"method": "GET", "path": "/status", "params": {}, "status_only": True},
        {"method": "GET", "path": "/fs", "params": {"path": str(fs)}},
        {"method": "GET", "path": "/fs", "params": {"path": str(fs), "hidden": True}},
        {"method": "GET", "path": "/fs", "params": {"path": str(fs / "missing")}},
        {"method": "GET", "path": "/fs", "params": {"path": str(fs / "a-file.txt")}},
        {"method": "POST", "path": "/repos/open", "params": {}, "body": {"path": str(fs)}},
        {"method": "POST", "path": "/repos/open", "params": {}, "body": {"path": str(fs / "missing")}},
        {"method": "POST", "path": "/repos/open", "params": {}, "body": {}, "status_only": True},
        {"method": "POST", "path": "/repos/init", "params": {}, "body": {"path": str(fs / "created")}},
        {"method": "POST", "path": "/repos/forget", "params": {}, "body": {"path": str(fs / "created")}},
    ]


def exchange(server: Server, step: dict[str, Any], root: str) -> dict[str, Any]:
    token = step.get("token", TOKEN)
    response = server.call(step["method"], step["path"], step.get("params", {}), step.get("body"), token)
    request = {k: step[k] for k in ("method", "path", "params", "body", "token", "status_only") if k in step and step[k] not in (False,)}
    if not step.get("params"):
        request.pop("params", None)
    return {"request": normalise(request, root), "response": compact(normalise(response, root))}


def pre_step_routes(step: dict[str, Any]) -> tuple[str, str]:
    return step["method"], step["path"]


def capture(command: list[str], cwd: Path | None) -> int:
    if GOLDENS.exists():
        shutil.rmtree(GOLDENS)
    (GOLDENS / "read").mkdir(parents=True)
    (GOLDENS / "mutate").mkdir()
    covered: set[tuple[str, str]] = set()

    with tempfile.TemporaryDirectory(prefix="gitcraft-parity-") as temp:
        base = Path(temp).resolve()
        server, env = start(base, command, cwd)
        try:
            root = base / "global"
            root.mkdir()
            records = []
            for step in global_cases(root):
                covered.add(pre_step_routes(step))
                records.append(exchange(server, step, str(root)))
            write(GOLDENS / "global.json", records)

            for name in available():
                root, repo = fresh_workspace(base, name)
                opened = server.call("POST", "/repos/open", {}, {"path": str(repo)})
                covered.add(("POST", "/repos/open"))
                if opened["status"] != 201:
                    raise RuntimeError(f"{name}: open failed: {opened}")
                records = []
                for step in read_sweep(repo, server):
                    covered.add(pre_step_routes(step))
                    records.append(exchange(server, step, str(root)))
                write(GOLDENS / "read" / f"{name}.json", {"scenario": name, "exchanges": records})

            for case_name, scenario, steps in CASES:
                if scenario not in available():
                    continue
                root, repo = fresh_workspace(base, scenario)
                server.call("POST", "/repos/open", {}, {"path": str(repo)})
                ctx = Ctx(repo, env, server)
                records = []
                for raw in steps:
                    step = dict(raw(ctx) if callable(raw) else raw)
                    step["params"] = {"repo": str(repo), **step.get("params", {})}
                    covered.add(pre_step_routes(step))
                    records.append(exchange(server, step, str(root)))
                final = normalise(git_state(repo, env), str(root))
                write(
                    GOLDENS / "mutate" / f"{case_name}.json",
                    {"scenario": scenario, "exchanges": records, "state": final},
                )

            records = capture_store_cases(server, base)
            covered.update({("GET", "/repos"), ("POST", "/repos/open"), ("POST", "/repos/forget"), ("POST", "/repos/clone")})
            write(GOLDENS / "store.json", records)
        finally:
            server.stop()

    version = subprocess.run(["git", "--version"], capture_output=True, text=True, check=True).stdout.strip()
    write(GOLDENS / "manifest.json", {"git": version, "platform": sys.platform})
    report_coverage(covered)
    return 0


def capture_store_cases(server: Server, base: Path) -> list[dict[str, Any]]:
    """Recent-repos behaviour, starting from an emptied store so the order is known."""
    root = base / "store"
    env = isolated_env(base / "home")
    src = build("linear", root / "src", env)
    second = build("empty", root / "second", env)
    for repo in server.call("GET", "/repos", {}, None)["body"]:
        server.call("POST", "/repos/forget", {}, {"path": repo["path"]})
    steps = [
        {"method": "GET", "path": "/repos", "params": {}},
        {"method": "POST", "path": "/repos/open", "params": {}, "body": {"path": str(src / "dir" / "..")}},
        {"method": "POST", "path": "/repos/open", "params": {}, "body": {"path": str(second)}},
        {"method": "GET", "path": "/repos", "params": {}},
        {"method": "POST", "path": "/repos/open", "params": {}, "body": {"path": str(src)}},
        {"method": "GET", "path": "/repos", "params": {}},
        {"method": "POST", "path": "/repos/clone", "params": {}, "body": {"url": str(src), "parent": str(root / "clones"), "name": "copy"}},
        {"method": "POST", "path": "/repos/clone", "params": {}, "body": {"url": str(root / "no-such"), "parent": str(root / "clones")}},
        {"method": "POST", "path": "/repos/forget", "params": {}, "body": {"path": str(second)}},
        {"method": "GET", "path": "/repos", "params": {}},
    ]
    (root / "clones").mkdir()
    return [exchange(server, step, str(root)) for step in steps]


def start(base: Path, command: list[str], cwd: Path | None) -> tuple[Server, dict[str, str]]:
    home = base / "home"
    home.mkdir(exist_ok=True)
    os.environ["GITCRAFT_PARITY_HOME"] = str(home)
    (home / ".gitconfig").write_text(GLOBAL_CONFIG)
    env = isolated_env(home)
    stamp = f"{SERVER_TIME} +0000"
    server_env = {
        **env,
        **IDENTITY,
        "GIT_AUTHOR_DATE": stamp,
        "GIT_COMMITTER_DATE": stamp,
        "GITCRAFT_TOKEN": TOKEN,
        "GITCRAFT_CONFIG_DIR": str(base / "config"),
    }
    return Server(command, cwd, server_env), env


def report_coverage(covered: set[tuple[str, str]]) -> None:
    routes = python_routes()
    if routes is None:
        return
    missing = sorted(routes - covered)
    print(f"route coverage: {len(routes) - len(missing)}/{len(routes)}")
    for method, path in missing:
        print(f"  not covered: {method} {path}")


def python_routes() -> set[tuple[str, str]] | None:
    code = (
        "import json;from gitcraft.app import create_app;a=create_app(token='x',static_dir=None);"
        "print(json.dumps([[m,r.path[4:]] for r in a.routes if r.path.startswith('/api/') and r.path!='/api/ws'"
        " for m in r.methods if m in ('GET','POST')]))"
    )
    python = BACKEND / ".venv" / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
    result = subprocess.run([str(python), "-c", code], capture_output=True, text=True, check=False, cwd=BACKEND)
    if result.returncode:
        print(f"route listing failed: {result.stderr.strip()[:300]}")
        return None
    return {tuple(item) for item in json.loads(result.stdout)}


def diff_values(expected: Any, actual: Any, path: str, compare_messages: bool, out: list[str]) -> None:
    if isinstance(expected, dict) and isinstance(actual, dict):
        for key in sorted(set(expected) | set(actual)):
            if not compare_messages and key in MESSAGE_KEYS:
                continue
            if key not in expected:
                out.append(f"{path}.{key}: unexpected {short(actual[key])}")
            elif key not in actual:
                out.append(f"{path}.{key}: missing, expected {short(expected[key])}")
            else:
                diff_values(expected[key], actual[key], f"{path}.{key}", compare_messages, out)
    elif isinstance(expected, list) and isinstance(actual, list):
        if len(expected) != len(actual):
            out.append(f"{path}: {len(expected)} items expected, got {len(actual)}")
        for index, (left, right) in enumerate(zip(expected, actual)):
            diff_values(left, right, f"{path}[{index}]", compare_messages, out)
    elif expected != actual:
        out.append(f"{path}: expected {short(expected)}, got {short(actual)}")


def short(value: Any) -> str:
    text = json.dumps(value, ensure_ascii=False)
    return text if len(text) < 160 else text[:157] + "..."


def replay(server: Server, record: dict[str, Any], root: str, compare_messages: bool, label: str, out: list[str]) -> None:
    step = denormalise(record["request"], root)
    token = step["token"] if "token" in step else TOKEN
    response = compact(normalise(server.call(step["method"], step["path"], step.get("params", {}), step.get("body"), token), root))
    expected = record["response"]
    where = f"{label} {step['method']} {step['path']}"
    if step.get("status_only"):
        if response["status"] != expected["status"]:
            out.append(f"{where}: status {expected['status']} expected, got {response['status']}")
        return
    diff_values(expected, response, where, compare_messages, out)


def check(command: list[str], cwd: Path | None, only: str | None, python_server: bool = False, allow_stale: bool = False) -> int:
    manifest = json.loads((GOLDENS / "manifest.json").read_text())
    version = subprocess.run(["git", "--version"], capture_output=True, text=True, check=True).stdout.strip()
    compare_messages = version == manifest["git"] and sys.platform == manifest["platform"]
    if not compare_messages:
        print(f"note: goldens from {manifest['git']} on {manifest['platform']}; skipping message text comparison")
    failures: list[str] = []
    total = 0
    with tempfile.TemporaryDirectory(prefix="gitcraft-parity-") as temp:
        base = Path(temp).resolve()
        server, env = start(base, command, cwd)
        try:
            if not only:
                root = base / "global"
                root.mkdir()
                global_cases(root)
                for record in json.loads((GOLDENS / "global.json").read_text()):
                    total += 1
                    replay(server, record, str(root), compare_messages, "global", failures)
            for name in available():
                if only and only != name:
                    continue
                path = GOLDENS / "read" / f"{name}.json"
                root, repo = fresh_workspace(base, name)
                server.call("POST", "/repos/open", {}, {"path": str(repo)})
                for record in json.loads(path.read_text())["exchanges"]:
                    total += 1
                    replay(server, record, str(root), compare_messages, f"read/{name}", failures)
            for path in sorted((GOLDENS / "mutate").glob("*.json")):
                if only and only != path.stem:
                    continue
                golden = json.loads(path.read_text())
                if golden["scenario"] not in available():
                    continue
                root, repo = fresh_workspace(base, golden["scenario"])
                server.call("POST", "/repos/open", {}, {"path": str(repo)})
                for record in golden["exchanges"]:
                    total += 1
                    replay(server, record, str(root), compare_messages, f"mutate/{path.stem}", failures)
                state = normalise(git_state(repo, env), str(root))
                diff_values(golden["state"], state, f"mutate/{path.stem} git-state", True, failures)
            if not only:
                failures.extend(check_store(server, base, compare_messages))
                total += len(json.loads((GOLDENS / "store.json").read_text()))
                failures.extend(check_websocket(server, base))
        finally:
            server.stop()
    accepted = json.loads(ACCEPTED.read_text()) if not python_server else []
    unexpected, used = [], set()
    for failure in failures:
        hit = next((entry for entry in accepted if re.search(entry["match"], failure, re.S)), None)
        if hit:
            used.add(hit["match"])
        else:
            unexpected.append(failure)
    for failure in unexpected:
        print(f"FAIL {failure}")
    if not only:
        for entry in accepted:
            if entry["match"] not in used:
                print(f"STALE accepted difference, no longer seen: {entry['match']}")
                if not allow_stale:
                    unexpected.append(entry["match"])
    print(f"{total} exchanges, {len(failures)} differences: {len(failures) - len(unexpected)} accepted, {len(unexpected)} unexpected")
    return 1 if unexpected else 0


def check_store(server: Server, base: Path, compare_messages: bool) -> list[str]:
    root = base / "store"
    env = isolated_env(base / "home")
    build("linear", root / "src", env)
    build("empty", root / "second", env)
    (root / "clones").mkdir()
    for repo in server.call("GET", "/repos", {}, None)["body"]:
        server.call("POST", "/repos/forget", {}, {"path": repo["path"]})
    out: list[str] = []
    for record in json.loads((GOLDENS / "store.json").read_text()):
        replay(server, record, str(root), compare_messages, "store", out)
    return out


def check_websocket(server: Server, base: Path) -> list[str]:
    """The event contract: watch notifications and progress lines. Not byte-compared."""
    try:
        from websockets.sync.client import connect
    except ImportError:
        print("note: websockets not installed; skipping the WebSocket check")
        return []
    root, repo = fresh_workspace(base, "remotes")
    opened = server.call("POST", "/repos/open", {}, {"path": str(repo)})["body"]["path"]
    out: list[str] = []
    url = server.base.replace("http://", "ws://") + f"/api/ws?token={TOKEN}"

    def wait(ws: Any, kind: str, seconds: float = 15) -> dict[str, Any] | None:
        deadline = time.time() + seconds
        while time.time() < deadline:
            try:
                message = json.loads(ws.recv(timeout=max(0.1, deadline - time.time())))
            except TimeoutError:
                return None
            if message.get("type") == kind:
                return message
        return None

    with connect(url) as ws:
        ws.send(json.dumps({"type": "watch", "repos": [opened]}))
        time.sleep(0.6)
        (repo / "watched.txt").write_text("x\n")
        changed = wait(ws, "changed")
        if not changed or not changed.get("worktree") or changed.get("repo") != opened:
            out.append(f"websocket: worktree change not reported ({changed})")
        time.sleep(0.8)
        subprocess.run(["git", "branch", "ws-branch"], cwd=repo, env=isolated_env(base / "home"), check=True)
        changed = wait(ws, "changed")
        if not changed or not changed.get("git"):
            out.append(f"websocket: git change not reported ({changed})")
        server.call("POST", "/fetch", {"repo": opened}, {"remote": "origin", "opId": "parity-op"})
        progress = wait(ws, "progress", 5)
        if not progress or progress.get("opId") != "parity-op" or progress.get("repo") != opened or "line" not in progress:
            out.append(f"websocket: progress not reported ({progress})")
    return out


def write(path: Path, data: Any) -> None:
    path.write_text(json.dumps(data, ensure_ascii=False, indent=1, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("mode", choices=["capture", "check"])
    parser.add_argument("--server", help="server command; default is the Python server in backend/")
    parser.add_argument("--only", help="check one scenario or mutation case")
    parser.add_argument("--allow-stale", action="store_true", help="only report accepted differences that didn't occur, as not all occur on every OS")
    args = parser.parse_args()
    if args.server:
        command, cwd = shlex.split(args.server), None
    else:
        python = BACKEND / ".venv" / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
        command, cwd = [str(python), "-m", "gitcraft.app"], BACKEND
    if args.mode == "capture":
        return capture(command, cwd)
    return check(command, cwd, args.only, python_server=not args.server, allow_stale=args.allow_stale)


if __name__ == "__main__":
    sys.exit(main())
