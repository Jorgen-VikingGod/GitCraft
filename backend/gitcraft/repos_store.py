"""Persistent list of repositories the user has opened."""

from __future__ import annotations

import json
import os
import time
from pathlib import Path
from typing import Any

from platformdirs import user_config_dir


def config_dir() -> Path:
    return Path(os.environ.get("GITCRAFT_CONFIG_DIR") or user_config_dir("gitcraft", appauthor=False))


class RepoStore:
    def __init__(self, path: Path | None = None) -> None:
        self.path = path or config_dir() / "repos.json"
        self._repos: list[dict[str, Any]] = []
        self._load()

    def _load(self) -> None:
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
            self._repos = [r for r in data.get("repos", []) if isinstance(r, dict) and "path" in r]
        except (OSError, ValueError):
            self._repos = []

    def _save(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_suffix(".tmp")
        tmp.write_text(json.dumps({"repos": self._repos}, indent=2), encoding="utf-8")
        tmp.replace(self.path)

    def list(self) -> list[dict[str, Any]]:
        return [dict(r, exists=Path(r["path"]).is_dir()) for r in self._repos]

    def add(self, path: str) -> dict[str, Any]:
        self._repos = [r for r in self._repos if r["path"] != path]
        entry = {"path": path, "name": Path(path).name, "lastOpened": int(time.time())}
        self._repos.insert(0, entry)
        del self._repos[50:]
        self._save()
        return entry

    def remove(self, path: str) -> None:
        self._repos = [r for r in self._repos if r["path"] != path]
        self._save()

    def is_known(self, path: str) -> bool:
        return any(r["path"] == path for r in self._repos)


store = RepoStore()
