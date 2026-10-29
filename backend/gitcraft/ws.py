"""WebSocket hub: file-system change notifications and operation progress."""

from __future__ import annotations

import asyncio
from pathlib import Path
from typing import Any

from litestar import WebSocket, websocket
from litestar.exceptions import WebSocketDisconnect
from watchfiles import Change, awatch

from .git.runner import ProgressCallback
from .repos_store import store

IGNORED_DIRS = {"node_modules", "__pycache__", ".venv", ".mypy_cache", ".pytest_cache", ".idea", ".gradle"}
IGNORED_GIT_DIRS = {"objects", "logs", "lfs", "hooks", "modules"}


class Hub:
    def __init__(self) -> None:
        self.sockets: set[WebSocket] = set()

    async def broadcast(self, message: dict[str, Any]) -> None:
        for socket in list(self.sockets):
            try:
                await socket.send_json(message)
            except Exception:  # noqa: BLE001 - a dead socket must not break the operation
                self.sockets.discard(socket)


hub = Hub()


def progress(op_id: str | None, repo: str | None = None) -> ProgressCallback | None:
    if not op_id:
        return None

    async def report(line: str) -> None:
        await hub.broadcast({"type": "progress", "opId": op_id, "repo": repo, "line": line})

    return report


def _classify(repo: Path, path: str) -> str | None:
    try:
        parts = Path(path).relative_to(repo).parts
    except ValueError:
        return None
    if path.endswith(".lock"):
        return None
    if parts and parts[0] == ".git":
        if len(parts) > 1 and parts[1] in IGNORED_GIT_DIRS:
            return None
        return "git"
    if any(part in IGNORED_DIRS for part in parts):
        return None
    return "worktree"


async def watch_repo(socket: WebSocket, repo: str) -> None:
    root = Path(repo)

    def keep(_: Change, path: str) -> bool:
        return _classify(root, path) is not None

    try:
        async for changes in awatch(repo, watch_filter=keep, debounce=400, step=100, recursive=True):
            kinds = {_classify(root, path) for _, path in changes}
            await socket.send_json(
                {"type": "changed", "repo": repo, "git": "git" in kinds, "worktree": "worktree" in kinds}
            )
    except (asyncio.CancelledError, WebSocketDisconnect):
        raise
    except Exception as exc:  # noqa: BLE001 - watching is best effort
        try:
            await socket.send_json({"type": "watch-error", "repo": repo, "message": str(exc)})
        except Exception:  # noqa: BLE001
            pass


@websocket("/ws")
async def events(socket: WebSocket) -> None:
    await socket.accept()
    hub.sockets.add(socket)
    watchers: dict[str, asyncio.Task] = {}
    try:
        while True:
            message = await socket.receive_json()
            if message.get("type") != "watch":
                continue
            wanted = {r for r in message.get("repos", []) if store.is_known(r)}
            for repo in set(watchers) - wanted:
                watchers.pop(repo).cancel()
            for repo in wanted - set(watchers):
                watchers[repo] = asyncio.create_task(watch_repo(socket, repo))
    except WebSocketDisconnect:
        pass
    finally:
        hub.sockets.discard(socket)
        for task in watchers.values():
            task.cancel()
