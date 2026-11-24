//! Data the git layer hands to its callers. Serialised names match the original JSON contract.

use serde::{Deserialize, Serialize};

use crate::graph::GraphRow;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefLabel {
    pub name: String,
    pub kind: String,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub time: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<Vec<RefLabel>>,
    pub subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<GraphRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<FileChange>,
    /// For a stash shown in the history: its `stash@{n}`. Its parents are then only the
    /// commit it was made on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogPage {
    pub commits: Vec<Commit>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDetail {
    pub sha: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub committer: String,
    pub committer_email: String,
    pub commit_time: i64,
    pub refs: Vec<RefLabel>,
    pub message: String,
    pub subject: String,
    pub files: Vec<FileChange>,
    /// `None` for a commit without a signature.
    pub signature: Option<Signature>,
}

/// A commit's signature as git checked it (`%G?`): `G` good, `U` good but of unknown trust,
/// `X` good but expired, `Y` made by an expired key, `R` by a revoked key, `B` bad, `E` can't
/// be checked (no key, or no gpg).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub status: char,
    pub signer: String,
    pub key: String,
}

/// A diff or merge tool git can start (`git difftool` / `git mergetool`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalTool {
    /// What git calls it: `kdiff3`, `bc`, or a name from `difftool.<name>.cmd`.
    pub name: String,
    /// For people: "KDiff3", "Beyond Compare".
    pub label: String,
}

/// Which two versions of a file an external diff tool compares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolTarget {
    /// The working tree against the index, or with `staged` the index against HEAD.
    Working { staged: bool },
    /// A commit against its first parent.
    Commit { sha: String },
    /// One commit against another.
    Compare { base: String, target: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub status: String,
    pub orig_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictFile {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchStatus {
    pub head: Option<String>,
    pub oid: Option<String>,
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
    pub detached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Operation {
    #[serde(rename = "am")]
    Am,
    #[serde(rename = "rebase", rename_all = "camelCase")]
    Rebase {
        branch: Option<String>,
        onto: Option<String>,
        step: Option<i64>,
        total: Option<i64>,
        stopped_sha: Option<String>,
    },
    #[serde(rename = "merge")]
    Merge { head: String },
    #[serde(rename = "cherry-pick")]
    CherryPick { head: String },
    #[serde(rename = "revert")]
    Revert { head: String },
}

impl Operation {
    /// The git command that continues, skips or aborts this operation.
    pub fn command(&self) -> &'static str {
        match self {
            Self::Am => "am",
            Self::Rebase { .. } => "rebase",
            Self::Merge { .. } => "merge",
            Self::CherryPick { .. } => "cherry-pick",
            Self::Revert { .. } => "revert",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub branch: BranchStatus,
    pub staged: Vec<FileChange>,
    pub unstaged: Vec<FileChange>,
    pub conflicts: Vec<ConflictFile>,
    pub operation: Option<Operation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBranch {
    pub name: String,
    pub sha: String,
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
    pub upstream_gone: bool,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteBranch {
    pub name: String,
    pub remote: String,
    pub branch: String,
    pub sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub sha: String,
    pub annotated: bool,
    pub message: String,
    pub time: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Remote {
    pub name: String,
    pub fetch_url: String,
    pub push_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stash {
    pub name: String,
    pub sha: String,
    pub time: i64,
    pub message: String,
}

/// One working tree of the repository, as listed by `git worktree list`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    pub path: String,
    pub sha: String,
    /// The checked-out branch, without `refs/heads/`; `None` when detached or bare.
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
    /// The first entry: the working tree that holds the repository itself.
    pub main: bool,
    /// The working tree of the repository this query was run on.
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submodule {
    pub sha: String,
    pub path: String,
    pub state: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refs {
    pub local: Vec<LocalBranch>,
    pub remote: Vec<RemoteBranch>,
    pub tags: Vec<Tag>,
    pub remotes: Vec<Remote>,
    pub stashes: Vec<Stash>,
    pub worktrees: Vec<Worktree>,
    pub submodules: Vec<Submodule>,
    pub head: Option<String>,
    /// A shallow clone: older history wasn't downloaded.
    #[serde(default)]
    pub shallow: bool,
    /// A partial clone: some files' contents are downloaded only when needed.
    #[serde(default)]
    pub partial: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub t: char,
    pub text: String,
    pub old: Option<u32>,
    pub new: Option<u32>,
    /// The original bytes, when they are not valid UTF-8 and `text` is therefore lossy.
    #[serde(skip)]
    pub raw: Option<Vec<u8>>,
}

impl DiffLine {
    pub fn bytes(&self) -> &[u8] {
        self.raw.as_deref().unwrap_or(self.text.as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub context: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub header: Vec<String>,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub status: String,
    pub binary: bool,
    pub truncated: bool,
    pub hunks: Vec<Hunk>,
    /// `header` as bytes, for rebuilding patches.
    #[serde(skip)]
    pub raw_header: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    pub sha: String,
    pub line: u32,
    pub text: String,
    pub author: String,
    pub time: i64,
    pub summary: String,
}

/// What a new repository starts with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitOptions {
    /// The first branch's name; git's default (`init.defaultBranch`) when `None`.
    pub branch: Option<String>,
    /// A `.gitignore` for this language (see `gitignore::LANGUAGES`).
    pub gitignore: Option<String>,
    /// Also ignore editor settings and operating-system clutter.
    pub ignore_editors: bool,
    /// A README.md with the folder's name as its title.
    pub readme: bool,
    /// Commit what was created (or an empty commit) as "Initial commit".
    pub commit: bool,
}

/// How much of a repository a clone downloads, and which branch it checks out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloneOptions {
    /// The branch to check out instead of the remote's default.
    pub branch: Option<String>,
    /// Only the newest commits (a shallow clone)…
    pub depth: Option<u32>,
    /// …or only those since a date, such as `2024-01-01`.
    pub since: Option<String>,
    pub submodules: bool,
    /// Files' contents are downloaded only when needed (`--filter=blob:none`).
    pub blobless: bool,
    /// Only these folders or patterns are checked out (sparse checkout); empty for all.
    pub sparse: Vec<String>,
}

impl Default for CloneOptions {
    fn default() -> Self {
        Self {
            branch: None,
            depth: None,
            since: None,
            submodules: true,
            blobless: false,
            sparse: Vec::new(),
        }
    }
}

/// How a commit is made, besides its message and whether it amends.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitOptions {
    /// Adds a `Signed-off-by` line for the committer.
    pub sign_off: bool,
    /// Skips the pre-commit and commit-msg hooks.
    pub no_verify: bool,
    /// Stages every change first, including new and deleted files.
    pub stage_all: bool,
    /// Trailers such as `Co-authored-by: Name <mail>`.
    pub trailers: Vec<String>,
}

/// What the history shows only: commits by an author, touching a path, or adding or removing
/// some text. Empty parts don't filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogFilter {
    /// Part of the author's name or address, any case.
    pub author: String,
    /// A file or folder, relative to the repository.
    pub path: String,
    /// Text a commit adds or removes (`-S`), or a regular expression its changed lines match
    /// (`-G`) when `regex` is set.
    pub text: String,
    pub regex: bool,
}

impl LogFilter {
    pub fn is_empty(&self) -> bool {
        self.author.trim().is_empty() && self.path.trim().is_empty() && self.text.is_empty()
    }

    /// With only a path, git connects the commits it keeps (history simplification), so the
    /// graph still makes sense; other filters leave gaps.
    pub fn keeps_graph(&self) -> bool {
        self.author.trim().is_empty() && self.text.is_empty()
    }
}

/// Which branches the graph of all branches leaves out (`hidden`), or the only ones it shows
/// (`solo`, which wins). Full ref names such as `refs/heads/topic`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RefView {
    pub hidden: Vec<String>,
    pub solo: Vec<String>,
}

impl RefView {
    pub fn is_empty(&self) -> bool {
        self.hidden.is_empty() && self.solo.is_empty()
    }
}

/// How much a commit changed: files, and lines added and removed (`--shortstat`; a merge
/// against its first parent).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitStats {
    pub files: u32,
    pub added: u32,
    pub removed: u32,
}

/// A local branch that looks done with, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupCandidate {
    pub name: String,
    pub sha: String,
    /// `merged` into the default branch, `squashed` (its changes are there as other
    /// commits: squash- or rebase-merged), or `gone` (its upstream was deleted).
    pub reason: String,
    /// Its upstream, as `remote/branch`, when it still exists.
    pub upstream: Option<String>,
    /// Unix seconds of its last commit.
    pub time: i64,
}

/// How the history is listed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogOptions {
    pub view: RefView,
    /// Newest first by commit date, rather than keeping each branch's commits together.
    pub date_order: bool,
    /// Stashes as rows right above the commit each was made on.
    pub stashes: bool,
}

/// A bisect in progress, read from `BISECT_LOG`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bisect {
    /// The newest commit marked bad.
    pub bad: Option<String>,
    /// Commits marked good and skipped so far.
    pub good: usize,
    pub skipped: usize,
    /// Set once git has found it.
    pub first_bad: Option<String>,
}

/// What undo and redo would do, as English labels such as "Commit".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoLabels {
    pub undo: Option<String>,
    pub redo: Option<String>,
}

/// Discarded changes kept so they can be put back (`refs/gitcraft/discards/…`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscardBackup {
    pub name: String,
    pub sha: String,
    /// Unix seconds of the discard.
    pub time: i64,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflogEntry {
    pub sha: String,
    pub selector: String,
    pub action: String,
    pub time: i64,
    pub subject: String,
    pub author: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub files: Vec<FileChange>,
    pub base_only: i64,
    pub target_only: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ConflictChunk {
    Common {
        lines: Vec<String>,
    },
    Conflict {
        ours: Vec<String>,
        base: Vec<String>,
        theirs: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictVersions {
    pub path: String,
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
    pub working: Option<String>,
    pub chunks: Vec<ConflictChunk>,
    pub ours_label: String,
    pub theirs_label: String,
}

/// What a mutating operation reports back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpResult {
    pub ok: bool,
    pub conflicts: bool,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<Operation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebaseStep {
    pub sha: String,
    pub action: String,
    #[serde(default)]
    pub message: Option<String>,
    /// A shell command run right after this step (an `exec` line); the rebase stops when it
    /// fails.
    #[serde(default)]
    pub exec: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoEntry {
    pub path: String,
    pub name: String,
    pub last_opened: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exists: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_repo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    pub path: String,
    pub parent: Option<String>,
    pub home: String,
    pub roots: Vec<String>,
    pub is_repo: bool,
    pub dirs: Vec<DirEntry>,
}
