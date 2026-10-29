"""Merge, rebase (incl. interactive), cherry-pick, revert, reset and in-progress operations."""

from __future__ import annotations

from typing import Any

from litestar import Controller, get, post

from ..git import ops, queries
from .common import Body


class MergeBody(Body):
    ref: str
    mode: str = "default"  # default | no-ff | ff-only | squash


class RebaseBody(Body):
    onto: str


class CherryPickBody(Body):
    shas: list[str]


class RevertBody(Body):
    sha: str
    mainline: int | None = None


class ResetBody(Body):
    target: str
    mode: str = "mixed"


class RebaseStep(Body):
    sha: str
    action: str
    message: str | None = None


class InteractiveRebaseBody(Body):
    base: str | None
    steps: list[RebaseStep]
    autostash: bool = False


class WorkflowController(Controller):
    path = "/"

    @post("/merge")
    async def merge(self, repo_dir: str, data: MergeBody) -> dict[str, Any]:
        return await ops.merge(repo_dir, data.ref, data.mode)

    @post("/rebase")
    async def rebase(self, repo_dir: str, data: RebaseBody) -> dict[str, Any]:
        return await ops.rebase(repo_dir, data.onto)

    @get("/rebase/commits")
    async def rebase_commits(self, repo_dir: str, base: str | None = None) -> list[dict[str, Any]]:
        return await queries.rebase_candidates(repo_dir, base)

    @post("/rebase/interactive")
    async def interactive(self, repo_dir: str, data: InteractiveRebaseBody) -> dict[str, Any]:
        steps = [{"sha": s.sha, "action": s.action, "message": s.message} for s in data.steps]
        return await ops.interactive_rebase(repo_dir, data.base, steps, data.autostash)

    @post("/cherry-pick")
    async def cherry_pick(self, repo_dir: str, data: CherryPickBody) -> dict[str, Any]:
        return await ops.cherry_pick(repo_dir, data.shas)

    @post("/revert")
    async def revert(self, repo_dir: str, data: RevertBody) -> dict[str, Any]:
        return await ops.revert(repo_dir, data.sha, data.mainline)

    @post("/reset")
    async def reset(self, repo_dir: str, data: ResetBody) -> dict[str, Any]:
        return await ops.reset(repo_dir, data.target, data.mode)

    @post("/operation/continue")
    async def continue_operation(self, repo_dir: str) -> dict[str, Any]:
        return await ops.continue_operation(repo_dir)

    @post("/operation/abort")
    async def abort_operation(self, repo_dir: str) -> dict[str, Any]:
        return await ops.abort_operation(repo_dir)

    @post("/operation/skip")
    async def skip_operation(self, repo_dir: str) -> dict[str, Any]:
        return await ops.skip_operation(repo_dir)
