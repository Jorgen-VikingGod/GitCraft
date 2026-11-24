//! Undo and redo of local operations.
//!
//! Before an operation, [`capture`] notes HEAD, the branches, the tags, the backups of
//! discarded changes and the branches' settings. Afterwards [`record`] compares that with the
//! new state and keeps the difference on a stack in the git folder. Undo and redo replay a
//! difference in one direction or the other, and only while the repository is still exactly
//! as the operation left it: anything changed since makes them refuse instead of guessing.
//! The commits a record needs are pinned under `refs/gitcraft/undo/` so `git gc` keeps them.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::model::UndoLabels;
use crate::runner::{Options, git, git_unchecked, repo_lock, run};
use crate::{Error, Result, backups, queries, store};

const FILE: &str = "gitcraft-undo.json";
const PINS: &str = "refs/gitcraft/undo/";
const LIMIT: usize = 50;
/// Refs an undo puts back. Remote-tracking branches follow the remote, and the stash has
/// its own list.
const TRACKED: [&str; 2] = ["refs/heads/", "refs/tags/"];

/// How an undo moves the current branch: `Soft` keeps the index and the working tree,
/// `Mixed` keeps the working tree, `Keep` keeps uncommitted changes and refuses when they
/// are in the way, `Hard` replaces both and needs a clean working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Soft,
    Mixed,
    Keep,
    Hard,
}

impl Mode {
    fn flag(self) -> &'static str {
        match self {
            Self::Soft => "--soft",
            Self::Mixed => "--mixed",
            Self::Keep => "--keep",
            Self::Hard => "--hard",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Head {
    /// The full ref name; the branch may not have a commit yet.
    Branch(String),
    Detached(String),
}

/// The state before an operation, to compare with afterwards.
#[derive(Debug, Clone)]
pub struct Snapshot {
    head: Head,
    refs: BTreeMap<String, String>,
    /// `branch.<name>.*` settings, by branch name.
    config: BTreeMap<String, Vec<(String, String)>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RefChange {
    name: String,
    old: Option<String>,
    new: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DiscardChange {
    /// The backup's ref, removed by an undo since the changes are back.
    name: String,
    before: String,
    after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    /// What was done, in English (the interface translates it).
    label: String,
    mode: Mode,
    head_old: Head,
    head_new: Head,
    refs: Vec<RefChange>,
    /// Settings of the branches the operation deleted, and of those it created.
    config_old: Vec<(String, String)>,
    config_new: Vec<(String, String)>,
    discard: Option<DiscardChange>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Stack {
    undo: Vec<Record>,
    redo: Vec<Record>,
}

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(message.into()))
}

fn short(name: &str) -> &str {
    name.strip_prefix("refs/heads/")
        .or_else(|| name.strip_prefix("refs/tags/"))
        .unwrap_or(name)
}

async fn head(repo: &str) -> Result<Head> {
    let symbolic = git_unchecked(repo, &["symbolic-ref", "-q", "HEAD"]).await?;
    if symbolic.code == 0 {
        return Ok(Head::Branch(symbolic.out().trim().to_owned()));
    }
    let sha = git(repo, &["rev-parse", "--verify", "-q", "HEAD"]).await?;
    Ok(Head::Detached(sha.out().trim().to_owned()))
}

async fn refs(repo: &str) -> Result<BTreeMap<String, String>> {
    let mut args = vec!["for-each-ref", "--format=%(refname)%00%(objectname)"];
    args.extend(TRACKED);
    args.push(backups::PREFIX);
    let result = git(repo, &args).await?;
    Ok(result
        .out()
        .lines()
        .filter_map(|line| line.split_once('\0'))
        .map(|(name, sha)| (name.to_owned(), sha.to_owned()))
        .collect())
}

async fn branch_config(repo: &str) -> Result<BTreeMap<String, Vec<(String, String)>>> {
    let result = git_unchecked(
        repo,
        &["config", "--local", "-z", "--get-regexp", r"^branch\."],
    )
    .await?;
    let mut config: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for entry in result.out().split('\0').filter(|entry| !entry.is_empty()) {
        let (key, value) = entry.split_once('\n').unwrap_or((entry, ""));
        // branch.<name>.<setting>, where the name itself may hold dots.
        let Some(rest) = key.strip_prefix("branch.") else {
            continue;
        };
        let Some((name, _)) = rest.rsplit_once('.') else {
            continue;
        };
        config
            .entry(name.to_owned())
            .or_default()
            .push((key.to_owned(), value.to_owned()));
    }
    Ok(config)
}

/// Notes the state before an operation that [`record`] may make undoable.
pub async fn capture(repo: &str) -> Result<Snapshot> {
    Ok(Snapshot {
        head: head(repo).await?,
        refs: refs(repo).await?,
        config: branch_config(repo).await?,
    })
}

async fn stack_file(repo: &str) -> Result<std::path::PathBuf> {
    Ok(queries::git_dir(repo).await?.join(FILE))
}

async fn load(repo: &str) -> Result<Stack> {
    let file = stack_file(repo).await?;
    Ok(std::fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default())
}

/// Pins are per worktree: each has its own stack, but all share the refs.
async fn pin_prefix(repo: &str) -> Result<String> {
    let dir = queries::git_dir(repo).await?;
    // FNV-1a: stable across runs and versions, unlike the standard hasher.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in dir.to_string_lossy().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    Ok(format!("{PINS}{hash:016x}/"))
}

fn shas(record: &Record) -> impl Iterator<Item = &String> {
    let heads = [&record.head_old, &record.head_new]
        .into_iter()
        .filter_map(|head| match head {
            Head::Detached(sha) => Some(sha),
            Head::Branch(_) => None,
        });
    let refs = record
        .refs
        .iter()
        .flat_map(|change| change.old.iter().chain(change.new.iter()));
    let discard = record
        .discard
        .iter()
        .flat_map(|discard| [&discard.before, &discard.after]);
    heads.chain(refs).chain(discard)
}

/// Writes the stack, then pins what it needs and lets go of the rest.
async fn save(repo: &str, stack: &Stack) -> Result<()> {
    let bytes = serde_json::to_vec(stack).map_err(|error| Error::Invalid(error.to_string()))?;
    store::write_private(&stack_file(repo).await?, &bytes)?;
    let prefix = pin_prefix(repo).await?;
    let wanted: BTreeSet<&String> = stack
        .undo
        .iter()
        .chain(&stack.redo)
        .flat_map(shas)
        .collect();
    let pinned = git(repo, &["for-each-ref", "--format=%(refname)", &prefix]).await?;
    let pinned: BTreeSet<String> = pinned.out().lines().map(str::to_owned).collect();
    let mut lines = String::new();
    for sha in &wanted {
        let name = format!("{prefix}{sha}");
        if !pinned.contains(&name) {
            lines.push_str(&format!("update {name} {sha}\n"));
        }
    }
    for name in &pinned {
        let sha = name.strip_prefix(&prefix).unwrap_or_default();
        if !wanted.iter().any(|wanted| wanted.as_str() == sha) {
            lines.push_str(&format!("delete {name}\n"));
        }
    }
    transaction(repo, &lines).await
}

async fn transaction(repo: &str, lines: &str) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    run(
        repo,
        &["update-ref", "--stdin"],
        Options {
            stdin: Some(lines.as_bytes()),
            ..Options::default()
        },
    )
    .await?;
    Ok(())
}

/// Keeps what an operation changed since `before`, so it can be undone. Nothing is kept when
/// nothing changed. `label` is English.
pub async fn record(repo: &str, label: &str, mode: Mode, before: Snapshot) -> Result<()> {
    let after = capture(repo).await?;
    let names: BTreeSet<&String> = before.refs.keys().chain(after.refs.keys()).collect();
    let mut refs = Vec::new();
    let mut discard = None;
    for name in names {
        let (old, new) = (before.refs.get(name), after.refs.get(name));
        if old == new {
            continue;
        }
        if name.starts_with(backups::PREFIX) {
            // A new backup means this operation discarded changes.
            if let (None, Some(after_sha)) = (old, new) {
                let parent = format!("{after_sha}^");
                let parent = git(repo, &["rev-parse", "--verify", "-q", &parent]).await?;
                discard = Some(DiscardChange {
                    name: name.clone(),
                    before: parent.out().trim().to_owned(),
                    after: after_sha.clone(),
                });
            }
            continue;
        }
        refs.push(RefChange {
            name: name.clone(),
            old: old.cloned(),
            new: new.cloned(),
        });
    }
    if refs.is_empty() && discard.is_none() && before.head == after.head {
        return Ok(());
    }
    let settings = |snapshot: &Snapshot, gone: fn(&RefChange) -> bool| -> Vec<(String, String)> {
        refs.iter()
            .filter(|change| gone(change))
            .filter_map(|change| change.name.strip_prefix("refs/heads/"))
            .filter_map(|branch| snapshot.config.get(branch))
            .flatten()
            .cloned()
            .collect()
    };
    let config_old = settings(&before, |change| change.new.is_none());
    let config_new = settings(&after, |change| change.old.is_none());
    let record = Record {
        label: label.to_owned(),
        mode,
        head_old: before.head,
        head_new: after.head,
        refs,
        config_old,
        config_new,
        discard,
    };
    let lock = repo_lock(repo);
    let _guard = lock.lock().await;
    let mut stack = load(repo).await?;
    stack.undo.push(record);
    if stack.undo.len() > LIMIT {
        stack.undo.remove(0);
    }
    stack.redo.clear();
    save(repo, &stack).await
}

/// What undo and redo would do, for the menu.
pub async fn labels(repo: &str) -> Result<UndoLabels> {
    let stack = load(repo).await?;
    Ok(UndoLabels {
        undo: stack.undo.last().map(|record| record.label.clone()),
        redo: stack.redo.last().map(|record| record.label.clone()),
    })
}

/// Undoes the last operation. Returns its label. The caller holds the repository lock.
pub(crate) async fn undo(repo: &str) -> Result<String> {
    step(repo, true).await
}

/// Does the last undone operation again. Returns its label. The caller holds the lock.
pub(crate) async fn redo(repo: &str) -> Result<String> {
    step(repo, false).await
}

async fn step(repo: &str, back: bool) -> Result<String> {
    let mut stack = load(repo).await?;
    let from = if back {
        &mut stack.undo
    } else {
        &mut stack.redo
    };
    let Some(record) = from.last().cloned() else {
        return invalid(if back {
            "There is nothing to undo"
        } else {
            "There is nothing to redo"
        });
    };
    replay(repo, &record, back).await?;
    let (from, to) = if back {
        (&mut stack.undo, &mut stack.redo)
    } else {
        (&mut stack.redo, &mut stack.undo)
    };
    from.pop();
    to.push(record.clone());
    save(repo, &stack).await?;
    Ok(record.label)
}

/// One ref change of a transaction: (name, expected now, wanted).
type Move<'a> = (&'a str, Option<&'a String>, Option<&'a String>);

fn lines(moves: &[Move<'_>]) -> String {
    let mut out = String::new();
    for (name, expect, set) in moves {
        let line = match (expect, set) {
            (Some(expect), Some(set)) => format!("update {name} {set} {expect}\n"),
            (None, Some(set)) => format!("create {name} {set}\n"),
            (Some(expect), None) => format!("delete {name} {expect}\n"),
            (None, None) => continue,
        };
        out.push_str(&line);
    }
    out
}

fn reversed<'a>(moves: &[Move<'a>]) -> Vec<Move<'a>> {
    moves
        .iter()
        .map(|(name, expect, set)| (*name, *set, *expect))
        .collect()
}

async fn require_clean(repo: &str) -> Result<()> {
    let status = queries::status(repo).await?;
    let dirty = !status.staged.is_empty()
        || !status.conflicts.is_empty()
        || status.unstaged.iter().any(|entry| entry.status != "?");
    if dirty {
        return invalid("Commit or stash your changes first: this would replace them");
    }
    Ok(())
}

async fn replay(repo: &str, record: &Record, back: bool) -> Result<()> {
    let verb = if back { "undo" } else { "redo" };
    if queries::operation_state(repo).await?.is_some() {
        return invalid(format!(
            "Finish or abort the operation in progress first, then {verb}"
        ));
    }
    let (from_head, to_head) = if back {
        (&record.head_new, &record.head_old)
    } else {
        (&record.head_old, &record.head_new)
    };
    let moves: Vec<Move<'_>> = record
        .refs
        .iter()
        .map(|change| {
            let (expect, set) = if back {
                (change.new.as_ref(), change.old.as_ref())
            } else {
                (change.old.as_ref(), change.new.as_ref())
            };
            (change.name.as_str(), expect, set)
        })
        .collect();

    // Only while everything is as the operation (or the last undo) left it.
    let now = capture(repo).await?;
    if now.head != *from_head {
        return invalid(format!(
            "HEAD has moved since, so there is nothing safe to {verb}"
        ));
    }
    if let Some((name, ..)) = moves
        .iter()
        .find(|(name, expect, _)| now.refs.get(*name) != *expect)
    {
        return invalid(format!(
            "{} has changed since, so it can't {verb}",
            short(name)
        ));
    }

    if let Some(discard) = &record.discard {
        let (from, to) = if back {
            (&discard.after, &discard.before)
        } else {
            (&discard.before, &discard.after)
        };
        backups::put(repo, from, to).await?;
        if back {
            // The changes are back, so the backup isn't offered for recovery any more.
            let _ = git_unchecked(repo, &["update-ref", "-d", &discard.name, &discard.after]).await;
        }
    }

    match (from_head, to_head) {
        (Head::Branch(a), Head::Branch(b)) if a == b => {
            move_head(repo, record.mode, Some(a.as_str()), None, &moves).await?;
        }
        (Head::Detached(_), Head::Detached(sha)) => {
            move_head(repo, record.mode, None, Some(sha.as_str()), &moves).await?;
        }
        _ => switch(repo, from_head, to_head, &moves).await?,
    }

    // Branch settings go with the branches: drop those of branches that are gone again,
    // put back those of branches that are back.
    let (gone, back_again) = if back {
        (&record.config_new, &record.config_old)
    } else {
        (&record.config_old, &record.config_new)
    };
    let sections: BTreeSet<&str> = gone
        .iter()
        .chain(back_again)
        .filter_map(|(key, _)| key.rsplit_once('.').map(|(section, _)| section))
        .collect();
    for section in sections {
        let _ = git_unchecked(repo, &["config", "--local", "--remove-section", section]).await;
    }
    for (key, value) in back_again {
        git(repo, &["config", "--local", "--add", key, value]).await?;
    }
    Ok(())
}

/// HEAD stays where it is (on `branch`, or detached and moving to `detached`); the other refs
/// change in one transaction, then the current branch or detached HEAD moves.
async fn move_head(
    repo: &str,
    mode: Mode,
    branch: Option<&str>,
    detached: Option<&str>,
    moves: &[Move<'_>],
) -> Result<()> {
    let (current, others): (Vec<Move<'_>>, Vec<Move<'_>>) =
        moves.iter().partition(|(name, ..)| Some(*name) == branch);
    transaction(repo, &lines(&others)).await?;
    let moved = async {
        let target = match (current.first(), detached) {
            (Some((name, expect, set)), _) => match (expect, set) {
                // Back to a branch without commits, or its first commit again: the index stays.
                (_, None) | (None, _) => {
                    return transaction(repo, &lines(&[(*name, *expect, *set)])).await;
                }
                (Some(_), Some(set)) => set.as_str(),
            },
            (None, Some(sha)) => sha,
            (None, None) => return Ok(()),
        };
        if mode == Mode::Hard {
            require_clean(repo).await?;
        }
        git(repo, &["reset", "-q", mode.flag(), target, "--"]).await?;
        Ok::<(), Error>(())
    };
    if let Err(error) = moved.await {
        let _ = transaction(repo, &lines(&reversed(&others))).await;
        return Err(error);
    }
    Ok(())
}

/// HEAD goes to another branch or commit. Refs that aren't the branch being left change
/// first (so a branch that comes back exists before it is checked out), then HEAD switches,
/// then the branch that was left, which is no longer checked out.
async fn switch(repo: &str, from: &Head, to: &Head, moves: &[Move<'_>]) -> Result<()> {
    let leaving = match from {
        Head::Branch(name) => Some(name.as_str()),
        Head::Detached(_) => None,
    };
    let (left, others): (Vec<Move<'_>>, Vec<Move<'_>>) =
        moves.iter().partition(|(name, ..)| Some(*name) == leaving);
    transaction(repo, &lines(&others)).await?;
    let switched = match to {
        Head::Branch(name) => git(repo, &["switch", "-q", "--no-guess", short(name)]).await,
        Head::Detached(sha) => git(repo, &["switch", "-q", "--detach", sha]).await,
    };
    if let Err(error) = switched {
        let _ = transaction(repo, &lines(&reversed(&others))).await;
        return Err(error.into());
    }
    transaction(repo, &lines(&left)).await
}
