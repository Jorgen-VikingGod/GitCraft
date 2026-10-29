from __future__ import annotations

from typing import Any

from litestar import Controller, get, post

from ..git import ops
from .common import Body


class ResolveBody(Body):
    path: str
    content: str | None = None
    side: str | None = None  # ours | theirs


class ConflictsController(Controller):
    path = "/conflict"

    @get("/")
    async def versions(self, repo_dir: str, path: str) -> dict[str, Any]:
        return await ops.conflict_versions(repo_dir, path)

    @post("/resolve")
    async def resolve(self, repo_dir: str, data: ResolveBody) -> dict[str, Any]:
        return await ops.resolve_conflict(repo_dir, data.path, data.content, data.side)
