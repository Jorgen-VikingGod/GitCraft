"""Smaller tools: patches, .gitignore, clean, reflog, compare, file checkout, fast-forward, submodules."""

from __future__ import annotations

from typing import Any

from litestar import Controller, Response, get, post

from ..git import ops, queries
from ..ws import progress
from .common import Body


class CheckoutFileBody(Body):
    sha: str
    path: str


class FastForwardBody(Body):
    branch: str
    target: str | None = None


class PatternBody(Body):
    pattern: str


class CleanBody(Body):
    directories: bool = True
    ignored: bool = False
    dry_run: bool = True


class ApplyPatchBody(Body):
    patch: str
    mode: str = "am"  # am | apply


class NameOpBody(Body):
    name: str
    op_id: str | None = None


class OpOnlyBody(Body):
    op_id: str | None = None


class ToolsController(Controller):
    path = "/"

    @post("/checkout-file")
    async def checkout_file(self, repo_dir: str, data: CheckoutFileBody) -> dict[str, Any]:
        return await ops.checkout_file(repo_dir, data.sha, data.path)

    @post("/branch/fast-forward")
    async def fast_forward(self, repo_dir: str, data: FastForwardBody) -> dict[str, Any]:
        return await ops.fast_forward(repo_dir, data.branch, data.target)

    @post("/ignore")
    async def ignore(self, repo_dir: str, data: PatternBody) -> dict[str, Any]:
        return await ops.add_to_gitignore(repo_dir, data.pattern)

    @post("/clean")
    async def clean(self, repo_dir: str, data: CleanBody) -> dict[str, Any]:
        return await ops.clean(repo_dir, data.directories, data.ignored, data.dry_run)

    @get("/patch")
    async def patch(self, repo_dir: str, sha: str) -> Response[str]:
        text = await ops.format_patch(repo_dir, sha)
        return Response(text, media_type="text/x-patch")

    @post("/patch/apply")
    async def apply_patch(self, repo_dir: str, data: ApplyPatchBody) -> dict[str, Any]:
        return await ops.apply_patch(repo_dir, data.patch, data.mode)

    @get("/reflog")
    async def reflog(self, repo_dir: str, ref: str = "HEAD", limit: int = 300) -> list[dict[str, Any]]:
        return await queries.reflog(repo_dir, ref, max(1, min(limit, 5000)))

    @get("/compare")
    async def compare(self, repo_dir: str, base: str, target: str) -> dict[str, Any]:
        return await queries.compare(repo_dir, base, target)

    @get("/diff/compare")
    async def compare_diff(
        self, repo_dir: str, base: str, target: str, path: str, context: int = 3
    ) -> dict[str, Any] | None:
        return await queries.range_diff(repo_dir, base, target, path, context)

    @post("/remote/prune")
    async def prune(self, repo_dir: str, data: NameOpBody) -> dict[str, Any]:
        return await ops.prune_remote(repo_dir, data.name, progress(data.op_id, repo_dir))

    @post("/submodules/update")
    async def update_submodules(self, repo_dir: str, data: OpOnlyBody) -> dict[str, Any]:
        return await ops.update_submodules(repo_dir, progress(data.op_id, repo_dir))
