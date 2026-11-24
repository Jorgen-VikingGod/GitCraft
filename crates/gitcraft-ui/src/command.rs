//! Every git operation the UI can start, as data: what to run and how to describe it.

use gitcraft_core::model::{CommitOptions, OpResult, RebaseStep};
use gitcraft_core::parsers::Selection;
use gitcraft_core::runner::Progress;
use gitcraft_core::undo::Mode;
use gitcraft_core::{Result, ops, queries};

use crate::i18n::{tr, trf};
use crate::logic::{plural, short_sha};

#[derive(Debug, Clone)]
pub enum Command {
    Fetch {
        remote: Option<String>,
        tags: bool,
    },
    Pull {
        remote: Option<String>,
        branch: Option<String>,
        /// "merge", "rebase", "ff-only", or "overwrite" to make the branch equal the remote's.
        mode: String,
        autostash: bool,
        /// With "overwrite": keep the old tip as a backup branch.
        backup: bool,
    },
    Push {
        remote: String,
        branch: String,
        set_upstream: bool,
        force: bool,
        tags: bool,
    },
    StashPush {
        message: Option<String>,
        include_untracked: bool,
        staged_only: bool,
        keep_index: bool,
        paths: Option<Vec<String>>,
    },
    StashApply {
        name: String,
        pop: bool,
    },
    StashDrop {
        name: String,
    },
    StashUntracked {
        message: Option<String>,
    },
    /// `id` is the stash's commit, or `stash@{n}`.
    StashRename {
        id: String,
        message: String,
    },
    StashFile {
        id: String,
        path: String,
    },
    WorktreeAdd {
        path: String,
        start: Option<String>,
        new_branch: Option<String>,
    },
    WorktreeRemove {
        path: String,
        force: bool,
    },
    WorktreePrune,
    Maintenance {
        gc: bool,
        graph: bool,
        prune: bool,
    },
    WorktreeLock {
        path: String,
        reason: Option<String>,
    },
    WorktreeUnlock {
        path: String,
    },
    CreateBranch {
        name: String,
        start: Option<String>,
        checkout: bool,
    },
    Checkout {
        reference: String,
        kind: &'static str,
    },
    /// A checkout with uncommitted changes in the way: they go into a stash first, and come back
    /// afterwards when `reapply` is set.
    CheckoutStashing {
        reference: String,
        kind: &'static str,
        reapply: bool,
    },
    RenameBranch {
        old: String,
        new: String,
    },
    DeleteBranch {
        name: String,
        force: bool,
        remote: Option<(String, String)>,
    },
    DeleteRemoteBranch {
        remote: String,
        branch: String,
    },
    /// Local branches reviewed as done with, and the (remote, branch) pairs to delete too.
    DeleteBranches {
        names: Vec<String>,
        remotes: Vec<(String, String)>,
    },
    SetUpstream {
        branch: String,
        upstream: Option<String>,
    },
    Merge {
        reference: String,
        mode: String,
    },
    Rebase {
        onto: String,
        autostash: bool,
    },
    InteractiveRebase {
        /// HEAD when the plan was made; the rebase is refused once HEAD has moved.
        expected_head: Option<String>,
        base: Option<String>,
        steps: Vec<RebaseStep>,
        autostash: bool,
        /// Replays the steps' commits on top of HEAD instead (interactive cherry-pick).
        replay: bool,
    },
    QuickRebase {
        sha: String,
        base: Option<String>,
        action: &'static str,
        message: Option<String>,
    },
    CherryPick {
        sha: String,
    },
    /// A merge commit, relative to parent `mainline` (1 is the first).
    CherryPickMerge {
        sha: String,
        mainline: i64,
    },
    /// Several commits, oldest first.
    CherryPickMany {
        shas: Vec<String>,
    },
    Revert {
        sha: String,
        mainline: Option<i64>,
    },
    Continue,
    Abort,
    Skip,
    CreateTag {
        name: String,
        target: String,
        message: Option<String>,
        push_to: Vec<String>,
    },
    DeleteTag {
        name: String,
        remotes: Vec<String>,
    },
    PushTag {
        remote: String,
        name: String,
    },
    /// Deletes the tag on `remote`, keeping it here.
    DeleteRemoteTag {
        remote: String,
        name: String,
    },
    AddRemote {
        name: String,
        url: String,
        fetch: bool,
    },
    EditRemote {
        name: String,
        new_name: String,
        url: String,
        /// Empty: pushes go to `url`.
        push_url: String,
    },
    RemoveRemote {
        name: String,
    },
    Reset {
        target: String,
        mode: String,
        undo: bool,
    },
    ApplyPatch {
        patch: String,
        mode: String,
    },
    FastForward {
        branch: String,
        target: Option<String>,
    },
    Ignore {
        patterns: Vec<String>,
    },
    Clean {
        directories: bool,
        ignored: bool,
    },
    CheckoutFile {
        sha: String,
        path: String,
    },
    PruneRemote {
        name: String,
    },
    UpdateSubmodules,
    SubmoduleAdd {
        url: String,
        path: String,
    },
    SubmoduleUpdate {
        path: String,
    },
    SubmoduleRemove {
        path: String,
    },
    Stage {
        paths: Vec<String>,
    },
    StageAll,
    Unstage {
        paths: Vec<String>,
    },
    UnstageAll,
    Discard {
        paths: Vec<String>,
    },
    RestoreDiscard {
        name: String,
    },
    /// Takes the selected hunks/lines of a file of commit `sha` back out of the working tree.
    RevertSelection {
        sha: String,
        path: String,
        selection: Selection,
        context: i64,
    },
    /// Stashes the selected hunks/lines of one file.
    StashSelection {
        path: String,
        selection: Selection,
        context: i64,
    },
    DeleteDiscard {
        name: String,
    },
    Selection {
        path: String,
        mode: &'static str,
        selection: Selection,
        context: i64,
    },
    Resolve {
        path: String,
        content: Option<String>,
        side: Option<String>,
    },
    Commit {
        message: String,
        amend: bool,
        options: CommitOptions,
    },
    /// Replaces the remote branch, but only while it is still at `expected`.
    ForcePushOver {
        remote: String,
        branch: String,
        expected: String,
    },
    /// Downloads what a shallow or partial clone left out: deepen, unshallow or refetch.
    FetchMore {
        how: &'static str,
    },
    /// Checks out only what the patterns match.
    SparseSet {
        patterns: Vec<String>,
    },
    SparseOff,
    BisectStart {
        bad: String,
        good: String,
    },
    /// `verdict` is good, bad or skip; `sha` is HEAD when `None`.
    BisectMark {
        verdict: &'static str,
        sha: Option<String>,
    },
    BisectReset,
    /// Puts the conflict markers back into resolved files.
    MarkConflicted {
        paths: Vec<String>,
    },
    /// A `fixup!` commit of the staged changes for `sha`.
    FixupCommit {
        sha: String,
    },
    /// `label` is the English label of what is undone, for the messages.
    Undo {
        label: String,
    },
    Redo {
        label: String,
    },
}

/// Every label [`Command::undo_kind`] gives, so a test can check each has a translation.
pub const UNDO_LABELS: [&str; 19] = [
    "Amend",
    "Commit",
    "Undo last commit",
    "Reset",
    "New branch",
    "Rename branch",
    "Delete branch",
    "New tag",
    "Delete tag",
    "Checkout",
    "Fast-forward",
    "Merge",
    "Rebase",
    "Edit history",
    "Cherry-pick",
    "Revert",
    "Pull",
    "Apply patch",
    "Discard",
];

impl Command {
    /// Shown while it runs, and in the failure message, in the interface language.
    pub fn label(&self) -> String {
        let text: String = match self {
            Self::Fetch {
                remote: Some(remote),
                ..
            } => trf("Fetching {remote}", &[("remote", remote)]),
            Self::Fetch { .. } => "Fetching".into(),
            Self::Pull { mode, .. } if mode == "rebase" => "Pulling (rebase)".into(),
            Self::Pull { mode, .. } if mode == "overwrite" => "Overwriting from remote".into(),
            Self::Pull { .. } => "Pulling".into(),
            Self::Push { .. } => "Pushing".into(),
            Self::StashPush { .. } => "Stashing".into(),
            Self::StashApply { pop: true, .. } => "Popping stash".into(),
            Self::StashApply { .. } => "Applying stash".into(),
            Self::StashDrop { .. } => "Dropping stash".into(),
            Self::StashUntracked { .. } => "Stashing".into(),
            Self::StashRename { .. } => "Renaming stash".into(),
            Self::StashFile { .. } => "Taking the file from the stash".into(),
            Self::WorktreeAdd { .. } => "Adding worktree".into(),
            Self::WorktreeRemove { .. } => "Removing worktree".into(),
            Self::WorktreePrune => "Pruning worktrees".into(),
            Self::Maintenance { .. } => "Tidying up the repository".into(),
            Self::WorktreeLock { .. } => "Locking worktree".into(),
            Self::WorktreeUnlock { .. } => "Unlocking worktree".into(),
            Self::CreateBranch { .. } => "Creating branch".into(),
            Self::Checkout { reference, kind }
            | Self::CheckoutStashing {
                reference, kind, ..
            } => {
                let name = if *kind == "commit" {
                    short_sha(reference)
                } else {
                    reference
                };
                trf("Checking out {name}", &[("name", name)])
            }
            Self::RenameBranch { .. } => "Renaming branch".into(),
            Self::DeleteBranch { .. } => "Deleting branch".into(),
            Self::DeleteRemoteBranch { .. } => "Deleting remote branch".into(),
            Self::DeleteBranches { .. } => "Deleting branches".into(),
            Self::SetUpstream { .. } => "Setting upstream".into(),
            Self::Merge { .. } => "Merging".into(),
            Self::Rebase { .. } | Self::InteractiveRebase { .. } => "Rebasing".into(),
            Self::QuickRebase { .. } => "Rewriting history".into(),
            Self::CherryPick { .. }
            | Self::CherryPickMany { .. }
            | Self::CherryPickMerge { .. } => "Cherry-picking".into(),
            Self::Revert { .. } => "Reverting".into(),
            Self::Continue => "Continuing".into(),
            Self::Abort => "Aborting".into(),
            Self::Skip => "Skipping".into(),
            Self::CreateTag { .. } => "Creating tag".into(),
            Self::DeleteTag { .. } => "Deleting tag".into(),
            Self::PushTag { .. } => "Pushing tag".into(),
            Self::DeleteRemoteTag { .. } => "Deleting tag from remote".into(),
            Self::AddRemote { .. } => "Adding remote".into(),
            Self::EditRemote { .. } => "Saving remote".into(),
            Self::RemoveRemote { .. } => "Removing remote".into(),
            Self::Reset { undo: true, .. } => "Undoing commit".into(),
            Self::Reset { mode, .. } => trf("Resetting ({mode})", &[("mode", mode)]),
            Self::ApplyPatch { .. } => "Applying patch".into(),
            Self::FastForward { .. } => "Fast-forwarding".into(),
            Self::Ignore { .. } => "Updating .gitignore".into(),
            Self::Clean { .. } => "Cleaning".into(),
            Self::CheckoutFile { .. } => "Checking out file".into(),
            Self::PruneRemote { name } => trf("Pruning {name}", &[("name", name)]),
            Self::UpdateSubmodules => "Updating submodules".into(),
            Self::SubmoduleAdd { .. } => "Adding submodule".into(),
            Self::SubmoduleUpdate { path } => trf("Updating {path}", &[("path", path)]),
            Self::SubmoduleRemove { .. } => "Removing submodule".into(),
            Self::Stage { .. } => "Staging".into(),
            Self::StageAll => "Staging all".into(),
            Self::Unstage { .. } => "Unstaging".into(),
            Self::UnstageAll => "Unstaging all".into(),
            Self::Discard { .. } => "Discarding".into(),
            Self::RestoreDiscard { .. } => "Restoring discarded changes".into(),
            Self::StashSelection { .. } => "Stashing".into(),
            Self::RevertSelection { .. } => "Reverting lines".into(),
            Self::DeleteDiscard { .. } => "Deleting backup".into(),
            Self::Selection { mode, .. } => match *mode {
                "stage" => "Staging".into(),
                "unstage" => "Unstaging".into(),
                _ => "Discarding".into(),
            },
            Self::Resolve { .. } => "Resolving conflict".into(),
            Self::Commit { amend: true, .. } => "Amending commit".into(),
            Self::Commit { .. } => "Committing".into(),
            Self::FixupCommit { .. } => "Committing".into(),
            Self::MarkConflicted { .. } => "Marking as conflicted".into(),
            Self::BisectStart { .. } | Self::BisectMark { .. } => "Bisecting".into(),
            Self::FetchMore { .. } => "Fetching more history".into(),
            Self::SparseSet { .. } | Self::SparseOff => "Changing the sparse checkout".into(),
            Self::ForcePushOver { .. } => "Force pushing".into(),
            Self::BisectReset => "Ending bisect".into(),
            Self::Undo { .. } => "Undoing".into(),
            Self::Redo { .. } => "Redoing".into(),
        };
        tr(&text).to_owned()
    }

    /// The toast after it succeeded, if it deserves one.
    pub fn success(&self) -> Option<String> {
        let text: String = match self {
            Self::Fetch { .. } => "Fetch complete".into(),
            Self::Pull { mode, .. } if mode == "overwrite" => "Branch overwritten".into(),
            Self::Pull { .. } => "Pull complete".into(),
            Self::Push { .. } => "Push complete".into(),
            Self::StashPush { paths: Some(_), .. } => "Files stashed".into(),
            Self::StashPush { .. } => "Changes stashed".into(),
            Self::WorktreeAdd { path, .. } => trf("Added worktree {path}", &[("path", path)]),
            Self::WorktreeRemove { .. } => "Worktree removed".into(),
            Self::WorktreePrune => "Stale worktrees pruned".into(),
            Self::Maintenance { .. } => "Repository tidied up".into(),
            Self::WorktreeLock { .. } => "Worktree locked".into(),
            Self::WorktreeUnlock { .. } => "Worktree unlocked".into(),
            Self::StashApply { pop: true, .. } => "Stash popped".into(),
            Self::StashApply { .. } => "Stash applied".into(),
            Self::StashUntracked { .. } => "Untracked files stashed".into(),
            Self::StashRename { .. } => "Stash renamed".into(),
            Self::StashFile { path, .. } => trf("{path} taken from the stash", &[("path", path)]),
            Self::CreateBranch { name, .. } => trf("Created {name}", &[("name", name)]),
            Self::DeleteBranches { names, .. } => trf(
                "Deleted {branches}",
                &[("branches", &plural(names.len(), "branch"))],
            ),
            Self::Merge { reference, .. } => trf("Merged {reference}", &[("reference", reference)]),
            Self::Rebase { .. } => "Rebase complete".into(),
            Self::InteractiveRebase { replay: true, .. } => "Commits cherry-picked".into(),
            Self::InteractiveRebase { .. } => "Interactive rebase complete".into(),
            Self::QuickRebase { .. } => "History rewritten".into(),
            Self::CherryPickMerge { sha, .. } | Self::CherryPick { sha } => {
                trf("Cherry-picked {commit}", &[("commit", short_sha(sha))])
            }
            Self::CherryPickMany { shas } => trf(
                "Cherry-picked {commits}",
                &[("commits", &plural(shas.len(), "commit"))],
            ),
            Self::Revert { .. } => "Commit reverted".into(),
            Self::CreateTag { name, .. } => trf("Created tag {name}", &[("name", name)]),
            Self::PushTag { name, .. } => trf("Pushed {name}", &[("name", name)]),
            Self::DeleteRemoteTag { remote, name } => trf(
                "Deleted {name} from {remote}",
                &[("name", name), ("remote", remote)],
            ),
            Self::Reset { undo: true, .. } => "Last commit undone; its changes are staged".into(),
            Self::Reset { target, .. } => {
                trf("Reset to {commit}", &[("commit", short_sha(target))])
            }
            Self::ApplyPatch { .. } => "Patch applied".into(),
            Self::FastForward { branch, .. } => {
                trf("{branch} fast-forwarded", &[("branch", branch)])
            }
            Self::Ignore { patterns } => match &patterns[..] {
                [one] => trf("Added “{pattern}” to .gitignore", &[("pattern", one)]),
                many => trf(
                    "Added {patterns} to .gitignore",
                    &[("patterns", &plural(many.len(), "pattern"))],
                ),
            },
            Self::Clean { .. } => "Cleaned".into(),
            Self::CheckoutFile { sha, path } => trf(
                "{path} restored from {commit}",
                &[("path", path), ("commit", short_sha(sha))],
            ),
            Self::PruneRemote { name } => trf("Pruned stale branches of {name}", &[("name", name)]),
            Self::UpdateSubmodules => "Submodules updated".into(),
            Self::SubmoduleAdd { path, .. } => trf(
                "Added submodule {path}; commit to keep it",
                &[("path", path)],
            ),
            Self::SubmoduleUpdate { path } => trf("{path} updated", &[("path", path)]),
            Self::SubmoduleRemove { path } => trf(
                "Removed submodule {path}; commit to keep it that way",
                &[("path", path)],
            ),
            Self::FetchMore { .. } => "More history fetched".into(),
            Self::SparseSet { .. } => "Sparse checkout updated".into(),
            Self::SparseOff => "Every file is checked out again".into(),
            Self::ForcePushOver { .. } => "Push complete".into(),
            Self::FixupCommit { sha } => trf(
                "Fixup commit for {commit} created; an interactive rebase with autosquash folds it in",
                &[("commit", short_sha(sha))],
            ),
            Self::Undo { label } => trf("Undone: {action}", &[("action", tr(label))]),
            Self::Redo { label } => trf("Redone: {action}", &[("action", tr(label))]),
            Self::RestoreDiscard { .. } => "Discarded changes restored".into(),
            Self::StashSelection { .. } => "Selected changes stashed".into(),
            Self::RevertSelection { path, .. } => trf(
                "Reverted in {path}; the change is uncommitted",
                &[("path", path)],
            ),
            Self::DeleteDiscard { .. } => "Backup deleted".into(),
            Self::Resolve { path, .. } => trf("Resolved {path}", &[("path", path)]),
            Self::CheckoutStashing { reapply: true, .. } => {
                "Checked out, with your changes brought along".into()
            }
            Self::CheckoutStashing { .. } => "Checked out; your changes are kept in a stash".into(),
            Self::Commit { amend: true, .. } => "Commit amended".into(),
            Self::Commit { .. } => "Committed".into(),
            _ => return None,
        };
        Some(tr(&text).to_owned())
    }

    /// What Edit › Undo calls this afterwards (English), and how undoing it moves the
    /// current branch. `None` for what can't be undone: anything that changed a remote, stash
    /// operations, settings of remotes and worktrees.
    pub fn undo_kind(&self) -> Option<(&'static str, Mode)> {
        Some(match self {
            Self::Commit { amend: true, .. } => ("Amend", Mode::Soft),
            Self::Commit { .. } | Self::FixupCommit { .. } => ("Commit", Mode::Soft),
            Self::Reset { undo: true, .. } => ("Undo last commit", Mode::Soft),
            Self::Reset { mode, .. } => (
                "Reset",
                match mode.as_str() {
                    "soft" => Mode::Soft,
                    "mixed" => Mode::Mixed,
                    "hard" => Mode::Hard,
                    _ => Mode::Keep,
                },
            ),
            Self::CreateBranch { .. } => ("New branch", Mode::Keep),
            Self::RenameBranch { .. } => ("Rename branch", Mode::Keep),
            Self::DeleteBranch { remote: None, .. } => ("Delete branch", Mode::Keep),
            Self::DeleteBranches { remotes, .. } if remotes.is_empty() => {
                ("Delete branch", Mode::Keep)
            }
            Self::CreateTag { push_to, .. } if push_to.is_empty() => ("New tag", Mode::Keep),
            Self::DeleteTag { remotes, .. } if remotes.is_empty() => ("Delete tag", Mode::Keep),
            Self::Checkout { .. } | Self::CheckoutStashing { .. } => ("Checkout", Mode::Keep),
            Self::FastForward { .. } => ("Fast-forward", Mode::Keep),
            Self::Merge { .. } => ("Merge", Mode::Keep),
            Self::Rebase { .. } => ("Rebase", Mode::Keep),
            Self::InteractiveRebase { replay: true, .. } => ("Cherry-pick", Mode::Keep),
            Self::InteractiveRebase { .. } | Self::QuickRebase { .. } => {
                ("Edit history", Mode::Keep)
            }
            Self::CherryPick { .. }
            | Self::CherryPickMany { .. }
            | Self::CherryPickMerge { .. } => ("Cherry-pick", Mode::Keep),
            Self::Revert { .. } => ("Revert", Mode::Keep),
            Self::Pull { mode, .. } if mode == "overwrite" => ("Pull", Mode::Hard),
            Self::Pull { .. } => ("Pull", Mode::Keep),
            Self::ApplyPatch { .. } => ("Apply patch", Mode::Keep),
            Self::Discard { .. } => ("Discard", Mode::Keep),
            Self::Selection { mode, .. } if *mode == "discard" => ("Discard", Mode::Keep),
            _ => return None,
        })
    }

    /// Network operations can be stopped at any point without leaving a mess; history rewrites can't.
    pub fn cancellable(&self) -> bool {
        matches!(
            self,
            Self::Fetch { .. }
                | Self::Push { .. }
                | Self::DeleteRemoteBranch { .. }
                | Self::PushTag { .. }
                | Self::DeleteRemoteTag { .. }
                | Self::PruneRemote { .. }
                | Self::UpdateSubmodules
                | Self::SubmoduleAdd { .. }
                | Self::SubmoduleUpdate { .. }
                | Self::FetchMore { .. }
                | Self::ForcePushOver { .. }
        )
    }

    /// True when only the working tree or index changed, so history needs no reload.
    pub fn status_only(&self) -> bool {
        matches!(
            self,
            Self::Stage { .. }
                | Self::StageAll
                | Self::Unstage { .. }
                | Self::UnstageAll
                | Self::Discard { .. }
                | Self::RestoreDiscard { .. }
                | Self::RevertSelection { .. }
                | Self::MarkConflicted { .. }
                | Self::DeleteDiscard { .. }
                | Self::Selection { .. }
                | Self::Resolve { .. }
        )
    }

    pub async fn run(self, repo: &str, progress: Option<Progress>) -> Result<OpResult> {
        let report = progress.as_ref();
        let done = || OpResult {
            ok: true,
            conflicts: false,
            message: String::new(),
            operation: None,
            files: None,
        };
        match self {
            Self::Fetch { remote, tags } => {
                ops::fetch(repo, remote.as_deref(), true, tags, report).await
            }
            Self::Pull {
                remote,
                branch,
                mode,
                autostash,
                backup,
            } if mode == "overwrite" => {
                let _ = autostash;
                ops::pull_overwrite(repo, remote.as_deref(), branch.as_deref(), backup, report)
                    .await
            }
            Self::Pull {
                remote,
                branch,
                mode,
                autostash,
                backup: _,
            } => {
                ops::pull(
                    repo,
                    remote.as_deref(),
                    branch.as_deref(),
                    &mode,
                    autostash,
                    report,
                )
                .await
            }
            Self::Push {
                remote,
                branch,
                set_upstream,
                force,
                tags,
            } => {
                ops::push(
                    repo,
                    &remote,
                    Some(&branch),
                    set_upstream,
                    force,
                    tags,
                    report,
                )
                .await
            }
            Self::StashPush {
                message,
                include_untracked,
                staged_only,
                keep_index,
                paths,
            } => {
                ops::stash_push(
                    repo,
                    message.as_deref(),
                    include_untracked,
                    staged_only,
                    keep_index,
                    paths.as_deref(),
                )
                .await
            }
            Self::StashApply { name, pop } => ops::stash_apply(repo, &name, pop).await,
            Self::StashDrop { name } => ops::stash_drop(repo, &name).await,
            Self::StashUntracked { message } => {
                ops::stash_untracked(repo, message.as_deref()).await
            }
            Self::StashRename { id, message } => ops::stash_rename(repo, &id, &message).await,
            Self::StashFile { id, path } => ops::stash_restore_file(repo, &id, &path).await,
            Self::WorktreeAdd {
                path,
                start,
                new_branch,
            } => ops::worktree_add(repo, &path, start.as_deref(), new_branch.as_deref()).await,
            Self::WorktreeRemove { path, force } => ops::worktree_remove(repo, &path, force).await,
            Self::WorktreePrune => ops::worktree_prune(repo).await,
            Self::Maintenance { gc, graph, prune } => {
                ops::maintenance(repo, gc, graph, prune, report).await
            }
            Self::WorktreeLock { path, reason } => {
                ops::worktree_lock(repo, &path, reason.as_deref()).await
            }
            Self::WorktreeUnlock { path } => ops::worktree_unlock(repo, &path).await,
            Self::CreateBranch {
                name,
                start,
                checkout,
            } => ops::create_branch(repo, &name, start.as_deref(), checkout).await,
            Self::Checkout { reference, kind } => ops::checkout(repo, &reference, kind).await,
            Self::CheckoutStashing {
                reference,
                kind,
                reapply,
            } => ops::checkout_stashing(repo, &reference, kind, reapply).await,
            Self::RenameBranch { old, new } => ops::rename_branch(repo, &old, &new).await,
            Self::DeleteBranch {
                name,
                force,
                remote,
            } => {
                let result = ops::delete_branch(repo, &name, force).await?;
                if let Some((remote, branch)) = remote {
                    ops::delete_remote_branch(repo, &remote, &branch, report).await?;
                }
                Ok(result)
            }
            Self::DeleteRemoteBranch { remote, branch } => {
                ops::delete_remote_branch(repo, &remote, &branch, report).await
            }
            Self::SetUpstream { branch, upstream } => {
                ops::set_upstream(repo, &branch, upstream.as_deref()).await
            }
            Self::Merge { reference, mode } => ops::merge(repo, &reference, &mode).await,
            Self::Rebase { onto, autostash } => ops::rebase(repo, &onto, autostash).await,
            Self::InteractiveRebase {
                expected_head,
                base,
                steps,
                autostash,
                replay,
            } => {
                if expected_head.is_some() && queries::head_sha(repo).await? != expected_head {
                    return Err(gitcraft_core::Error::Invalid(
                        tr("The branch has changed since this plan was made; open the editor again")
                            .to_owned(),
                    ));
                }
                if replay {
                    ops::interactive_cherry_pick(repo, &steps, autostash).await
                } else {
                    ops::interactive_rebase(repo, base.as_deref(), &steps, autostash).await
                }
            }
            Self::QuickRebase {
                sha,
                base,
                action,
                message,
            } => {
                let candidates = queries::rebase_candidates(repo, base.as_deref()).await?;
                if !candidates.iter().any(|commit| commit.sha == sha) {
                    return Err(gitcraft_core::Error::Invalid(
                        "Only commits on the current branch can be edited".to_owned(),
                    ));
                }
                if let Some(up) = match action {
                    "up" => Some(true),
                    "down" => Some(false),
                    _ => None,
                } {
                    let mut steps: Vec<RebaseStep> = candidates
                        .into_iter()
                        .map(|commit| RebaseStep {
                            sha: commit.sha,
                            action: "pick".to_owned(),
                            message: None,
                            exec: None,
                        })
                        .collect();
                    let at = steps.iter().position(|step| step.sha == sha).unwrap_or(0);
                    // Oldest first: "up" (newer) is later in the list.
                    let other = if up { at + 1 } else { at.wrapping_sub(1) };
                    if other >= steps.len() {
                        return Err(gitcraft_core::Error::Invalid(
                            tr("The commit can't move further that way").to_owned(),
                        ));
                    }
                    steps.swap(at, other);
                    return ops::interactive_rebase(repo, base.as_deref(), &steps, true).await;
                }
                let steps: Vec<RebaseStep> = candidates
                    .into_iter()
                    .map(|commit| {
                        let chosen = commit.sha == sha;
                        RebaseStep {
                            action: if chosen { action } else { "pick" }.to_owned(),
                            message: if chosen { message.clone() } else { None },
                            sha: commit.sha,
                            exec: None,
                        }
                    })
                    .collect();
                ops::interactive_rebase(repo, base.as_deref(), &steps, true).await
            }
            Self::CherryPick { sha } => ops::cherry_pick(repo, &[sha]).await,
            Self::CherryPickMerge { sha, mainline } => {
                ops::cherry_pick_merge(repo, &sha, mainline).await
            }
            Self::CherryPickMany { shas } => ops::cherry_pick(repo, &shas).await,
            Self::Revert { sha, mainline } => ops::revert(repo, &sha, mainline).await,
            Self::Continue => ops::continue_operation(repo).await,
            Self::Abort => ops::abort_operation(repo).await,
            Self::Skip => ops::skip_operation(repo).await,
            Self::CreateTag {
                name,
                target,
                message,
                push_to,
            } => {
                let result = ops::create_tag(repo, &name, &target, message.as_deref()).await?;
                for remote in push_to {
                    ops::push_tag(repo, &remote, &name, false, report).await?;
                }
                Ok(result)
            }
            Self::DeleteTag { name, remotes } => {
                let result = ops::delete_tag(repo, &name).await?;
                for remote in remotes {
                    ops::push_tag(repo, &remote, &name, true, report).await?;
                }
                Ok(result)
            }
            Self::PushTag { remote, name } => {
                ops::push_tag(repo, &remote, &name, false, report).await
            }
            Self::DeleteRemoteTag { remote, name } => {
                ops::push_tag(repo, &remote, &name, true, report).await
            }
            Self::AddRemote { name, url, fetch } => {
                let result = ops::add_remote(repo, &name, &url).await?;
                if fetch {
                    ops::fetch(repo, Some(&name), true, false, report).await?;
                }
                Ok(result)
            }
            Self::EditRemote {
                name,
                new_name,
                url,
                push_url,
            } => ops::edit_remote(repo, &name, Some(&new_name), Some(&url), Some(&push_url)).await,
            Self::RemoveRemote { name } => ops::remove_remote(repo, &name).await,
            Self::Reset { target, mode, .. } => ops::reset(repo, &target, &mode).await,
            Self::ApplyPatch { patch, mode } => ops::apply_patch(repo, &patch, &mode).await,
            Self::FastForward { branch, target } => {
                ops::fast_forward(repo, &branch, target.as_deref()).await
            }
            Self::Ignore { patterns } => {
                for pattern in &patterns {
                    ops::add_to_gitignore(repo, pattern).await?;
                }
                Ok(done())
            }
            Self::Clean {
                directories,
                ignored,
            } => ops::clean(repo, directories, ignored, false).await,
            Self::CheckoutFile { sha, path } => ops::checkout_file(repo, &sha, &path).await,
            Self::PruneRemote { name } => ops::prune_remote(repo, &name, report).await,
            Self::UpdateSubmodules => ops::update_submodules(repo, report).await,
            Self::SubmoduleAdd { url, path } => ops::submodule_add(repo, &url, &path, report).await,
            Self::SubmoduleUpdate { path } => ops::submodule_update(repo, &path, report).await,
            Self::SubmoduleRemove { path } => ops::submodule_remove(repo, &path).await,
            Self::Stage { paths } => ops::stage_files(repo, &paths).await.map(|()| done()),
            Self::StageAll => ops::stage_all(repo).await.map(|()| done()),
            Self::Unstage { paths } => ops::unstage_files(repo, &paths).await.map(|()| done()),
            Self::UnstageAll => ops::unstage_all(repo).await.map(|()| done()),
            Self::Discard { paths } => ops::discard_files(repo, &paths).await.map(|()| done()),
            Self::RevertSelection {
                sha,
                path,
                selection,
                context,
            } => ops::revert_selection(repo, &sha, &path, &selection, context).await,
            Self::StashSelection {
                path,
                selection,
                context,
            } => {
                let message = trf("Selected changes in {path}", &[("path", &path)]);
                ops::stash_selection(repo, &path, &selection, context, &message).await
            }
            Self::RestoreDiscard { name } => {
                ops::restore_discard(repo, &name).await.map(|()| done())
            }
            Self::DeleteDiscard { name } => ops::delete_discard(repo, &name).await.map(|()| done()),
            Self::Selection {
                path,
                mode,
                selection,
                context,
            } => ops::apply_selection(repo, &path, mode, &selection, context)
                .await
                .map(|()| done()),
            Self::Resolve {
                path,
                content,
                side,
            } => ops::resolve_conflict(repo, &path, content.as_deref(), side.as_deref()).await,
            Self::Commit {
                message,
                amend,
                options,
            } => ops::commit_with(repo, &message, amend, &options).await,
            Self::FixupCommit { sha } => ops::commit_fixup(repo, &sha).await,
            Self::MarkConflicted { paths } => ops::mark_conflicted(repo, &paths).await,
            Self::BisectStart { bad, good } => ops::bisect_start(repo, &bad, &good).await,
            Self::FetchMore { how } => ops::fetch_more(repo, how, report).await,
            Self::SparseSet { patterns } => ops::sparse_set(repo, &patterns).await,
            Self::SparseOff => ops::sparse_disable(repo).await,
            Self::DeleteBranches { names, remotes } => {
                let result = ops::delete_branches(repo, &names).await?;
                for (remote, branch) in remotes {
                    ops::delete_remote_branch(repo, &remote, &branch, report).await?;
                }
                Ok(result)
            }
            Self::ForcePushOver {
                remote,
                branch,
                expected,
            } => ops::force_push_over(repo, &remote, &branch, &expected, report).await,
            Self::BisectMark { verdict, sha } => {
                ops::bisect_mark(repo, verdict, sha.as_deref()).await
            }
            Self::BisectReset => ops::bisect_reset(repo).await,
            Self::Undo { .. } => ops::undo(repo).await.map(|_| done()),
            Self::Redo { .. } => ops::redo(repo).await.map(|_| done()),
        }
    }
}
