from __future__ import annotations

from typing import Any

from litestar import Controller, get, post

from ..git import ops, queries
from ..ws import progress
from .common import Body


class CheckoutBody(Body):
    ref: str
    kind: str = "local"  # local | remote | commit


class BranchBody(Body):
    name: str
    start: str | None = None
    checkout: bool = True


class RenameBody(Body):
    old: str
    new: str


class DeleteBranchBody(Body):
    name: str
    force: bool = False


class DeleteRemoteBranchBody(Body):
    remote: str
    branch: str
    op_id: str | None = None


class UpstreamBody(Body):
    branch: str
    upstream: str | None = None


class TagBody(Body):
    name: str
    target: str = "HEAD"
    message: str | None = None


class NameBody(Body):
    name: str


class PushTagBody(Body):
    remote: str
    name: str
    delete: bool = False
    op_id: str | None = None


class RemoteBody(Body):
    name: str
    url: str


class EditRemoteBody(Body):
    name: str
    new_name: str | None = None
    url: str | None = None


class RefsController(Controller):
    path = "/"

    @get("/refs")
    async def refs(self, repo_dir: str) -> dict[str, Any]:
        return await queries.refs(repo_dir)

    @post("/checkout")
    async def checkout(self, repo_dir: str, data: CheckoutBody) -> dict[str, Any]:
        return await ops.checkout(repo_dir, data.ref, data.kind)

    @post("/branch")
    async def create_branch(self, repo_dir: str, data: BranchBody) -> dict[str, Any]:
        return await ops.create_branch(repo_dir, data.name, data.start, data.checkout)

    @post("/branch/rename")
    async def rename_branch(self, repo_dir: str, data: RenameBody) -> dict[str, Any]:
        return await ops.rename_branch(repo_dir, data.old, data.new)

    @post("/branch/delete")
    async def delete_branch(self, repo_dir: str, data: DeleteBranchBody) -> dict[str, Any]:
        return await ops.delete_branch(repo_dir, data.name, data.force)

    @post("/branch/delete-remote")
    async def delete_remote_branch(self, repo_dir: str, data: DeleteRemoteBranchBody) -> dict[str, Any]:
        return await ops.delete_remote_branch(repo_dir, data.remote, data.branch, progress(data.op_id, repo_dir))

    @post("/branch/upstream")
    async def upstream(self, repo_dir: str, data: UpstreamBody) -> dict[str, Any]:
        return await ops.set_upstream(repo_dir, data.branch, data.upstream)

    @post("/tag")
    async def create_tag(self, repo_dir: str, data: TagBody) -> dict[str, Any]:
        return await ops.create_tag(repo_dir, data.name, data.target, data.message)

    @post("/tag/delete")
    async def delete_tag(self, repo_dir: str, data: NameBody) -> dict[str, Any]:
        return await ops.delete_tag(repo_dir, data.name)

    @post("/tag/push")
    async def push_tag(self, repo_dir: str, data: PushTagBody) -> dict[str, Any]:
        return await ops.push_tag(repo_dir, data.remote, data.name, data.delete, progress(data.op_id, repo_dir))

    @post("/remote")
    async def add_remote(self, repo_dir: str, data: RemoteBody) -> dict[str, Any]:
        return await ops.add_remote(repo_dir, data.name, data.url)

    @post("/remote/remove")
    async def remove_remote(self, repo_dir: str, data: NameBody) -> dict[str, Any]:
        return await ops.remove_remote(repo_dir, data.name)

    @post("/remote/edit")
    async def edit_remote(self, repo_dir: str, data: EditRemoteBody) -> dict[str, Any]:
        return await ops.edit_remote(repo_dir, data.name, data.new_name, data.url)
