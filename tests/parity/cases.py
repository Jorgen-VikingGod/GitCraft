"""What the parity harness sends: read sweeps and one case per mutating behaviour.

A case is (name, scenario, steps). A step is a dict with method, path and
optional params/body, or a function taking a `Ctx` and returning that dict, for
requests that depend on repository contents (object ids, hunk line numbers).
The `Ctx` is only used during capture: goldens store the resolved request, so a
replay never derives anything from the server under test.
"""

from __future__ import annotations

from typing import Any, Callable


def get(path: str, status_only: bool = False, **params: Any) -> dict[str, Any]:
    return {"method": "GET", "path": path, "params": params, "status_only": status_only}


def post(path: str, body: dict[str, Any] | None = None, status_only: bool = False, **params: Any) -> dict[str, Any]:
    return {"method": "POST", "path": path, "params": params, "body": body or {}, "status_only": status_only}


Step = dict[str, Any] | Callable[[Any], dict[str, Any]]

STATIC_READS = [
    get("/status"),
    get("/refs"),
    get("/log"),
    get("/log", all="false"),
    get("/log", limit=2),
    get("/log", limit=1, ref="HEAD"),
    get("/commit/head-message"),
    get("/commit/template"),
    get("/rebase/commits"),
    get("/reflog"),
    get("/reflog", limit=2),
    get("/search", q="e", mode="message"),
    get("/search", q="Parity", mode="author"),
    get("/search", q="line", mode="content"),
    get("/search", q="nothing-matches-this", mode="message"),
]


def _cases() -> list[tuple[str, str, list[Step]]]:
    def later(path: str, make: Callable[[Any], dict[str, Any]]) -> Callable[[Any], dict[str, Any]]:
        return lambda ctx: post(path, make(ctx))

    cases: list[tuple[str, str, list[Step]]] = [
        # ---------------------------------------------------------------- index
        ("stage-one", "dirty", [post("/stage", {"paths": ["multi.txt"]})]),
        ("stage-deleted", "dirty", [post("/stage", {"paths": ["gone.txt"]})]),
        ("stage-untracked-dir", "dirty", [post("/stage", {"paths": ["nested"]})]),
        ("stage-all", "dirty", [post("/stage/all")]),
        ("unstage-one", "dirty", [post("/unstage", {"paths": ["both.txt"]})]),
        ("unstage-all", "dirty", [post("/unstage/all")]),
        ("unstage-unborn", "unborn_dirty", [post("/unstage", {"paths": ["staged.txt"]})]),
        ("unstage-all-unborn", "unborn_dirty", [post("/unstage/all")]),
        ("discard-tracked", "dirty", [post("/discard", {"paths": ["multi.txt", "gone.txt"]})]),
        ("discard-untracked", "dirty", [post("/discard", {"paths": ["untracked.txt", "nested/deep/untracked.txt"]})]),
        ("discard-escape", "dirty", [post("/discard", {"paths": ["../outside.txt"]}, status_only=True)]),
        ("stage-odd-unicode", "encodings", [post("/stage", {"paths": ["unicode-ÄÖÜ-日本語.txt"]})]),
        # ------------------------------------------------------- partial staging
        ("selection-stage-hunk", "dirty", [post("/selection", {"path": "multi.txt", "mode": "stage", "selection": {"1": None}})]),
        (
            "selection-stage-line",
            "dirty",
            [lambda ctx: post("/selection", {"path": "multi.txt", "mode": "stage", "selection": {"0": [ctx.hunk_line("multi.txt", 0, "+")]}})],
        ),
        (
            "selection-stage-removal",
            "dirty",
            [lambda ctx: post("/selection", {"path": "multi.txt", "mode": "stage", "selection": {"0": [ctx.hunk_line("multi.txt", 0, "-")]}})],
        ),
        (
            "selection-unstage-line",
            "dirty",
            [lambda ctx: post("/selection", {"path": "both.txt", "mode": "unstage", "selection": {"0": [ctx.hunk_line("both.txt", 0, "+", True)]}})],
        ),
        (
            "selection-discard-line",
            "dirty",
            [lambda ctx: post("/selection", {"path": "multi.txt", "mode": "discard", "selection": {"0": [ctx.hunk_line("multi.txt", 0, "+")]}})],
        ),
        ("selection-discard-hunk", "dirty", [post("/selection", {"path": "multi.txt", "mode": "discard", "selection": {"0": None}})]),
        ("selection-context-0", "dirty", [post("/selection", {"path": "multi.txt", "mode": "stage", "selection": {"0": None}, "context": 0})]),
        ("selection-untracked", "dirty", [post("/selection", {"path": "untracked.txt", "mode": "stage", "selection": {"0": [0]}})]),
        ("selection-out-of-range", "dirty", [post("/selection", {"path": "multi.txt", "mode": "stage", "selection": {"99": None}})]),
        ("selection-partly-staged", "dirty", [post("/selection", {"path": "both.txt", "mode": "stage", "selection": {"0": None}})]),
        ("selection-no-newline", "dirty", [post("/selection", {"path": "tail.txt", "mode": "stage", "selection": {"0": None}})]),
        ("selection-crlf", "encodings", [post("/selection", {"path": "crlf.txt", "mode": "stage", "selection": {"0": None}})]),
        # Non-UTF-8 partial staging is a listed deliberate fix: this golden records today's result.
        ("selection-latin1", "encodings", [post("/selection", {"path": "latin1.txt", "mode": "stage", "selection": {"0": None}})]),
        # ---------------------------------------------------------------- commit
        ("commit-plain", "dirty", [post("/commit", {"message": "a commit\n\nbody"})]),
        ("commit-amend", "dirty", [post("/commit", {"message": "amended", "amend": True})]),
        ("commit-signoff", "dirty", [post("/commit", {"message": "signed", "signOff": True})]),
        ("commit-unicode", "dirty", [post("/commit", {"message": "Grüße 日本語"})]),
        ("commit-comment-kept", "dirty", [post("/commit", {"message": "# not a comment?\nreal line"})]),
        ("commit-nothing", "linear", [post("/commit", {"message": "nothing staged"})]),
        ("commit-empty-message", "dirty", [post("/commit", {"message": ""})]),
        ("commit-first", "unborn_dirty", [post("/commit", {"message": "root commit"})]),
        # --------------------------------------------------------------- branches
        ("checkout-local", "linear", [post("/checkout", {"ref": "feature/one"})]),
        ("checkout-commit", "linear", [later("/checkout", lambda c: {"ref": c.rev("HEAD~2"), "kind": "commit"})]),
        ("checkout-remote", "remotes", [post("/checkout", {"ref": "origin/doomed", "kind": "remote"})]),
        ("checkout-missing", "linear", [post("/checkout", {"ref": "no-such-branch"})]),
        ("checkout-dirty-refused", "dirty", [post("/branch", {"name": "older", "start": "HEAD~3", "checkout": False}), post("/checkout", {"ref": "older"})]),
        ("branch-create", "linear", [post("/branch", {"name": "created"})]),
        ("branch-create-no-checkout", "linear", [post("/branch", {"name": "created", "start": "HEAD~1", "checkout": False})]),
        ("branch-create-invalid", "linear", [post("/branch", {"name": "bad..name"})]),
        ("branch-rename", "linear", [post("/branch/rename", {"old": "feature/one", "new": "feature/renamed"})]),
        ("branch-delete-merged", "linear", [post("/branch/delete", {"name": "feature/one"})]),
        ("branch-delete-unmerged", "merges", [post("/branch/delete", {"name": "island"})]),
        ("branch-delete-force", "merges", [post("/branch/delete", {"name": "island", "force": True})]),
        ("branch-delete-remote-refused", "remotes", [post("/branch/delete-remote", {"remote": "origin", "branch": "main"})]),
        (
            "branch-delete-remote",
            "remotes",
            [
                post("/branch", {"name": "short-lived"}),
                post("/push", {"remote": "origin", "branch": "short-lived"}),
                post("/branch/delete-remote", {"remote": "origin", "branch": "short-lived"}),
            ],
        ),
        ("branch-upstream-set", "remotes", [post("/branch/upstream", {"branch": "doomed", "upstream": "origin/main"})]),
        ("branch-upstream-unset", "remotes", [post("/branch/upstream", {"branch": "main"})]),
        ("branch-fast-forward", "linear", [post("/branch/fast-forward", {"branch": "feature/one", "target": "main"})]),
        ("branch-fast-forward-upstream", "behind", [post("/branch/fast-forward", {"branch": "main"})]),
        ("branch-fast-forward-no-upstream", "linear", [post("/branch/fast-forward", {"branch": "feature/one"})]),
        ("branch-fast-forward-current", "linear", [post("/branch/fast-forward", {"branch": "main", "target": "feature/one"})]),
        # ------------------------------------------------------------------- tags
        ("tag-light", "linear", [post("/tag", {"name": "t1"})]),
        ("tag-annotated", "linear", [post("/tag", {"name": "t2", "target": "HEAD~1", "message": "tag message"})]),
        ("tag-duplicate", "linear", [post("/tag", {"name": "v1-light"})]),
        ("tag-delete", "linear", [post("/tag/delete", {"name": "v1-light"})]),
        ("tag-push", "remotes", [post("/tag", {"name": "fresh"}), post("/tag/push", {"remote": "origin", "name": "fresh"})]),
        ("tag-push-delete", "remotes", [post("/tag/push", {"remote": "origin", "name": "shared-tag", "delete": True})]),
        # ---------------------------------------------------------------- remotes
        ("remote-add", "linear", [post("/remote", {"name": "up", "url": "https://example.invalid/r.git"})]),
        ("remote-remove", "remotes", [post("/remote/remove", {"name": "tokened"})]),
        ("remote-edit-url", "remotes", [post("/remote/edit", {"name": "tokened", "url": "https://example.invalid/y.git"})]),
        ("remote-edit-name", "remotes", [post("/remote/edit", {"name": "tokened", "newName": "renamed"})]),
        ("remote-prune", "remotes", [post("/remote/prune", {"name": "origin"})]),
        ("fetch-one", "remote_conflict", [post("/fetch", {"remote": "origin"})]),
        ("fetch-all", "behind", [post("/fetch", {})]),
        ("fetch-tags-noprune", "remotes", [post("/fetch", {"remote": "origin", "prune": False, "tags": True})]),
        ("fetch-prunes", "remotes", [post("/fetch", {"remote": "origin"})]),
        ("pull-merge", "remotes", [post("/pull", {"remote": "origin", "branch": "main", "mode": "merge"})]),
        ("pull-rebase", "remotes", [post("/pull", {"remote": "origin", "branch": "main", "mode": "rebase"})]),
        ("pull-ff-refused", "remotes", [post("/pull", {"remote": "origin", "branch": "main", "mode": "ff-only"})]),
        ("pull-ff", "behind", [post("/checkout", {"ref": "main"}), post("/pull", {"remote": "origin", "branch": "main", "mode": "ff-only"})]),
        ("pull-autostash", "behind", [post("/checkout", {"ref": "main"}), post("/remote", {"name": "second", "url": "../origin.git"}), post("/pull", {"mode": "rebase", "autostash": True})]),
        ("pull-merge-conflict", "remote_conflict", [post("/pull", {"remote": "origin", "branch": "main", "mode": "merge"})]),
        ("pull-rebase-conflict", "remote_conflict", [post("/pull", {"remote": "origin", "branch": "main", "mode": "rebase"})]),
        ("push-rejected", "remotes", [post("/push", {"remote": "origin", "branch": "main"})]),
        ("push-force-stale", "remotes", [post("/push", {"remote": "origin", "branch": "main", "force": True})]),
        ("push-force", "remotes", [post("/fetch", {"remote": "origin"}), post("/push", {"remote": "origin", "branch": "main", "force": True})]),
        ("push-set-upstream", "remotes", [post("/branch", {"name": "published"}), post("/push", {"remote": "origin", "branch": "published", "setUpstream": True})]),
        ("push-no-such-remote", "linear", [post("/push", {"remote": "dest", "branch": "main"})]),
        ("push-tags", "remotes", [post("/tag", {"name": "pushed"}), post("/fetch", {"remote": "origin"}), post("/pull", {"remote": "origin", "branch": "main", "mode": "rebase"}), post("/push", {"remote": "origin", "branch": "main", "tags": True})]),
        # ------------------------------------------------------------------ stash
        ("stash-default", "dirty", [post("/stash", {"message": "saved"})]),
        ("stash-tracked-only", "dirty", [post("/stash", {"includeUntracked": False})]),
        ("stash-staged-only-with-untracked", "dirty", [post("/stash", {"stagedOnly": True})]),
        ("stash-staged-only", "dirty", [post("/stash", {"stagedOnly": True, "includeUntracked": False})]),
        ("stash-keep-index", "dirty", [post("/stash", {"keepIndex": True})]),
        ("stash-paths", "dirty", [post("/stash", {"paths": ["multi.txt"]})]),
        ("stash-nothing", "linear", [post("/stash", {})]),
        ("stash-apply", "stashes", [post("/discard", {"paths": ["s.txt"]}), post("/stash/apply", {"name": "stash@{1}"})]),
        ("stash-pop", "stashes", [post("/discard", {"paths": ["s.txt"]}), post("/stash/apply", {"name": "stash@{0}", "pop": True})]),
        ("stash-drop", "stashes", [post("/stash/drop", {"name": "stash@{0}"})]),
        ("stash-apply-conflict", "stash_conflict", [post("/stash/apply", {"name": "stash@{0}"})]),
        ("stash-missing", "stashes", [post("/stash/drop", {"name": "stash@{9}"})]),
        # ------------------------------------------------------------------ merge
        ("merge-ff", "linear", [post("/checkout", {"ref": "feature/one"}), post("/merge", {"ref": "main"})]),
        ("merge-unrelated", "merges", [post("/merge", {"ref": "island"})]),
        ("merge-default", "merges", [post("/checkout", {"ref": "left"}), post("/merge", {"ref": "right"})]),
        ("merge-no-ff", "linear", [post("/checkout", {"ref": "feature/one"}), post("/merge", {"ref": "main", "mode": "no-ff"})]),
        ("merge-ff-only-refused", "conflict_ready", [post("/merge", {"ref": "topic", "mode": "ff-only"})]),
        ("merge-squash", "linear", [post("/checkout", {"ref": "feature/one"}), post("/merge", {"ref": "main", "mode": "squash"})]),
        ("merge-conflict", "conflict_ready", [post("/merge", {"ref": "topic"})]),
        ("merge-missing-ref", "linear", [post("/merge", {"ref": "nope"})]),
        ("merge-abort", "merge_conflict", [post("/operation/abort")]),
        ("merge-continue-unresolved", "merge_conflict", [post("/operation/continue")]),
        (
            "merge-resolve-continue",
            "merge_conflict",
            [
                post("/conflict/resolve", {"path": "c.txt", "side": "ours"}),
                post("/conflict/resolve", {"path": "add.txt", "content": "manually merged\n"}),
                post("/conflict/resolve", {"path": "del.txt", "side": "theirs"}),
                post("/operation/continue"),
            ],
        ),
        ("resolve-ours", "merge_conflict", [post("/conflict/resolve", {"path": "c.txt", "side": "ours"})]),
        ("resolve-theirs", "merge_conflict", [post("/conflict/resolve", {"path": "c.txt", "side": "theirs"})]),
        ("resolve-content", "merge_conflict", [post("/conflict/resolve", {"path": "c.txt", "content": "merged by hand\n"})]),
        ("resolve-delete-side", "merge_conflict", [post("/conflict/resolve", {"path": "del.txt", "side": "ours"})]),
        ("resolve-nothing-given", "merge_conflict", [post("/conflict/resolve", {"path": "c.txt"})]),
        ("resolve-escape", "merge_conflict", [post("/conflict/resolve", {"path": "../evil.txt", "content": "x"}, status_only=True)]),
        # ----------------------------------------------------------------- rebase
        ("rebase-simple", "linear", [post("/checkout", {"ref": "feature/one"}), post("/rebase", {"onto": "main"})]),
        ("rebase-conflict", "conflict_ready", [post("/rebase", {"onto": "topic"})]),
        ("rebase-dirty-refused", "dirty", [post("/branch", {"name": "x", "start": "HEAD~1", "checkout": False}), post("/rebase", {"onto": "x"})]),
        ("rebase-autostash", "dirty", [post("/branch", {"name": "x", "start": "HEAD~1", "checkout": False}), post("/rebase", {"onto": "x", "autostash": True})]),
        ("rebase-abort", "rebase_conflict", [post("/operation/abort")]),
        ("rebase-skip", "rebase_conflict", [post("/operation/skip")]),
        (
            "rebase-resolve-continue",
            "rebase_conflict",
            [
                post("/conflict/resolve", {"path": "c.txt", "side": "theirs"}),
                post("/conflict/resolve", {"path": "add.txt", "side": "theirs"}),
                post("/conflict/resolve", {"path": "del.txt", "side": "theirs"}),
                post("/operation/continue"),
            ],
        ),
        ("operation-none-abort", "linear", [post("/operation/abort")]),
        ("operation-none-continue", "linear", [post("/operation/continue")]),
        ("operation-none-skip", "linear", [post("/operation/skip")]),
        # ----------------------------------------------------- interactive rebase
        (
            "irebase-reorder",
            "rebase_ready",
            [lambda c: post("/rebase/interactive", {"base": c.rev("HEAD~3"), "steps": [{"sha": c.rev("HEAD"), "action": "pick"}, {"sha": c.rev("HEAD~2"), "action": "pick"}, {"sha": c.rev("HEAD~1"), "action": "pick"}]})],
        ),
        (
            "irebase-reword",
            "rebase_ready",
            [lambda c: post("/rebase/interactive", {"base": c.rev("HEAD~2"), "steps": [{"sha": c.rev("HEAD~1"), "action": "reword", "message": "reworded\n\nwith body"}, {"sha": c.rev("HEAD"), "action": "pick"}]})],
        ),
        (
            "irebase-squash",
            "rebase_ready",
            [lambda c: post("/rebase/interactive", {"base": c.rev("HEAD~3"), "steps": [{"sha": c.rev("HEAD~2"), "action": "pick"}, {"sha": c.rev("HEAD~1"), "action": "squash", "message": "squashed together"}, {"sha": c.rev("HEAD"), "action": "pick"}]})],
        ),
        (
            "irebase-fixup-drop",
            "rebase_ready",
            [lambda c: post("/rebase/interactive", {"base": c.rev("HEAD~3"), "steps": [{"sha": c.rev("HEAD~2"), "action": "pick"}, {"sha": c.rev("HEAD~1"), "action": "fixup"}, {"sha": c.rev("HEAD"), "action": "drop"}]})],
        ),
        (
            "irebase-edit-stop-continue",
            "rebase_ready",
            [lambda c: post("/rebase/interactive", {"base": c.rev("HEAD~2"), "steps": [{"sha": c.rev("HEAD~1"), "action": "edit"}, {"sha": c.rev("HEAD"), "action": "pick"}]}), post("/operation/continue")],
        ),
        (
            "irebase-root",
            "rebase_ready",
            [lambda c: post("/rebase/interactive", {"base": None, "steps": [{"sha": c.rev(f"HEAD~{i}"), "action": "pick"} for i in range(5, -1, -1)]})],
        ),
        ("irebase-first-squash", "rebase_ready", [lambda c: post("/rebase/interactive", {"base": c.rev("HEAD~1"), "steps": [{"sha": c.rev("HEAD"), "action": "squash"}]})]),
        ("irebase-bad-action", "rebase_ready", [lambda c: post("/rebase/interactive", {"base": c.rev("HEAD~1"), "steps": [{"sha": c.rev("HEAD"), "action": "explode"}]})]),
        (
            "irebase-conflict",
            "conflict_ready",
            [lambda c: post("/rebase/interactive", {"base": c.rev("HEAD~2"), "steps": [{"sha": c.rev("topic"), "action": "pick"}, {"sha": c.rev("HEAD"), "action": "pick"}]})],
        ),
        # ------------------------------------------------- cherry-pick, revert, reset
        ("cherry-pick-one", "conflict_ready", [lambda c: post("/cherry-pick", {"shas": [c.rev("topic")]})]),
        ("cherry-pick-many", "merges", [post("/checkout", {"ref": "island"}), lambda c: post("/cherry-pick", {"shas": [c.rev("left"), c.rev("right")]})]),
        ("cherry-pick-abort", "cherry_pick_conflict", [post("/operation/abort")]),
        ("revert-plain", "revert_ready", [lambda c: post("/revert", {"sha": c.rev("HEAD")})]),
        ("revert-conflict", "revert_ready", [lambda c: post("/revert", {"sha": c.rev("HEAD~1")})]),
        ("revert-merge-mainline", "merges", [lambda c: post("/revert", {"sha": c.rev("HEAD"), "mainline": 1})]),
        ("revert-merge-no-mainline", "merges", [lambda c: post("/revert", {"sha": c.rev("HEAD")})]),
        ("reset-soft", "linear", [post("/reset", {"target": "HEAD~1", "mode": "soft"})]),
        ("reset-mixed", "linear", [post("/reset", {"target": "HEAD~1"})]),
        ("reset-hard", "dirty", [post("/reset", {"target": "HEAD~1", "mode": "hard"})]),
        ("reset-keep", "linear", [post("/reset", {"target": "HEAD~1", "mode": "keep"})]),
        ("reset-keep-refused", "dirty", [post("/reset", {"target": "HEAD~1", "mode": "keep"})]),
        ("reset-bad-mode", "linear", [post("/reset", {"target": "HEAD~1", "mode": "weird"})]),
        # ------------------------------------------------------------------ tools
        ("checkout-file", "linear", [lambda c: post("/checkout-file", {"sha": c.rev("HEAD~2"), "path": "a.txt"})]),
        ("checkout-file-escape", "linear", [post("/checkout-file", {"sha": "HEAD", "path": "../x"}, status_only=True)]),
        ("ignore-new", "dirty", [post("/ignore", {"pattern": "untracked.txt"})]),
        ("ignore-tracked", "dirty", [post("/ignore", {"pattern": "multi.txt"})]),
        ("clean-dry", "dirty", [post("/clean", {})]),
        ("clean-real", "dirty", [post("/clean", {"dryRun": False})]),
        ("clean-spaced-name", "encodings", [post("/clean", {"dryRun": True})]),
        ("clean-spaced-name-real", "encodings", [post("/clean", {"dryRun": False})]),
        ("clean-no-directories", "dirty", [post("/clean", {"directories": False, "dryRun": False})]),
        ("clean-ignored", "dirty", [post("/ignore", {"pattern": "untracked.txt"}), post("/clean", {"ignored": True, "dryRun": False})]),
        ("patch-apply", "linear", [post("/patch/apply", {"patch": "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-one\n+ONE\n two\n", "mode": "apply"})]),
        ("patch-apply-bad", "linear", [post("/patch/apply", {"patch": "not a patch", "mode": "apply"})]),
        ("patch-am-already-applied", "linear", [lambda c: post("/patch/apply", {"patch": c.format_patch("HEAD~3"), "mode": "am"})]),
        ("patch-am", "merges", [lambda c: post("/patch/apply", {"patch": c.format_patch("island"), "mode": "am"})]),
        ("patch-am-bad", "linear", [post("/patch/apply", {"patch": "not a patch"})]),
        ("submodules-update", "submodule_uninit", [post("/submodules/update", {})]),
        ("submodules-sync-modified", "submodules", [post("/submodules/update", {})]),
        # ----------------------------------------------------------- odd locations
        ("worktree-stage-commit", "worktree", [post("/stage/all"), post("/commit", {"message": "from worktree"})]),
        ("dash-ref-merge", "linear", [post("/merge", {"ref": "--abort"}, status_only=True)]),
        ("dash-ref-checkout", "linear", [post("/checkout", {"ref": "--detach"}, status_only=True)]),
    ]
    return cases


CASES = _cases()
