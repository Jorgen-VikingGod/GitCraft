//! Backups of discarded changes, so a discard can be taken back.
//!
//! A backup is two commits on top of HEAD's tree, kept alive by a ref under
//! `refs/gitcraft/discards/`. The ref's commit holds the files as the discard left them,
//! its parent the files as they were before. Restoring writes the parent's version back
//! into the working tree, and only while the files are still as the discard left them,
//! so nothing written since is overwritten. The index is never touched.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::DiscardBackup;
use crate::runner::{Options, git, git_unchecked, run};
use crate::{Error, Result, check_ref, paths, queries};

pub const PREFIX: &str = "refs/gitcraft/discards/";
const KEEP: usize = 50;
const MAX_AGE: i64 = 30 * 24 * 60 * 60;
const TITLE: &str = "GitCraft: discarded changes";

/// A private index file that is removed again however the work ends.
struct TempIndex(PathBuf);

impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

fn nul_list(list: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for path in list {
        out.extend_from_slice(path.as_bytes());
        out.push(0);
    }
    out
}

/// Backup commits always get the same author and committer, and are never signed, so a
/// discard never fails or waits for a missing identity or a signing prompt.
fn commit_env() -> Vec<(&'static str, String)> {
    vec![
        ("GIT_AUTHOR_NAME", "GitCraft".to_owned()),
        ("GIT_AUTHOR_EMAIL", "gitcraft@localhost".to_owned()),
        ("GIT_COMMITTER_NAME", "GitCraft".to_owned()),
        ("GIT_COMMITTER_EMAIL", "gitcraft@localhost".to_owned()),
    ]
}

/// The tree of `base` (HEAD, or the empty tree before the first commit, when `None`)
/// with the working-tree state of `list` written over it. Paths are taken literally.
async fn snapshot(repo: &str, base: Option<&str>, list: &[String]) -> Result<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let file = queries::git_dir(repo).await?.join(format!(
        "gitcraft-snapshot-{}-{nanos}.index",
        std::process::id()
    ));
    let index = TempIndex(file);
    let env = || vec![("GIT_INDEX_FILE", index.0.to_string_lossy().into_owned())];
    let base = match base {
        Some(base) => vec!["read-tree", check_ref(base)?],
        None if queries::has_head(repo).await? => vec!["read-tree", "HEAD"],
        None => vec!["read-tree", "--empty"],
    };
    run(
        repo,
        &base,
        Options {
            env: env(),
            ..Options::default()
        },
    )
    .await?;
    let input = nul_list(list);
    run(
        repo,
        &["update-index", "--add", "--remove", "-z", "--stdin"],
        Options {
            stdin: Some(&input),
            env: env(),
            ..Options::default()
        },
    )
    .await?;
    let tree = run(
        repo,
        &["write-tree"],
        Options {
            env: env(),
            ..Options::default()
        },
    )
    .await?;
    Ok(tree.out().trim().to_owned())
}

async fn commit_tree(
    repo: &str,
    tree: &str,
    parent: Option<&str>,
    message: &str,
) -> Result<String> {
    let mut args = vec!["commit-tree", "--no-gpg-sign", tree];
    if let Some(parent) = parent {
        args.extend(["-p", parent]);
    }
    args.extend(["-F", "-"]);
    let result = run(
        repo,
        &args,
        Options {
            stdin: Some(message.as_bytes()),
            env: commit_env(),
            ..Options::default()
        },
    )
    .await?;
    Ok(result.out().trim().to_owned())
}

/// The files of `list` as they are now, recorded before they are discarded.
pub(crate) struct Before {
    commit: String,
    list: Vec<String>,
}

/// Records the files of `list` before a discard. The caller holds the repository lock.
pub(crate) async fn before(repo: &str, list: &[String]) -> Result<Before> {
    let tree = snapshot(repo, None, list).await?;
    let head = git_unchecked(repo, &["rev-parse", "--verify", "-q", "HEAD"]).await?;
    let head = (head.code == 0).then(|| head.out().trim().to_owned());
    let commit = commit_tree(repo, &tree, head.as_deref(), TITLE).await?;
    Ok(Before {
        commit,
        list: list.to_vec(),
    })
}

/// Keeps the backup once the discard went through, and lets old backups go.
pub(crate) async fn keep(repo: &str, before: Before) -> Result<()> {
    let tree = snapshot(repo, None, &before.list).await?;
    let message = format!("{TITLE}\n\n{}\n", before.list.join("\n"));
    let after = commit_tree(repo, &tree, Some(&before.commit), &message).await?;
    let short: String = after.chars().take(12).collect();
    let name = format!("{PREFIX}{:012}-{short}", now());
    git(repo, &["update-ref", &name, &after]).await?;
    prune(repo).await
}

async fn prune(repo: &str) -> Result<()> {
    let oldest = now() - MAX_AGE;
    let stale: String = list(repo)
        .await?
        .iter()
        .enumerate()
        .filter(|(index, backup)| *index >= KEEP || backup.time < oldest)
        .map(|(_, backup)| format!("delete {}\n", backup.name))
        .collect();
    if !stale.is_empty() {
        run(
            repo,
            &["update-ref", "--stdin"],
            Options {
                stdin: Some(stale.as_bytes()),
                ..Options::default()
            },
        )
        .await?;
    }
    Ok(())
}

/// Every backup, newest first.
pub async fn list(repo: &str) -> Result<Vec<DiscardBackup>> {
    let result = git(
        repo,
        &[
            "for-each-ref",
            "--sort=-refname",
            "--format=%(refname)%00%(objectname)%00%(contents:body)%00",
            PREFIX,
        ],
    )
    .await?;
    let out = result.out();
    let fields: Vec<&str> = out.split('\0').collect();
    let mut backups = Vec::new();
    for [name, sha, body] in fields.as_chunks::<3>().0 {
        let name = name.trim_start_matches('\n');
        let Some(stamp) = name.strip_prefix(PREFIX) else {
            continue;
        };
        let time = stamp
            .split('-')
            .next()
            .and_then(|secs| secs.parse().ok())
            .unwrap_or(0);
        backups.push(DiscardBackup {
            name: name.to_owned(),
            sha: (*sha).to_owned(),
            time,
            paths: body
                .lines()
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
        });
    }
    // By the time in the name, not the name itself, should its digits ever differ in count.
    backups.sort_by_key(|backup| std::cmp::Reverse(backup.time));
    Ok(backups)
}

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(message.into()))
}

async fn resolve(repo: &str, name: &str) -> Result<String> {
    if !name.starts_with(PREFIX) {
        return invalid(format!("Not a backup of discarded changes: {name}"));
    }
    let spec = format!("{}^{{commit}}", check_ref(name)?);
    let result = git_unchecked(repo, &["rev-parse", "--verify", "-q", &spec]).await?;
    if result.code != 0 {
        return invalid("This backup no longer exists");
    }
    Ok(result.out().trim().to_owned())
}

/// Puts the discarded files back. Refuses when any of them changed since the discard.
/// The caller holds the repository lock.
pub(crate) async fn restore(repo: &str, name: &str) -> Result<()> {
    let after = resolve(repo, name).await?;
    let parent = format!("{after}^");
    let before = git(repo, &["rev-parse", "--verify", "-q", &parent]).await?;
    put(repo, &after, before.out().trim()).await?;
    git(repo, &["update-ref", "-d", name, &after]).await?;
    Ok(())
}

/// Changes the files that differ between the commits `from` and `to` from their `from`
/// version to their `to` version in the working tree, but only while they are all still as
/// in `from`. Undo puts a backup back this way, and redo discards again.
pub(crate) async fn put(repo: &str, from: &str, to: &str) -> Result<()> {
    let diff = git(
        repo,
        &[
            "diff-tree",
            "-r",
            "-z",
            "--no-renames",
            "--name-status",
            from,
            to,
        ],
    )
    .await?;
    let out = diff.out();
    let fields: Vec<&str> = out.split('\0').filter(|field| !field.is_empty()).collect();
    let mut write = Vec::new();
    let mut remove = Vec::new();
    let mut all = Vec::new();
    for [status, path] in fields.as_chunks::<2>().0 {
        let path = (*path).to_owned();
        // Missing in `to`: the file goes.
        if *status == "D" {
            remove.push(path.clone());
        } else {
            write.push(path.clone());
        }
        all.push(path);
    }
    if all.is_empty() {
        return Ok(());
    }
    let now_tree = snapshot(repo, Some(from), &all).await?;
    let tree_of = format!("{from}^{{tree}}");
    let from_tree = git(repo, &["rev-parse", &tree_of]).await?;
    let from_tree = from_tree.out().trim().to_owned();
    if now_tree != from_tree {
        let changed = git(
            repo,
            &[
                "diff-tree",
                "-r",
                "-z",
                "--name-only",
                &from_tree,
                &now_tree,
            ],
        )
        .await?;
        let changed: Vec<String> = changed
            .out()
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect();
        return invalid(format!(
            "These files changed since, so this would overwrite your work: {}",
            changed.join(", ")
        ));
    }
    for path in &remove {
        let Some(target) = paths::inside(repo, path) else {
            return invalid(format!("Path escapes repository: {path}"));
        };
        if target.symlink_metadata().is_ok_and(|meta| !meta.is_dir()) {
            std::fs::remove_file(target)?;
        }
    }
    if !write.is_empty() {
        let input = nul_list(&write);
        run(
            repo,
            &[
                "restore",
                "--source",
                to,
                "--worktree",
                "--pathspec-from-file=-",
                "--pathspec-file-nul",
            ],
            Options {
                stdin: Some(&input),
                env: vec![("GIT_LITERAL_PATHSPECS", "1".to_owned())],
                ..Options::default()
            },
        )
        .await?;
    }
    Ok(())
}

/// Deletes one backup. The caller holds the repository lock.
pub(crate) async fn delete(repo: &str, name: &str) -> Result<()> {
    let after = resolve(repo, name).await?;
    git(repo, &["update-ref", "-d", name, &after]).await?;
    Ok(())
}
