from pathlib import Path

from litestar.testing import TestClient

from gitcraft.app import create_app

from .conftest import write

TOKEN = "secret"


def client() -> TestClient:
    return TestClient(app=create_app(token=TOKEN, static_dir=None))


def test_token_required():
    with client() as c:
        assert c.get("/api/health").status_code == 401
        assert c.get("/api/health", headers={"x-gitcraft-token": "wrong"}).status_code == 401
        assert c.get("/api/health", headers={"x-gitcraft-token": TOKEN}).json()["status"] == "ok"


def test_repo_must_be_opened(repo_with_history: Path):
    with client() as c:
        c.headers["x-gitcraft-token"] = TOKEN
        assert c.get("/api/status", params={"repo": str(repo_with_history / "x")}).status_code == 403


def test_commit_flow(repo_with_history: Path):
    with client() as c:
        c.headers["x-gitcraft-token"] = TOKEN
        opened = c.post("/api/repos/open", json={"path": str(repo_with_history / "sub" / "..")})
        assert opened.status_code == 201, opened.text
        repo = opened.json()["path"]
        assert repo == str(repo_with_history)
        assert any(r["path"] == repo for r in c.get("/api/repos").json())

        params = {"repo": repo}
        write(repo_with_history, "a.txt", "changed\n")
        status = c.get("/api/status", params=params).json()
        assert status["unstaged"][0]["path"] == "a.txt"

        diff = c.get("/api/diff/working", params={**params, "path": "a.txt", "context": 1}).json()
        assert diff["hunks"]

        assert c.post("/api/stage", params=params, json={"paths": ["a.txt"]}).status_code == 201
        result = c.post("/api/commit", params=params, json={"message": "api commit", "signOff": True}).json()
        assert result["ok"]

        log = c.get("/api/log", params={**params, "limit": 2}).json()
        assert log["commits"][0]["subject"] == "api commit"
        assert log["hasMore"] is True
        assert "graph" in log["commits"][0]

        detail = c.get("/api/commit", params={**params, "sha": log["commits"][0]["sha"]}).json()
        assert "Signed-off-by: Test User" in detail["message"]

        error = c.post("/api/merge", params=params, json={"ref": "nope"})
        assert error.status_code == 400 and "nope" in error.json()["error"]


def test_fs_listing(repo_with_history: Path):
    with client() as c:
        c.headers["x-gitcraft-token"] = TOKEN
        listing = c.get("/api/fs", params={"path": str(repo_with_history.parent)}).json()
        assert {"name": "repo", "path": str(repo_with_history), "isRepo": True} in listing["dirs"]


def test_websocket_progress_and_watch(repo_with_history: Path):
    with client() as c:
        c.headers["x-gitcraft-token"] = TOKEN
        repo = c.post("/api/repos/open", json={"path": str(repo_with_history)}).json()["path"]
        with c.websocket_connect(f"/api/ws?token={TOKEN}") as ws:
            ws.send_json({"type": "watch", "repos": [repo]})
            import time

            time.sleep(0.5)
            write(repo_with_history, "watched.txt", "x\n")
            message = ws.receive_json(timeout=10)
            assert message["type"] == "changed" and message["worktree"]
