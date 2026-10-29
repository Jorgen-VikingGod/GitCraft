from pathlib import Path

import pytest

from gitcraft.git import ops, queries
from gitcraft.git.runner import GitError

from .conftest import commit_file, git, write


async def test_log_and_detail(repo_with_history: Path):
    repo = str(repo_with_history)
    data = await queries.log(repo, 10)
    assert [c["subject"] for c in data["commits"]] == ["second", "first"]
    assert data["commits"][0]["refs"][0] == {"name": "main", "kind": "local", "current": True}
    assert not data["hasMore"]
    detail = await queries.commit_detail(repo, data["commits"][0]["sha"])
    assert detail["files"] == [{"status": "M", "origPath": None, "path": "a.txt"}]
    diff = await queries.commit_diff(repo, detail["sha"], "a.txt", None)
    assert [l["text"] for l in diff["hunks"][0]["lines"] if l["t"] == "+"] == ["four"]
    root = await queries.commit_detail(repo, data["commits"][1]["sha"])
    assert root["files"][0]["status"] == "A"


async def test_empty_repo(repo: Path):
    assert (await queries.log(str(repo), 10))["commits"] == []
    status = await queries.status(str(repo))
    assert status["branch"]["head"] == "main"


async def test_stage_lines_and_commit(repo_with_history: Path):
    repo = str(repo_with_history)
    write(repo_with_history, "a.txt", "ONE\ntwo\nthree\nfour\nfive\n")
    diff = await queries.working_diff(repo, "a.txt", staged=False, context=0)
    assert len(diff["hunks"]) == 2
    # Stage only the second hunk ("five").
    await ops.apply_selection(repo, "a.txt", "stage", {1: None}, context=0)
    staged = await queries.working_diff(repo, "a.txt", staged=True)
    assert [l["text"] for l in staged["hunks"][0]["lines"] if l["t"] != " "] == ["five"]

    # Stage just the "+ONE" line of the first hunk, leaving "-one" unstaged.
    diff = await queries.working_diff(repo, "a.txt", staged=False)
    hunk = diff["hunks"][0]
    plus = next(i for i, l in enumerate(hunk["lines"]) if l["t"] == "+")
    await ops.apply_selection(repo, "a.txt", "stage", {0: [plus]})
    assert git(repo_with_history, "show", ":a.txt") == "one\nONE\ntwo\nthree\nfour\nfive\n"

    # Unstage it again.
    staged = await queries.working_diff(repo, "a.txt", staged=True)
    plus = next(i for i, l in enumerate(staged["hunks"][0]["lines"]) if l["t"] == "+" and l["text"] == "ONE")
    await ops.apply_selection(repo, "a.txt", "unstage", {0: [plus]})
    assert git(repo_with_history, "show", ":a.txt") == "one\ntwo\nthree\nfour\nfive\n"

    result = await ops.commit(repo, "add five\n\nbody")
    assert result["ok"]
    assert await queries.head_message(repo) == "add five\n\nbody"
    status = await queries.status(repo)
    assert status["staged"] == [] and status["unstaged"][0]["path"] == "a.txt"


async def test_discard_lines_and_files(repo_with_history: Path):
    repo = str(repo_with_history)
    write(repo_with_history, "a.txt", "one\ntwo\nthree\nfour\nextra1\nextra2\n")
    write(repo_with_history, "new.txt", "new\n")
    diff = await queries.working_diff(repo, "a.txt", staged=False)
    first = next(i for i, l in enumerate(diff["hunks"][0]["lines"]) if l["text"] == "extra1")
    await ops.apply_selection(repo, "a.txt", "discard", {0: [first]})
    assert (repo_with_history / "a.txt").read_text() == "one\ntwo\nthree\nfour\nextra2\n"
    await ops.discard_files(repo, ["a.txt", "new.txt"])
    assert not (repo_with_history / "new.txt").exists()
    assert (await queries.status(repo))["unstaged"] == []


async def test_untracked_diff_and_stage(repo: Path):
    write(repo, "n.txt", "hello\n")
    diff = await queries.working_diff(str(repo), "n.txt", staged=False)
    assert diff["status"] == "?"
    with pytest.raises(ValueError):
        await ops.apply_selection(str(repo), "n.txt", "stage", {0: [0]})
    await ops.stage_files(str(repo), ["n.txt"])
    status = await queries.status(str(repo))
    assert status["staged"][0] == {"path": "n.txt", "status": "A", "origPath": None}
    await ops.unstage_files(str(repo), ["n.txt"])  # no HEAD yet
    assert (await queries.status(str(repo)))["staged"] == []


async def test_branches_and_refs(repo_with_history: Path):
    repo = str(repo_with_history)
    await ops.create_branch(repo, "feature/x", None, checkout_after=True)
    await ops.create_tag(repo, "v1", "HEAD", "release")
    refs = await queries.refs(repo)
    assert {b["name"]: b["current"] for b in refs["local"]} == {"feature/x": True, "main": False}
    assert refs["tags"][0]["name"] == "v1" and refs["tags"][0]["annotated"]
    await ops.checkout(repo, "main")
    await ops.rename_branch(repo, "feature/x", "feature/y")
    await ops.delete_branch(repo, "feature/y")
    await ops.delete_tag(repo, "v1")
    refs = await queries.refs(repo)
    assert [b["name"] for b in refs["local"]] == ["main"] and refs["tags"] == []


async def make_conflict(repo: Path) -> None:
    git(repo, "switch", "-q", "-c", "other")
    commit_file(repo, "a.txt", "one\ntwo\nthree\nTHEIRS\n", "theirs")
    git(repo, "switch", "-q", "main")
    commit_file(repo, "a.txt", "one\ntwo\nthree\nOURS\n", "ours")


async def test_merge_conflict_resolve_and_continue(repo_with_history: Path):
    repo = str(repo_with_history)
    await make_conflict(repo_with_history)
    result = await ops.merge(repo, "other")
    assert result == {**result, "ok": False, "conflicts": True}
    assert result["operation"]["kind"] == "merge"
    versions = await ops.conflict_versions(repo, "a.txt")
    assert versions["ours"].endswith("OURS\n") and versions["theirs"].endswith("THEIRS\n")
    assert any(c["type"] == "conflict" for c in versions["chunks"])
    await ops.resolve_conflict(repo, "a.txt", "one\ntwo\nthree\nOURS\nTHEIRS\n", None)
    assert (await queries.status(repo))["conflicts"] == []
    assert (await ops.continue_operation(repo))["ok"]
    assert await queries.operation_state(repo) is None
    log = await queries.log(repo, 10)
    assert len(log["commits"][0]["parents"]) == 2


async def test_abort_and_take_side(repo_with_history: Path):
    repo = str(repo_with_history)
    await make_conflict(repo_with_history)
    await ops.merge(repo, "other")
    await ops.abort_operation(repo)
    assert await queries.operation_state(repo) is None
    await ops.merge(repo, "other")
    await ops.resolve_conflict(repo, "a.txt", None, "theirs")
    assert (repo_with_history / "a.txt").read_text().endswith("THEIRS\n")


async def test_merge_error_raises(repo_with_history: Path):
    with pytest.raises(GitError):
        await ops.merge(str(repo_with_history), "does-not-exist")


async def test_interactive_rebase_squash_reword_reorder(repo_with_history: Path):
    repo = str(repo_with_history)
    base = git(repo_with_history, "rev-parse", "HEAD").strip()
    a = commit_file(repo_with_history, "b.txt", "b\n", "add b")
    b = commit_file(repo_with_history, "c.txt", "c\n", "add c")
    c = commit_file(repo_with_history, "d.txt", "d\n", "add d")
    candidates = await queries.rebase_candidates(repo, base)
    assert [x["sha"] for x in candidates] == [a, b, c]
    result = await ops.interactive_rebase(
        repo,
        base,
        [
            {"sha": c, "action": "reword", "message": "add d (reworded)"},
            {"sha": a, "action": "pick"},
            {"sha": b, "action": "fixup"},
        ],
    )
    assert result["ok"], result
    subjects = [x["subject"] for x in (await queries.log(repo, 10))["commits"]]
    assert subjects == ["add b", "add d (reworded)", "second", "first"]
    assert sorted(p.name for p in repo_with_history.iterdir() if p.suffix == ".txt") == [
        "a.txt", "b.txt", "c.txt", "d.txt"
    ]
    assert not (await queries.git_dir(repo) / "gitcraft-rebase-messages.json").exists()


async def test_rebase_with_conflict_then_continue(repo_with_history: Path):
    repo = str(repo_with_history)
    await make_conflict(repo_with_history)
    git(repo_with_history, "switch", "-q", "other")
    result = await ops.rebase(repo, "main")
    assert result["conflicts"] and result["operation"]["kind"] == "rebase"
    await ops.resolve_conflict(repo, "a.txt", "one\ntwo\nthree\nOURS\nTHEIRS\n", None)
    assert (await ops.continue_operation(repo))["ok"]
    assert [x["subject"] for x in (await queries.log(repo, 3, all_refs=False))["commits"]] == ["theirs", "ours", "second"]


async def test_stash_cycle(repo_with_history: Path):
    repo = str(repo_with_history)
    write(repo_with_history, "a.txt", "changed\n")
    write(repo_with_history, "u.txt", "untracked\n")
    await ops.stash_push(repo, "wip", include_untracked=True, staged_only=False, keep_index=False)
    refs = await queries.refs(repo)
    assert refs["stashes"][0]["message"].endswith("wip")
    files = await queries.stash_files(repo, "stash@{0}")
    assert {f["path"] for f in files} == {"a.txt", "u.txt"}
    assert (await queries.stash_diff(repo, "stash@{0}", "u.txt"))["hunks"]
    assert (await queries.status(repo))["unstaged"] == []
    await ops.stash_apply(repo, "stash@{0}", pop=True)
    assert (repo_with_history / "u.txt").exists()
    assert (await queries.refs(repo))["stashes"] == []


async def test_cherry_pick_revert_reset(repo_with_history: Path):
    repo = str(repo_with_history)
    git(repo_with_history, "switch", "-q", "-c", "side")
    picked = commit_file(repo_with_history, "s.txt", "s\n", "side commit")
    git(repo_with_history, "switch", "-q", "main")
    assert (await ops.cherry_pick(repo, [picked]))["ok"]
    assert (repo_with_history / "s.txt").exists()
    head = git(repo_with_history, "rev-parse", "HEAD").strip()
    assert (await ops.revert(repo, head))["ok"]
    assert not (repo_with_history / "s.txt").exists()
    await ops.reset(repo, "HEAD~2", "hard")
    assert (await queries.log(repo, 10, all_refs=False))["commits"][0]["subject"] == "second"


async def test_history_search_blame(repo_with_history: Path):
    repo = str(repo_with_history)
    git(repo_with_history, "mv", "a.txt", "renamed.txt")
    git(repo_with_history, "commit", "-q", "-m", "rename")
    history = await queries.file_history(repo, "renamed.txt")
    assert [h["subject"] for h in history] == ["rename", "second", "first"]
    assert history[0]["file"]["status"] == "R" and history[-1]["file"]["path"] == "a.txt"
    assert [c["subject"] for c in await queries.search(repo, "SEC", "message")] == ["second"]
    assert [c["subject"] for c in await queries.search(repo, "four", "content")] == ["second"]
    assert len(await queries.search(repo, "test user", "author")) == 3
    sha = git(repo_with_history, "rev-parse", "HEAD~1").strip()
    assert (await queries.search(repo, sha[:8], "sha"))[0]["sha"] == sha
    blame = await queries.blame(repo, "renamed.txt")
    assert [b["summary"] for b in blame] == ["first", "first", "first", "second"]
    assert blame[3]["text"] == "four"


async def test_remote_fetch_pull_push(tmp_path: Path, repo_with_history: Path):
    remote = tmp_path / "remote.git"
    git(tmp_path, "init", "-q", "--bare", "-b", "main", str(remote))
    repo = str(repo_with_history)
    await ops.add_remote(repo, "origin", str(remote))
    lines: list[str] = []

    async def collect(line: str) -> None:
        lines.append(line)

    assert (await ops.push(repo, "origin", "main", True, False, False, collect))["ok"]
    refs = await queries.refs(repo)
    assert refs["remote"][0]["name"] == "origin/main"
    assert refs["local"][0]["upstream"] == "origin/main"

    clone_dir = await ops.clone(str(tmp_path), str(remote), "clone", collect)
    git(Path(clone_dir), "config", "user.name", "Other")
    git(Path(clone_dir), "config", "user.email", "o@example.com")
    commit_file(Path(clone_dir), "x.txt", "x\n", "from clone")
    git(Path(clone_dir), "push", "-q")

    await ops.fetch(repo, None, True, False, collect)
    assert (await queries.refs(repo))["local"][0]["behind"] == 1
    assert (await ops.pull(repo, None, None, "ff-only", collect))["ok"]
    assert (repo_with_history / "x.txt").exists()
