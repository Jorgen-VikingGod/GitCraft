//! Read-only git queries.

use std::path::{Path, PathBuf};

use crate::graph::Layout;
use crate::model::*;
use crate::parsers::{
    self, DETAIL_FORMAT, LOG_FORMAT, REF_FORMAT, REFLOG_FORMAT, RS, STASH_FORMAT,
};
use crate::runner::{Options, git, git_unchecked, run};
use crate::{Error, Result, check_ref, paths};

pub async fn toplevel(path: &str) -> Result<String> {
    let result = git(path, &["rev-parse", "--show-toplevel"]).await?;
    Ok(paths::resolve_str(result.out().trim()))
}

pub async fn git_dir(repo: &str) -> Result<PathBuf> {
    let result = git(repo, &["rev-parse", "--absolute-git-dir"]).await?;
    Ok(PathBuf::from(result.out().trim()))
}

/// The directory shared by all linked worktrees of this repository.
pub async fn common_dir(repo: &str) -> Result<PathBuf> {
    let result = git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .await?;
    Ok(PathBuf::from(result.out().trim()))
}

/// How much each of the given commits changed, in one run of git.
pub async fn commit_stats(repo: &str, shas: &[String]) -> Result<Vec<(String, CommitStats)>> {
    if shas.is_empty() {
        return Ok(Vec::new());
    }
    let mut input = String::new();
    for sha in shas {
        input.push_str(check_ref(sha)?);
        input.push('\n');
    }
    let result = run(
        repo,
        &[
            "log",
            "--stdin",
            "--no-walk=unsorted",
            "--format=%x1e%H",
            "--shortstat",
            "--diff-merges=first-parent",
            "--no-ext-diff",
            "--no-textconv",
        ],
        Options {
            stdin: Some(input.as_bytes()),
            ..Options::default()
        },
    )
    .await?;
    Ok(parsers::parse_shortstats(&result.out()))
}

/// The bisect in progress, if any.
pub async fn bisect_state(repo: &str) -> Result<Option<Bisect>> {
    let log = git_dir(repo).await?.join("BISECT_LOG");
    Ok(std::fs::read_to_string(log)
        .ok()
        .map(|text| parsers::parse_bisect_log(&text)))
}

/// The commit HEAD points at, if there is one yet.
pub async fn head_sha(repo: &str) -> Result<Option<String>> {
    let result = git_unchecked(repo, &["rev-parse", "--verify", "-q", "HEAD"]).await?;
    Ok((result.code == 0).then(|| result.out().trim().to_owned()))
}

pub async fn has_head(repo: &str) -> Result<bool> {
    Ok(
        git_unchecked(repo, &["rev-parse", "--verify", "-q", "HEAD"])
            .await?
            .code
            == 0,
    )
}

/// The id of the empty tree in this repository's hash format.
pub async fn empty_tree(repo: &str) -> Result<String> {
    let options = Options {
        stdin: Some(b""),
        ..Options::default()
    };
    let result = run(repo, &["hash-object", "-t", "tree", "--stdin"], options).await?;
    Ok(result.out().trim().to_owned())
}

fn read_trimmed(path: &Path) -> String {
    std::fs::read(path).map_or_else(
        |_| String::new(),
        |bytes| String::from_utf8_lossy(&bytes).trim().to_owned(),
    )
}

pub async fn operation_state(repo: &str) -> Result<Option<Operation>> {
    let gd = git_dir(repo).await?;
    let merge_dir = gd.join("rebase-merge");
    let apply_dir = gd.join("rebase-apply");
    if merge_dir.is_dir() || apply_dir.is_dir() {
        let state = if merge_dir.is_dir() {
            merge_dir
        } else {
            apply_dir
        };
        if state.join("applying").exists() {
            return Ok(Some(Operation::Am));
        }
        let read = |name: &str| read_trimmed(&state.join(name));
        let first_of = |a: &str, b: &str| {
            let value = read(a);
            if value.is_empty() { read(b) } else { value }
        };
        let number = |text: String| text.parse::<i64>().ok();
        let some = |text: String| (!text.is_empty()).then_some(text);
        let head_name = read("head-name");
        return Ok(Some(Operation::Rebase {
            branch: some(
                head_name
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&head_name)
                    .to_owned(),
            ),
            onto: some(read("onto")),
            step: number(first_of("msgnum", "next")),
            total: number(first_of("end", "last")),
            stopped_sha: some(read("stopped-sha")),
        }));
    }
    let first_line = |path: PathBuf| {
        read_trimmed(&path)
            .split('\n')
            .next()
            .unwrap_or("")
            .to_owned()
    };
    if gd.join("MERGE_HEAD").exists() {
        return Ok(Some(Operation::Merge {
            head: first_line(gd.join("MERGE_HEAD")),
        }));
    }
    if gd.join("CHERRY_PICK_HEAD").exists() {
        return Ok(Some(Operation::CherryPick {
            head: first_line(gd.join("CHERRY_PICK_HEAD")),
        }));
    }
    if gd.join("REVERT_HEAD").exists() {
        return Ok(Some(Operation::Revert {
            head: first_line(gd.join("REVERT_HEAD")),
        }));
    }
    // Between the steps of a multi-commit pick or revert only the sequencer is left.
    let todo = read_trimmed(&gd.join("sequencer/todo"));
    if let Some(step) = todo.split('\n').next().filter(|step| !step.is_empty()) {
        let head = first_line(gd.join("sequencer/head"));
        return Ok(Some(if step.starts_with("revert") {
            Operation::Revert { head }
        } else {
            Operation::CherryPick { head }
        }));
    }
    Ok(None)
}

/// Refs left out of "all branches": the stash, and GitCraft's own backups.
const HIDDEN_REFS: [&str; 2] = ["--exclude=refs/stash", "--exclude=refs/gitcraft/*"];

/// Backups of discarded changes, newest first.
pub async fn discard_backups(repo: &str) -> Result<Vec<DiscardBackup>> {
    crate::backups::list(repo).await
}

pub async fn log(
    repo: &str,
    limit: usize,
    all_refs: bool,
    reference: Option<&str>,
) -> Result<LogPage> {
    log_filtered(
        repo,
        limit,
        all_refs,
        reference,
        &LogFilter::default(),
        &LogOptions::default(),
    )
    .await
}

/// The history, only with the commits `filter` lets through; for all branches, without the
/// ones `view` hides (or only those it solos). Refs that no longer exist are ignored.
pub async fn log_filtered(
    repo: &str,
    limit: usize,
    all_refs: bool,
    reference: Option<&str>,
    filter: &LogFilter,
    options: &LogOptions,
) -> Result<LogPage> {
    let view = &options.view;
    let existing: Vec<String> = if all_refs && reference.is_none() && !view.is_empty() {
        let listed = git(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads/",
                "refs/remotes/",
                "refs/tags/",
            ],
        )
        .await?;
        listed.out().lines().map(str::to_owned).collect()
    } else {
        Vec::new()
    };
    let solo: Vec<&str> = view
        .solo
        .iter()
        .filter(|name| existing.contains(name))
        .map(String::as_str)
        .collect();
    let excludes: Vec<String> = view
        .hidden
        .iter()
        .filter(|name| existing.contains(name))
        .map(|name| format!("--exclude={name}"))
        .collect();
    let max_count = format!("--max-count={}", limit + 1);
    let mut args = vec![
        "log",
        if options.date_order {
            "--date-order"
        } else {
            "--topo-order"
        },
        "--decorate=full",
        LOG_FORMAT,
        &max_count,
    ];
    let author = format!("--author={}", filter.author.trim());
    let text = format!("{}{}", if filter.regex { "-G" } else { "-S" }, filter.text);
    let path = filter.path.trim().trim_start_matches("./");
    if !filter.author.trim().is_empty() {
        args.extend(["--regexp-ignore-case", "--fixed-strings", &author]);
    }
    if !filter.text.is_empty() {
        args.push(&text);
    }
    if !path.is_empty() {
        // Parents as the simplified history has them, so the graph joins up.
        args.push("--parents");
    }
    if let Some(reference) = reference {
        args.push(check_ref(reference)?);
    } else if all_refs && !solo.is_empty() {
        for name in &solo {
            args.push(check_ref(name)?);
        }
    } else if all_refs {
        args.extend(excludes.iter().map(String::as_str));
        args.extend(HIDDEN_REFS);
        args.push("--all");
    }
    if reference.is_none() && !has_head(repo).await? {
        let refs = git(
            repo,
            &[
                "for-each-ref",
                "--count=1",
                "--format=%(refname)",
                "refs/heads/",
                "refs/remotes/",
                "refs/tags/",
            ],
        )
        .await?;
        if refs.out().trim().is_empty() {
            return Ok(LogPage {
                commits: Vec::new(),
                has_more: false,
            });
        }
    }
    args.push("--");
    if !path.is_empty() {
        paths::inside(repo, path)
            .ok_or_else(|| Error::Validation(format!("Not inside the repository: {path}")))?;
        args.push(path);
    }
    let result = git(repo, &args).await?;
    let mut commits = parsers::parse_log(&result.out());
    let has_more = commits.len() > limit;
    commits.truncate(limit);
    if options.stashes && all_refs && reference.is_none() && filter.is_empty() {
        place_stashes(repo, &mut commits).await?;
    }
    let mut layout = Layout::new();
    for commit in &mut commits {
        commit.graph = Some(layout.push(&commit.sha, &commit.parents));
    }
    Ok(LogPage { commits, has_more })
}

/// Puts each stash right above the commit it was made on, newest first, with that commit
/// as its only parent. Stashes on commits beyond the loaded page are left out.
async fn place_stashes(repo: &str, commits: &mut Vec<Commit>) -> Result<()> {
    let listed = git_unchecked(repo, &["stash", "list", LOG_FORMAT]).await?;
    if listed.code != 0 {
        return Ok(());
    }
    for (index, mut stash) in parsers::parse_log(&listed.out()).into_iter().enumerate() {
        let Some(base) = stash.parents.first().cloned() else {
            continue;
        };
        let Some(at) = commits.iter().position(|commit| commit.sha == base) else {
            continue;
        };
        stash.parents = vec![base];
        stash.refs = None;
        stash.stash = Some(format!("stash@{{{index}}}"));
        // Right above the commit: an older stash on the same commit lands below the newer.
        commits.insert(at, stash);
    }
    Ok(())
}

pub async fn commit_detail(repo: &str, sha: &str) -> Result<CommitDetail> {
    let result = git(
        repo,
        &[
            "show",
            "-s",
            "--decorate=full",
            DETAIL_FORMAT,
            check_ref(sha)?,
            "--",
        ],
    )
    .await?;
    let mut detail = parsers::parse_commit_detail(&result.out())
        .ok_or_else(|| Error::Invalid(format!("Could not read commit {sha}")))?;
    detail.files = commit_files(repo, &detail.sha, &detail.parents).await?;
    // Only a signed commit makes git start gpg or ssh-keygen here.
    let signature = git_unchecked(
        repo,
        &["log", "-1", "--format=%G?%x1f%GS%x1f%GK", &detail.sha, "--"],
    )
    .await?;
    if signature.code == 0 {
        detail.signature = parsers::parse_signature(&signature.out());
    }
    Ok(detail)
}

async fn base_or_empty(repo: &str, base: Option<&String>) -> Result<String> {
    match base {
        Some(base) => Ok(base.clone()),
        None => empty_tree(repo).await,
    }
}

pub async fn commit_files(repo: &str, sha: &str, parents: &[String]) -> Result<Vec<FileChange>> {
    let base = base_or_empty(repo, parents.first()).await?;
    let result = git(
        repo,
        &["diff", "--name-status", "-z", "-M", &base, sha, "--"],
    )
    .await?;
    Ok(parsers::parse_name_status(&result.out()))
}

fn diff_flags(context: i64, ignore_whitespace: bool) -> Vec<String> {
    let mut flags = vec![
        format!("-U{}", context.clamp(0, 1000)),
        "-M".to_owned(),
        "--no-ext-diff".to_owned(),
    ];
    if ignore_whitespace {
        flags.push("-w".to_owned());
    }
    flags
}

async fn diff_one(repo: &str, args: &[&str]) -> Result<Option<FileDiff>> {
    let result = git(repo, args).await?;
    Ok(parsers::parse_diff(&result.stdout).into_iter().next())
}

pub async fn commit_diff(
    repo: &str,
    sha: &str,
    path: &str,
    orig_path: Option<&str>,
    context: i64,
    ignore_whitespace: bool,
) -> Result<Option<FileDiff>> {
    let detail = git(repo, &["rev-list", "--parents", "-n", "1", check_ref(sha)?]).await?;
    let text = detail.out();
    let parts: Vec<&str> = text.split_whitespace().collect();
    let base = base_or_empty(repo, parts.get(1).map(|p| (*p).to_owned()).as_ref()).await?;
    let flags = diff_flags(context, ignore_whitespace);
    let mut args = vec!["diff"];
    args.extend(flags.iter().map(String::as_str));
    args.extend([base.as_str(), parts.first().copied().unwrap_or(""), "--"]);
    args.extend(orig_path.filter(|p| !p.is_empty()));
    args.push(path);
    diff_one(repo, &args).await
}

pub async fn range_diff(
    repo: &str,
    base: &str,
    target: &str,
    path: &str,
    context: i64,
) -> Result<Option<FileDiff>> {
    let flags = diff_flags(context, false);
    let mut args = vec!["diff"];
    args.extend(flags.iter().map(String::as_str));
    if target == WORKTREE || base == WORKTREE {
        let commit = check_ref(if target == WORKTREE { base } else { target })?;
        if base == WORKTREE {
            args.push("-R");
        }
        args.extend([commit, "--", path]);
        let diff = diff_one(repo, &args).await?;
        if diff.is_none() && target == WORKTREE {
            // A new file git doesn't track yet.
            return working_diff(repo, path, false, context, false, None).await;
        }
        return Ok(diff);
    }
    args.extend([check_ref(base)?, check_ref(target)?, "--", path]);
    diff_one(repo, &args).await
}

async fn compare_worktree(repo: &str, base: &str, target: &str) -> Result<Comparison> {
    let reverse = base == WORKTREE;
    let commit = check_ref(if reverse { target } else { base })?;
    let mut args = vec!["diff", "--name-status", "-z", "-M"];
    if reverse {
        args.push("-R");
    }
    args.extend([commit, "--"]);
    let mut files = parsers::parse_name_status(&git(repo, &args).await?.out());
    if !reverse {
        let status = status(repo).await?;
        files.extend(
            status
                .unstaged
                .into_iter()
                .filter(|entry| entry.status == "?")
                .map(|entry| FileChange {
                    status: "A".to_owned(),
                    ..entry
                }),
        );
    }
    // The working tree sits on HEAD: count the commits between it and the commit.
    let range = format!("{commit}...HEAD");
    let counts = git_unchecked(repo, &["rev-list", "--left-right", "--count", &range]).await?;
    let text = counts.out();
    let mut numbers = text.split_whitespace().map(|n| n.parse().unwrap_or(0));
    let (commit_only, head_only) = (numbers.next().unwrap_or(0), numbers.next().unwrap_or(0));
    let (base_only, target_only) = if reverse {
        (head_only, commit_only)
    } else {
        (commit_only, head_only)
    };
    Ok(Comparison {
        files,
        base_only,
        target_only,
    })
}

pub async fn status(repo: &str) -> Result<Status> {
    let result = git(
        repo,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
    )
    .await?;
    let mut data = parsers::parse_status(&result.out());
    data.operation = operation_state(repo).await?;
    Ok(data)
}

pub async fn working_diff(
    repo: &str,
    path: &str,
    staged: bool,
    context: i64,
    ignore_whitespace: bool,
    orig_path: Option<&str>,
) -> Result<Option<FileDiff>> {
    if !staged {
        let tracked = git_unchecked(repo, &["ls-files", "--error-unmatch", "--", path]).await?;
        if tracked.code != 0 {
            let file = Path::new(repo).join(path);
            if file.is_file() {
                return Ok(Some(parsers::untracked_diff(path, &std::fs::read(file)?)));
            }
            return Ok(None);
        }
    }
    let flags = diff_flags(context, ignore_whitespace);
    let mut args = vec!["diff"];
    args.extend(flags.iter().map(String::as_str));
    if staged {
        args.push("--cached");
    }
    args.push("--");
    args.extend(orig_path.filter(|p| !p.is_empty()));
    args.push(path);
    diff_one(repo, &args).await
}

pub async fn refs(repo: &str) -> Result<Refs> {
    let (remotes_out, refs_out, stash_out, sub_out, tree_out, top) = tokio::join!(
        git(repo, &["remote", "-v"]),
        git(
            repo,
            &[
                "for-each-ref",
                REF_FORMAT,
                "refs/heads",
                "refs/remotes",
                "refs/tags"
            ]
        ),
        git_unchecked(repo, &["stash", "list", STASH_FORMAT]),
        git_unchecked(repo, &["submodule", "status"]),
        git_unchecked(repo, &["worktree", "list", "--porcelain"]),
        toplevel(repo),
    );
    let (remotes_out, refs_out, stash_out, sub_out, tree_out) =
        (remotes_out?, refs_out?, stash_out?, sub_out?, tree_out?);
    let remotes = parsers::parse_remotes(&remotes_out.out());
    let names: Vec<String> = remotes.iter().map(|remote| remote.name.clone()).collect();
    let mut data = parsers::parse_refs(&refs_out.out(), &names);
    data.remotes = remotes;
    data.stashes = parsers::parse_stashes(&stash_out.out());
    data.submodules = parsers::parse_submodules(&sub_out.out());
    data.worktrees = parsers::parse_worktrees(&tree_out.out());
    if let Ok(top) = top {
        for tree in &mut data.worktrees {
            tree.current = tree.path == top;
        }
    }
    let head = git_unchecked(repo, &["rev-parse", "HEAD"]).await?;
    data.head = (head.code == 0).then(|| head.out().trim().to_owned());
    Ok(data)
}

pub async fn stash_files(repo: &str, name: &str) -> Result<Vec<FileChange>> {
    let name = check_ref(name)?;
    let result = git(repo, &["stash", "show", "--name-status", "-z", "-M", name]).await?;
    let mut files = parsers::parse_name_status(&result.out());
    let third = format!("{name}^3");
    if git_unchecked(repo, &["rev-parse", "-q", "--verify", &third])
        .await?
        .code
        == 0
    {
        let listing = git(repo, &["ls-tree", "-r", "-z", "--name-only", &third]).await?;
        files.extend(
            listing
                .out()
                .split('\0')
                .filter(|path| !path.is_empty())
                .map(|path| FileChange {
                    status: "?".to_owned(),
                    orig_path: None,
                    path: path.to_owned(),
                }),
        );
    }
    Ok(files)
}

pub async fn stash_diff(
    repo: &str,
    name: &str,
    path: &str,
    context: i64,
) -> Result<Option<FileDiff>> {
    let name = check_ref(name)?;
    let diff = range_diff(repo, &format!("{name}^1"), name, path, context).await?;
    if diff.is_some() {
        return Ok(diff);
    }
    let third = format!("{name}^3");
    if git_unchecked(repo, &["rev-parse", "-q", "--verify", &third])
        .await?
        .code
        == 0
    {
        return range_diff(repo, &empty_tree(repo).await?, &third, path, context).await;
    }
    Ok(None)
}

pub async fn search(repo: &str, query: &str, mode: &str, limit: usize) -> Result<Vec<Commit>> {
    if mode == "sha" {
        let spec = format!("{query}^{{commit}}");
        let resolved = git_unchecked(repo, &["rev-parse", "--verify", "-q", &spec]).await?;
        if resolved.code != 0 {
            return Ok(Vec::new());
        }
        let sha = resolved.out().trim().to_owned();
        let result = git(
            repo,
            &["show", "-s", "--decorate=full", LOG_FORMAT, &sha, "--"],
        )
        .await?;
        return Ok(parsers::parse_log(&result.out()));
    }
    let max_count = format!("--max-count={limit}");
    let mut args = vec!["log", "--decorate=full", LOG_FORMAT, &max_count];
    args.extend(HIDDEN_REFS);
    args.push("--all");
    let filter;
    match mode {
        "author" => {
            filter = format!("--author={query}");
            args.extend(["-i", &filter]);
        }
        "content" => {
            filter = format!("-S{query}");
            args.push(&filter);
        }
        _ => {
            filter = format!("--grep={query}");
            args.extend(["-i", "--fixed-strings", &filter]);
        }
    }
    args.push("--");
    let result = git(repo, &args).await?;
    Ok(parsers::parse_log(&result.out()))
}

pub async fn file_history(repo: &str, path: &str, limit: usize) -> Result<Vec<Commit>> {
    let max_count = format!("--max-count={limit}");
    let args = [
        "log",
        "--follow",
        "--decorate=full",
        LOG_FORMAT,
        "--name-status",
        "-z",
        &max_count,
        "--",
        path,
    ];
    let result = git(repo, &args).await?;
    let out = result.out();
    let mut commits = Vec::new();
    for record in out.split(RS).skip(1) {
        let (meta, files) = record.split_once('\0').unwrap_or((record, ""));
        let Some(mut entry) = parsers::parse_log(&format!("{RS}{meta}"))
            .into_iter()
            .next()
        else {
            continue;
        };
        let changed = parsers::parse_name_status(files.trim_start_matches('\n'));
        entry.file = Some(changed.into_iter().next().unwrap_or_else(|| FileChange {
            status: "M".to_owned(),
            orig_path: None,
            path: path.to_owned(),
        }));
        commits.push(entry);
    }
    Ok(commits)
}

pub async fn blame(repo: &str, path: &str, rev: Option<&str>) -> Result<Vec<BlameLine>> {
    Ok(blame_with(repo, path, rev, false).await?.0)
}

/// The list of commits blame can skip (formatting changes and the like), by convention.
pub const BLAME_IGNORE_FILE: &str = ".git-blame-ignore-revs";

/// Whether the repository has a [`BLAME_IGNORE_FILE`].
pub async fn has_blame_ignore_file(repo: &str) -> Result<bool> {
    Ok(Path::new(&toplevel(repo).await?)
        .join(BLAME_IGNORE_FILE)
        .is_file())
}

/// Blame, skipping the commits in [`BLAME_IGNORE_FILE`] when `ignore_revs` is set and the
/// file is there. The flag returned says whether they were skipped: a file git can't read
/// falls back to plain blame rather than failing.
pub async fn blame_with(
    repo: &str,
    path: &str,
    rev: Option<&str>,
    ignore_revs: bool,
) -> Result<(Vec<BlameLine>, bool)> {
    let mut args = vec!["blame", "--porcelain"];
    if let Some(rev) = rev {
        args.push(check_ref(rev)?);
    }
    args.extend(["--", path]);
    if ignore_revs && has_blame_ignore_file(repo).await? {
        let file = Path::new(&toplevel(repo).await?).join(BLAME_IGNORE_FILE);
        let flag = format!("--ignore-revs-file={}", file.to_string_lossy());
        let mut ignoring = vec!["blame", "--porcelain", &flag];
        ignoring.extend(&args[2..]);
        let result = git_unchecked(repo, &ignoring).await?;
        if result.code == 0 {
            return Ok((parsers::parse_blame(&result.out()), true));
        }
    }
    let result = git(repo, &args).await?;
    Ok((parsers::parse_blame(&result.out()), false))
}

/// The diff tools (`merge`: the merge tools) git can start here: installed ones that open a
/// window, and those set up in git's config. `cwd` may be any folder.
pub async fn external_tools(cwd: &str, merge: bool) -> Result<Vec<ExternalTool>> {
    let command = if merge { "mergetool" } else { "difftool" };
    let result = git_unchecked(cwd, &[command, "--tool-help"]).await?;
    Ok(parsers::parse_tool_help(&result.out()))
}

pub async fn head_message(repo: &str) -> Result<String> {
    let result = git_unchecked(repo, &["log", "-1", "--format=%B"]).await?;
    Ok(if result.code == 0 {
        result.out().trim().to_owned()
    } else {
        String::new()
    })
}

pub async fn commit_template(repo: &str) -> Result<String> {
    let result = git_unchecked(repo, &["config", "--get", "commit.template"]).await?;
    if result.code != 0 {
        return Ok(String::new());
    }
    let configured = result.out().trim().to_owned();
    if configured.is_empty() {
        return Ok(String::new());
    }
    let path = match configured.strip_prefix("~/") {
        Some(rest) => crate::store::home_dir().join(rest),
        None => PathBuf::from(&configured),
    };
    let path = if path.is_absolute() {
        path
    } else {
        Path::new(repo).join(path)
    };
    Ok(std::fs::read(path).map_or_else(
        |_| String::new(),
        |bytes| String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

pub async fn rebase_candidates(repo: &str, base: Option<&str>) -> Result<Vec<Commit>> {
    let range = match base {
        Some(base) => format!("{}..HEAD", check_ref(base)?),
        None => "HEAD".to_owned(),
    };
    let result = git(
        repo,
        &[
            "log",
            "--reverse",
            "--topo-order",
            "--no-merges",
            LOG_FORMAT,
            &range,
            "--",
        ],
    )
    .await?;
    let mut commits = parsers::parse_log(&result.out());
    for commit in &mut commits {
        commit.refs = None;
    }
    Ok(commits)
}

/// A blob by `git show` spec, or nothing if it doesn't exist.
/// A file's text as it is in `rev`, exactly as stored (no filters), for copying.
pub async fn file_text(repo: &str, rev: &str, path: &str) -> Result<String> {
    const LIMIT: usize = 10 * 1024 * 1024;
    let spec = format!("{}:{path}", check_ref(rev)?);
    let result = git_unchecked(repo, &["cat-file", "blob", &spec]).await?;
    if result.code != 0 {
        return Err(Error::Invalid(format!("{path} is not in that commit")));
    }
    if result.stdout.len() > LIMIT {
        return Err(Error::Invalid("The file is too large to copy".into()));
    }
    if result.stdout.contains(&0) {
        return Err(Error::Invalid(
            "Binary files can't be copied as text".into(),
        ));
    }
    Ok(result.out().into_owned())
}

pub async fn raw_show(repo: &str, spec: &str) -> Result<Option<String>> {
    let result = git_unchecked(repo, &["show", spec]).await?;
    Ok((result.code == 0).then(|| result.out().into_owned()))
}

pub async fn reflog(repo: &str, reference: &str, limit: usize) -> Result<Vec<ReflogEntry>> {
    let count = format!("-n{limit}");
    let args = [
        "reflog",
        "show",
        REFLOG_FORMAT,
        &count,
        check_ref(reference)?,
        "--",
    ];
    let result = git_unchecked(repo, &args).await?;
    Ok(parsers::parse_reflog(&result.out()))
}

/// The reflogs of HEAD and every branch, newest first: also where a branch was before
/// it was deleted, reset or rebased while another one was checked out.
pub async fn reflog_all(repo: &str, limit: usize) -> Result<Vec<ReflogEntry>> {
    let count = format!("-n{limit}");
    let args = ["reflog", "show", REFLOG_FORMAT, &count, "--all", "--"];
    let result = git_unchecked(repo, &args).await?;
    Ok(parsers::parse_reflog(&result.out()))
}

/// Files changed between two commits (base to target), plus commit counts each way.
/// The files a merge of `target` into HEAD would leave conflicted, worked out without
/// touching the working tree or the index (`git merge-tree --write-tree`). An empty list
/// means it would merge cleanly; `None` that git can't tell (it is older than 2.38, or the
/// two have no common history).
pub async fn conflict_check(repo: &str, target: &str) -> Result<Option<Vec<String>>> {
    let target = check_ref(target)?;
    let result = git_unchecked(
        repo,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            "-z",
            "HEAD",
            target,
        ],
    )
    .await?;
    match result.code {
        0 => Ok(Some(Vec::new())),
        1 => {
            let out = result.out();
            // The tree written first, then one name per conflicted file.
            let files = out
                .split('\0')
                .skip(1)
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .collect::<std::collections::BTreeSet<_>>();
            Ok(Some(files.into_iter().collect()))
        }
        _ => Ok(None),
    }
}

/// How two versions of a series of commits differ (`git range-diff old new`), for example a
/// branch before and after a rebase: which commits match, which changed and how.
/// Each range is `A..B`, or `A...B` for two branches from where they part.
pub async fn range_diff_ranges(repo: &str, old: &str, new: &str) -> Result<String> {
    const LIMIT: usize = 2 * 1024 * 1024;
    let (old, new) = (check_ref(old.trim())?, check_ref(new.trim())?);
    if old.is_empty() || new.is_empty() {
        return Err(Error::Validation(
            "Give both ranges, such as main..topic".into(),
        ));
    }
    let result = git_unchecked(
        repo,
        &["range-diff", "--no-color", "--no-ext-diff", old, new],
    )
    .await?;
    if result.code != 0 {
        return Err(Error::Invalid(result.output()));
    }
    let mut text = result.out().into_owned();
    if text.len() > LIMIT {
        let mut cut = LIMIT;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n…");
    }
    Ok(text)
}

/// Stands for the working tree (with what is staged) as either side of [`compare`].
pub const WORKTREE: &str = "";

/// Files changed between two commits (base to target), plus commit counts each way. Either
/// side may be [`WORKTREE`]; new untracked files then count as added.
pub async fn compare(repo: &str, base: &str, target: &str) -> Result<Comparison> {
    if base == WORKTREE || target == WORKTREE {
        return compare_worktree(repo, base, target).await;
    }
    let (base, target) = (check_ref(base)?, check_ref(target)?);
    let files = git(
        repo,
        &["diff", "--name-status", "-z", "-M", base, target, "--"],
    )
    .await?;
    let range = format!("{base}...{target}");
    let counts = git(repo, &["rev-list", "--left-right", "--count", &range]).await?;
    let text = counts.out();
    let mut numbers = text.split_whitespace().map(|n| n.parse().unwrap_or(0));
    Ok(Comparison {
        files: parsers::parse_name_status(&files.out()),
        base_only: numbers.next().unwrap_or(0),
        target_only: numbers.next().unwrap_or(0),
    })
}
