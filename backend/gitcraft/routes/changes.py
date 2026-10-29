from __future__ import annotations

from typing import Any

from litestar import Controller, get, post

from ..git import ops, queries
from .common import Body, PathsBody


class SelectionBody(Body):
    path: str
    mode: str  # stage | unstage | discard
    selection: dict[str, list[int] | None]
    context: int = 3


class CommitBody(Body):
    message: str
    amend: bool = False
    sign_off: bool = False


class ChangesController(Controller):
    path = "/"

    @get("/status")
    async def status(self, repo_dir: str) -> dict[str, Any]:
        return await queries.status(repo_dir)

    @get("/diff/working")
    async def working_diff(
        self,
        repo_dir: str,
        path: str,
        staged: bool = False,
        context: int = 3,
        ignore_ws: bool = False,
        orig_path: str | None = None,
    ) -> dict[str, Any] | None:
        return await queries.working_diff(repo_dir, path, staged, context, ignore_ws, orig_path)

    @post("/stage")
    async def stage(self, repo_dir: str, data: PathsBody) -> None:
        await ops.stage_files(repo_dir, data.paths)

    @post("/stage/all")
    async def stage_all(self, repo_dir: str) -> None:
        await ops.stage_all(repo_dir)

    @post("/unstage")
    async def unstage(self, repo_dir: str, data: PathsBody) -> None:
        await ops.unstage_files(repo_dir, data.paths)

    @post("/unstage/all")
    async def unstage_all(self, repo_dir: str) -> None:
        await ops.unstage_all(repo_dir)

    @post("/discard")
    async def discard(self, repo_dir: str, data: PathsBody) -> None:
        await ops.discard_files(repo_dir, data.paths)

    @post("/selection")
    async def apply_selection(self, repo_dir: str, data: SelectionBody) -> None:
        selection = {int(k): v for k, v in data.selection.items()}
        await ops.apply_selection(repo_dir, data.path, data.mode, selection, data.context)

    @post("/commit")
    async def commit(self, repo_dir: str, data: CommitBody) -> dict[str, Any]:
        return await ops.commit(repo_dir, data.message, data.amend, data.sign_off)

    @get("/commit/head-message")
    async def head_message(self, repo_dir: str) -> dict[str, str]:
        return {"message": await queries.head_message(repo_dir)}

    @get("/commit/template")
    async def template(self, repo_dir: str) -> dict[str, str]:
        return {"template": await queries.commit_template(repo_dir)}
