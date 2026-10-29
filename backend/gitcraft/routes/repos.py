from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Any

from litestar import Controller, get, post
from litestar.exceptions import ValidationException

from ..git import ops, queries
from ..git.runner import GitError
from ..repos_store import store
from ..ws import progress
from .common import Body


class PathBody(Body):
    path: str


class CloneBody(Body):
    url: str
    parent: str
    name: str | None = None
    op_id: str | None = None


def _expand(path: str) -> str:
    return str(Path(os.path.expandvars(path)).expanduser().resolve())


class ReposController(Controller):
    path = "/"

    @get("/repos")
    async def recent(self) -> list[dict[str, Any]]:
        return store.list()

    @post("/repos/open")
    async def open_repo(self, data: PathBody) -> dict[str, Any]:
        path = _expand(data.path)
        if not Path(path).is_dir():
            raise ValidationException(f"Folder does not exist: {path}")
        try:
            top = await queries.toplevel(path)
        except GitError as exc:
            raise ValidationException(f"Not a git repository: {path}") from exc
        return store.add(top)

    @post("/repos/forget")
    async def forget(self, data: PathBody) -> None:
        store.remove(data.path)

    @post("/repos/init")
    async def init_repo(self, data: PathBody) -> dict[str, Any]:
        return store.add(await ops.init(_expand(data.path)))

    @post("/repos/clone")
    async def clone(self, data: CloneBody) -> dict[str, Any]:
        path = await ops.clone(_expand(data.parent), data.url.strip(), data.name or None, progress(data.op_id))
        return store.add(path)

    @get("/fs")
    async def list_dir(self, path: str | None = None, hidden: bool = False) -> dict[str, Any]:
        directory = Path(_expand(path)) if path else Path.home()
        if not directory.is_dir():
            raise ValidationException(f"Not a folder: {directory}")
        entries = []
        try:
            children = sorted(directory.iterdir(), key=lambda p: p.name.lower())
        except PermissionError:
            children = []
        for child in children:
            if not hidden and child.name.startswith("."):
                continue
            try:
                if child.is_dir():
                    entries.append({"name": child.name, "path": str(child), "isRepo": (child / ".git").exists()})
            except OSError:
                continue
        roots = [f"{d}:\\" for d in "CDEFGHIJKLMNOPQRSTUVWXYZ" if Path(f"{d}:\\").exists()] if sys.platform == "win32" else ["/"]
        parent = directory.parent if directory.parent != directory else None
        return {
            "path": str(directory),
            "parent": str(parent) if parent else None,
            "home": str(Path.home()),
            "roots": roots,
            "isRepo": (directory / ".git").exists(),
            "dirs": entries,
        }
