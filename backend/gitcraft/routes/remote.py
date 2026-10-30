from __future__ import annotations

from typing import Any

from litestar import Controller, post

from ..git import ops
from ..ws import progress
from .common import Body


class FetchBody(Body):
    remote: str | None = None
    prune: bool = True
    tags: bool = False
    op_id: str | None = None


class PullBody(Body):
    remote: str | None = None
    branch: str | None = None
    mode: str = "merge"  # merge | rebase | ff-only
    autostash: bool = False
    op_id: str | None = None


class PushBody(Body):
    remote: str
    branch: str | None = None
    set_upstream: bool = False
    force: bool = False
    tags: bool = False
    op_id: str | None = None


class RemoteController(Controller):
    path = "/"

    @post("/fetch")
    async def fetch(self, repo_dir: str, data: FetchBody) -> dict[str, Any]:
        return await ops.fetch(repo_dir, data.remote, data.prune, data.tags, progress(data.op_id, repo_dir))

    @post("/pull")
    async def pull(self, repo_dir: str, data: PullBody) -> dict[str, Any]:
        return await ops.pull(repo_dir, data.remote, data.branch, data.mode, progress(data.op_id, repo_dir),
                              data.autostash)

    @post("/push")
    async def push(self, repo_dir: str, data: PushBody) -> dict[str, Any]:
        return await ops.push(
            repo_dir, data.remote, data.branch, data.set_upstream, data.force, data.tags, progress(data.op_id, repo_dir)
        )
