//! Every git operation the UI can start, as data: what to run and how to describe it.

use gitcraft_core::model::{OpResult, RebaseStep};
use gitcraft_core::parsers::Selection;
use gitcraft_core::runner::Progress;
use gitcraft_core::{Result, ops, queries};

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
        mode: String,
        autostash: bool,
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
    CreateBranch {
        name: String,
        start: Option<String>,
        checkout: bool,
    },
    Checkout {
        reference: String,
        kind: &'static str,
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
    /// Shown while it runs, and in the failure message.
    pub fn label(&self) -> String {
        match self {
            Self::Fetch {
                remote: Some(remote),
                ..
            } => format!("Fetching {remote}"),
            Self::Fetch { .. } => "Fetching".into(),
            Self::Pull { mode, .. } if mode == "rebase" => "Pulling (rebase)".into(),
            Self::Pull { .. } => "Pulling".into(),
            Self::Push { .. } => "Pushing".into(),
            Self::StashPush { .. } => "Stashing".into(),
            Self::StashApply { pop: true, .. } => "Popping stash".into(),
            Self::StashApply { .. } => "Applying stash".into(),
            Self::StashDrop { .. } => "Dropping stash".into(),
            Self::CreateBranch { .. } => "Creating branch".into(),
            Self::Checkout { reference, kind } => {
                let name = if *kind == "commit" {
                    short_sha(reference)
                } else {
                    reference
                };
                format!("Checking out {name}")
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
            Self::Reset { mode, .. } => format!("Resetting ({mode})"),
            Self::ApplyPatch { .. } => "Applying patch".into(),
            Self::FastForward { .. } => "Fast-forwarding".into(),
            Self::Ignore { .. } => "Updating .gitignore".into(),
            Self::Clean { .. } => "Cleaning".into(),
            Self::CheckoutFile { .. } => "Checking out file".into(),
            Self::PruneRemote { name } => format!("Pruning {name}"),
            Self::UpdateSubmodules => "Updating submodules".into(),
            Self::Stage { .. } => "Staging".into(),
            Self::StageAll => "Staging all".into(),
            Self::Unstage { .. } => "Unstaging".into(),
            Self::UnstageAll => "Unstaging all".into(),
            Self::Discard { .. } => "Discarding".into(),
            Self::Selection { mode, .. } => match *mode {
                "stage" => "Staging".into(),
                "unstage" => "Unstaging".into(),
                _ => "Discarding".into(),
            },
            Self::Resolve { .. } => "Resolving conflict".into(),
            Self::Commit { amend: true, .. } => "Amending commit".into(),
            Self::Commit { .. } => "Committing".into(),
        }
    }

    /// The toast after it succeeded, if it deserves one.
    pub fn success(&self) -> Option<String> {
        Some(match self {
            Self::Fetch { .. } => "Fetch complete".into(),
            Self::Pull { .. } => "Pull complete".into(),
            Self::Push { .. } => "Push complete".into(),
            Self::StashPush { paths: Some(_), .. } => "Files stashed".into(),
            Self::StashPush { .. } => "Changes stashed".into(),
            Self::StashApply { pop: true, .. } => "Stash popped".into(),
            Self::StashApply { .. } => "Stash applied".into(),
            Self::CreateBranch { name, .. } => format!("Created {name}"),
            Self::Merge { reference, .. } => format!("Merged {reference}"),
            Self::Rebase { .. } => "Rebase complete".into(),
            Self::InteractiveRebase { .. } => "Interactive rebase complete".into(),
            Self::QuickRebase { .. } => "History rewritten".into(),
            Self::CherryPick { sha } => format!("Cherry-picked {}", short_sha(sha)),
            Self::Revert { .. } => "Commit reverted".into(),
            Self::CreateTag { name, .. } => format!("Created tag {name}"),
            Self::PushTag { name, .. } => format!("Pushed {name}"),
            Self::Reset { undo: true, .. } => "Last commit undone; its changes are staged".into(),
            Self::Reset { target, .. } => format!("Reset to {}", short_sha(target)),
            Self::ApplyPatch { .. } => "Patch applied".into(),
            Self::FastForward { branch, .. } => format!("{branch} fast-forwarded"),
            Self::Ignore { patterns } => match &patterns[..] {
                [one] => format!("Added \"{one}\" to .gitignore"),
                many => format!("Added {} to .gitignore", plural(many.len(), "pattern")),
            },
            Self::Clean { .. } => "Cleaned".into(),
            Self::CheckoutFile { sha, path } => format!("{path} restored from {}", short_sha(sha)),
            Self::PruneRemote { name } => format!("Pruned stale branches of {name}"),
            Self::UpdateSubmodules => "Submodules updated".into(),
            Self::Resolve { path, .. } => format!("Resolved {path}"),
            Self::Commit { amend: true, .. } => "Commit amended".into(),
            Self::Commit { .. } => "Committed".into(),
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
            Self::CreateBranch {
                name,
                start,
                checkout,
            } => ops::create_branch(repo, &name, start.as_deref(), checkout).await,
            Self::Checkout { reference, kind } => ops::checkout(repo, &reference, kind).await,
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
