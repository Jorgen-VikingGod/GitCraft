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

pub async fn log(
    repo: &str,
    limit: usize,
    all_refs: bool,
    reference: Option<&str>,
) -> Result<LogPage> {
    let max_count = format!("--max-count={}", limit + 1);
    let mut args = vec![
        "log",
        "--topo-order",
        "--decorate=full",
        LOG_FORMAT,
        &max_count,
    ];
    if let Some(reference) = reference {
        args.push(check_ref(reference)?);
    } else if all_refs {
        args.extend(["--exclude=refs/stash", "--all"]);
    }
    if reference.is_none() && !has_head(repo).await? {
        let refs = git(repo, &["for-each-ref", "--count=1", "--format=%(refname)"]).await?;
        if refs.out().trim().is_empty() {
            return Ok(LogPage {
                commits: Vec::new(),
                has_more: false,
            });
        }
    }
    args.push("--");
    let result = git(repo, &args).await?;
    let mut commits = parsers::parse_log(&result.out());
    let has_more = commits.len() > limit;
    commits.truncate(limit);
    let mut layout = Layout::new();
    for commit in &mut commits {
        commit.graph = Some(layout.push(&commit.sha, &commit.parents));
    }
    Ok(LogPage { commits, has_more })
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
    args.extend([check_ref(base)?, check_ref(target)?, "--", path]);
    diff_one(repo, &args).await
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
    let (remotes_out, refs_out, stash_out, sub_out) = tokio::join!(
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
    );
    let (remotes_out, refs_out, stash_out, sub_out) =
        (remotes_out?, refs_out?, stash_out?, sub_out?);
    let remotes = parsers::parse_remotes(&remotes_out.out());
    let names: Vec<String> = remotes.iter().map(|remote| remote.name.clone()).collect();
    let mut data = parsers::parse_refs(&refs_out.out(), &names);
    data.remotes = remotes;
    data.stashes = parsers::parse_stashes(&stash_out.out());
    data.submodules = parsers::parse_submodules(&sub_out.out());
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
    let mut args = vec![
        "log",
        "--decorate=full",
        LOG_FORMAT,
        &max_count,
        "--exclude=refs/stash",
        "--all",
    ];
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
    let mut args = vec!["blame", "--porcelain"];
    if let Some(rev) = rev {
        args.push(check_ref(rev)?);
    }
    args.extend(["--", path]);
    let result = git(repo, &args).await?;
    Ok(parsers::parse_blame(&result.out()))
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

/// Files changed between two commits (base to target), plus commit counts each way.
pub async fn compare(repo: &str, base: &str, target: &str) -> Result<Comparison> {
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
