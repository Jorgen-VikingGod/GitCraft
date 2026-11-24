//! Every git operation the UI can start, as data: what to run and how to describe it.

use gitcraft_core::model::{OpResult, RebaseStep};
use gitcraft_core::parsers::Selection;
use gitcraft_core::runner::Progress;
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
        base: Option<String>,
        steps: Vec<RebaseStep>,
        autostash: bool,
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
    AddRemote {
        name: String,
        url: String,
        fetch: bool,
    },
    EditRemote {
        name: String,
        new_name: String,
        url: String,
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
    },
}

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
            Self::WorktreeAdd { .. } => "Adding worktree".into(),
            Self::WorktreeRemove { .. } => "Removing worktree".into(),
            Self::WorktreePrune => "Pruning worktrees".into(),
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
            Self::SetUpstream { .. } => "Setting upstream".into(),
            Self::Merge { .. } => "Merging".into(),
            Self::Rebase { .. } | Self::InteractiveRebase { .. } => "Rebasing".into(),
            Self::QuickRebase { .. } => "Rewriting history".into(),
            Self::CherryPick { .. } => "Cherry-picking".into(),
            Self::Revert { .. } => "Reverting".into(),
            Self::Continue => "Continuing".into(),
            Self::Abort => "Aborting".into(),
            Self::Skip => "Skipping".into(),
            Self::CreateTag { .. } => "Creating tag".into(),
            Self::DeleteTag { .. } => "Deleting tag".into(),
            Self::PushTag { .. } => "Pushing tag".into(),
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
            Self::Stage { .. } => "Staging".into(),
            Self::StageAll => "Staging all".into(),
            Self::Unstage { .. } => "Unstaging".into(),
            Self::UnstageAll => "Unstaging all".into(),
            Self::Discard { .. } => "Discarding".into(),
            Self::RestoreDiscard { .. } => "Restoring discarded changes".into(),
            Self::DeleteDiscard { .. } => "Deleting backup".into(),
            Self::Selection { mode, .. } => match *mode {
                "stage" => "Staging".into(),
                "unstage" => "Unstaging".into(),
                _ => "Discarding".into(),
            },
            Self::Resolve { .. } => "Resolving conflict".into(),
            Self::Commit { amend: true, .. } => "Amending commit".into(),
            Self::Commit { .. } => "Committing".into(),
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
            Self::StashApply { pop: true, .. } => "Stash popped".into(),
            Self::StashApply { .. } => "Stash applied".into(),
            Self::CreateBranch { name, .. } => trf("Created {name}", &[("name", name)]),
            Self::Merge { reference, .. } => trf("Merged {reference}", &[("reference", reference)]),
            Self::Rebase { .. } => "Rebase complete".into(),
            Self::InteractiveRebase { .. } => "Interactive rebase complete".into(),
            Self::QuickRebase { .. } => "History rewritten".into(),
            Self::CherryPick { sha } => {
                trf("Cherry-picked {commit}", &[("commit", short_sha(sha))])
            }
            Self::Revert { .. } => "Commit reverted".into(),
            Self::CreateTag { name, .. } => trf("Created tag {name}", &[("name", name)]),
            Self::PushTag { name, .. } => trf("Pushed {name}", &[("name", name)]),
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
            Self::RestoreDiscard { .. } => "Discarded changes restored".into(),
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

    /// Network operations can be stopped at any point without leaving a mess; history rewrites can't.
    pub fn cancellable(&self) -> bool {
        matches!(
            self,
            Self::Fetch { .. }
                | Self::Push { .. }
                | Self::DeleteRemoteBranch { .. }
                | Self::PushTag { .. }
                | Self::PruneRemote { .. }
                | Self::UpdateSubmodules
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
            Self::WorktreeAdd {
                path,
                start,
                new_branch,
            } => ops::worktree_add(repo, &path, start.as_deref(), new_branch.as_deref()).await,
            Self::WorktreeRemove { path, force } => ops::worktree_remove(repo, &path, force).await,
            Self::WorktreePrune => ops::worktree_prune(repo).await,
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
                base,
                steps,
                autostash,
            } => ops::interactive_rebase(repo, base.as_deref(), &steps, autostash).await,
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
                let steps: Vec<RebaseStep> = candidates
                    .into_iter()
                    .map(|commit| {
                        let chosen = commit.sha == sha;
                        RebaseStep {
                            action: if chosen { action } else { "pick" }.to_owned(),
                            message: if chosen { message.clone() } else { None },
                            sha: commit.sha,
                        }
                    })
                    .collect();
                ops::interactive_rebase(repo, base.as_deref(), &steps, true).await
            }
            Self::CherryPick { sha } => ops::cherry_pick(repo, &[sha]).await,
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
            } => ops::edit_remote(repo, &name, Some(&new_name), Some(&url)).await,
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
            Self::Stage { paths } => ops::stage_files(repo, &paths).await.map(|()| done()),
            Self::StageAll => ops::stage_all(repo).await.map(|()| done()),
            Self::Unstage { paths } => ops::unstage_files(repo, &paths).await.map(|()| done()),
            Self::UnstageAll => ops::unstage_all(repo).await.map(|()| done()),
            Self::Discard { paths } => ops::discard_files(repo, &paths).await.map(|()| done()),
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
            Self::Commit { message, amend } => ops::commit(repo, &message, amend, false).await,
        }
    }
}
