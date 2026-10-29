"""Parsers for machine-readable git output."""

from __future__ import annotations

import re
from typing import Any

RS = "\x1e"  # record separator
US = "\x1f"  # unit separator

LOG_FIELDS = ["%H", "%P", "%an", "%ae", "%at", "%D", "%s"]
LOG_FORMAT = "--format=" + "%x1e" + "%x1f".join(LOG_FIELDS)

DETAIL_FIELDS = ["%H", "%P", "%an", "%ae", "%at", "%cn", "%ce", "%ct", "%D", "%B"]
DETAIL_FORMAT = "--format=" + "%x1f".join(DETAIL_FIELDS)


# ---------------------------------------------------------------- log / refs


def parse_decorations(text: str) -> list[dict[str, Any]]:
    """Parse `%D` produced with --decorate=full."""
    refs: list[dict[str, Any]] = []
    for item in filter(None, (part.strip() for part in text.split(","))):
        current = False
        if item.startswith("HEAD -> "):
            item = item[len("HEAD -> ") :]
            current = True
        if item == "HEAD":
            refs.append({"name": "HEAD", "kind": "head", "current": True})
        elif item.startswith("tag: refs/tags/"):
            refs.append({"name": item[len("tag: refs/tags/") :], "kind": "tag", "current": False})
        elif item.startswith("refs/heads/"):
            refs.append({"name": item[len("refs/heads/") :], "kind": "local", "current": current})
        elif item.startswith("refs/remotes/"):
            name = item[len("refs/remotes/") :]
            if not name.endswith("/HEAD"):
                refs.append({"name": name, "kind": "remote", "current": False})
    return refs


def parse_log(out: str) -> list[dict[str, Any]]:
    commits = []
    for record in out.split(RS)[1:]:
        fields = record.rstrip("\n").split(US)
        if len(fields) < len(LOG_FIELDS):
            continue
        sha, parents, author, email, time, decorations, subject = fields[: len(LOG_FIELDS)]
        commits.append(
            {
                "sha": sha,
                "parents": parents.split() if parents else [],
                "author": author,
                "email": email,
                "time": int(time or 0),
                "refs": parse_decorations(decorations),
                "subject": subject,
            }
        )
    return commits


def parse_commit_detail(out: str) -> dict[str, Any]:
    fields = out.split(US)
    sha, parents, an, ae, at, cn, ce, ct, decorations = fields[:9]
    message = US.join(fields[9:]).rstrip("\n")
    return {
        "sha": sha,
        "parents": parents.split() if parents else [],
        "author": an,
        "email": ae,
        "time": int(at or 0),
        "committer": cn,
        "committerEmail": ce,
        "commitTime": int(ct or 0),
        "refs": parse_decorations(decorations),
        "message": message,
        "subject": message.split("\n", 1)[0],
    }


REF_FORMAT = "--format=" + "%1f".join(
    [
        "%(refname)",
        "%(objectname)",
        "%(upstream:short)",
        "%(upstream:track,nobracket)",
        "%(HEAD)",
        "%(*objectname)",
        "%(contents:subject)",
        "%(creatordate:unix)",
    ]
)


def _track(text: str) -> tuple[int, int, bool]:
    ahead = behind = 0
    if m := re.search(r"ahead (\d+)", text):
        ahead = int(m.group(1))
    if m := re.search(r"behind (\d+)", text):
        behind = int(m.group(1))
    return ahead, behind, text.strip() == "gone"


def parse_refs(out: str, remote_names: list[str]) -> dict[str, Any]:
    local, remote, tags = [], [], []
    by_length = sorted(remote_names, key=len, reverse=True)
    for line in out.splitlines():
        fields = line.split(US)
        if len(fields) < 8:
            continue
        refname, sha, upstream, track, head, peeled, subject, date = fields[:8]
        if refname.startswith("refs/heads/"):
            ahead, behind, gone = _track(track)
            local.append(
                {
                    "name": refname[len("refs/heads/") :],
                    "sha": sha,
                    "upstream": upstream or None,
                    "ahead": ahead,
                    "behind": behind,
                    "upstreamGone": gone,
                    "current": head == "*",
                }
            )
        elif refname.startswith("refs/remotes/"):
            name = refname[len("refs/remotes/") :]
            if name.endswith("/HEAD"):
                continue
            remote_name = next((r for r in by_length if name.startswith(r + "/")), name.split("/", 1)[0])
            remote.append({"name": name, "remote": remote_name, "branch": name[len(remote_name) + 1 :], "sha": sha})
        elif refname.startswith("refs/tags/"):
            tags.append(
                {
                    "name": refname[len("refs/tags/") :],
                    "sha": peeled or sha,
                    "annotated": bool(peeled),
                    "message": subject if peeled else "",
                    "time": int(date or 0),
                }
            )
    return {"local": local, "remote": remote, "tags": tags}


def parse_remotes(out: str) -> list[dict[str, str]]:
    remotes: dict[str, dict[str, str]] = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 3:
            entry = remotes.setdefault(parts[0], {"name": parts[0], "fetchUrl": "", "pushUrl": ""})
            entry["fetchUrl" if parts[2] == "(fetch)" else "pushUrl"] = parts[1]
    return list(remotes.values())


def parse_stashes(out: str) -> list[dict[str, Any]]:
    stashes = []
    for line in out.splitlines():
        fields = line.split(US)
        if len(fields) >= 4:
            name, sha, time, message = fields[:4]
            stashes.append({"name": name, "sha": sha, "time": int(time or 0), "message": message})
    return stashes


def parse_submodules(out: str) -> list[dict[str, str]]:
    modules = []
    for line in out.splitlines():
        if len(line) < 42:
            continue
        state = {" ": "ok", "-": "uninitialized", "+": "modified", "U": "conflict"}.get(line[0], "ok")
        parts = line[1:].split()
        modules.append({"sha": parts[0], "path": parts[1], "state": state})
    return modules


# -------------------------------------------------------------------- status


def parse_status(out: str) -> dict[str, Any]:
    branch: dict[str, Any] = {"head": None, "oid": None, "upstream": None, "ahead": 0, "behind": 0, "detached": False}
    staged: list[dict[str, Any]] = []
    unstaged: list[dict[str, Any]] = []
    conflicts: list[dict[str, Any]] = []
    entries = out.split("\0")
    i = 0
    while i < len(entries):
        entry = entries[i]
        i += 1
        if not entry:
            continue
        kind = entry[0]
        if entry.startswith("# "):
            key, _, value = entry[2:].partition(" ")
            if key == "branch.oid":
                branch["oid"] = None if value == "(initial)" else value
            elif key == "branch.head":
                branch["detached"] = value == "(detached)"
                branch["head"] = None if branch["detached"] else value
            elif key == "branch.upstream":
                branch["upstream"] = value
            elif key == "branch.ab":
                ahead, behind = value.split()
                branch["ahead"], branch["behind"] = int(ahead), -int(behind)
        elif kind == "1":
            parts = entry.split(" ", 8)
            _add_change(staged, unstaged, parts[1], parts[8], None)
        elif kind == "2":
            parts = entry.split(" ", 9)
            orig = entries[i] if i < len(entries) else None
            i += 1
            _add_change(staged, unstaged, parts[1], parts[9], orig)
        elif kind == "u":
            parts = entry.split(" ", 10)
            conflicts.append({"path": parts[10], "status": parts[1]})
        elif kind == "?":
            unstaged.append({"path": entry[2:], "status": "?", "origPath": None})
    return {"branch": branch, "staged": staged, "unstaged": unstaged, "conflicts": conflicts}


def _add_change(staged: list, unstaged: list, xy: str, path: str, orig: str | None) -> None:
    x, y = xy[0], xy[1]
    if x != ".":
        staged.append({"path": path, "status": x, "origPath": orig})
    if y != ".":
        unstaged.append({"path": path, "status": y, "origPath": None})


def parse_name_status(out: str) -> list[dict[str, Any]]:
    """Parse `--name-status -z` output."""
    files = []
    parts = out.split("\0")
    i = 0
    while i < len(parts):
        status = parts[i]
        i += 1
        if not status:
            continue
        if status[0] in "RC":
            files.append({"status": status[0], "origPath": parts[i], "path": parts[i + 1]})
            i += 2
        else:
            files.append({"status": status[0], "origPath": None, "path": parts[i]})
            i += 1
    return files


# ---------------------------------------------------------------------- diff

HUNK_RE = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@ ?(.*)$")
MAX_DIFF_LINES = 20000


def _strip_prefix(path: str) -> str | None:
    if path == "/dev/null":
        return None
    path = path.split("\t", 1)[0]
    return path[2:] if path[:2] in ("a/", "b/") else path


def parse_diff(text: str) -> list[dict[str, Any]]:
    files: list[dict[str, Any]] = []
    current: dict[str, Any] | None = None
    hunk: dict[str, Any] | None = None
    old_no = new_no = 0
    total = 0
    for line in text.split("\n"):
        if line.startswith("diff --git "):
            current = {
                "header": [line],
                "oldPath": None,
                "newPath": None,
                "status": "M",
                "binary": False,
                "truncated": False,
                "hunks": [],
            }
            files.append(current)
            hunk = None
            continue
        if current is None:
            continue
        if line.startswith("@@"):
            m = HUNK_RE.match(line)
            if not m:
                continue
            old_no, new_no = int(m.group(1)), int(m.group(3))
            hunk = {
                "header": line,
                "oldStart": old_no,
                "oldLines": int(m.group(2) or 1),
                "newStart": new_no,
                "newLines": int(m.group(4) or 1),
                "context": m.group(5),
                "lines": [],
            }
            current["hunks"].append(hunk)
            continue
        if hunk is None:
            current["header"].append(line)
            if line.startswith("--- "):
                current["oldPath"] = _strip_prefix(line[4:])
            elif line.startswith("+++ "):
                current["newPath"] = _strip_prefix(line[4:])
            elif line.startswith("new file mode"):
                current["status"] = "A"
            elif line.startswith("deleted file mode"):
                current["status"] = "D"
            elif line.startswith("rename from "):
                current["status"] = "R"
                current["oldPath"] = line[len("rename from ") :]
            elif line.startswith("rename to "):
                current["newPath"] = line[len("rename to ") :]
            elif line.startswith("Binary files") or line.startswith("GIT binary patch"):
                current["binary"] = True
            continue
        if not line:
            continue
        if total >= MAX_DIFF_LINES:
            current["truncated"] = True
            continue
        total += 1
        tag = line[0]
        if tag == " ":
            hunk["lines"].append({"t": " ", "text": line[1:], "old": old_no, "new": new_no})
            old_no += 1
            new_no += 1
        elif tag == "-":
            hunk["lines"].append({"t": "-", "text": line[1:], "old": old_no, "new": None})
            old_no += 1
        elif tag == "+":
            hunk["lines"].append({"t": "+", "text": line[1:], "old": None, "new": new_no})
            new_no += 1
        elif tag == "\\":
            hunk["lines"].append({"t": "\\", "text": line[1:].strip(), "old": None, "new": None})
    for f in files:
        if f["oldPath"] is None and f["newPath"] is None:
            # Mode-only change or binary file without ---/+++ lines.
            m = re.match(r"diff --git a/(.*) b/(.*)$", f["header"][0])
            if m:
                f["oldPath"], f["newPath"] = m.group(1), m.group(2)
    return files


def untracked_diff(path: str, content: bytes) -> dict[str, Any]:
    """Synthesise a diff for an untracked file (everything added)."""
    header = [f"diff --git a/{path} b/{path}", "new file mode 100644", "--- /dev/null", f"+++ b/{path}"]
    result = {
        "header": header,
        "oldPath": None,
        "newPath": path,
        "status": "?",
        "binary": False,
        "truncated": False,
        "hunks": [],
    }
    if b"\0" in content[:8000]:
        result["binary"] = True
        return result
    text = content.decode("utf-8", errors="replace")
    lines = text.split("\n")
    no_newline = not text.endswith("\n")
    if not no_newline:
        lines.pop()
    if not lines:
        return result
    truncated = len(lines) > MAX_DIFF_LINES
    lines = lines[:MAX_DIFF_LINES]
    hunk_lines = [{"t": "+", "text": line, "old": None, "new": n} for n, line in enumerate(lines, 1)]
    if no_newline and not truncated:
        hunk_lines.append({"t": "\\", "text": "No newline at end of file", "old": None, "new": None})
    result["truncated"] = truncated
    result["hunks"].append(
        {
            "header": f"@@ -0,0 +1,{len(lines)} @@",
            "oldStart": 0,
            "oldLines": 0,
            "newStart": 1,
            "newLines": len(lines),
            "context": "",
            "lines": hunk_lines,
        }
    )
    return result


def build_partial_patch(file_diff: dict[str, Any], selection: dict[int, set[int] | None], reverse: bool) -> str:
    """Build a patch containing only the selected hunks/lines of `file_diff`.

    `selection` maps hunk index -> selected line indices (None = whole hunk).
    For a forward patch (staging) unselected additions are dropped and unselected
    removals become context. For a reverse patch (unstaging/discarding, applied
    with -R) it is the other way round.
    """
    header = [
        line
        for line in file_diff["header"]
        if not line.startswith(("Binary files", "similarity index", "dissimilarity index"))
    ]
    out = list(header)
    for index in sorted(selection):
        hunk = file_diff["hunks"][index]
        chosen = selection[index]
        body: list[str] = []
        old_count = new_count = 0
        changed = False
        previous_kept = True
        for line_index, line in enumerate(hunk["lines"]):
            tag, text = line["t"], line["text"]
            if tag == "\\":
                if previous_kept:
                    body.append("\\ " + text)
                continue
            selected = chosen is None or line_index in chosen
            if tag == " ":
                body.append(" " + text)
                old_count += 1
                new_count += 1
            elif tag == "+":
                if selected:
                    body.append("+" + text)
                    new_count += 1
                    changed = True
                elif reverse:
                    body.append(" " + text)
                    old_count += 1
                    new_count += 1
                else:
                    previous_kept = False
                    continue
            elif tag == "-":
                if selected:
                    body.append("-" + text)
                    old_count += 1
                    changed = True
                elif not reverse:
                    body.append(" " + text)
                    old_count += 1
                    new_count += 1
                else:
                    previous_kept = False
                    continue
            previous_kept = True
        if changed:
            out.append(f"@@ -{hunk['oldStart']},{old_count} +{hunk['newStart']},{new_count} @@")
            out.extend(body)
    return "\n".join(out) + "\n"


# --------------------------------------------------------------------- blame


def parse_blame(out: str) -> list[dict[str, Any]]:
    commits: dict[str, dict[str, Any]] = {}
    lines: list[dict[str, Any]] = []
    current: dict[str, Any] | None = None
    sha = ""
    for raw in out.split("\n"):
        if raw.startswith("\t"):
            lines.append({"sha": sha, "line": len(lines) + 1, "text": raw[1:], **_blame_meta(commits[sha])})
            continue
        parts = raw.split(" ")
        if len(parts[0]) == 40 and len(parts) >= 3 and re.fullmatch(r"[0-9a-f]{40}", parts[0]):
            sha = parts[0]
            current = commits.setdefault(sha, {"author": "", "time": 0, "summary": ""})
            continue
        if current is None:
            continue
        key, _, value = raw.partition(" ")
        if key == "author":
            current["author"] = value
        elif key == "author-time":
            current["time"] = int(value)
        elif key == "summary":
            current["summary"] = value
    return lines


def _blame_meta(meta: dict[str, Any]) -> dict[str, Any]:
    return {"author": meta["author"], "time": meta["time"], "summary": meta["summary"]}


# ----------------------------------------------------------------- conflicts

CONFLICT_START = "<<<<<<<"
CONFLICT_BASE = "|||||||"
CONFLICT_SEP = "======="
CONFLICT_END = ">>>>>>>"


def parse_conflict_markers(text: str) -> list[dict[str, Any]]:
    """Split a file with conflict markers into common and conflict chunks."""
    chunks: list[dict[str, Any]] = []
    common: list[str] = []
    ours: list[str] = []
    base: list[str] = []
    theirs: list[str] = []
    state = "common"
    for line in text.split("\n"):
        if state == "common" and line.startswith(CONFLICT_START):
            if common:
                chunks.append({"type": "common", "lines": common})
            common, ours, base, theirs = [], [], [], []
            state = "ours"
        elif state == "ours" and line.startswith(CONFLICT_BASE):
            state = "base"
        elif state in ("ours", "base") and line.startswith(CONFLICT_SEP):
            state = "theirs"
        elif state == "theirs" and line.startswith(CONFLICT_END):
            chunks.append({"type": "conflict", "ours": ours, "base": base, "theirs": theirs})
            state = "common"
        else:
            {"common": common, "ours": ours, "base": base, "theirs": theirs}[state].append(line)
    if state != "common":
        # Unterminated conflict: treat the rest as plain text.
        common = ours + base + theirs
    if common:
        chunks.append({"type": "common", "lines": common})
    return chunks
