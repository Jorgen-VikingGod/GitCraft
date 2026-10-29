"""Shared pieces for the API routes."""

from __future__ import annotations

from pathlib import Path

import msgspec
from litestar.exceptions import NotFoundException, PermissionDeniedException

from ..repos_store import store


class Body(msgspec.Struct, rename="camel", kw_only=True):
    """Base for request bodies: JSON keys are camelCase."""


def resolve_repo(repo: str) -> str:
    """Dependency: the `repo` query parameter, checked against opened repositories."""
    path = str(Path(repo).resolve())
    if not store.is_known(path):
        raise PermissionDeniedException("Repository has not been opened in GitCraft")
    if not Path(path).is_dir():
        raise NotFoundException("Repository no longer exists")
    return path


class PathsBody(Body):
    paths: list[str]


class OpBody(Body):
    op_id: str | None = None
