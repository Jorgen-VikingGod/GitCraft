"""Read-only git queries."""

from __future__ import annotations

import asyncio
from pathlib import Path
from typing import Any

from . import graph, parsers
from .parsers import DETAIL_FORMAT, LOG_FORMAT
from .runner import GitError, run_git

EMPTY_TREE = "4b825dc642cb6eb9a060e54bf8d69288fbee4904"


async def toplevel(path: str) -> str:
    result = await run_git(path, "rev-parse", "--show-toplevel")
    return str(Path(result.out.strip()).resolve())


async def git_dir(repo: str) -> Path:
    result = await run_git(repo, "rev-parse", "--absolute-git-dir")
    return Path(result.out.strip())


async def has_head(repo: str) -> bool:
    result = await run_git(repo, "rev-parse", "--verify", "-q", "HEAD", check=False)
    return result.code == 0


async def operation_state(repo: str) -> dict[str, Any] | None:
    gd = await git_dir(repo)
    if (gd / "rebase-merge").is_dir() or (gd / "rebase-apply").is_dir():
        state_dir = gd / "rebase-merge" if (gd / "rebase-merge").is_dir() else gd / "rebase-apply"
        if (state_dir / "applying").exists():
            return {"kind": "am"}

        def read(name: str) -> str:
            path = state_dir / name
            return path.read_text(encoding="utf-8", errors="replace").strip() if path.exists() else ""

        head_name = read("head-name").removeprefix("refs/heads/")
        step = read("msgnum") or read("next")
        total = read("end") or read("last")
        stopped = read("stopped-sha")
        return {
            "kind": "rebase",
            "branch": head_name or None,
            "onto": read("onto") or None,
            "step": int(step) if step.isdigit() else None,
            "total": int(total) if total.isdigit() else None,
            "stoppedSha": stopped or None,
        }
    for name, kind in (("MERGE_HEAD", "merge"), ("CHERRY_PICK_HEAD", "cherry-pick"), ("REVERT_HEAD", "revert")):
        path = gd / name
        if path.exists():
            return {"kind": kind, "head": path.read_text().strip().split("\n")[0]}
    return None


async def log(repo: str, limit: int, all_refs: bool = True, ref: str | None = None) -> dict[str, Any]:
    args = ["log", "--topo-order", "--decorate=full", LOG_FORMAT, f"--max-count={limit + 1}"]
    if ref:
        args.append(ref)
    elif all_refs:
        args += ["--exclude=refs/stash", "--all"]
    if not await has_head(repo) and not ref:
        refs = await run_git(repo, "for-each-ref", "--count=1", "--format=%(refname)")
        if not refs.out.strip():
            return {"commits": [], "hasMore": False}
    result = await run_git(repo, *args, "--")
    commits = parsers.parse_log(result.out)
    has_more = len(commits) > limit
    commits = commits[:limit]
    for commit, row in zip(commits, graph.layout(commits)):
        commit["graph"] = row
    return {"commits": commits, "hasMore": has_more}


async def commit_detail(repo: str, sha: str) -> dict[str, Any]:
    result = await run_git(repo, "show", "-s", "--decorate=full", DETAIL_FORMAT, sha, "--")
    detail = parsers.parse_commit_detail(result.out)
    detail["files"] = await commit_files(repo, detail["sha"], detail["parents"])
    return detail


async def commit_files(repo: str, sha: str, parents: list[str]) -> list[dict[str, Any]]:
    base = parents[0] if parents else EMPTY_TREE
    result = await run_git(repo, "diff", "--name-status", "-z", "-M", base, sha, "--")
    return parsers.parse_name_status(result.out)


def _diff_flags(context: int, ignore_whitespace: bool) -> list[str]:
    flags = [f"-U{max(0, min(context, 1000))}", "-M", "--no-ext-diff"]
    if ignore_whitespace:
        flags.append("-w")
    return flags


async def commit_diff(
    repo: str, sha: str, path: str, orig_path: str | None, context: int = 3, ignore_whitespace: bool = False
) -> dict[str, Any] | None:
    detail = await run_git(repo, "rev-list", "--parents", "-n", "1", sha)
    parts = detail.out.split()
    base = parts[1] if len(parts) > 1 else EMPTY_TREE
    paths = [p for p in (orig_path, path) if p]
    result = await run_git(repo, "diff", *_diff_flags(context, ignore_whitespace), base, parts[0], "--", *paths)
    files = parsers.parse_diff(result.out)
    return files[0] if files else None


async def range_diff(repo: str, base: str, target: str, path: str, context: int = 3) -> dict[str, Any] | None:
    result = await run_git(repo, "diff", *_diff_flags(context, False), base, target, "--", path)
    files = parsers.parse_diff(result.out)
    return files[0] if files else None


async def status(repo: str) -> dict[str, Any]:
    result = await run_git(repo, "status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all")
    data = parsers.parse_status(result.out)
    data["operation"] = await operation_state(repo)
    return data


async def working_diff(
    repo: str, path: str, staged: bool, context: int = 3, ignore_whitespace: bool = False, orig_path: str | None = None
) -> dict[str, Any] | None:
    if not staged:
        tracked = await run_git(repo, "ls-files", "--error-unmatch", "--", path, check=False)
        if tracked.code != 0:
            file_path = Path(repo) / path
            if file_path.is_file():
                return parsers.untracked_diff(path, file_path.read_bytes())
            return None
    args = ["diff", *_diff_flags(context, ignore_whitespace)]
    if staged:
        args.append("--cached")
    paths = [p for p in (orig_path, path) if p]
    result = await run_git(repo, *args, "--", *paths)
    files = parsers.parse_diff(result.out)
    return files[0] if files else None


async def refs(repo: str) -> dict[str, Any]:
    remotes_out, refs_out, stash_out, sub_out = await asyncio.gather(
        run_git(repo, "remote", "-v"),
        run_git(repo, "for-each-ref", parsers.REF_FORMAT, "refs/heads", "refs/remotes", "refs/tags"),
        run_git(repo, "stash", "list", "--format=%gd%x1f%H%x1f%at%x1f%gs", check=False),
        run_git(repo, "submodule", "status", check=False),
    )
    remotes = parsers.parse_remotes(remotes_out.out)
    data = parsers.parse_refs(refs_out.out, [r["name"] for r in remotes])
    data["remotes"] = remotes
    data["stashes"] = parsers.parse_stashes(stash_out.out)
    data["submodules"] = parsers.parse_submodules(sub_out.out)
    head = await run_git(repo, "rev-parse", "HEAD", check=False)
    data["head"] = head.out.strip() if head.code == 0 else None
    return data


async def stash_files(repo: str, name: str) -> list[dict[str, Any]]:
    result = await run_git(repo, "stash", "show", "--name-status", "-z", "-M", name)
    files = parsers.parse_name_status(result.out)
    untracked = await run_git(repo, "rev-parse", "-q", "--verify", f"{name}^3", check=False)
    if untracked.code == 0:
        listing = await run_git(repo, "ls-tree", "-r", "-z", "--name-only", f"{name}^3")
        files += [{"status": "?", "origPath": None, "path": p} for p in listing.out.split("\0") if p]
    return files


async def stash_diff(repo: str, name: str, path: str, context: int = 3) -> dict[str, Any] | None:
    diff = await range_diff(repo, f"{name}^1", name, path, context)
    if diff is None:
        untracked = await run_git(repo, "rev-parse", "-q", "--verify", f"{name}^3", check=False)
        if untracked.code == 0:
            diff = await range_diff(repo, EMPTY_TREE, f"{name}^3", path, context)
    return diff


async def search(repo: str, query: str, mode: str, limit: int = 500) -> list[dict[str, Any]]:
    args = ["log", "--decorate=full", LOG_FORMAT, f"--max-count={limit}", "--exclude=refs/stash", "--all"]
    if mode == "sha":
        resolved = await run_git(repo, "rev-parse", "--verify", "-q", f"{query}^{{commit}}", check=False)
        if resolved.code != 0:
            return []
        result = await run_git(repo, "show", "-s", "--decorate=full", LOG_FORMAT, resolved.out.strip(), "--")
        return parsers.parse_log(result.out)
    if mode == "author":
        args += ["-i", f"--author={query}"]
    elif mode == "content":
        args += [f"-S{query}"]
    else:
        args += ["-i", "--fixed-strings", f"--grep={query}"]
    result = await run_git(repo, *args, "--")
    return parsers.parse_log(result.out)


async def file_history(repo: str, path: str, limit: int = 1000) -> list[dict[str, Any]]:
    result = await run_git(
        repo, "log", "--follow", "--decorate=full", LOG_FORMAT, "--name-status", "-z", f"--max-count={limit}", "--", path
    )
    commits = []
    for record in result.out.split(parsers.RS)[1:]:
        meta, _, files = record.partition("\0")
        commit = parsers.parse_log(parsers.RS + meta)
        if not commit:
            continue
        entry = commit[0]
        changed = parsers.parse_name_status(files.lstrip("\n"))
        entry["file"] = changed[0] if changed else {"status": "M", "origPath": None, "path": path}
        commits.append(entry)
    return commits


async def blame(repo: str, path: str, rev: str | None = None) -> list[dict[str, Any]]:
    args = ["blame", "--porcelain"]
    if rev:
        args.append(rev)
    result = await run_git(repo, *args, "--", path)
    return parsers.parse_blame(result.out)


async def head_message(repo: str) -> str:
    result = await run_git(repo, "log", "-1", "--format=%B", check=False)
    return result.out.strip() if result.code == 0 else ""


async def config_value(repo: str, key: str) -> str | None:
    result = await run_git(repo, "config", "--get", key, check=False)
    return result.out.strip() if result.code == 0 else None


async def commit_template(repo: str) -> str:
    path = await config_value(repo, "commit.template")
    if not path:
        return ""
    template = Path(path).expanduser()
    if not template.is_absolute():
        template = Path(repo) / template
    try:
        return template.read_text(encoding="utf-8")
    except OSError:
        return ""


async def rebase_candidates(repo: str, base: str | None) -> list[dict[str, Any]]:
    args = ["log", "--reverse", "--topo-order", "--no-merges", LOG_FORMAT]
    args.append(f"{base}..HEAD" if base else "HEAD")
    result = await run_git(repo, *args, "--")
    commits = parsers.parse_log(result.out)
    for commit in commits:
        commit.pop("refs", None)
    return commits


async def raw_show(repo: str, spec: str) -> str | None:
    try:
        result = await run_git(repo, "show", spec)
    except GitError:
        return None
    return result.out


REFLOG_FORMAT = "--format=" + "%x1f".join(["%H", "%gd", "%gs", "%ct", "%s", "%an"])


async def reflog(repo: str, ref: str = "HEAD", limit: int = 300) -> list[dict[str, Any]]:
    result = await run_git(repo, "reflog", "show", REFLOG_FORMAT, f"-n{limit}", ref, "--", check=False)
    entries = []
    for line in result.out.splitlines():
        fields = line.split(parsers.US)
        if len(fields) >= 6:
            sha, selector, action, time, subject, author = fields[:6]
            entries.append(
                {"sha": sha, "selector": selector, "action": action, "time": int(time or 0), "subject": subject,
                 "author": author}
            )
    return entries


async def compare(repo: str, base: str, target: str) -> dict[str, Any]:
    """Files changed between two commits (base -> target), plus commit counts each way."""
    files = await run_git(repo, "diff", "--name-status", "-z", "-M", base, target, "--")
    counts = await run_git(repo, "rev-list", "--left-right", "--count", f"{base}...{target}")
    left, right = (int(n) for n in counts.out.split())
    return {"files": parsers.parse_name_status(files.out), "baseOnly": left, "targetOnly": right}
