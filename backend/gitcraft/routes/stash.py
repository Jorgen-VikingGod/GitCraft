from __future__ import annotations

from typing import Any

from litestar import Controller, get, post

from ..git import ops, queries
from .common import Body


class StashBody(Body):
    message: str | None = None
    include_untracked: bool = True
    staged_only: bool = False
    keep_index: bool = False
    paths: list[str] | None = None


class StashRefBody(Body):
    name: str
    pop: bool = False


class StashController(Controller):
    path = "/stash"

    @post("/")
    async def push(self, repo_dir: str, data: StashBody) -> dict[str, Any]:
        return await ops.stash_push(
            repo_dir, data.message, data.include_untracked, data.staged_only, data.keep_index, data.paths
        )

    @post("/apply")
    async def apply(self, repo_dir: str, data: StashRefBody) -> dict[str, Any]:
        return await ops.stash_apply(repo_dir, data.name, data.pop)

    @post("/drop")
    async def drop(self, repo_dir: str, data: StashRefBody) -> dict[str, Any]:
        return await ops.stash_drop(repo_dir, data.name)

    @get("/files")
    async def files(self, repo_dir: str, name: str) -> list[dict[str, Any]]:
        return await queries.stash_files(repo_dir, name)

    @get("/diff")
    async def diff(self, repo_dir: str, name: str, path: str, context: int = 3) -> dict[str, Any] | None:
        return await queries.stash_diff(repo_dir, name, path, context)
