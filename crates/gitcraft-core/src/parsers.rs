//! Parsers for machine-readable git output, and the partial patch builder.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::model::*;
use crate::paths;
use crate::text::{lines, mask_urls};

pub const RS: char = '\x1e';
pub const US: char = '\x1f';

pub const LOG_FORMAT: &str = "--format=%x1e%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%D%x1f%s";
pub const DETAIL_FORMAT: &str =
    "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%cn%x1f%ce%x1f%ct%x1f%D%x1f%B";
pub const REF_FORMAT: &str = "--format=%(refname)%1f%(objectname)%1f%(upstream:short)%1f%(upstream:track,nobracket)%1f%(HEAD)%1f%(*objectname)%1f%(contents:subject)%1f%(creatordate:unix)";
pub const STASH_FORMAT: &str = "--format=%gd%x1f%H%x1f%at%x1f%gs";
pub const REFLOG_FORMAT: &str = "--format=%H%x1f%gd%x1f%gs%x1f%ct%x1f%s%x1f%an";
pub const MAX_DIFF_LINES: usize = 20_000;

fn int(text: &str) -> i64 {
    text.trim().parse().unwrap_or(0)
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

fn opt(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_owned())
}

// ---------------------------------------------------------------- log / refs

/// Parses `%D` produced with --decorate=full.
pub fn parse_decorations(text: &str) -> Vec<RefLabel> {
    let label = |name: &str, kind: &str, current| RefLabel {
        name: name.to_owned(),
        kind: kind.to_owned(),
        current,
    };
    let mut refs = Vec::new();
    for item in text
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let (item, current) = match item.strip_prefix("HEAD -> ") {
            Some(rest) => (rest, true),
            None => (item, false),
        };
        if item == "HEAD" {
            refs.push(label("HEAD", "head", true));
        } else if let Some(name) = item.strip_prefix("tag: refs/tags/") {
            refs.push(label(name, "tag", false));
        } else if let Some(name) = item.strip_prefix("refs/heads/") {
            refs.push(label(name, "local", current));
        } else if let Some(name) = item.strip_prefix("refs/remotes/")
            && !name.ends_with("/HEAD")
        {
            refs.push(label(name, "remote", false));
        }
    }
    refs
}

pub fn parse_log(out: &str) -> Vec<Commit> {
    let mut commits = Vec::new();
    for record in out.split(RS).skip(1) {
        let fields: Vec<&str> = record.trim_end_matches('\n').split(US).collect();
        let [sha, parents, author, email, time, decorations, subject, ..] = fields[..] else {
            continue;
        };
        commits.push(Commit {
            sha: sha.to_owned(),
            parents: words(parents),
            author: author.to_owned(),
            email: email.to_owned(),
            time: int(time),
            refs: Some(parse_decorations(decorations)),
            subject: subject.to_owned(),
            graph: None,
            file: None,
            stash: None,
        });
    }
    commits
}

pub fn parse_commit_detail(out: &str) -> Option<CommitDetail> {
    let fields: Vec<&str> = out.split(US).collect();
    let [
        sha,
        parents,
        author,
        email,
        time,
        committer,
        committer_email,
        commit_time,
        decorations,
        rest @ ..,
    ] = &fields[..]
    else {
        return None;
    };
    let message = rest.join("\x1f");
    let message = message.trim_end_matches('\n').to_owned();
    Some(CommitDetail {
        sha: (*sha).to_owned(),
        parents: words(parents),
        author: (*author).to_owned(),
        email: (*email).to_owned(),
        time: int(time),
        committer: (*committer).to_owned(),
        committer_email: (*committer_email).to_owned(),
        commit_time: int(commit_time),
        refs: parse_decorations(decorations),
        subject: message.split('\n').next().unwrap_or("").to_owned(),
        message,
        files: Vec::new(),
        signature: None,
    })
}

/// The protocol and host (with port) of an http(s) remote URL, without any user name or
/// password in it; `None` for ssh, file and other URLs, whose credentials git doesn't keep.
pub fn credential_host(url: &str) -> Option<(String, String)> {
    let (protocol, rest) = url.split_once("://")?;
    if protocol != "https" && protocol != "http" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    (!host.is_empty()).then(|| (protocol.to_owned(), host.to_owned()))
}

#[cfg(test)]
mod credential_tests {
    use super::*;

    #[test]
    fn only_http_hosts_have_credentials_to_forget() {
        assert_eq!(
            credential_host("https://me:secret@example.com:8443/team/x.git"),
            Some(("https".to_owned(), "example.com:8443".to_owned()))
        );
        assert_eq!(
            credential_host("http://example.com"),
            Some(("http".to_owned(), "example.com".to_owned()))
        );
        assert_eq!(credential_host("git@example.com:team/x.git"), None);
        assert_eq!(credential_host("ssh://git@example.com/x.git"), None);
        assert_eq!(credential_host("file:///srv/x.git"), None);
    }
}

/// `log --format=%x1e%H --shortstat`: each commit's sha, then a line such as
/// " 3 files changed, 10 insertions(+), 2 deletions(-)" (absent when nothing changed).
pub fn parse_shortstats(out: &str) -> Vec<(String, CommitStats)> {
    let mut found = Vec::new();
    for record in out.split(RS).skip(1) {
        let mut lines = record.lines().filter(|line| !line.trim().is_empty());
        let Some(sha) = lines.next() else { continue };
        let mut stats = CommitStats::default();
        if let Some(line) = lines.next() {
            for part in line.split(',') {
                let mut words = part.split_whitespace();
                let number = words.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                match words.next() {
                    Some(word) if word.starts_with("file") => stats.files = number,
                    Some(word) if word.starts_with("insertion") => stats.added = number,
                    Some(word) if word.starts_with("deletion") => stats.removed = number,
                    _ => {}
                }
            }
        }
        found.push((sha.trim().to_owned(), stats));
    }
    found
}

/// The bisect log (`.git/BISECT_LOG`): git notes each verdict as `# good: [sha] subject`
/// (also `bad`, `skip`, and those given at the start) and, once done,
/// `# first bad commit: [sha] subject` (newer versions quote the term: `'bad'`).
pub fn parse_bisect_log(log: &str) -> Bisect {
    let mut state = Bisect::default();
    let sha = |rest: &str| {
        rest.strip_prefix('[')
            .and_then(|rest| rest.split(']').next())
            .map(str::to_owned)
    };
    for line in log.lines() {
        let Some(comment) = line.strip_prefix("# ") else {
            continue;
        };
        if let Some(rest) = comment.strip_prefix("first ") {
            if let Some((_, rest)) = rest.split_once("commit: ") {
                state.first_bad = sha(rest);
            }
        } else if let Some(rest) = comment.strip_prefix("bad: ") {
            state.bad = sha(rest);
        } else if comment.starts_with("good: ") {
            state.good += 1;
        } else if comment.starts_with("skip: ") {
            state.skipped += 1;
        }
    }
    state
}

#[cfg(test)]
mod bisect_tests {
    use super::*;

    #[test]
    fn the_bisect_log_of_old_and_new_git_versions() {
        let log = "# bad: [bbb] commit 8\n# good: [aaa] commit 1\ngit bisect start 'HEAD' 'HEAD~7'\n\
                   # good: [ccc] commit 4\ngit bisect good ccc\n# bad: [ddd] commit 6\n\
                   # skip: [eee] commit 5\n# first 'bad' commit: [ddd] commit 6\n";
        let state = parse_bisect_log(log);
        assert_eq!(state.bad.as_deref(), Some("ddd"));
        assert_eq!((state.good, state.skipped), (2, 1));
        assert_eq!(state.first_bad.as_deref(), Some("ddd"));
        let old = parse_bisect_log("# first bad commit: [fff] x\n");
        assert_eq!(old.first_bad.as_deref(), Some("fff"));
    }
}

/// `%G?%x1f%GS%x1f%GK`: `None` when the commit is not signed.
pub fn parse_signature(out: &str) -> Option<Signature> {
    let line = out.trim_end_matches('\n');
    let mut fields = line.split('\x1f');
    let status = fields.next()?.chars().next()?;
    if status == 'N' {
        return None;
    }
    Some(Signature {
        status,
        signer: fields.next().unwrap_or("").to_owned(),
        key: fields.next().unwrap_or("").to_owned(),
    })
}

fn tracked_count(text: &str, word: &str) -> i64 {
    text.find(word)
        .map(|at| {
            let digits: String = text[at + word.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse().unwrap_or(0)
        })
        .unwrap_or(0)
}

pub fn parse_refs(out: &str, remote_names: &[String]) -> Refs {
    let mut refs = Refs::default();
    let mut by_length: Vec<&String> = remote_names.iter().collect();
    by_length.sort_by_key(|name| std::cmp::Reverse(name.len()));
    for line in lines(out) {
        let fields: Vec<&str> = line.split(US).collect();
        let [
            refname,
            sha,
            upstream,
            track,
            head,
            peeled,
            subject,
            date,
            ..,
        ] = fields[..]
        else {
            continue;
        };
        if let Some(name) = refname.strip_prefix("refs/heads/") {
            refs.local.push(LocalBranch {
                name: name.to_owned(),
                sha: sha.to_owned(),
                upstream: opt(upstream),
                ahead: tracked_count(track, "ahead "),
                behind: tracked_count(track, "behind "),
                upstream_gone: track.trim() == "gone",
                current: head == "*",
            });
        } else if let Some(name) = refname.strip_prefix("refs/remotes/") {
            if name.ends_with("/HEAD") {
                continue;
            }
            let remote = by_length
                .iter()
                .find(|remote| name.starts_with(&format!("{remote}/")))
                .map_or_else(
                    || name.split('/').next().unwrap_or(name),
                    |remote| remote.as_str(),
                );
            refs.remote.push(RemoteBranch {
                name: name.to_owned(),
                remote: remote.to_owned(),
                branch: name.get(remote.len() + 1..).unwrap_or("").to_owned(),
                sha: sha.to_owned(),
            });
        } else if let Some(name) = refname.strip_prefix("refs/tags/") {
            refs.tags.push(Tag {
                name: name.to_owned(),
                sha: if peeled.is_empty() { sha } else { peeled }.to_owned(),
                annotated: !peeled.is_empty(),
                message: if peeled.is_empty() { "" } else { subject }.to_owned(),
                time: int(date),
            });
        }
    }
    refs
}

pub fn parse_remotes(out: &str) -> Vec<Remote> {
    let mut remotes: Vec<Remote> = Vec::new();
    for line in lines(out) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let [name, url, kind, ..] = parts[..] else {
            continue;
        };
        let index = match remotes.iter().position(|remote| remote.name == name) {
            Some(index) => index,
            None => {
                remotes.push(Remote {
                    name: name.to_owned(),
                    fetch_url: String::new(),
                    push_url: String::new(),
                });
                remotes.len() - 1
            }
        };
        let url = mask_urls(url);
        if kind == "(fetch)" {
            remotes[index].fetch_url = url;
        } else {
            remotes[index].push_url = url;
        }
    }
    remotes
}

/// Parses `git worktree list --porcelain`: one block per working tree, separated by blank lines.
pub fn parse_worktrees(out: &str) -> Vec<Worktree> {
    let mut trees: Vec<Worktree> = Vec::new();
    for line in out.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if key == "worktree" {
            trees.push(Worktree {
                path: paths::resolve_str(value),
                main: trees.is_empty(),
                ..Worktree::default()
            });
            continue;
        }
        let Some(tree) = trees.last_mut() else {
            continue;
        };
        match key {
            "HEAD" => tree.sha = value.to_owned(),
            "branch" => {
                tree.branch = Some(
                    value
                        .strip_prefix("refs/heads/")
                        .unwrap_or(value)
                        .to_owned(),
                );
            }
            "detached" => tree.detached = true,
            "bare" => tree.bare = true,
            "locked" => tree.locked = true,
            "prunable" => tree.prunable = true,
            _ => {}
        }
    }
    trees
}

pub fn parse_stashes(out: &str) -> Vec<Stash> {
    lines(out)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(US).collect();
            let [name, sha, time, message, ..] = fields[..] else {
                return None;
            };
            Some(Stash {
                name: name.to_owned(),
                sha: sha.to_owned(),
                time: int(time),
                message: message.to_owned(),
            })
        })
        .collect()
}

pub fn parse_reflog(out: &str) -> Vec<ReflogEntry> {
    lines(out)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(US).collect();
            let [sha, selector, action, time, subject, author, ..] = fields[..] else {
                return None;
            };
            Some(ReflogEntry {
                sha: sha.to_owned(),
                selector: selector.to_owned(),
                action: action.to_owned(),
                time: int(time),
                subject: subject.to_owned(),
                author: author.to_owned(),
            })
        })
        .collect()
}

pub fn parse_submodules(out: &str) -> Vec<Submodule> {
    let mut modules = Vec::new();
    for line in lines(out) {
        if line.chars().count() < 42 {
            continue;
        }
        let mut chars = line.chars();
        let state = match chars.next() {
            Some('-') => "uninitialized",
            Some('+') => "modified",
            Some('U') => "conflict",
            _ => "ok",
        };
        let parts: Vec<&str> = chars.as_str().split_whitespace().collect();
        if let [sha, path, ..] = parts[..] {
            modules.push(Submodule {
                sha: sha.to_owned(),
                path: path.to_owned(),
                state: state.to_owned(),
            });
        }
    }
    modules
}

// -------------------------------------------------------------------- status

pub fn parse_status(out: &str) -> Status {
    let mut status = Status::default();
    let entries: Vec<&str> = out.split('\0').collect();
    let mut i = 0;
    while i < entries.len() {
        let entry = entries[i];
        i += 1;
        if entry.is_empty() {
            continue;
        }
        if let Some(header) = entry.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            let branch = &mut status.branch;
            match key {
                "branch.oid" => branch.oid = (value != "(initial)").then(|| value.to_owned()),
                "branch.head" => {
                    branch.detached = value == "(detached)";
                    branch.head = (!branch.detached).then(|| value.to_owned());
                }
                "branch.upstream" => branch.upstream = Some(value.to_owned()),
                "branch.ab" => {
                    let mut parts = value.split_whitespace();
                    branch.ahead = parts.next().map_or(0, int);
                    branch.behind = -parts.next().map_or(0, int);
                }
                _ => {}
            }
            continue;
        }
        match entry.as_bytes()[0] {
            b'1' => {
                let parts: Vec<&str> = entry.splitn(9, ' ').collect();
                if let [_, xy, .., path] = parts[..] {
                    add_change(&mut status, xy, path, None);
                }
            }
            b'2' => {
                let parts: Vec<&str> = entry.splitn(10, ' ').collect();
                let orig = entries.get(i).map(|orig| (*orig).to_owned());
                i += 1;
                if let [_, xy, .., path] = parts[..] {
                    add_change(&mut status, xy, path, orig);
                }
            }
            b'u' => {
                let parts: Vec<&str> = entry.splitn(11, ' ').collect();
                if let [_, xy, .., path] = parts[..] {
                    status.conflicts.push(ConflictFile {
                        path: path.to_owned(),
                        status: xy.to_owned(),
                    });
                }
            }
            b'?' => status.unstaged.push(FileChange {
                path: entry.get(2..).unwrap_or("").to_owned(),
                status: "?".to_owned(),
                orig_path: None,
            }),
            _ => {}
        }
    }
    status
}

fn add_change(status: &mut Status, xy: &str, path: &str, orig: Option<String>) {
    let mut codes = xy.chars();
    let (x, y) = (codes.next().unwrap_or('.'), codes.next().unwrap_or('.'));
    if x != '.' {
        status.staged.push(FileChange {
            path: path.to_owned(),
            status: x.to_string(),
            orig_path: orig,
        });
    }
    if y != '.' {
        status.unstaged.push(FileChange {
            path: path.to_owned(),
            status: y.to_string(),
            orig_path: None,
        });
    }
}

/// Parses `--name-status -z` output.
pub fn parse_name_status(out: &str) -> Vec<FileChange> {
    let parts: Vec<&str> = out.split('\0').collect();
    let mut files = Vec::new();
    let mut i = 0;
    while i < parts.len() {
        let status = parts[i];
        i += 1;
        let Some(code) = status.chars().next() else {
            continue;
        };
        let take = |index: usize| parts.get(index).copied().unwrap_or("").to_owned();
        if code == 'R' || code == 'C' {
            files.push(FileChange {
                status: code.to_string(),
                orig_path: Some(take(i)),
                path: take(i + 1),
            });
            i += 2;
        } else {
            files.push(FileChange {
                status: code.to_string(),
                orig_path: None,
                path: take(i),
            });
            i += 1;
        }
    }
    files
}

// ---------------------------------------------------------------------- diff

fn strip_prefix(path: &str) -> Option<String> {
    if path == "/dev/null" {
        return None;
    }
    let path = path.split('\t').next().unwrap_or(path);
    Some(
        path.strip_prefix("a/")
            .or_else(|| path.strip_prefix("b/"))
            .unwrap_or(path)
            .to_owned(),
    )
}

/// `@@ -a[,b] +c[,d] @@ context` into (a, b, c, d, context).
fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32, &str)> {
    fn range(text: &str) -> Option<(u32, u32)> {
        match text.split_once(',') {
            Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
            None => Some((text.parse().ok()?, 1)),
        }
    }
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, rest) = rest.split_once(" @@")?;
    let (old_start, old_lines) = range(old)?;
    let (new_start, new_lines) = range(new)?;
    Some((
        old_start,
        old_lines,
        new_start,
        new_lines,
        rest.strip_prefix(' ').unwrap_or(rest),
    ))
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Parses a unified diff. Content stays exact: lines that aren't valid UTF-8 keep their bytes.
pub fn parse_diff(text: &[u8]) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut in_hunk = false;
    let (mut old_no, mut new_no) = (0u32, 0u32);
    let mut total = 0;
    for line in text.split(|b| *b == b'\n') {
        if line.starts_with(b"diff --git ") {
            files.push(FileDiff {
                header: vec![lossy(line)],
                raw_header: vec![line.to_vec()],
                old_path: None,
                new_path: None,
                status: "M".to_owned(),
                binary: false,
                truncated: false,
                hunks: Vec::new(),
            });
            in_hunk = false;
            continue;
        }
        let Some(current) = files.last_mut() else {
            continue;
        };
        if line.starts_with(b"@@") {
            let decoded = lossy(line);
            let Some((old_start, old_lines, new_start, new_lines, context)) =
                parse_hunk_header(&decoded)
            else {
                continue;
            };
            old_no = old_start;
            new_no = new_start;
            current.hunks.push(Hunk {
                old_start,
                old_lines,
                new_start,
                new_lines,
                context: context.to_owned(),
                header: decoded.clone(),
                lines: Vec::new(),
            });
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            let decoded = lossy(line);
            current.header.push(decoded.clone());
            current.raw_header.push(line.to_vec());
            if let Some(path) = decoded.strip_prefix("--- ") {
                current.old_path = strip_prefix(path);
            } else if let Some(path) = decoded.strip_prefix("+++ ") {
                current.new_path = strip_prefix(path);
            } else if decoded.starts_with("new file mode") {
                current.status = "A".to_owned();
            } else if decoded.starts_with("deleted file mode") {
                current.status = "D".to_owned();
            } else if let Some(path) = decoded.strip_prefix("rename from ") {
                current.status = "R".to_owned();
                current.old_path = Some(path.to_owned());
            } else if let Some(path) = decoded.strip_prefix("rename to ") {
                current.new_path = Some(path.to_owned());
            } else if decoded.starts_with("Binary files") || decoded.starts_with("GIT binary patch")
            {
                current.binary = true;
            }
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if total >= MAX_DIFF_LINES {
            current.truncated = true;
            continue;
        }
        total += 1;
        let (tag, body) = (line[0], &line[1..]);
        let text = lossy(body);
        let raw = std::str::from_utf8(body).is_err().then(|| body.to_vec());
        let hunk = current.hunks.last_mut().expect("in_hunk implies a hunk");
        match tag {
            b' ' => {
                hunk.lines.push(DiffLine {
                    t: ' ',
                    text,
                    old: Some(old_no),
                    new: Some(new_no),
                    raw,
                });
                old_no += 1;
                new_no += 1;
            }
            b'-' => {
                hunk.lines.push(DiffLine {
                    t: '-',
                    text,
                    old: Some(old_no),
                    new: None,
                    raw,
                });
                old_no += 1;
            }
            b'+' => {
                hunk.lines.push(DiffLine {
                    t: '+',
                    text,
                    old: None,
                    new: Some(new_no),
                    raw,
                });
                new_no += 1;
            }
            b'\\' => {
                let text = text.trim().to_owned();
                hunk.lines.push(DiffLine {
                    t: '\\',
                    text,
                    old: None,
                    new: None,
                    raw: None,
                });
            }
            _ => {}
        }
    }
    for file in &mut files {
        if file.old_path.is_none() && file.new_path.is_none() {
            // Mode-only change or binary file without ---/+++ lines.
            if let Some(rest) = file.header[0].strip_prefix("diff --git a/")
                && let Some((old, new)) = rest.rsplit_once(" b/")
            {
                file.old_path = Some(old.to_owned());
                file.new_path = Some(new.to_owned());
            }
        }
    }
    files
}

/// Synthesises a diff for an untracked file (everything added).
pub fn untracked_diff(path: &str, content: &[u8]) -> FileDiff {
    let header = vec![
        format!("diff --git a/{path} b/{path}"),
        "new file mode 100644".to_owned(),
        "--- /dev/null".to_owned(),
        format!("+++ b/{path}"),
    ];
    let mut result = FileDiff {
        raw_header: header.iter().map(|line| line.as_bytes().to_vec()).collect(),
        header,
        old_path: None,
        new_path: Some(path.to_owned()),
        status: "?".to_owned(),
        binary: false,
        truncated: false,
        hunks: Vec::new(),
    };
    if content[..content.len().min(8000)].contains(&0) {
        result.binary = true;
        return result;
    }
    let text = String::from_utf8_lossy(content);
    let no_newline = !text.ends_with('\n');
    let mut all: Vec<&str> = text.split('\n').collect();
    if !no_newline {
        all.pop();
    }
    // An empty file splits into one empty "line" and has no trailing newline.
    if text.is_empty() {
        all.clear();
    }
    if all.is_empty() {
        return result;
    }
    let truncated = all.len() > MAX_DIFF_LINES;
    all.truncate(MAX_DIFF_LINES);
    let count = all.len() as u32;
    let mut hunk_lines: Vec<DiffLine> = all
        .iter()
        .zip(1..)
        .map(|(line, number)| DiffLine {
            t: '+',
            text: (*line).to_owned(),
            old: None,
            new: Some(number),
            raw: None,
        })
        .collect();
    if no_newline && !truncated {
        hunk_lines.push(DiffLine {
            t: '\\',
            text: "No newline at end of file".to_owned(),
            old: None,
            new: None,
            raw: None,
        });
    }
    result.truncated = truncated;
    result.hunks.push(Hunk {
        header: format!("@@ -0,0 +1,{count} @@"),
        old_start: 0,
        old_lines: 0,
        new_start: 1,
        new_lines: count,
        context: String::new(),
        lines: hunk_lines,
    });
    result
}

/// Selected line indices per hunk index; `None` selects the whole hunk.
pub type Selection = BTreeMap<usize, Option<BTreeSet<usize>>>;

/// Builds a patch containing only the selected hunks/lines of `file_diff`.
///
/// For a forward patch (staging) unselected additions are dropped and unselected
/// removals become context. For a reverse patch (unstaging/discarding, applied
/// with -R) it is the other way round.
pub fn build_partial_patch(file_diff: &FileDiff, selection: &Selection, reverse: bool) -> Vec<u8> {
    let mut out: Vec<Vec<u8>> = file_diff
        .raw_header
        .iter()
        .filter(|line| {
            !(line.starts_with(b"Binary files")
                || line.starts_with(b"similarity index")
                || line.starts_with(b"dissimilarity index"))
        })
        .cloned()
        .collect();
    let prefixed = |prefix: &[u8], body: &[u8]| [prefix, body].concat();
    for (&index, chosen) in selection {
        let Some(hunk) = file_diff.hunks.get(index) else {
            continue;
        };
        let mut body: Vec<Vec<u8>> = Vec::new();
        let (mut old_count, mut new_count) = (0, 0);
        let mut changed = false;
        let mut previous_kept = true;
        for (line_index, line) in hunk.lines.iter().enumerate() {
            if line.t == '\\' {
                if previous_kept {
                    body.push(prefixed(b"\\ ", line.bytes()));
                }
                continue;
            }
            let selected = chosen.as_ref().is_none_or(|set| set.contains(&line_index));
            match line.t {
                ' ' => {
                    body.push(prefixed(b" ", line.bytes()));
                    old_count += 1;
                    new_count += 1;
                }
                '+' if selected => {
                    body.push(prefixed(b"+", line.bytes()));
                    new_count += 1;
                    changed = true;
                }
                '+' if reverse => {
                    body.push(prefixed(b" ", line.bytes()));
                    old_count += 1;
                    new_count += 1;
                }
                '-' if selected => {
                    body.push(prefixed(b"-", line.bytes()));
                    old_count += 1;
                    changed = true;
                }
                '-' if !reverse => {
                    body.push(prefixed(b" ", line.bytes()));
                    old_count += 1;
                    new_count += 1;
                }
                _ => {
                    previous_kept = false;
                    continue;
                }
            }
            previous_kept = true;
        }
        if changed {
            out.push(
                format!(
                    "@@ -{},{old_count} +{},{new_count} @@",
                    hunk.old_start, hunk.new_start
                )
                .into_bytes(),
            );
            out.append(&mut body);
        }
    }
    let mut patch = out.join(&b'\n');
    patch.push(b'\n');
    patch
}

// --------------------------------------------------------------------- blame

#[derive(Clone, Default)]
struct BlameMeta {
    author: String,
    time: i64,
    summary: String,
}

pub fn parse_blame(out: &str) -> Vec<BlameLine> {
    let mut commits: HashMap<String, BlameMeta> = HashMap::new();
    let mut result = Vec::new();
    let mut sha = String::new();
    for raw in out.split('\n') {
        if let Some(text) = raw.strip_prefix('\t') {
            let meta = commits.get(&sha).cloned().unwrap_or_default();
            result.push(BlameLine {
                sha: sha.clone(),
                line: result.len() as u32 + 1,
                text: text.to_owned(),
                author: meta.author,
                time: meta.time,
                summary: meta.summary,
            });
            continue;
        }
        let first = raw.split(' ').next().unwrap_or("");
        let is_header = first.len() == 40
            && first
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            && raw.split(' ').count() >= 3;
        if is_header {
            sha = first.to_owned();
            commits.entry(sha.clone()).or_default();
            continue;
        }
        let Some(current) = commits.get_mut(&sha) else {
            continue;
        };
        let (key, value) = raw.split_once(' ').unwrap_or((raw, ""));
        match key {
            "author" => current.author = value.to_owned(),
            "author-time" => current.time = int(value),
            "summary" => current.summary = value.to_owned(),
            _ => {}
        }
    }
    result
}

// ----------------------------------------------------------------- conflicts

/// Splits a file with conflict markers into common and conflict chunks.
pub fn parse_conflict_markers(text: &str) -> Vec<ConflictChunk> {
    #[derive(PartialEq)]
    enum Section {
        Common,
        Ours,
        Base,
        Theirs,
    }
    let mut chunks = Vec::new();
    let (mut common, mut ours, mut base, mut theirs) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut section = Section::Common;
    for line in text.split('\n') {
        if section == Section::Common && line.starts_with("<<<<<<<") {
            if !common.is_empty() {
                chunks.push(ConflictChunk::Common {
                    lines: std::mem::take(&mut common),
                });
            }
            ours.clear();
            base.clear();
            theirs.clear();
            section = Section::Ours;
        } else if section == Section::Ours && line.starts_with("|||||||") {
            section = Section::Base;
        } else if matches!(section, Section::Ours | Section::Base) && line.starts_with("=======") {
            section = Section::Theirs;
        } else if section == Section::Theirs && line.starts_with(">>>>>>>") {
            chunks.push(ConflictChunk::Conflict {
                ours: std::mem::take(&mut ours),
                base: std::mem::take(&mut base),
                theirs: std::mem::take(&mut theirs),
            });
            section = Section::Common;
        } else {
            let target = match section {
                Section::Common => &mut common,
                Section::Ours => &mut ours,
                Section::Base => &mut base,
                Section::Theirs => &mut theirs,
            };
            target.push(line.to_owned());
        }
    }
    if section != Section::Common {
        // Unterminated conflict: treat the rest as plain text.
        common = [ours, base, theirs].concat();
    }
    if !common.is_empty() {
        chunks.push(ConflictChunk::Common { lines: common });
    }
    chunks
}

/// The tools `git difftool --tool-help` (or `mergetool`) lists as usable: the installed ones
/// that open a window, and any set up with `difftool.<name>.cmd`. Tools that run in a terminal
/// (vimdiff, emerge) are left out, as there is no terminal to run them in.
pub fn parse_tool_help(out: &str) -> Vec<ExternalTool> {
    let mut tools = Vec::new();
    let mut usable = false;
    let mut own = false;
    for line in out.lines() {
        let trimmed = line.trim();
        if line.contains("may be set to one of the following") {
            (usable, own) = (true, false);
            continue;
        }
        if trimmed == "user-defined:" {
            (usable, own) = (true, true);
            continue;
        }
        if line.contains("not currently available") {
            usable = false;
            continue;
        }
        // Tools are the indented lines of a usable section; the rest is prose.
        if !usable || trimmed.is_empty() || !line.starts_with('\t') {
            continue;
        }
        let (word, rest) = trimmed
            .split_once(char::is_whitespace)
            .unwrap_or((trimmed, ""));
        if own {
            // "mytool.cmd echo $LOCAL"
            let name = word.strip_suffix(".cmd").unwrap_or(word);
            tools.push(ExternalTool {
                name: name.to_owned(),
                label: name.to_owned(),
            });
        } else if rest.contains("graphical session") {
            let label = rest.trim().trim_start_matches("Use ");
            let label = label.split(" (requires").next().unwrap_or(label).trim();
            tools.push(ExternalTool {
                name: word.to_owned(),
                label: label.to_owned(),
            });
        } else if rest.is_empty() && !runs_in_terminal(word) {
            // git before 2.37 lists names only, without saying which tools need a window.
            tools.push(ExternalTool {
                name: word.to_owned(),
                label: word.to_owned(),
            });
        }
    }
    tools.dedup_by(|a, b| a.name == b.name);
    tools
}

/// The tools git knows that run in a terminal rather than a window of their own.
fn runs_in_terminal(name: &str) -> bool {
    name == "emerge" || name.starts_with("vimdiff") || name.starts_with("nvimdiff")
}

#[cfg(test)]
mod tests {

    #[test]
    fn tool_help_of_older_git_lists_names_only() {
        // git 2.35 and 2.36 print no descriptions.
        let out = "'git difftool --tool=<tool>' may be set to one of the following:\n\
\t\temerge\n\
\t\tgvimdiff\n\
\t\tkdiff3\n\
\t\tnvimdiff2\n\
\t\tvimdiff\n\
\n\
\tuser-defined:\n\
\t\tmytool.cmd echo $LOCAL\n\
\n\
The following tools are valid, but not currently available:\n\
\t\tmeld\n\
\n\
Some of the tools listed above only work in a windowed\n\
environment. If run in a terminal-only session, they will fail.\n";
        let names: Vec<String> = parse_tool_help(out)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert_eq!(names, ["gvimdiff", "kdiff3", "mytool"]);
    }

    #[test]
    fn tool_help_gives_the_installed_window_tools() {
        let out = "'git difftool --tool=<tool>' may be set to one of the following:\n\
\t\tkdiff3           Use KDiff3 (requires a graphical session)\n\
\t\tvimdiff          Use Vim\n\
\t\tbc               Use Beyond Compare (requires a graphical session)\n\
\n\
\tuser-defined:\n\
\t\tmytool.cmd echo $LOCAL\n\
\n\
The following tools are valid, but not currently available:\n\
\t\tmeld             Use Meld (requires a graphical session)\n\
\n\
Some of the tools listed above only work in a windowed\n\
environment. If run in a terminal-only session, they will fail.\n";
        let tools = parse_tool_help(out);
        let found: Vec<(&str, &str)> = tools
            .iter()
            .map(|tool| (tool.name.as_str(), tool.label.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                ("kdiff3", "KDiff3"),
                ("bc", "Beyond Compare"),
                ("mytool", "mytool")
            ]
        );
    }
    use super::*;

    const DIFF: &str = "diff --git a/f.txt b/f.txt\nindex 1111111..2222222 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@ ctx\n one\n-two\n+TWO\n three\n";

    #[test]
    fn status_v2() {
        let out = [
            "# branch.oid abc",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
            "1 M. N... 100644 100644 100644 aaa bbb staged.txt",
            "1 .M N... 100644 100644 100644 aaa bbb dirty file.txt",
            "2 R. N... 100644 100644 100644 aaa bbb R100 new.txt",
            "old.txt",
            "u UU N... 100644 100644 100644 100644 a b c conflict.txt",
            "? untracked.txt",
            "",
        ]
        .join("\0");
        let status = parse_status(&out);
        assert_eq!(
            status.branch,
            BranchStatus {
                head: Some("main".into()),
                oid: Some("abc".into()),
                upstream: Some("origin/main".into()),
                ahead: 2,
                behind: 1,
                detached: false,
            }
        );
        let paths = |files: &[FileChange]| files.iter().map(|f| f.path.clone()).collect::<Vec<_>>();
        assert_eq!(paths(&status.staged), ["staged.txt", "new.txt"]);
        assert_eq!(status.staged[1].orig_path.as_deref(), Some("old.txt"));
        assert_eq!(paths(&status.unstaged), ["dirty file.txt", "untracked.txt"]);
        assert_eq!(
            status.conflicts,
            [ConflictFile {
                path: "conflict.txt".into(),
                status: "UU".into()
            }]
        );
    }

    #[test]
    fn diff_and_line_numbers() {
        let files = parse_diff(DIFF.as_bytes());
        let [file] = &files[..] else {
            panic!("one file")
        };
        assert_eq!(file.old_path.as_deref(), Some("f.txt"));
        assert_eq!(file.new_path.as_deref(), Some("f.txt"));
        let [hunk] = &file.hunks[..] else {
            panic!("one hunk")
        };
        assert_eq!(hunk.context, "ctx");
        let lines: Vec<_> = hunk.lines.iter().map(|l| (l.t, l.old, l.new)).collect();
        assert_eq!(
            lines,
            [
                (' ', Some(1), Some(1)),
                ('-', Some(2), None),
                ('+', None, Some(2)),
                (' ', Some(3), Some(3))
            ]
        );
    }

    fn select(lines: &[usize]) -> Selection {
        BTreeMap::from([(0, Some(lines.iter().copied().collect()))])
    }

    #[test]
    fn partial_patch_forward_and_reverse() {
        let files = parse_diff(DIFF.as_bytes());
        // Stage only the addition: the removal becomes context.
        let forward =
            String::from_utf8(build_partial_patch(&files[0], &select(&[2]), false)).unwrap();
        assert!(
            forward.contains("@@ -1,3 +1,4 @@\n one\n two\n+TWO\n three\n"),
            "{forward}"
        );
        // Reverse with only the removal selected: the addition becomes context.
        let reverse =
            String::from_utf8(build_partial_patch(&files[0], &select(&[1]), true)).unwrap();
        assert!(
            reverse.contains("@@ -1,4 +1,3 @@\n one\n-two\n TWO\n three\n"),
            "{reverse}"
        );
    }

    #[test]
    fn partial_patch_keeps_non_utf8_bytes_exact() {
        let diff =
            b"diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n B\xfcro\n-alt\n+neu \xe4\n";
        let files = parse_diff(diff);
        assert_eq!(files[0].hunks[0].lines[0].text, "B\u{fffd}ro");
        let patch = build_partial_patch(&files[0], &BTreeMap::from([(0, None)]), false);
        assert!(patch.ends_with(b"@@ -1,2 +1,2 @@\n B\xfcro\n-alt\n+neu \xe4\n"));
    }

    #[test]
    fn decorations() {
        let refs = parse_decorations(
            "HEAD -> refs/heads/main, refs/remotes/origin/main, tag: refs/tags/v1",
        );
        let summary: Vec<_> = refs
            .iter()
            .map(|r| (r.name.as_str(), r.kind.as_str(), r.current))
            .collect();
        assert_eq!(
            summary,
            [
                ("main", "local", true),
                ("origin/main", "remote", false),
                ("v1", "tag", false)
            ]
        );
    }

    #[test]
    fn conflict_markers() {
        let text =
            "a\n<<<<<<< HEAD\nours\n||||||| base\norig\n=======\ntheirs\n>>>>>>> branch\nz\n";
        assert_eq!(
            parse_conflict_markers(text),
            [
                ConflictChunk::Common {
                    lines: vec!["a".into()]
                },
                ConflictChunk::Conflict {
                    ours: vec!["ours".into()],
                    base: vec!["orig".into()],
                    theirs: vec!["theirs".into()],
                },
                ConflictChunk::Common {
                    lines: vec!["z".into(), String::new()]
                },
            ]
        );
    }

    #[test]
    fn untracked() {
        let diff = untracked_diff("n.txt", b"x\ny");
        let tags: Vec<char> = diff.hunks[0].lines.iter().map(|l| l.t).collect();
        assert_eq!(tags, ['+', '+', '\\']);
        assert!(untracked_diff("b.bin", b"\0\x01").binary);
    }

    #[test]
    fn subjects_keep_form_feeds() {
        let out =
            "abc\x1fstash@{0}\x1faction\x0cmore\x1f1\x1fsubject\x0cwith\x1cseparators\x1fme\n";
        let entries = parse_reflog(out);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].subject, "subject\x0cwith\x1cseparators");
    }

    #[test]
    fn worktree_list_porcelain() {
        let out = "worktree /r/main\nHEAD aaa\nbranch refs/heads/main\n\n\
                   worktree /r/feat\nHEAD bbb\nbranch refs/heads/feat/x\nlocked why\n\n\
                   worktree /r/old\nHEAD ccc\ndetached\nprunable gone\n";
        let trees = parse_worktrees(out);
        assert_eq!(trees.len(), 3);
        assert!(trees[0].main && !trees[1].main);
        assert_eq!(trees[1].branch.as_deref(), Some("feat/x"));
        assert!(trees[1].locked);
        assert!(trees[2].detached && trees[2].prunable && trees[2].branch.is_none());
    }
}
