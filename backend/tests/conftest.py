import os
import subprocess
import tempfile
from pathlib import Path

import pytest

# The repo store is created at import time, keep it away from the real config.
os.environ["GITCRAFT_CONFIG_DIR"] = tempfile.mkdtemp(prefix="gitcraft-test-config-")


def git(repo: Path, *args: str, input: str | None = None) -> str:
    env = {**os.environ, "GIT_EDITOR": "true", "GIT_CONFIG_NOSYSTEM": "1"}
    result = subprocess.run(
        ["git", *args], cwd=repo, capture_output=True, text=True, input=input, env=env, check=True
    )
    return result.stdout


def write(repo: Path, name: str, content: str) -> None:
    path = repo / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8", newline="")


def commit_file(repo: Path, name: str, content: str, message: str) -> str:
    write(repo, name, content)
    git(repo, "add", name)
    git(repo, "commit", "-q", "-m", message)
    return git(repo, "rev-parse", "HEAD").strip()


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    path = tmp_path / "repo"
    path.mkdir()
    git(path, "init", "-q", "-b", "main")
    git(path, "config", "user.name", "Test User")
    git(path, "config", "user.email", "test@example.com")
    git(path, "config", "commit.gpgsign", "false")
    return path.resolve()


@pytest.fixture
def repo_with_history(repo: Path) -> Path:
    commit_file(repo, "a.txt", "one\ntwo\nthree\n", "first")
    commit_file(repo, "a.txt", "one\ntwo\nthree\nfour\n", "second")
    return repo
