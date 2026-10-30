//! Mutating git operations. Every public function that changes a repository holds its lock.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use crate::model::*;
use crate::parsers::{self, Selection};
use crate::runner::{
    GitResult, Options, Progress, editor_command, git, git_unchecked, repo_lock, run, run_unchecked,
};
use crate::{Error, GitError, Result, check_ref, editor, paths, queries};

fn ok(result: &GitResult) -> OpResult {
    OpResult {
        ok: true,
        conflicts: false,
        message: result.output(),
        operation: None,
        files: None,
    }
}

fn ok_empty() -> OpResult {
    OpResult {
        ok: true,
        conflicts: false,
        message: String::new(),
        operation: None,
        files: None,
    }
}

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(message.into()))
}

fn with_progress<'a>(progress: Option<&Progress>) -> Options<'a> {
    Options {
        progress: progress.cloned(),
        ..Options::default()
    }
}

/// Runs an operation that can stop half-way (conflicts, `edit` steps).
async fn may_conflict(repo: &str, args: &[&str], options: Options<'_>) -> Result<OpResult> {
    let result = run_unchecked(repo, args, options).await?;
    if result.code == 0 {
        cleanup_rebase_messages(repo).await?;
        return Ok(ok(&result));
    }
    let Some(state) = queries::operation_state(repo).await? else {
        return Err(GitError::new(args, result.code, &result.err, &result.out()).into());
    };
    let status = queries::status(repo).await?;
    Ok(OpResult {
        ok: false,
        conflicts: !status.conflicts.is_empty(),
        message: result.output(),
        operation: Some(state),
        files: None,
    })
}

async fn cleanup_rebase_messages(repo: &str) -> Result<()> {
    let messages = queries::git_dir(repo).await?.join(editor::MESSAGES_FILE);
    if messages.exists() && queries::operation_state(repo).await?.is_none() {
        std::fs::remove_file(messages)?;
    }
    Ok(())
}

fn safe_path(repo: &str, path: &str) -> Result<PathBuf> {
    paths::inside(repo, path)
        .ok_or_else(|| Error::Invalid(format!("Path escapes repository: {path}")))
}

// ------------------------------------------------------------------ staging

fn with_paths<'a>(mut args: Vec<&'a str>, list: &'a [String]) -> Vec<&'a str> {
    args.extend(list.iter().map(String::as_str));
    args
}

pub async fn stage_files(repo: &str, list: &[String]) -> Result<()> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    git(repo, &with_paths(vec!["add", "-A", "--"], list)).await?;
    Ok(())
}

pub async fn stage_all(repo: &str) -> Result<()> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    git(repo, &["add", "-A"]).await?;
    Ok(())
}

pub async fn unstage_files(repo: &str, list: &[String]) -> Result<()> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let prefix = if queries::has_head(repo).await? {
        vec!["reset", "-q", "HEAD", "--"]
    } else {
        vec!["rm", "-q", "--cached", "-r", "--"]
    };
    git(repo, &with_paths(prefix, list)).await?;
    Ok(())
}

pub async fn unstage_all(repo: &str) -> Result<()> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    if queries::has_head(repo).await? {
        git(repo, &["reset", "-q", "HEAD"]).await?;
    } else {
        git_unchecked(repo, &["rm", "-q", "--cached", "-r", "-f", "."]).await?;
    }
    Ok(())
}

pub async fn discard_files(repo: &str, list: &[String]) -> Result<()> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let status = queries::status(repo).await?;
    let untracked: HashSet<&str> = status
        .unstaged
        .iter()
        .filter(|entry| entry.status == "?")
        .map(|entry| entry.path.as_str())
        .collect();
    let tracked: Vec<String> = list
        .iter()
        .filter(|path| !untracked.contains(path.as_str()))
        .cloned()
        .collect();
    for path in list.iter().filter(|path| untracked.contains(path.as_str())) {
        let target = safe_path(repo, path)?;
        if target.symlink_metadata().is_ok_and(|meta| !meta.is_dir()) {
            std::fs::remove_file(target)?;
        }
    }
    if !tracked.is_empty() {
        git(repo, &with_paths(vec!["checkout", "--"], &tracked)).await?;
    }
    Ok(())
}

/// Stages, unstages or discards selected hunks/lines of one file.
///
/// mode: "stage" (index += worktree changes), "unstage" (index -= staged
/// changes) or "discard" (worktree -= unstaged changes).
pub async fn apply_selection(
    repo: &str,
    path: &str,
    mode: &str,
    selection: &Selection,
    context: i64,
) -> Result<()> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let staged = mode == "unstage";
    let Some(file_diff) = queries::working_diff(repo, path, staged, context, false, None).await?
    else {
        return invalid("No changes to apply for this file");
    };
    if file_diff.hunks.is_empty() {
        return invalid("No changes to apply for this file");
    }
    if file_diff.status == "?" {
        return invalid("Untracked files can only be staged as a whole");
    }
    if selection.keys().any(|hunk| *hunk >= file_diff.hunks.len()) {
        return invalid("The diff changed, please refresh");
    }
    let reverse = mode != "stage";
    let patch = parsers::build_partial_patch(&file_diff, selection, reverse);
    let mut args = vec!["apply", "--recount", "--whitespace=nowarn"];
    if context <= 0 {
        args.push("--unidiff-zero");
    }
    if mode != "discard" {
        args.push("--cached");
    }
    if reverse {
        args.push("-R");
    }
    args.push("-");
    run(
        repo,
        &args,
        Options {
            stdin: Some(&patch),
            ..Options::default()
        },
    )
    .await?;
    Ok(())
}

// ------------------------------------------------------------------- commit

pub async fn commit(repo: &str, message: &str, amend: bool, sign_off: bool) -> Result<OpResult> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let mut args = vec!["commit", "-F", "-", "--cleanup=strip"];
    if amend {
        args.push("--amend");
    }
    if sign_off {
        args.push("--signoff");
    }
    let result = run(
        repo,
        &args,
        Options {
            stdin: Some(message.as_bytes()),
            ..Options::default()
        },
    )
    .await?;
    Ok(ok(&result))
}

// --------------------------------------------------------------- branches

pub async fn checkout(repo: &str, reference: &str, kind: &str) -> Result<OpResult> {
    check_ref(reference)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let result = match kind {
        "remote" => {
            let local = reference
                .split_once('/')
                .map_or(reference, |(_, branch)| branch);
            let spec = format!("refs/heads/{local}");
            if git_unchecked(repo, &["rev-parse", "--verify", "-q", &spec])
                .await?
                .code
                == 0
            {
                git(repo, &["switch", local]).await?
            } else {
                git(repo, &["switch", "--track", "-c", local, reference]).await?
            }
        }
        "commit" => git(repo, &["switch", "--detach", reference]).await?,
        _ => git(repo, &["switch", reference]).await?,
    };
    Ok(ok(&result))
}

pub async fn create_branch(
    repo: &str,
    name: &str,
    start: Option<&str>,
    checkout_after: bool,
) -> Result<OpResult> {
    check_ref(name)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let mut args = if checkout_after {
        vec!["switch", "-c", name]
    } else {
        vec!["branch", name]
    };
    if let Some(start) = start {
        args.push(check_ref(start)?);
    }
    Ok(ok(&git(repo, &args).await?))
}

pub async fn rename_branch(repo: &str, old: &str, new: &str) -> Result<OpResult> {
    let (old, new) = (check_ref(old)?, check_ref(new)?);
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    Ok(ok(&git(repo, &["branch", "-m", old, new]).await?))
}

pub async fn delete_branch(repo: &str, name: &str, force: bool) -> Result<OpResult> {
    let name = check_ref(name)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    Ok(ok(&git(
        repo,
        &["branch", if force { "-D" } else { "-d" }, name],
    )
    .await?))
}

pub async fn delete_remote_branch(
    repo: &str,
    remote: &str,
    branch: &str,
    progress: Option<&Progress>,
) -> Result<OpResult> {
    let (remote, branch) = (check_ref(remote)?, check_ref(branch)?);
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    Ok(ok(&run(
        repo,
        &["push", "--progress", remote, "--delete", branch],
        with_progress(progress),
    )
    .await?))
}

pub async fn set_upstream(repo: &str, branch: &str, upstream: Option<&str>) -> Result<OpResult> {
    let branch = check_ref(branch)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let result = match upstream {
        Some(upstream) => {
            git(
                repo,
                &["branch", &format!("--set-upstream-to={upstream}"), branch],
            )
            .await?
        }
        None => git(repo, &["branch", "--unset-upstream", branch]).await?,
    };
    Ok(ok(&result))
}

// -------------------------------------------------------------- tags/remotes

pub async fn create_tag(
    repo: &str,
    name: &str,
    target: &str,
    message: Option<&str>,
) -> Result<OpResult> {
    let (name, target) = (check_ref(name)?, check_ref(target)?);
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let result = match message.filter(|message| !message.is_empty()) {
        Some(message) => {
            let options = Options {
                stdin: Some(message.as_bytes()),
                ..Options::default()
            };
            run(repo, &["tag", "-a", name, target, "-F", "-"], options).await?
        }
        None => git(repo, &["tag", name, target]).await?,
    };
    Ok(ok(&result))
}

pub async fn delete_tag(repo: &str, name: &str) -> Result<OpResult> {
    let name = check_ref(name)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    Ok(ok(&git(repo, &["tag", "-d", name]).await?))
}

pub async fn push_tag(
    repo: &str,
    remote: &str,
    name: &str,
    delete: bool,
    progress: Option<&Progress>,
) -> Result<OpResult> {
    let remote = check_ref(remote)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let spec = format!("refs/tags/{name}");
    let mut args = vec!["push", "--progress", remote];
    if delete {
        args.push("--delete");
    }
    args.push(&spec);
    Ok(ok(&run(repo, &args, with_progress(progress)).await?))
}

pub async fn add_remote(repo: &str, name: &str, url: &str) -> Result<OpResult> {
    let (name, url) = (check_ref(name)?, check_ref(url)?);
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    Ok(ok(&git(repo, &["remote", "add", name, url]).await?))
}

pub async fn remove_remote(repo: &str, name: &str) -> Result<OpResult> {
    let name = check_ref(name)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    Ok(ok(&git(repo, &["remote", "remove", name]).await?))
}

pub async fn edit_remote(
    repo: &str,
    name: &str,
    new_name: Option<&str>,
    url: Option<&str>,
) -> Result<OpResult> {
    let name = check_ref(name)?;
    let url = url.filter(|url| !url.is_empty());
    // Remote URLs are shown with their password masked; saving that back would destroy the real one.
    if url.is_some_and(|url| url.contains(":***@") || url.contains("://***@")) {
        return invalid(
            "The URL still contains the hidden password. Type the full URL, including the password.",
        );
    }
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    if let Some(url) = url {
        git(repo, &["remote", "set-url", name, check_ref(url)?]).await?;
    }
    if let Some(new_name) = new_name.filter(|new_name| !new_name.is_empty() && *new_name != name) {
        git(repo, &["remote", "rename", name, check_ref(new_name)?]).await?;
    }
    Ok(ok_empty())
}

// ------------------------------------------------------------ remote ops

pub async fn fetch(
    repo: &str,
    remote: Option<&str>,
    prune: bool,
    tags: bool,
    progress: Option<&Progress>,
) -> Result<OpResult> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let mut args = vec!["fetch", "--progress"];
    if prune {
        args.push("--prune");
    }
    if tags {
        args.push("--tags");
    }
    match remote {
        Some(remote) => args.push(check_ref(remote)?),
        None => args.push("--all"),
    }
    Ok(ok(&run(repo, &args, with_progress(progress)).await?))
}

/// Pulls with mode merge, rebase or ff-only; autostash stashes local changes and reapplies them.
pub async fn pull(
    repo: &str,
    remote: Option<&str>,
    branch: Option<&str>,
    mode: &str,
    autostash: bool,
    progress: Option<&Progress>,
) -> Result<OpResult> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let mut args = vec!["pull", "--progress", "--no-edit"];
    args.push(match mode {
        "rebase" => "--rebase",
        "ff-only" => "--ff-only",
        _ => "--no-rebase",
    });
    args.push(if autostash {
        "--autostash"
    } else {
        "--no-autostash"
    });
    if let Some(remote) = remote {
        args.push(check_ref(remote)?);
        if let Some(branch) = branch {
            args.push(check_ref(branch)?);
        }
    }
    may_conflict(repo, &args, with_progress(progress)).await
}

pub async fn push(
    repo: &str,
    remote: &str,
    branch: Option<&str>,
    set_upstream: bool,
    force: bool,
    tags: bool,
    progress: Option<&Progress>,
) -> Result<OpResult> {
    let remote = check_ref(remote)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let mut args = vec!["push", "--progress"];
    if set_upstream {
        args.push("--set-upstream");
    }
    if force {
        args.push("--force-with-lease");
    }
    args.push(remote);
    let spec = branch.map(|branch| format!("refs/heads/{branch}:refs/heads/{branch}"));
    args.extend(spec.as_deref());
    let result = run(repo, &args, with_progress(progress)).await?;
    if tags {
        run(
            repo,
            &["push", "--progress", "--tags", remote],
            with_progress(progress),
        )
        .await?;
    }
    Ok(ok(&result))
}

pub async fn clone(
    parent: &str,
    url: &str,
    name: Option<&str>,
    progress: Option<&Progress>,
) -> Result<String> {
    check_ref(url)?;
    std::fs::create_dir_all(parent)?;
    let last = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let last = last.rsplit(':').next().unwrap_or("");
    let last = last.strip_suffix(".git").unwrap_or(last);
    let target = name
        .filter(|name| !name.is_empty())
        .unwrap_or(if last.is_empty() { "repository" } else { last });
    let destination = PathBuf::from(parent).join(target);
    if destination.exists() && std::fs::read_dir(&destination)?.next().is_some() {
        return invalid(format!(
            "Destination already exists and is not empty: {}",
            destination.display()
        ));
    }
    let args = [
        "clone",
        "--progress",
        "--recurse-submodules",
        url,
        &destination.to_string_lossy(),
    ];
    run(parent, &args, with_progress(progress)).await?;
    Ok(paths::resolve_str(destination))
}

pub async fn init(path: &str) -> Result<String> {
    std::fs::create_dir_all(path)?;
    git(path, &["init"]).await?;
    queries::toplevel(path).await
}

// -------------------------------------------------- merge / rebase / reset

pub async fn merge(repo: &str, reference: &str, mode: &str) -> Result<OpResult> {
    let reference = check_ref(reference)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let mut args = vec!["merge", "--no-edit"];
    match mode {
        "no-ff" => args.push("--no-ff"),
        "ff-only" => args.push("--ff-only"),
        "squash" => args.push("--squash"),
        _ => {}
    }
    args.push(reference);
    may_conflict(repo, &args, Options::default()).await
}

pub async fn rebase(repo: &str, onto: &str, autostash: bool) -> Result<OpResult> {
    let onto = check_ref(onto)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let stash = if autostash {
        "--autostash"
    } else {
        "--no-autostash"
    };
    may_conflict(repo, &["rebase", stash, onto], Options::default()).await
}

pub async fn cherry_pick(repo: &str, shas: &[String]) -> Result<OpResult> {
    for sha in shas {
        check_ref(sha)?;
    }
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    may_conflict(
        repo,
        &with_paths(vec!["cherry-pick"], shas),
        Options::default(),
    )
    .await
}

pub async fn revert(repo: &str, sha: &str, mainline: Option<i64>) -> Result<OpResult> {
    let sha = check_ref(sha)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let mut args = vec!["revert", "--no-edit"];
    let parent = mainline
        .filter(|parent| *parent != 0)
        .map(|parent| parent.to_string());
    if let Some(parent) = &parent {
        args.extend(["-m", parent]);
    }
    args.push(sha);
    may_conflict(repo, &args, Options::default()).await
}

pub async fn reset(repo: &str, target: &str, mode: &str) -> Result<OpResult> {
    if !matches!(mode, "soft" | "mixed" | "hard" | "keep") {
        return invalid(format!("Unknown reset mode {mode}"));
    }
    let target = check_ref(target)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    Ok(ok(
        &git(repo, &["reset", &format!("--{mode}"), target]).await?
    ))
}

pub async fn continue_operation(repo: &str) -> Result<OpResult> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let Some(state) = queries::operation_state(repo).await? else {
        return invalid("No operation in progress");
    };
    if matches!(state, Operation::Merge { .. }) {
        return may_conflict(repo, &["commit", "--no-edit"], Options::default()).await;
    }
    may_conflict(repo, &[state.command(), "--continue"], Options::default()).await
}

pub async fn abort_operation(repo: &str) -> Result<OpResult> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let Some(state) = queries::operation_state(repo).await? else {
        return invalid("No operation in progress");
    };
    let result = git(repo, &[state.command(), "--abort"]).await?;
    cleanup_rebase_messages(repo).await?;
    Ok(ok(&result))
}

pub async fn skip_operation(repo: &str) -> Result<OpResult> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    match queries::operation_state(repo).await? {
        None | Some(Operation::Merge { .. }) => invalid("Nothing to skip"),
        Some(state) => may_conflict(repo, &[state.command(), "--skip"], Options::default()).await,
    }
}

// ----------------------------------------------------------- interactive

const REBASE_ACTIONS: [&str; 6] = ["pick", "reword", "edit", "squash", "fixup", "drop"];

/// Runs `git rebase -i` with a todo list prepared by the UI.
///
/// `steps` are oldest first. Messages for `reword` (and the final `squash` of a
/// chain) are applied by the editor mode of this executable.
pub async fn interactive_rebase(
    repo: &str,
    base: Option<&str>,
    steps: &[RebaseStep],
    autostash: bool,
) -> Result<OpResult> {
    if steps.is_empty() {
        return invalid("Nothing to rebase");
    }
    let mut lines = Vec::new();
    let mut messages = BTreeMap::new();
    for step in steps {
        if !REBASE_ACTIONS.contains(&step.action.as_str()) {
            return invalid(format!("Unknown rebase action {}", step.action));
        }
        check_ref(&step.sha)?;
        lines.push(format!("{} {}", step.action, step.sha));
        if matches!(step.action.as_str(), "reword" | "squash")
            && let Some(message) = step.message.as_ref().filter(|message| !message.is_empty())
        {
            messages.insert(step.sha.clone(), message.clone());
        }
    }
    if matches!(steps[0].action.as_str(), "squash" | "fixup") {
        return invalid("The first commit cannot be squashed into a previous commit");
    }

    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    if !messages.is_empty() {
        let body = serde_json::to_string(&messages).expect("plain data serialises");
        std::fs::write(
            queries::git_dir(repo).await?.join(editor::MESSAGES_FILE),
            body,
        )?;
    }
    let todo = tempfile::Builder::new()
        .prefix("gitcraft-todo-")
        .tempfile()?;
    std::fs::write(todo.path(), lines.join("\n") + "\n")?;
    let prepared = format!("\"{}\"", todo.path().to_string_lossy().replace('\\', "/"));
    let options = Options {
        env: vec![("GIT_SEQUENCE_EDITOR", editor_command(&["todo", &prepared]))],
        ..Options::default()
    };
    let mut args = vec![
        "rebase",
        "-i",
        if autostash {
            "--autostash"
        } else {
            "--no-autostash"
        },
    ];
    match base {
        Some(base) => args.push(check_ref(base)?),
        None => args.push("--root"),
    }
    may_conflict(repo, &args, options).await
}

// ----------------------------------------------------------------- stash

pub async fn stash_push(
    repo: &str,
    message: Option<&str>,
    include_untracked: bool,
    staged_only: bool,
    keep_index: bool,
    list: Option<&[String]>,
) -> Result<OpResult> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let mut args = vec!["stash", "push"];
    if include_untracked {
        args.push("--include-untracked");
    }
    if staged_only {
        args.push("--staged");
    }
    if keep_index {
        args.push("--keep-index");
    }
    if let Some(message) = message.filter(|message| !message.is_empty()) {
        args.extend(["-m", message]);
    }
    if let Some(list) = list.filter(|list| !list.is_empty()) {
        args.push("--");
        args.extend(list.iter().map(String::as_str));
    }
    Ok(ok(&git(repo, &args).await?))
}

pub async fn stash_apply(repo: &str, name: &str, pop: bool) -> Result<OpResult> {
    let name = check_ref(name)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let result = git_unchecked(
        repo,
        &["stash", if pop { "pop" } else { "apply" }, "--index", name],
    )
    .await?;
    if result.code != 0 {
        if !queries::status(repo).await?.conflicts.is_empty() {
            return Ok(OpResult {
                ok: false,
                conflicts: true,
                message: result.output(),
                operation: None,
                files: None,
            });
        }
        return Err(GitError::new(
            &["stash", "apply", name],
            result.code,
            &result.err,
            &result.out(),
        )
        .into());
    }
    Ok(ok(&result))
}

pub async fn stash_drop(repo: &str, name: &str) -> Result<OpResult> {
    let name = check_ref(name)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    Ok(ok(&git(repo, &["stash", "drop", name]).await?))
}

// ------------------------------------------------------------- conflicts

pub async fn conflict_versions(repo: &str, path: &str) -> Result<ConflictVersions> {
    let target = safe_path(repo, path)?;
    let working = if target.is_file() {
        Some(String::from_utf8_lossy(&std::fs::read(&target)?).into_owned())
    } else {
        None
    };
    let (base_spec, ours_spec, theirs_spec) = (
        format!(":1:{path}"),
        format!(":2:{path}"),
        format!(":3:{path}"),
    );
    let (base, ours, theirs) = tokio::join!(
        queries::raw_show(repo, &base_spec),
        queries::raw_show(repo, &ours_spec),
        queries::raw_show(repo, &theirs_spec),
    );
    let rebasing = matches!(
        queries::operation_state(repo).await?,
        Some(Operation::Rebase { .. })
    );
    Ok(ConflictVersions {
        path: path.to_owned(),
        base: base?,
        ours: ours?,
        theirs: theirs?,
        chunks: working
            .as_deref()
            .map(parsers::parse_conflict_markers)
            .unwrap_or_default(),
        working,
        ours_label: if rebasing {
            "Upstream (being rebased onto)"
        } else {
            "Ours (HEAD)"
        }
        .to_owned(),
        theirs_label: if rebasing {
            "Your commit (being replayed)"
        } else {
            "Theirs (incoming)"
        }
        .to_owned(),
    })
}

/// Resolves a conflicted file by taking a side, by writing edited content, or
/// (given neither) by marking the file as resolved the way it now is.
pub async fn resolve_conflict(
    repo: &str,
    path: &str,
    content: Option<&str>,
    side: Option<&str>,
) -> Result<OpResult> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    match (side, content) {
        (Some(side @ ("ours" | "theirs")), _) => {
            let stage = if side == "ours" { ":2:" } else { ":3:" };
            if git_unchecked(repo, &["cat-file", "-e", &format!("{stage}{path}")])
                .await?
                .code
                != 0
            {
                return Ok(ok(&git(repo, &["rm", "-q", "--", path]).await?));
            }
            git(repo, &["checkout", &format!("--{side}"), "--", path]).await?;
        }
        (_, Some(content)) => {
            let target = safe_path(repo, path)?;
            // Edited text is UTF-8 and the file isn't: writing it would silently change the encoding.
            if let Ok(existing) = std::fs::read(&target)
                && std::str::from_utf8(&existing).is_err()
            {
                return invalid(
                    "This file is not UTF-8, so it can't be edited here without damaging it. \
                         Take one side, or resolve it in another editor and mark it as resolved.",
                );
            }
            std::fs::write(target, content)?;
        }
        _ => {
            if !safe_path(repo, path)?.exists() {
                return Ok(ok(&git(repo, &["rm", "-q", "--", path]).await?));
            }
        }
    }
    Ok(ok(&git(repo, &["add", "--", path]).await?))
}

// ----------------------------------------------------------- misc tools

/// Restores one file (index and working tree) to its state in `sha`.
pub async fn checkout_file(repo: &str, sha: &str, path: &str) -> Result<OpResult> {
    let sha = check_ref(sha)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    safe_path(repo, path)?;
    Ok(ok(&git(repo, &["checkout", sha, "--", path]).await?))
}

/// Fast-forwards `branch` to `target` (default: its upstream) without checking it out.
pub async fn fast_forward(repo: &str, branch: &str, target: Option<&str>) -> Result<OpResult> {
    let branch = check_ref(branch)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let target = match target.filter(|target| !target.is_empty()) {
        Some(target) => check_ref(target)?.to_owned(),
        None => format!("{branch}@{{upstream}}"),
    };
    let spec = format!("{target}^{{commit}}");
    let resolved = git(repo, &["rev-parse", "--verify", &spec])
        .await?
        .out()
        .trim()
        .to_owned();
    let local = format!("refs/heads/{branch}");
    if git_unchecked(repo, &["merge-base", "--is-ancestor", &local, &resolved])
        .await?
        .code
        != 0
    {
        return invalid(format!(
            "{branch} cannot be fast-forwarded to {target} (the branches have diverged)"
        ));
    }
    let current = git_unchecked(repo, &["symbolic-ref", "-q", "--short", "HEAD"]).await?;
    if current.out().trim() == branch {
        return Ok(ok(&git(repo, &["merge", "--ff-only", &resolved]).await?));
    }
    let reason = format!("fast-forward to {target}");
    Ok(ok(&git(
        repo,
        &["update-ref", "-m", &reason, &local, &resolved],
    )
    .await?))
}

pub async fn add_to_gitignore(repo: &str, pattern: &str) -> Result<OpResult> {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern.contains('\n') {
        return invalid("Invalid ignore pattern");
    }
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let path = PathBuf::from(repo).join(".gitignore");
    let existing = std::fs::read(&path).unwrap_or_default();
    if String::from_utf8_lossy(&existing)
        .lines()
        .any(|line| line == pattern)
    {
        return Ok(ok_empty());
    }
    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with(b"\n") {
        updated.push(b'\n');
    }
    updated.extend_from_slice(pattern.as_bytes());
    updated.push(b'\n');
    std::fs::write(&path, updated)?;
    // Ignoring a tracked file has no effect unless it is removed from the index.
    let target = pattern.trim_start_matches('/');
    let tracked = git_unchecked(repo, &["ls-files", "--", target]).await?;
    if !tracked.out().trim().is_empty() {
        git_unchecked(repo, &["rm", "-r", "-q", "--cached", "--", target]).await?;
    }
    Ok(ok_empty())
}

pub async fn clean(
    repo: &str,
    directories: bool,
    ignored: bool,
    dry_run: bool,
) -> Result<OpResult> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let mut args = vec!["clean", if dry_run { "-n" } else { "-f" }];
    if directories {
        args.push("-d");
    }
    if ignored {
        args.push("-x");
    }
    let result = git(repo, &args).await?;
    let files = result
        .out()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let name = line
                .strip_prefix("Would remove ")
                .or_else(|| line.strip_prefix("Removing "));
            name.unwrap_or(line).to_owned()
        })
        .collect();
    Ok(OpResult {
        files: Some(files),
        ..ok(&result)
    })
}

pub async fn format_patch(repo: &str, sha: &str) -> Result<String> {
    let result = git(repo, &["format-patch", "-1", "--stdout", check_ref(sha)?]).await?;
    let patch = result.out();
    if patch.trim().is_empty() {
        return invalid(
            "No patch could be created for this commit (merge commits are not supported)",
        );
    }
    Ok(patch.into_owned())
}

/// mode: "am" (create commits from mailbox patches) or "apply" (only change the working tree/index).
pub async fn apply_patch(repo: &str, patch: &str, mode: &str) -> Result<OpResult> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let options = Options {
        stdin: Some(patch.as_bytes()),
        ..Options::default()
    };
    if mode == "am" {
        return may_conflict(repo, &["am", "--3way"], options).await;
    }
    let result = run_unchecked(
        repo,
        &["apply", "--3way", "--whitespace=nowarn", "-"],
        options,
    )
    .await?;
    if result.code != 0 {
        if !queries::status(repo).await?.conflicts.is_empty() {
            return Ok(OpResult {
                ok: false,
                conflicts: true,
                message: result.output(),
                operation: None,
                files: None,
            });
        }
        return Err(GitError::new(&["apply"], result.code, &result.err, &result.out()).into());
    }
    Ok(ok(&result))
}

pub async fn prune_remote(repo: &str, name: &str, progress: Option<&Progress>) -> Result<OpResult> {
    let name = check_ref(name)?;
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    Ok(ok(&run(
        repo,
        &["remote", "prune", name],
        with_progress(progress),
    )
    .await?))
}

/// Makes sure the repository has a commit-graph file, which speeds up walking history
/// many times over. It's only a cache: git keeps it correct and works without it.
pub async fn ensure_commit_graph(repo: &str) -> Result<bool> {
    let info = queries::common_dir(repo)
        .await?
        .join("objects")
        .join("info");
    if info.join("commit-graph").exists() || info.join("commit-graphs").exists() {
        return Ok(false);
    }
    if !queries::has_head(repo).await? {
        return Ok(false);
    }
    git(repo, &["commit-graph", "write", "--reachable"]).await?;
    Ok(true)
}

pub async fn update_submodules(repo: &str, progress: Option<&Progress>) -> Result<OpResult> {
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let args = ["submodule", "update", "--init", "--recursive", "--progress"];
    Ok(ok(&run(repo, &args, with_progress(progress)).await?))
}
