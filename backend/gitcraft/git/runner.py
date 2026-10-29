"""Thin async wrapper around the git command line."""

from __future__ import annotations

import asyncio
import os
import re
import subprocess
import sys
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Awaitable, Callable

EDITOR_HELPER = Path(__file__).with_name("editor_helper.py")

ProgressCallback = Callable[[str], Awaitable[None]]


class GitError(Exception):
    def __init__(self, args: list[str], code: int, stderr: str, stdout: str = "") -> None:
        self.git_args = list(args)
        self.code = code
        self.stderr = stderr.strip()
        self.stdout = stdout
        message = self.stderr or stdout.strip() or f"git {' '.join(args)} failed with exit code {code}"
        super().__init__(message)


@dataclass
class GitResult:
    code: int
    out: str
    err: str

    @property
    def output(self) -> str:
        return "\n".join(part for part in (self.out.strip(), self.err.strip()) if part)


def _quote(path: Path | str) -> str:
    # Git runs editors through a shell (also on Windows, via its bundled sh),
    # which copes with forward slashes everywhere.
    return '"' + str(path).replace("\\", "/") + '"'


EDITOR_FLAG = "--gitcraft-editor"


def editor_command(*extra: str) -> str:
    if getattr(sys, "frozen", False):
        # Bundled server (PyInstaller): there is no Python interpreter, so the
        # executable itself acts as the editor when called with EDITOR_FLAG.
        return " ".join([_quote(sys.executable), EDITOR_FLAG, *extra])
    return " ".join([_quote(sys.executable), _quote(EDITOR_HELPER), *extra])


def git_env(extra: dict[str, str] | None = None) -> dict[str, str]:
    env = os.environ.copy()
    for key in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "LC_ALL"):
        env.pop(key, None)
    env.update(
        {
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_OPTIONAL_LOCKS": "0",
            "LANGUAGE": "C",
            "LC_MESSAGES": "C",
            "GIT_EDITOR": editor_command("msg"),
            "GIT_SEQUENCE_EDITOR": editor_command("todo"),
            "GIT_MERGE_AUTOEDIT": "no",
            "GIT_PAGER": "cat",
        }
    )
    if extra:
        env.update(extra)
    return env


def _detach_kwargs() -> dict:
    # Without a controlling terminal ssh/gpg cannot block waiting for a prompt.
    if sys.platform == "win32":
        return {"creationflags": subprocess.CREATE_NO_WINDOW}
    return {"start_new_session": True}


def _decode(data: bytes) -> str:
    return data.decode("utf-8", errors="replace")


async def run_git(
    cwd: str | Path,
    *args: str,
    stdin: str | bytes | None = None,
    env: dict[str, str] | None = None,
    check: bool = True,
    on_progress: ProgressCallback | None = None,
) -> GitResult:
    cmd = ["git", "-c", "color.ui=false", "-c", "core.quotepath=false", *args]
    proc = await asyncio.create_subprocess_exec(
        *cmd,
        cwd=str(cwd),
        stdin=asyncio.subprocess.PIPE if stdin is not None else asyncio.subprocess.DEVNULL,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
        env=git_env(env),
        **_detach_kwargs(),
    )
    data = stdin.encode() if isinstance(stdin, str) else stdin

    if on_progress is None:
        out, err = await proc.communicate(data)
    else:
        if data is not None and proc.stdin:
            proc.stdin.write(data)
            await proc.stdin.drain()
            proc.stdin.close()
        out_task = asyncio.create_task(proc.stdout.read())  # type: ignore[union-attr]
        err = await _pump_progress(proc.stderr, on_progress)  # type: ignore[arg-type]
        out = await out_task
        await proc.wait()

    result = GitResult(proc.returncode or 0, _decode(out), _decode(err))
    if check and result.code != 0:
        raise GitError(list(args), result.code, result.err, result.out)
    return result


async def _pump_progress(stream: asyncio.StreamReader, on_progress: ProgressCallback) -> bytes:
    chunks: list[bytes] = []
    buffer = b""
    while True:
        data = await stream.read(1024)
        if not data:
            break
        chunks.append(data)
        buffer += data
        *lines, buffer = re.split(rb"[\r\n]", buffer)
        for line in lines:
            if line.strip():
                await on_progress(_decode(line))
    if buffer.strip():
        await on_progress(_decode(buffer))
    # Keep only final lines of each \r-updated progress counter for error reports.
    text = b"".join(chunks)
    return b"\n".join(part.split(b"\r")[-1] for part in text.split(b"\n"))


_locks: dict[str, asyncio.Lock] = defaultdict(asyncio.Lock)


def repo_lock(repo: str | Path) -> asyncio.Lock:
    """Serialises mutating operations per repository to avoid index.lock races."""
    return _locks[str(repo)]
