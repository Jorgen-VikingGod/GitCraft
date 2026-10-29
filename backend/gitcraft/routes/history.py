from __future__ import annotations

from typing import Any

from litestar import Controller, get

from ..git import queries


class HistoryController(Controller):
    path = "/"

    @get("/log")
    async def log(self, repo_dir: str, limit: int = 1500, all: bool = True, ref: str | None = None) -> dict[str, Any]:
        return await queries.log(repo_dir, max(1, min(limit, 200_000)), all, ref)

    @get("/commit")
    async def commit(self, repo_dir: str, sha: str) -> dict[str, Any]:
        return await queries.commit_detail(repo_dir, sha)

    @get("/diff/commit")
    async def commit_diff(
        self,
        repo_dir: str,
        sha: str,
        path: str,
        orig_path: str | None = None,
        context: int = 3,
        ignore_ws: bool = False,
    ) -> dict[str, Any] | None:
        return await queries.commit_diff(repo_dir, sha, path, orig_path, context, ignore_ws)

    @get("/search")
    async def search(self, repo_dir: str, q: str, mode: str = "message") -> list[dict[str, Any]]:
        if not q.strip():
            return []
        return await queries.search(repo_dir, q.strip(), mode)

    @get("/file-history")
    async def file_history(self, repo_dir: str, path: str) -> list[dict[str, Any]]:
        return await queries.file_history(repo_dir, path)

    @get("/blame")
    async def blame(self, repo_dir: str, path: str, rev: str | None = None) -> list[dict[str, Any]]:
        return await queries.blame(repo_dir, path, rev)
