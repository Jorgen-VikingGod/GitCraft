from pathlib import Path

import pytest

from gitcraft.git import ops, queries

from .conftest import commit_file, git, write


async def test_checkout_file_at_revision(repo_with_history: Path):
    repo = str(repo_with_history)
    first = git(repo_with_history, "rev-parse", "HEAD~1").strip()
    await ops.checkout_file(repo, first, "a.txt")
    assert (repo_with_history / "a.txt").read_text() == "one\ntwo\nthree\n"
    assert (await queries.status(repo))["staged"][0]["path"] == "a.txt"


async def test_fast_forward_without_checkout(repo_with_history: Path):
    repo = str(repo_with_history)
    git(repo_with_history, "branch", "behind", "HEAD~1")
    git(repo_with_history, "branch", "--set-upstream-to=main", "behind")
    await ops.fast_forward(repo, "behind", None)
    assert git(repo_with_history, "rev-parse", "behind") == git(repo_with_history, "rev-parse", "main")
    # Diverged branches are refused.
    git(repo_with_history, "switch", "-q", "-c", "side", "HEAD~1")
    commit_file(repo_with_history, "s.txt", "s\n", "side")
    git(repo_with_history, "switch", "-q", "main")
    with pytest.raises(ValueError):
        await ops.fast_forward(repo, "side", "main")


async def test_fast_forward_current_branch(repo_with_history: Path):
    repo = str(repo_with_history)
    git(repo_with_history, "switch", "-q", "-c", "old", "HEAD~1")
    await ops.fast_forward(repo, "old", "main")
    assert git(repo_with_history, "rev-parse", "HEAD") == git(repo_with_history, "rev-parse", "main")
    assert (repo_with_history / "a.txt").read_text().endswith("four\n")


async def test_gitignore(repo_with_history: Path):
    repo = str(repo_with_history)
    write(repo_with_history, "build.log", "x")
    write(repo_with_history, ".gitignore", "node_modules/")
    await ops.add_to_gitignore(repo, "*.log")
    await ops.add_to_gitignore(repo, "*.log")
    assert (repo_with_history / ".gitignore").read_text() == "node_modules/\n*.log\n"
    unstaged = (await queries.status(repo))["unstaged"]
    assert [e["path"] for e in unstaged] == [".gitignore"]
    # Ignoring a tracked file also removes it from the index.
    await ops.add_to_gitignore(repo, "a.txt")
    staged = (await queries.status(repo))["staged"]
    assert {"path": "a.txt", "status": "D", "origPath": None} in staged
    with pytest.raises(ValueError):
        await ops.add_to_gitignore(repo, "a\nb")


async def test_clean(repo_with_history: Path):
    repo = str(repo_with_history)
    write(repo_with_history, "junk.txt", "x")
    write(repo_with_history, "tmp/inner.txt", "x")
    preview = await ops.clean(repo, directories=True, ignored=False, dry_run=True)
    assert sorted(preview["files"]) == ["junk.txt", "tmp/"]
    assert (repo_with_history / "junk.txt").exists()
    await ops.clean(repo, directories=True, ignored=False, dry_run=False)
    assert not (repo_with_history / "junk.txt").exists() and not (repo_with_history / "tmp").exists()


async def test_patch_roundtrip(tmp_path: Path, repo_with_history: Path):
    repo = str(repo_with_history)
    patch = await ops.format_patch(repo, "HEAD")
    assert "Subject: [PATCH] second" in patch
    git(repo_with_history, "reset", "-q", "--hard", "HEAD~1")
    result = await ops.apply_patch(repo, patch, "am")
    assert result["ok"]
    assert (await queries.log(repo, 1, all_refs=False))["commits"][0]["subject"] == "second"
    git(repo_with_history, "reset", "-q", "--hard", "HEAD~1")
    assert (await ops.apply_patch(repo, patch, "apply"))["ok"]
    assert (await queries.status(repo))["staged"][0]["path"] == "a.txt"  # --3way updates the index


async def test_stash_selected_paths(repo_with_history: Path):
    repo = str(repo_with_history)
    write(repo_with_history, "a.txt", "changed\n")
    commit_file(repo_with_history, "b.txt", "b\n", "add b")
    write(repo_with_history, "b.txt", "changed b\n")
    await ops.stash_push(repo, "only a", False, False, False, paths=["a.txt"])
    unstaged = [e["path"] for e in (await queries.status(repo))["unstaged"]]
    assert unstaged == ["b.txt"]
    assert [f["path"] for f in await queries.stash_files(repo, "stash@{0}")] == ["a.txt"]


async def test_reflog_and_compare(repo_with_history: Path):
    repo = str(repo_with_history)
    git(repo_with_history, "reset", "-q", "--hard", "HEAD~1")
    entries = await queries.reflog(repo)
    assert entries[0]["selector"] == "HEAD@{0}" and entries[0]["action"].startswith("reset")
    lost = entries[1]["sha"]
    assert entries[1]["subject"] == "second"
    result = await queries.compare(repo, "HEAD", lost)
    assert result["files"] == [{"status": "M", "origPath": None, "path": "a.txt"}]
    assert (result["baseOnly"], result["targetOnly"]) == (0, 1)
