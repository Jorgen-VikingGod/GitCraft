//! User-facing commands with their dialogs, and all context menus.

use egui::Pos2;
use gitcraft_core::model::{Commit, CommitDetail, LocalBranch, Remote, ToolTarget};
use gitcraft_core::{ops, queries};

use crate::task::Task;

use crate::app::{App, Area, Effect, Message, PatchOf, Pick, RepoTab, ToastKind, View};
use crate::command::Command;
use crate::form::{Field, Form, Values};
use crate::i18n::{tr, trf};
use crate::logic::{self, Choice, plural, short_sha, split_upstream};
use crate::menu::MenuItem;

/// Something the user asked for that needs a question first, or more information.
#[derive(Debug, Clone)]
pub enum Action {
    Pull,
    Push(Option<String>),
    Stash,
    StashFiles(Vec<String>),
    DropStash(String),
    RenameStash(String),
    /// Adds a working tree; `start` is the branch or commit it begins at.
    AddWorktree(Option<String>),
    RemoveWorktree(String),
    NewBranch {
        start: Option<String>,
        label: Option<String>,
    },
    CheckoutCommit(String),
    RenameBranch(String),
    DeleteBranch(String),
    DeleteRemoteBranch(String),
    SetUpstream(String),
    Merge(String),
    Rebase {
        onto: String,
        label: String,
    },
    Revert(String),
    /// Asks which parent counts for a merge commit; other commits are picked right away.
    CherryPick(String),
    /// Deletes a tag on one remote only, after asking.
    DeleteRemoteTag {
        remote: String,
        name: String,
    },
    /// Asks what to narrow the history to.
    FilterHistory,
    /// Asks for two ranges of commits to compare.
    RangeDiff,
    /// Asks for a commit that still worked, then bisects up to this broken one.
    BisectStart(String),
    /// Asks for a command to run after step `index` of the rebase editor.
    RebaseExec(usize),
    /// Swaps a commit with its child (`up`) or its parent.
    MoveCommit(String, bool),
    Abort,
    CreateTag(String),
    DeleteTag(String),
    PushTag(String),
    AddRemote,
    EditRemote(String),
    RemoveRemote(String),
    ResetTo {
        sha: String,
        mode: &'static str,
    },
    UndoLast,
    EditMessage(String),
    EditMessageLoaded(String, Result<String, String>),
    Squash(String),
    SquashBase {
        sha: String,
        base: Result<Option<String>, String>,
    },
    DropCommit(String),
    SavePatch(String),
    /// The commits from this one up to HEAD, as patches.
    SavePatchSeries(String),
    SaveChangesPatch,
    PatchSaved(Result<Option<String>, String>),
    ApplyPatch,
    PatchPicked(Option<(String, String)>),
    Clean,
    CleanPreview {
        directories: bool,
        ignored: bool,
    },
    CleanPreviewed {
        directories: bool,
        ignored: bool,
        files: Result<Vec<String>, String>,
    },
    CheckoutFile {
        sha: String,
        path: String,
    },
    Discard(Vec<String>),
    DeleteDiscard(String),
    CoAuthors,
    Clone,
    CloneTo {
        url: String,
        parent: String,
        name: Option<String>,
    },
    Cloned(Result<String, String>),
    Init,
    InitAt(String),
    ConflictOverwrite(Option<usize>, Choice),
    ConflictResolve {
        path: String,
        side: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefTarget {
    Local(String),
    Remote(String),
    Tag(String),
}

#[derive(Debug, Clone)]
pub enum MenuRequest {
    Commit {
        sha: String,
        focus: Option<RefTarget>,
    },
    /// Several commits picked in the history, newest first.
    Commits(Vec<String>),
    Stash(String),
    Worktree(String),
    Remote(String),
    Repository,
    /// `area` for a file in Local Changes, `rev` for a file in a commit.
    File {
        area: Option<Area>,
        path: String,
        rev: Option<String>,
    },
    Tab(usize),
    /// The options of the commit box.
    CommitOptions,
    /// Which columns the history list shows.
    Columns,
}

fn run(command: Command) -> Message {
    Message::Run(command)
}

fn act(action: Action) -> Message {
    Message::Action(action)
}

fn remote_field(remotes: &[Remote], preferred: Option<&str>) -> Field {
    let value = preferred
        .map(str::to_owned)
        .or_else(|| {
            remotes
                .iter()
                .find(|remote| remote.name == "origin")
                .map(|remote| remote.name.clone())
        })
        .or_else(|| remotes.first().map(|remote| remote.name.clone()))
        .unwrap_or_default();
    let options = remotes
        .iter()
        .map(|remote| {
            (
                remote.name.clone(),
                format!("{}  ({})", remote.name, remote.fetch_url),
            )
        })
        .collect();
    Field::select("remote", "Remote", &value, options)
}

fn options(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(value, label)| ((*value).to_owned(), (*label).to_owned()))
        .collect()
}

pub fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else if cfg!(windows) {
        "Show in Explorer"
    } else {
        "Show in file manager"
    }
}

pub fn join_path(root: &str, path: &str) -> String {
    std::path::Path::new(root)
        .join(path)
        .to_string_lossy()
        .into_owned()
}

impl App {
    fn ask(&mut self, form: Form) -> Task<Message> {
        self.form = Some(form);
        Task::none()
    }

    /// Works out in the background which files merging `target` would conflict in, for the
    /// dialog just opened (a rebase conflicts in the same files, near enough).
    fn preview_conflicts(&mut self, target: &str) -> Task<Message> {
        let Some(repo) = self.current().map(|tab| tab.path.clone()) else {
            return Task::none();
        };
        if !self.settings.history.conflict_check {
            return Task::none();
        }
        self.conflict_preview = Some(target.to_owned());
        let target = target.to_owned();
        Task::perform(
            async move {
                let result = crate::app::answer(
                    gitcraft_core::queries::conflict_check(&repo, &target).await,
                );
                (target, result)
            },
            |(target, result)| Message::ConflictPreview(target, result),
        )
    }

    fn warn(&mut self, message: &str) -> Task<Message> {
        self.toast(ToastKind::Warning, message, None);
        Task::none()
    }

    fn fail(&mut self, title: &str, error: String) -> Task<Message> {
        self.toast(ToastKind::Error, title, Some(error));
        Task::none()
    }

    fn commit_by_sha(&self, sha: &str) -> Option<&Commit> {
        self.current()?
            .commits
            .iter()
            .find(|commit| commit.sha == sha)
    }

    pub fn perform(&mut self, action: Action) -> Task<Message> {
        // These work without an open repository.
        match action {
            Action::Clone => return self.clone_dialog(),
            Action::Init => return self.init_dialog(),
            Action::CloneTo { url, parent, name } => return clone_task(url, parent, name),
            Action::Cloned(result) => return self.cloned(result),
            Action::InitAt(path) => return init_task(path),
            _ => {}
        }
        let Some(tab) = self.current() else {
            return Task::none();
        };
        let remotes = tab.refs.remotes.clone();
        let current = tab.current_branch().map(str::to_owned);
        let head = current.clone().unwrap_or_else(|| "HEAD".to_owned());
        let branch_info = |name: &str| {
            tab.refs
                .local
                .iter()
                .find(|branch| branch.name == name)
                .cloned()
        };
        match action {
            Action::Pull => {
                if remotes.is_empty() {
                    return self.warn("This repository has no remotes");
                }
                let branch = tab.refs.local.iter().find(|branch| branch.current);
                let (up_remote, up_branch) = split_upstream(branch.and_then(|b| b.upstream.as_deref()), &remotes);
                let name = up_branch.or_else(|| branch.map(|b| b.name.clone())).unwrap_or_default();
                let mut form = Form::new("Pull", "Pull", |values: &Values| Message::PullChosen {
                    remote: values.opt("remote"),
                    branch: values.opt("branch"),
                    mode: values.text("mode"),
                    autostash: values.check("autostash"),
                    backup: values.check("backup"),
                })
                .field(remote_field(&remotes, up_remote.as_deref()))
                .field(Field::text("branch", "Remote branch", &name))
                .field(Field::select(
                    "mode",
                    "Strategy",
                    &self.settings.pull.mode,
                    options(&[
                        ("merge", "Merge (create a merge commit if needed)"),
                        ("rebase", "Rebase (replay local commits on top)"),
                        ("ff-only", "Fast-forward only"),
                        ("overwrite", "Overwrite local branch with the remote branch"),
                    ]),
                ))
                .field(Field::check("autostash", "Stash and reapply local changes", self.settings.pull.autostash))
                .field(Field::check(
                    "backup",
                    "Overwrite only: keep a backup branch of the current state",
                    self.settings.pull.backup,
                ));
                if let Some(branch) = branch {
                    form = form.message(&trf("Pull into {branch}", &[("branch", &branch.name)]));
                }
                self.ask(form)
            }
            Action::Push(name) => {
                if remotes.is_empty() {
                    return self.warn("This repository has no remotes. Add one from the sidebar first.");
                }
                let wanted = name.or(current);
                let Some(branch) = wanted.as_deref().and_then(branch_info) else {
                    return self.warn("Check out a branch before pushing");
                };
                let (up_remote, _) = split_upstream(branch.upstream.as_deref(), &remotes);
                let name = branch.name.clone();
                let form = Form::new("Push", "Push", move |values: &Values| {
                    run(Command::Push {
                        remote: values.text("remote"),
                        branch: name.clone(),
                        set_upstream: values.check("setUpstream"),
                        force: values.check("force"),
                        tags: values.check("tags"),
                    })
                })
                .message(&trf("Push {branch}", &[("branch", &branch.name)]))
                .field(remote_field(&remotes, up_remote.as_deref()))
                .field(Field::check("setUpstream", "Track remote branch (set upstream)", branch.upstream.is_none()))
                .field(Field::check("tags", "Push all tags", false))
                .field(Field::check("force", "Force push (with lease)", false));
                self.ask(form)
            }
            Action::Stash => {
                if tab.change_count() == 0 {
                    self.toast(ToastKind::Info, "There are no local changes to stash", None);
                    return Task::none();
                }
                let form = Form::new("Stash changes", "Stash", |values: &Values| {
                    if values.check("untrackedOnly") {
                        return run(Command::StashUntracked {
                            message: values.opt("message"),
                        });
                    }
                    run(Command::StashPush {
                        message: values.opt("message"),
                        include_untracked: values.check("includeUntracked"),
                        staged_only: values.check("stagedOnly"),
                        keep_index: values.check("keepIndex"),
                        paths: None,
                    })
                })
                .field(Field::text("message", "Message", "").hint("Optional description"))
                .field(Field::check("includeUntracked", "Include untracked files", true))
                .field(Field::check("stagedOnly", "Stash staged changes only", false))
                .field(Field::check("keepIndex", "Keep staged changes in the working tree", false))
                .field(Field::check("untrackedOnly", "Only untracked files", false));
                self.ask(form)
            }
            Action::StashFiles(paths) => {
                let what = match &paths[..] {
                    [one] => one.clone(),
                    many => plural(many.len(), "file"),
                };
                let form = Form::new("Stash selected files", "Stash", move |values: &Values| {
                    run(Command::StashPush {
                        message: values.opt("message"),
                        include_untracked: true,
                        staged_only: false,
                        keep_index: false,
                        paths: Some(paths.clone()),
                    })
                })
                .message(&what)
                .field(Field::text("message", "Message", "").hint("Optional description"));
                self.ask(form)
            }
            Action::DropStash(name) => {
                // By its commit, so the stash meant is dropped even if the list moved meanwhile.
                let found = tab.refs.stashes.iter().find(|stash| stash.name == name || stash.sha == name);
                let message = found.map_or(name.clone(), |stash| stash.message.clone());
                let name = found.map_or(name, |stash| stash.sha.clone());
                self.ask(Form::confirm(
                    "Drop stash",
                    &trf("Delete “{message}”? This cannot be undone.", &[("message", &message)]),
                    "Drop",
                    true,
                    run(Command::StashDrop { name }),
                ))
            }
            Action::RenameStash(name) => {
                let Some(stash) = tab.refs.stashes.iter().find(|stash| stash.name == name || stash.sha == name) else {
                    return Task::none();
                };
                let id = stash.sha.clone();
                self.ask(
                    Form::new("Rename stash", "Rename", move |values: &Values| {
                        run(Command::StashRename {
                            id: id.clone(),
                            message: values.text("message"),
                        })
                    })
                    .message("The stash moves to the top of the list.")
                    .field(Field::text("message", "Message", &stash.message).required()),
                )
            }
            Action::AddWorktree(start) => {
                let parent = std::path::Path::new(&tab.path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                let suggested = join_path(&parent, &format!("{}-worktree", tab.name));
                let based = start.as_deref().map_or_else(|| head.clone(), |s| short_sha(s).to_owned());
                let form = Form::new("Add worktree", "Add worktree", move |values: &Values| {
                    let branch = values.text("branch").split_whitespace().collect::<Vec<_>>().join("-");
                    run(Command::WorktreeAdd {
                        path: values.text("path"),
                        start: start.clone(),
                        new_branch: (!branch.is_empty()).then_some(branch),
                    })
                })
                .message(&trf("Based on {based}", &[("based", &based)]))
                .field(Field::text("path", "Folder", &suggested).required())
                .field(Field::text("branch", "New branch", "").hint("Leave empty to check out detached"));
                self.ask(form)
            }
            Action::RemoveWorktree(path) => self.ask(Form::confirm(
                "Remove worktree",
                &trf("Delete the working folder “{path}”? Uncommitted changes in it are lost.", &[("path", &path)]),
                "Remove",
                true,
                run(Command::WorktreeRemove { path, force: true }),
            )),
            Action::NewBranch { start, label } => {
                let based = label.or_else(|| start.as_deref().map(|sha| short_sha(sha).to_owned())).unwrap_or(head);
                let form = Form::new("New branch", "Create branch", move |values: &Values| {
                    let name = values.text("name").split_whitespace().collect::<Vec<_>>().join("-");
                    run(Command::CreateBranch { name, start: start.clone(), checkout: values.check("checkout") })
                })
                .message(&trf("Based on {based}", &[("based", &based)]))
                .field(Field::text("name", "Branch name", "").required().hint("feature/my-change"))
                .field(Field::check("checkout", "Check out after creating", true));
                self.ask(form)
            }
            Action::CheckoutCommit(sha) => self.ask(Form::confirm(
                "Checkout commit",
                &trf(
                    "Check out {commit}? You will be in a “detached HEAD” state; create a branch to keep new commits.",
                    &[("commit", short_sha(&sha))],
                ),
                "Checkout",
                false,
                run(Command::Checkout { reference: sha, kind: "commit" }),
            )),
            Action::RenameBranch(name) => {
                let old = name.clone();
                let form = Form::new("Rename branch", "Rename", move |values: &Values| {
                    run(Command::RenameBranch { old: old.clone(), new: values.text("name") })
                })
                .field(Field::text("name", "New name", &name).required());
                self.ask(form)
            }
            Action::DeleteBranch(name) => {
                let upstream = branch_info(&name).and_then(|branch| branch.upstream);
                let split = split_upstream(upstream.as_deref(), &remotes);
                let branch = name.clone();
                let mut form = Form::new("Delete branch", "Delete", move |values: &Values| {
                    let remote = match (&split, values.check("remote")) {
                        ((Some(remote), Some(branch)), true) => Some((remote.clone(), branch.clone())),
                        _ => None,
                    };
                    run(Command::DeleteBranch { name: branch.clone(), force: values.check("force"), remote })
                })
                .message(&trf("Delete local branch {name}?", &[("name", &name)]))
                .danger()
                .field(Field::check("force", "Force delete, even if it is not merged", false));
                if let Some(upstream) = upstream {
                    form = form.field(Field::check("remote", &trf("Also delete {upstream} on the remote", &[("upstream", &upstream)]), false));
                }
                self.ask(form)
            }
            Action::DeleteRemoteBranch(name) => {
                let Some(branch) = tab.refs.remote.iter().find(|branch| branch.name == name).cloned() else {
                    return Task::none();
                };
                self.ask(Form::confirm(
                    "Delete remote branch",
                    &trf("Delete {branch} from {remote}?", &[("branch", &branch.name), ("remote", &branch.remote)]),
                    "Delete",
                    true,
                    run(Command::DeleteRemoteBranch { remote: branch.remote, branch: branch.branch }),
                ))
            }
            Action::SetUpstream(name) => {
                let upstream = branch_info(&name).and_then(|branch| branch.upstream).unwrap_or_default();
                let mut choices = vec![(String::new(), "(none)".to_owned())];
                choices.extend(tab.refs.remote.iter().map(|branch| (branch.name.clone(), branch.name.clone())));
                let branch = name.clone();
                let form = Form::new("Tracking branch", "Save", move |values: &Values| {
                    run(Command::SetUpstream { branch: branch.clone(), upstream: values.opt("upstream") })
                })
                .message(&trf("Choose the upstream of {name}", &[("name", &name)]))
                .field(Field::select("upstream", "Upstream", &upstream, choices));
                self.ask(form)
            }
            Action::Merge(reference) => {
                let target = reference.clone();
                let form = Form::new("Merge", "Merge", move |values: &Values| {
                    run(Command::Merge { reference: target.clone(), mode: values.text("mode") })
                })
                .message(&trf("Merge {reference} into {head}", &[("reference", &reference), ("head", &head)]))
                .field(Field::select(
                    "mode",
                    "Options",
                    "default",
                    options(&[
                        ("default", "Default (fast-forward if possible)"),
                        ("no-ff", "Always create a merge commit (--no-ff)"),
                        ("ff-only", "Fast-forward only"),
                        ("squash", "Squash (stage changes, do not commit)"),
                    ]),
                ));
                let check = self.preview_conflicts(&reference);
                Task::batch([self.ask(form), check])
            }
            Action::Rebase { onto, label } => {
                let target = onto.clone();
                let form = Form::new("Rebase", "Rebase", move |values: &Values| Message::RebaseChosen {
                    onto: onto.clone(),
                    autostash: values.check("autostash"),
                })
                .message(&trf("Rebase {head} onto {label}?", &[("head", &head), ("label", &label)]))
                .field(Field::check("autostash", "Stash and reapply local changes", self.settings.rebase.autostash));
                let check = self.preview_conflicts(&target);
                Task::batch([self.ask(form), check])
            }
            Action::DeleteRemoteTag { remote, name } => self.ask(Form::confirm(
                "Delete tag from remote",
                &trf(
                    "Delete tag {name} from {remote}? It stays in this repository.",
                    &[("name", &name), ("remote", &remote)],
                ),
                "Delete",
                true,
                run(Command::DeleteRemoteTag { remote, name }),
            )),
            Action::FilterHistory => {
                let current = tab.history_filter.clone().unwrap_or_default();
                self.ask(
                    Form::new("Filter history", "Filter", |values: &Values| {
                        Message::SetHistoryFilter(Some(gitcraft_core::model::LogFilter {
                            author: values.text("author"),
                            path: values.text("path"),
                            text: values.text("text"),
                            regex: values.check("regex"),
                        }))
                    })
                    .message("Shows only the commits that match every filled-in field.")
                    .field(Field::text("author", "Author", &current.author).hint("Part of a name or address"))
                    .field(Field::text("path", "File or folder", &current.path).hint("src/main.rs"))
                    .field(Field::text("text", "Changed text", &current.text).hint("Text a commit added or removed"))
                    .field(Field::check("regex", "Changed text is a regular expression", current.regex)),
                )
            }
            Action::RangeDiff => {
                let upstream = tab.status.as_ref().and_then(|status| status.branch.upstream.clone());
                let new = match &upstream {
                    Some(upstream) => format!("{upstream}..HEAD"),
                    None => String::new(),
                };
                self.ask(
                    Form::new("Compare commit ranges", "Compare", |values: &Values| {
                        Message::RangeDiff(values.text("old"), values.text("new"))
                    })
                    .message("Shows how two versions of a series of commits differ, such as a branch before and after a rebase: which commits match, which changed and how.")
                    .field(Field::text("old", "Old range", "").required().hint("main..topic@{1}"))
                    .field(Field::text("new", "New range", &new).required().hint("main..topic")),
                )
            }
            Action::BisectStart(bad) => {
                let subject = self.commit_by_sha(&bad).map_or_else(String::new, |commit| commit.subject.clone());
                self.ask(
                    Form::new("Start bisect", "Start", move |values: &Values| {
                        run(Command::BisectStart { bad: bad.clone(), good: values.text("good") })
                    })
                    .message(&trf(
                        "“{subject}” is broken. Name an older commit that still worked: GitCraft then checks out commits in between for you to test, until the first broken one is found.",
                        &[("subject", &subject)],
                    ))
                    .field(Field::text("good", "Good commit", "").required().hint("A tag, branch or SHA")),
                )
            }
            Action::RebaseExec(index) => {
                let current = match &self.overlay {
                    Some(crate::app::Overlay::Rebase(state)) => {
                        state.steps.get(index).and_then(|step| step.exec.clone()).unwrap_or_default()
                    }
                    _ => return Task::none(),
                };
                self.ask(
                    Form::new("Run a command", "Save", move |values: &Values| {
                        Message::RebaseExec(index, values.text("command"))
                    })
                    .message("Runs in the repository right after this commit is applied (exec). The rebase stops when it fails, for example to fix a failing test. Leave it empty to remove it.")
                    .field(Field::text("command", "Command", &current).hint("cargo test")),
                )
            }
            Action::MoveCommit(sha, up) => {
                let Some(commit) = self.commit_by_sha(&sha).cloned() else { return Task::none() };
                if commit.parents.len() > 1 {
                    return self.warn("Merge commits cannot be edited this way");
                }
                let Some(parent) = commit.parents.first().cloned() else {
                    return self.warn("The first commit has nothing below it");
                };
                // Moving down swaps it with its parent, so the rebase starts one further back.
                let base = if up {
                    Some(parent)
                } else {
                    match self.commit_by_sha(&parent).map(|parent| parent.parents.clone()) {
                        Some(grand) if grand.is_empty() => None,
                        Some(grand) => grand.first().cloned(),
                        None => Some(format!("{parent}^")),
                    }
                };
                Task::done(run(Command::QuickRebase { sha, base, action: if up { "up" } else { "down" }, message: None }))
            }
            Action::CherryPick(sha) => {
                let Some(commit) = self.commit_by_sha(&sha).cloned() else {
                    return Task::done(run(Command::CherryPick { sha }));
                };
                if commit.parents.len() < 2 {
                    return Task::done(run(Command::CherryPick { sha }));
                }
                let choices = commit
                    .parents
                    .iter()
                    .enumerate()
                    .map(|(i, parent)| ((i + 1).to_string(), format!("{}: {}", i + 1, short_sha(parent))))
                    .collect();
                let form = Form::new("Cherry-pick merge commit", "Cherry-pick", move |values: &Values| {
                    run(Command::CherryPickMerge {
                        sha: sha.clone(),
                        mainline: values.text("mainline").parse().unwrap_or(1),
                    })
                })
                .message("A merge commit is taken as its changes against one parent: choose the mainline it was merged into.")
                .field(Field::select("mainline", "Mainline parent", "1", choices));
                self.ask(form)
            }
            Action::Revert(sha) => {
                let Some(commit) = self.commit_by_sha(&sha).cloned() else { return Task::none() };
                if commit.parents.len() > 1 {
                    let choices = commit
                        .parents
                        .iter()
                        .enumerate()
                        .map(|(i, parent)| ((i + 1).to_string(), format!("{}: {}", i + 1, short_sha(parent))))
                        .collect();
                    let form = Form::new("Revert merge commit", "Revert", move |values: &Values| {
                        run(Command::Revert { sha: sha.clone(), mainline: values.text("mainline").parse().ok() })
                    })
                    .message("Choose the parent that should be kept (the mainline).")
                    .field(Field::select("mainline", "Mainline parent", "1", choices));
                    return self.ask(form);
                }
                self.ask(Form::confirm(
                    "Revert commit",
                    &trf("Create a commit that reverts “{subject}”?", &[("subject", &commit.subject)]),
                    "Revert",
                    false,
                    run(Command::Revert { sha, mainline: None }),
                ))
            }
            Action::Abort => {
                let kind = tab.status.as_ref().and_then(|s| s.operation.as_ref()).map_or("operation", |op| op.command());
                self.ask(Form::confirm(
                    &trf("Abort {kind}", &[("kind", tr(kind))]),
                    &trf("Abort the {kind} and restore the previous state?", &[("kind", tr(kind))]),
                    "Abort",
                    true,
                    run(Command::Abort),
                ))
            }
            Action::CreateTag(target) => {
                let names: Vec<String> = remotes.iter().map(|remote| remote.name.clone()).collect();
                let has_remotes = !names.is_empty();
                let tagged = target.clone();
                let mut form = Form::new("New tag", "Create tag", move |values: &Values| {
                    run(Command::CreateTag {
                        name: values.text("name").trim().to_owned(),
                        target: tagged.clone(),
                        message: values.opt("message"),
                        push_to: if values.check("push") { names.clone() } else { Vec::new() },
                    })
                })
                .message(&trf("Tag {commit}", &[("commit", short_sha(&target))]))
                .field(Field::text("name", "Tag name", "").required().hint("v1.0.0"))
                .field(Field::area("message", "Message (creates an annotated tag)", ""));
                if has_remotes {
                    form = form.field(Field::check("push", "Push to all remotes", false));
                }
                self.ask(form)
            }
            Action::DeleteTag(name) => {
                let names: Vec<String> = remotes.iter().map(|remote| remote.name.clone()).collect();
                let has_remotes = !names.is_empty();
                let tag = name.clone();
                let all = names.clone();
                let mut form = Form::new("Delete tag", "Delete", move |values: &Values| {
                    // "" only here, "*" on every remote too, otherwise that one remote too.
                    let remotes = match values.text("remotes").as_str() {
                        "" => Vec::new(),
                        "*" => all.clone(),
                        one => vec![one.to_owned()],
                    };
                    run(Command::DeleteTag { name: tag.clone(), remotes })
                })
                .message(&trf("Delete tag {name}?", &[("name", &name)]))
                .danger();
                if has_remotes {
                    let mut choices = vec![
                        (String::new(), tr("Only in this repository").to_owned()),
                        ("*".to_owned(), tr("Here and on every remote").to_owned()),
                    ];
                    choices.extend(
                        names.iter().map(|remote| (remote.clone(), trf("Here and on {remote}", &[("remote", remote)]))),
                    );
                    form = form.field(Field::select("remotes", "Delete", "", choices));
                }
                self.ask(form)
            }
            Action::PushTag(name) => {
                let tag = name.clone();
                let form = Form::new("Push tag", "Push", move |values: &Values| {
                    run(Command::PushTag { remote: values.text("remote"), name: tag.clone() })
                })
                .message(&name)
                .field(remote_field(&remotes, None));
                self.ask(form)
            }
            Action::AddRemote => {
                let form = Form::new("Add remote", "Add remote", |values: &Values| {
                    run(Command::AddRemote { name: values.text("name"), url: values.text("url"), fetch: values.check("fetch") })
                })
                .field(Field::text("name", "Name", if remotes.is_empty() { "origin" } else { "" }).required())
                .field(Field::text("url", "URL", "").required().hint("git@github.com:user/repo.git"))
                .field(Field::check("fetch", "Fetch after adding", true));
                self.ask(form)
            }
            Action::EditRemote(name) => {
                let Some(remote) = remotes.iter().find(|remote| remote.name == name).cloned() else {
                    return Task::none();
                };
                let form = Form::new("Edit remote", "Save", move |values: &Values| {
                    run(Command::EditRemote {
                        name: name.clone(),
                        new_name: values.text("newName"),
                        url: values.text("url"),
                        push_url: values.text("pushUrl"),
                    })
                })
                .field(Field::text("newName", "Name", &remote.name).required())
                .field(Field::text("url", "URL", &remote.fetch_url).required())
                .field(
                    Field::text(
                        "pushUrl",
                        "Push URL",
                        if remote.push_url == remote.fetch_url { "" } else { &remote.push_url },
                    )
                    .hint("Empty: push to the URL above"),
                );
                self.ask(form)
            }
            Action::RemoveRemote(name) => self.ask(Form::confirm(
                "Remove remote",
                &trf("Remove remote {name} and its remote-tracking branches?", &[("name", &name)]),
                "Remove",
                true,
                run(Command::RemoveRemote { name }),
            )),
            Action::ResetTo { sha, mode } => {
                let command = Command::Reset { target: sha.clone(), mode: mode.to_owned(), undo: false };
                if mode != "hard" {
                    return Task::done(run(command));
                }
                let subject = self.commit_by_sha(&sha).map_or_else(String::new, |commit| commit.subject.clone());
                self.ask(Form::confirm(
                    "Hard reset",
                    &trf(
                        "Reset {head} to {commit} “{subject}” and discard ALL uncommitted changes? This cannot be undone.",
                        &[("head", &head), ("commit", short_sha(&sha)), ("subject", &subject)],
                    ),
                    "Reset (hard)",
                    true,
                    run(command),
                ))
            }
            Action::UndoLast => self.ask(Form::confirm(
                "Undo last commit",
                "Remove the last commit from the branch and keep its changes staged?",
                "Undo commit",
                false,
                run(Command::Reset { target: "HEAD~1".to_owned(), mode: "soft".to_owned(), undo: true }),
            )),
            Action::EditMessage(sha) => {
                if self.commit_by_sha(&sha).is_some_and(|commit| commit.parents.len() > 1) {
                    return self.warn("Merge commits cannot be edited this way");
                }
                let repo = tab.path.clone();
                Task::perform(
                    async move {
                        let message = queries::commit_detail(&repo, &sha).await.map(|detail: CommitDetail| detail.message);
                        (sha, message.map_err(|error| error.to_string()))
                    },
                    |(sha, message)| act(Action::EditMessageLoaded(sha, message)),
                )
            }
            Action::EditMessageLoaded(sha, message) => {
                let message = match message {
                    Ok(message) => message,
                    Err(error) => return self.fail("Could not edit the commit", error),
                };
                let base = self.commit_by_sha(&sha).and_then(|commit| commit.parents.first().cloned());
                let target = sha.clone();
                let form = Form::new("Edit commit message", "Save message", move |values: &Values| {
                    run(Command::QuickRebase {
                        sha: target.clone(),
                        base: base.clone(),
                        action: "reword",
                        message: Some(values.text("message")),
                    })
                })
                .message(&trf("{commit}: commits after it are rewritten.", &[("commit", short_sha(&sha))]))
                .field(Field::area("message", "Message", &message).required());
                self.ask(form)
            }
            Action::Squash(sha) => {
                let Some(commit) = self.commit_by_sha(&sha).cloned() else { return Task::none() };
                if commit.parents.len() > 1 {
                    return self.warn("Merge commits cannot be edited this way");
                }
                let Some(parent) = commit.parents.first().cloned() else {
                    return self.warn("The first commit has no parent to squash into");
                };
                let repo = tab.path.clone();
                Task::perform(
                    async move {
                        // The rebase starts at the parent's parent, so the parent is part of it.
                        let base = queries::commit_detail(&repo, &parent)
                            .await
                            .map(|detail| detail.parents.first().cloned())
                            .map_err(|error| error.to_string());
                        (sha, base)
                    },
                    |(sha, base)| act(Action::SquashBase { sha, base }),
                )
            }
            Action::SquashBase { sha, base } => {
                let base = match base {
                    Ok(base) => base,
                    Err(error) => return self.fail("Could not edit the commit", error),
                };
                let subject = self.commit_by_sha(&sha).map_or_else(String::new, |commit| commit.subject.clone());
                let form = Form::new("Squash into parent", "Squash", move |values: &Values| {
                    let action = if values.text("mode") == "fixup" { "fixup" } else { "squash" };
                    run(Command::QuickRebase { sha: sha.clone(), base: base.clone(), action, message: None })
                })
                .message(&trf("Fold “{subject}” into its parent commit.", &[("subject", &subject)]))
                .field(Field::select(
                    "mode",
                    "Commit message",
                    "squash",
                    options(&[
                        ("squash", "Combine both messages"),
                        ("fixup", "Keep the parent's message, discard this one"),
                    ]),
                ));
                self.ask(form)
            }
            Action::DropCommit(sha) => {
                let Some(commit) = self.commit_by_sha(&sha).cloned() else { return Task::none() };
                if commit.parents.len() > 1 {
                    return self.warn("Merge commits cannot be edited this way");
                }
                self.ask(Form::confirm(
                    "Drop commit",
                    &trf("Remove “{subject}” from the branch history?", &[("subject", &commit.subject)]),
                    "Drop",
                    true,
                    run(Command::QuickRebase { base: commit.parents.first().cloned(), sha, action: "drop", message: None }),
                ))
            }
            Action::SavePatch(sha) => {
                let repo = tab.path.clone();
                let subject = self.commit_by_sha(&sha).map_or_else(String::new, |commit| commit.subject.clone());
                let name = logic::patch_file_name(&sha, &subject);
                self.effects.push(Effect::Ask(Pick::SavePatch { repo, of: PatchOf::Commit(sha), name }));
                Task::none()
            }
            Action::SavePatchSeries(sha) => {
                let repo = tab.path.clone();
                let name = format!("{}-to-head.patch", short_sha(&sha));
                self.effects.push(Effect::Ask(Pick::SavePatch { repo, of: PatchOf::Series(sha), name }));
                Task::none()
            }
            Action::SaveChangesPatch => {
                if tab.change_count() == 0 {
                    self.toast(ToastKind::Info, "There are no local changes", None);
                    return Task::none();
                }
                let repo = tab.path.clone();
                let name = "local-changes.patch".to_owned();
                self.effects.push(Effect::Ask(Pick::SavePatch { repo, of: PatchOf::Changes, name }));
                Task::none()
            }
            Action::PatchSaved(result) => {
                match result {
                    Ok(Some(path)) => self.toast(ToastKind::Success, trf("Saved {path}", &[("path", &path)]), None),
                    Ok(None) => {}
                    Err(error) => self.toast(ToastKind::Error, "Could not create patch", Some(error)),
                }
                Task::none()
            }
            Action::ApplyPatch => {
                self.effects.push(Effect::Ask(Pick::ApplyPatch));
                Task::none()
            }
            Action::PatchPicked(None) => Task::none(),
            Action::PatchPicked(Some((name, patch))) => {
                let is_mailbox = patch.lines().any(|line| {
                    line.strip_prefix("From ").is_some_and(|rest| {
                        rest.split(' ').next().is_some_and(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
                    })
                });
                let form = Form::new("Apply patch", "Apply", move |values: &Values| {
                    run(Command::ApplyPatch { patch: patch.clone(), mode: values.text("mode") })
                })
                .message(&name)
                .field(Field::select(
                    "mode",
                    "How",
                    if is_mailbox { "am" } else { "apply" },
                    options(&[
                        ("am", "Create commits (git am, for format-patch files)"),
                        ("apply", "Apply changes only, staged (git apply)"),
                    ]),
                ));
                self.ask(form)
            }
            Action::Clean => {
                let form = Form::new("Clean working tree", "Preview…", |values: &Values| {
                    act(Action::CleanPreview { directories: values.check("directories"), ignored: values.check("ignored") })
                })
                .message("Delete untracked files from the working tree.")
                .field(Field::check("directories", "Include untracked folders", true))
                .field(Field::check("ignored", "Also delete ignored files (build output, dependencies…)", false));
                self.ask(form)
            }
            Action::CleanPreview { directories, ignored } => {
                let repo = tab.path.clone();
                Task::perform(
                    async move {
                        ops::clean(&repo, directories, ignored, true)
                            .await
                            .map(|result| result.files.unwrap_or_default())
                            .map_err(|error| error.to_string())
                    },
                    move |files| act(Action::CleanPreviewed { directories, ignored, files }),
                )
            }
            Action::CleanPreviewed { directories, ignored, files } => {
                let files = match files {
                    Ok(files) => files,
                    Err(error) => return self.fail("Clean failed", error),
                };
                if files.is_empty() {
                    self.toast(ToastKind::Info, "Nothing to clean", None);
                    return Task::none();
                }
                let mut list = files.iter().take(25).cloned().collect::<Vec<_>>().join("\n");
                if files.len() > 25 {
                    list.push('\n');
                    list.push_str(&trf("… and {count} more", &[("count", &(files.len() - 25).to_string())]));
                }
                self.ask(Form::confirm(
                    &trf("Delete {count} item(s)?", &[("count", &files.len().to_string())]),
                    &list,
                    "Delete",
                    true,
                    run(Command::Clean { directories, ignored }),
                ))
            }
            Action::CheckoutFile { sha, path } => self.ask(Form::confirm(
                "Checkout file",
                &trf(
                    "Replace {path} in the working tree and index with its version from {commit}? Local changes to it are lost.",
                    &[("path", &path), ("commit", short_sha(&sha))],
                ),
                "Checkout file",
                true,
                run(Command::CheckoutFile { sha, path }),
            )),
            Action::Discard(paths) => {
                let what = match &paths[..] {
                    [one] => one.clone(),
                    many => plural(many.len(), "file"),
                };
                self.ask(Form::confirm(
                    "Discard changes",
                    &trf(
                        "Discard all changes to {what}? A backup is kept: Repository › Recover discarded changes.",
                        &[("what", &what)],
                    ),
                    "Discard",
                    true,
                    run(Command::Discard { paths }),
                ))
            }
            Action::CoAuthors => {
                let current = self.current().map(|tab| tab.co_authors.clone()).unwrap_or_default();
                self.ask(
                    Form::new("Co-authors", "Save", |values| {
                        Message::CoAuthors(values.text("authors"))
                    })
                    .message("Each co-author gets a “Co-authored-by” line in the commits you make next.")
                    .field(
                        Field::area("authors", "One per line", &current)
                            .hint("Name <name@example.com>"),
                    ),
                )
            }
            Action::DeleteDiscard(name) => self.ask(Form::confirm(
                "Delete backup",
                "Delete this backup of discarded changes? It can't be restored afterwards.",
                "Delete",
                true,
                run(Command::DeleteDiscard { name }),
            )),
            Action::ConflictOverwrite(index, choice) => self.conflict_choose(index, choice, true),
            Action::ConflictResolve { path, side } => Task::done(run(Command::Resolve { path, content: None, side })),
            Action::Clone | Action::Init | Action::CloneTo { .. } | Action::Cloned(_) | Action::InitAt(_) => Task::none(),
        }
    }

    fn clone_dialog(&mut self) -> Task<Message> {
        let home = gitcraft_core::store::home_dir()
            .to_string_lossy()
            .into_owned();
        let form = Form::new("Clone repository", "Clone", |values: &Values| {
            act(Action::CloneTo {
                url: values.text("url"),
                parent: values.text("parent"),
                name: values.opt("name"),
            })
        })
        .field(
            Field::text("url", "URL", "")
                .required()
                .hint("https://… or git@…"),
        )
        .field(Field::text("parent", "Into folder", &home).required())
        .field(Field::text("name", "Folder name", "").hint("Derived from the URL"));
        self.ask(form)
    }

    fn init_dialog(&mut self) -> Task<Message> {
        let form = Form::new("New repository", "Create", |values: &Values| {
            act(Action::InitAt(values.text("path")))
        })
        .field(
            Field::text("path", "Folder", "")
                .required()
                .hint("A new or existing folder"),
        );
        self.ask(form)
    }

    fn cloned(&mut self, result: Result<String, String>) -> Task<Message> {
        match result {
            Ok(path) => {
                self.toast(ToastKind::Success, "Clone complete", None);
                Task::done(Message::OpenPath(path))
            }
            Err(error) => self.fail("Clone failed", error),
        }
    }

    // ---------------------------------------------------------------- menus

    pub fn open_menu(&mut self, request: MenuRequest) -> Task<Message> {
        let at = self.cursor;
        match &request {
            MenuRequest::Commit { sha, .. } => {
                let Some(tab) = self.current() else {
                    return Task::none();
                };
                let known = tab
                    .commits
                    .iter()
                    .chain(tab.visible())
                    .find(|commit| commit.sha == *sha)
                    .cloned();
                if let Some(commit) = known {
                    self.show_commit_menu(request, &commit, at);
                    return Task::none();
                }
                // A commit outside the loaded history, such as from the reflog.
                let (repo, sha) = (tab.path.clone(), sha.clone());
                Task::perform(
                    async move {
                        let detail = queries::commit_detail(&repo, &sha).await;
                        detail
                            .map(|detail| Commit {
                                sha: detail.sha,
                                parents: detail.parents,
                                author: detail.author,
                                email: detail.email,
                                time: detail.time,
                                refs: Some(detail.refs),
                                subject: detail.subject,
                                graph: None,
                                file: None,
                                stash: None,
                            })
                            .map_err(|error| error.to_string())
                    },
                    move |result| Message::MenuResolved(request.clone(), at, result),
                )
            }
            MenuRequest::Commits(shas) => {
                let Some(tab) = self.current() else {
                    return Task::none();
                };
                let items = commits_menu_items(tab, shas);
                self.menu_at_cursor(items);
                Task::none()
            }
            MenuRequest::Stash(name) => {
                let Some(tab) = self.current() else {
                    return Task::none();
                };
                // Commands name the stash by its commit: see `ops::stash_apply`.
                let sha = tab
                    .refs
                    .stashes
                    .iter()
                    .find(|stash| stash.name == *name)
                    .map_or_else(|| name.clone(), |stash| stash.sha.clone());
                let name = name.clone();
                self.menu_at_cursor(vec![
                    MenuItem::entry(
                        "Apply",
                        run(Command::StashApply {
                            name: sha.clone(),
                            pop: false,
                        }),
                    ),
                    MenuItem::entry(
                        "Pop (apply and drop)",
                        run(Command::StashApply {
                            name: sha.clone(),
                            pop: true,
                        }),
                    ),
                    MenuItem::entry("Rename…", act(Action::RenameStash(sha.clone()))),
                    MenuItem::entry(
                        "New branch from stash…",
                        act(Action::NewBranch {
                            start: Some(format!("{name}^")),
                            label: Some(name.clone()),
                        }),
                    ),
                    MenuItem::danger("Drop…", act(Action::DropStash(sha))),
                ]);
                Task::none()
            }
            MenuRequest::Worktree(path) => {
                let Some(tree) = self
                    .current()
                    .and_then(|tab| tab.refs.worktrees.iter().find(|tree| tree.path == *path))
                    .cloned()
                else {
                    return Task::none();
                };
                let mut items = vec![
                    MenuItem::entry_if(
                        "Open in new tab",
                        !tree.current,
                        Message::OpenPath(tree.path.clone()),
                    ),
                    MenuItem::entry("Copy path", Message::Copy(tree.path.clone())),
                ];
                if tree.prunable {
                    items.push(MenuItem::entry(
                        "Prune stale worktrees",
                        run(Command::WorktreePrune),
                    ));
                } else if !tree.main && !tree.current {
                    items.push(MenuItem::danger(
                        "Remove…",
                        act(Action::RemoveWorktree(tree.path.clone())),
                    ));
                }
                self.menu_at_cursor(items);
                Task::none()
            }
            MenuRequest::Remote(name) => {
                let url = self
                    .current()
                    .and_then(|tab| tab.refs.remotes.iter().find(|remote| remote.name == *name))
                    .map_or_else(String::new, |remote| remote.fetch_url.clone());
                let name = name.clone();
                self.menu_at_cursor(vec![
                    MenuItem::entry(
                        trf("Fetch {name}", &[("name", &name)]),
                        run(Command::Fetch {
                            remote: Some(name.clone()),
                            tags: false,
                        }),
                    ),
                    MenuItem::entry(
                        "Prune stale branches",
                        run(Command::PruneRemote { name: name.clone() }),
                    ),
                    MenuItem::entry("Edit…", act(Action::EditRemote(name.clone()))),
                    MenuItem::danger("Remove…", act(Action::RemoveRemote(name))),
                    MenuItem::Separator,
                    MenuItem::entry("Copy URL", Message::Copy(url)),
                ]);
                Task::none()
            }
            MenuRequest::Repository => {
                let Some(tab) = self.current() else {
                    return Task::none();
                };
                let items = repository_items(tab);
                self.menu_at_cursor(items);
                Task::none()
            }
            MenuRequest::File { area, path, rev } => {
                let Some(tab) = self.current() else {
                    return Task::none();
                };
                let tools = self.tool_labels();
                let items = file_menu_items(tab, *area, path, rev.as_deref(), &tools);
                self.menu_at_cursor(items);
                Task::none()
            }
            MenuRequest::Columns => {
                use crate::app::Column;
                use crate::menu::Mark;
                let columns = &self.settings.columns;
                let toggle = |label: &str, column: Column, on: bool| {
                    MenuItem::entry(label, Message::ToggleColumn(column)).marked(Mark::Check(on))
                };
                self.menu_at_cursor(vec![
                    MenuItem::Header(tr("Columns").to_owned()),
                    toggle("Author", Column::Author, columns.author),
                    toggle("Date", Column::Date, columns.date),
                    toggle("Commit", Column::Sha, columns.sha),
                    toggle("Changes", Column::Changes, columns.changes),
                    MenuItem::Separator,
                    MenuItem::entry("Reset columns", Message::ColumnsReset),
                ]);
                Task::none()
            }
            MenuRequest::CommitOptions => {
                use crate::app::CommitOption;
                use crate::menu::Mark;
                let prefs = &self.settings.commit;
                let toggle = |label: &str, option: CommitOption, on: bool| {
                    MenuItem::entry(label, Message::CommitOption(option, !on))
                        .marked(Mark::Check(on))
                };
                let co_authors = self
                    .current()
                    .map_or(0, |tab| logic::co_author_trailers(&tab.co_authors).len());
                let co_label = if co_authors == 0 {
                    tr("Co-authors…").to_owned()
                } else {
                    trf(
                        "Co-authors ({count})…",
                        &[("count", &co_authors.to_string())],
                    )
                };
                self.menu_at_cursor(vec![
                    toggle(
                        "Sign off (Signed-off-by line)",
                        CommitOption::SignOff,
                        prefs.sign_off,
                    ),
                    toggle(
                        "Skip commit hooks",
                        CommitOption::SkipHooks,
                        prefs.skip_hooks,
                    ),
                    toggle(
                        "Push after committing",
                        CommitOption::PushAfter,
                        prefs.push_after,
                    ),
                    MenuItem::Separator,
                    MenuItem::entry(co_label, act(Action::CoAuthors)),
                ]);
                Task::none()
            }
            MenuRequest::Tab(index) => {
                let last = self.tabs.len().saturating_sub(1);
                let path = self
                    .tabs
                    .get(*index)
                    .map_or_else(String::new, |tab| tab.path.clone());
                let index = *index;
                self.menu_at_cursor(vec![
                    MenuItem::entry("Close", Message::Close(index)),
                    MenuItem::entry_if("Move left", index > 0, Message::MoveTab(index, -1)),
                    MenuItem::entry_if("Move right", index < last, Message::MoveTab(index, 1)),
                    MenuItem::Separator,
                    MenuItem::entry("Copy repository path", Message::Copy(path.clone())),
                    MenuItem::entry(reveal_label(), Message::Reveal(path)),
                ]);
                Task::none()
            }
        }
    }

    /// The chosen diff and merge tools, as the menus name them; `None` when none is chosen.
    fn tool_labels(&self) -> ToolLabels {
        let chosen = &self.settings.tools;
        let label = |merge: bool, name: &str| {
            (!name.is_empty()).then(|| self.found_tools.label(merge, name))
        };
        ToolLabels {
            diff: label(false, &chosen.diff),
            merge: label(true, &chosen.merge),
        }
    }

    /// The menus of the menu bar: title and entries. The bar itself is drawn by craft-menu.
    pub fn bar_menus(&self) -> Vec<(&'static str, Vec<MenuItem>)> {
        use crate::menu::{Mark, Shortcut};
        use egui::Key;
        let tab = self.current();
        let has_repo = tab.is_some();
        let in_changes = tab.is_some_and(|tab| tab.view == View::Changes);
        let idle = self.busy.is_none();

        let recent: Vec<MenuItem> = self
            .recent
            .iter()
            .take(15)
            .map(|repo| MenuItem::entry(repo.name.clone(), Message::OpenPath(repo.path.clone())))
            .collect();
        let mut file = vec![
            MenuItem::entry("Open repository…", Message::OpenDialog).key(Shortcut::command(Key::O)),
            MenuItem::entry("Clone repository…", act(Action::Clone)),
            MenuItem::entry("New repository…", act(Action::Init)),
        ];
        if !recent.is_empty() {
            file.push(MenuItem::sub("Recent", recent));
        }
        file.extend([
            MenuItem::Separator,
            MenuItem::entry_if("Close tab", has_repo, Message::Close(self.active))
                .key(Shortcut::command(Key::W)),
            MenuItem::Separator,
            MenuItem::entry("Settings…", Message::OpenSettings).key(Shortcut::command(Key::Comma)),
            MenuItem::Separator,
            MenuItem::entry("Quit", Message::Quit),
        ]);

        let no_labels = gitcraft_core::model::UndoLabels::default();
        let labels = tab.map_or(&no_labels, |tab| &tab.undo);
        let undo = match &labels.undo {
            Some(label) => MenuItem::entry_if(
                trf("Undo: {action}", &[("action", tr(label))]),
                idle,
                Message::Undo,
            ),
            None => MenuItem::entry_if("Undo", false, Message::Undo),
        };
        let redo = match &labels.redo {
            Some(label) => MenuItem::entry_if(
                trf("Redo: {action}", &[("action", tr(label))]),
                idle,
                Message::Redo,
            ),
            None => MenuItem::entry_if("Redo", false, Message::Redo),
        };
        let edit = vec![
            undo.key(Shortcut::command(Key::Z)),
            redo.key(Shortcut {
                key: Key::Z,
                command: true,
                shift: true,
            }),
        ];

        let mut repository = match tab {
            Some(tab) => {
                let mut items = vec![
                    MenuItem::entry_if(
                        "Fetch",
                        idle,
                        run(Command::Fetch {
                            remote: None,
                            tags: false,
                        }),
                    ),
                    MenuItem::entry_if("Pull…", idle, act(Action::Pull)),
                    MenuItem::entry_if("Push…", idle, act(Action::Push(None))),
                    MenuItem::Separator,
                    MenuItem::entry_if("Stash…", idle, act(Action::Stash)),
                    MenuItem::entry_if(
                        "New branch…",
                        idle,
                        act(Action::NewBranch {
                            start: None,
                            label: None,
                        }),
                    ),
                    MenuItem::Separator,
                ];
                items.extend(repository_items(tab));
                items
            }
            None => Vec::new(),
        };
        if repository.is_empty() {
            repository.push(MenuItem::entry_if(
                "No repository is open",
                false,
                Message::Refresh,
            ));
        }

        let theme = &self.settings.appearance.theme;
        let themes = [
            ("system", "Follow system"),
            ("light", "Light"),
            ("dark", "Dark"),
        ]
        .into_iter()
        .map(|(value, label)| {
            MenuItem::entry(label, Message::SetTheme(value.to_owned()))
                .marked(Mark::Radio(theme == value))
        })
        .collect();
        let chosen = &self.settings.general.language;
        let mut languages = vec![
            MenuItem::entry(
                "System language",
                Message::SetLanguage(crate::i18n::AUTO.to_owned()),
            )
            .marked(Mark::Radio(chosen == crate::i18n::AUTO)),
            MenuItem::Separator,
        ];
        languages.extend(crate::i18n::languages().into_iter().map(|(code, name)| {
            MenuItem::entry(name, Message::SetLanguage(code.to_owned()))
                .marked(Mark::Radio(chosen == code))
        }));
        // The open repositories, so that one can be picked when the tab bar runs out of room.
        let open_tabs: Vec<MenuItem> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                MenuItem::entry(tab.name.clone(), Message::Activate(index))
                    .marked(Mark::Radio(index == self.active))
            })
            .collect();
        let mut view = Vec::new();
        if !open_tabs.is_empty() {
            view.push(MenuItem::sub("Tabs", open_tabs));
            view.push(MenuItem::Separator);
        }
        view.extend([
            MenuItem::entry_if("All commits", has_repo, Message::SetView(View::History))
                .key(Shortcut::command(Key::Num1)),
            MenuItem::entry_if("Local changes", has_repo, Message::SetView(View::Changes))
                .key(Shortcut::command(Key::Num2)),
            MenuItem::entry_if("Select all files", in_changes, Message::SelectAllFiles)
                .key(Shortcut::command(Key::A)),
            MenuItem::entry_if("Search commits", has_repo, Message::SearchFocus)
                .key(Shortcut::command(Key::F)),
            MenuItem::entry_if("Filter history…", has_repo, act(Action::FilterHistory)),
            MenuItem::entry_if("Refresh", has_repo, Message::Refresh).key(Shortcut::plain(Key::F5)),
            MenuItem::Separator,
            MenuItem::entry("Zoom in", Message::Zoom(1.1)).key(Shortcut::command(Key::Equals)),
            MenuItem::entry("Zoom out", Message::Zoom(1.0 / 1.1))
                .key(Shortcut::command(Key::Minus)),
            MenuItem::entry("Actual size", Message::Zoom(0.0)).key(Shortcut::command(Key::Num0)),
            MenuItem::entry(
                "Full screen",
                Message::Window(crate::app::WindowAction::ToggleFullscreen),
            )
            .key(Shortcut::plain(Key::F11))
            .marked(Mark::Check(self.fullscreen)),
            MenuItem::Separator,
            MenuItem::sub("Theme", themes),
            MenuItem::sub("Language", languages),
            MenuItem::entry("Use system title bar", Message::ToggleSystemTitleBar)
                .marked(Mark::Check(self.settings.appearance.system_title_bar)),
        ]);

        let folder = gitcraft_core::store::config_dir()
            .to_string_lossy()
            .into_owned();
        let help = vec![MenuItem::entry(
            "Show data folder",
            Message::OpenFile(folder),
        )];

        vec![
            ("File", file),
            ("Edit", edit),
            ("Repository", repository),
            ("View", view),
            ("Help", help),
        ]
    }

    pub fn show_commit_menu(&mut self, request: MenuRequest, commit: &Commit, at: Pos2) {
        let MenuRequest::Commit { focus, .. } = request else {
            return;
        };
        let Some(tab) = self.current() else { return };
        let items = commit_menu_items(tab, commit, focus.as_ref());
        self.menu = Some(crate::menu::Menu::new(at, items));
    }
}

fn clone_task(url: String, parent: String, name: Option<String>) -> Task<Message> {
    Task::perform(
        async move {
            let parent = gitcraft_core::paths::expand(&parent);
            ops::clone(&parent, url.trim(), name.as_deref(), None)
                .await
                .map_err(|error| error.to_string())
        },
        |result| act(Action::Cloned(result)),
    )
}

fn init_task(path: String) -> Task<Message> {
    Task::perform(
        async move {
            let path = gitcraft_core::paths::expand(&path);
            ops::init(&path).await.map_err(|error| error.to_string())
        },
        |result| match result {
            Ok(path) => Message::OpenPath(path),
            Err(error) => Message::Located(Err(error)),
        },
    )
}

fn refs_of(tab: &RepoTab, commit: &Commit) -> Vec<RefTarget> {
    let mut targets = Vec::new();
    for label in commit.refs.iter().flatten() {
        match label.kind.as_str() {
            "local"
                if tab
                    .refs
                    .local
                    .iter()
                    .any(|branch| branch.name == label.name) =>
            {
                targets.push(RefTarget::Local(label.name.clone()));
            }
            "remote"
                if tab
                    .refs
                    .remote
                    .iter()
                    .any(|branch| branch.name == label.name) =>
            {
                targets.push(RefTarget::Remote(label.name.clone()));
            }
            "tag" if tab.refs.tags.iter().any(|tag| tag.name == label.name) => {
                targets.push(RefTarget::Tag(label.name.clone()));
            }
            _ => {}
        }
    }
    targets
}

fn ref_title(target: &RefTarget) -> String {
    match target {
        RefTarget::Local(name) => trf("Branch {name}", &[("name", name)]),
        RefTarget::Remote(name) => name.clone(),
        RefTarget::Tag(name) => trf("Tag {name}", &[("name", name)]),
    }
}

fn local_ref_items(tab: &RepoTab, branch: &LocalBranch) -> Vec<MenuItem> {
    let current = tab.current_branch();
    let shown = current.unwrap_or("HEAD");
    let name = branch.name.clone();
    let other = !branch.current && current.is_some();
    let mut items = vec![
        MenuItem::entry_if(
            trf("Checkout {name}", &[("name", &name)]),
            !branch.current,
            run(Command::Checkout {
                reference: name.clone(),
                kind: "local",
            }),
        ),
        MenuItem::entry_if(
            trf(
                "Merge {name} into {shown}…",
                &[("name", &name), ("shown", shown)],
            ),
            other,
            act(Action::Merge(name.clone())),
        ),
        MenuItem::entry_if(
            trf(
                "Rebase {shown} onto {name}…",
                &[("name", &name), ("shown", shown)],
            ),
            other,
            act(Action::Rebase {
                onto: name.clone(),
                label: name.clone(),
            }),
        ),
    ];
    if let Some(upstream) = &branch.upstream {
        items.push(MenuItem::entry_if(
            trf("Fast-forward to {upstream}", &[("upstream", upstream)]),
            branch.behind > 0 && branch.ahead == 0,
            run(Command::FastForward {
                branch: name.clone(),
                target: None,
            }),
        ));
    }
    items.push(MenuItem::Separator);
    items.push(MenuItem::entry(
        "Push…",
        act(Action::Push(Some(name.clone()))),
    ));
    if branch.current {
        items.push(MenuItem::entry("Pull…", act(Action::Pull)));
    }
    items.extend([
        MenuItem::entry("Set upstream…", act(Action::SetUpstream(name.clone()))),
        MenuItem::Separator,
        MenuItem::entry("Rename…", act(Action::RenameBranch(name.clone()))),
        MenuItem::danger_if(
            "Delete…",
            !branch.current,
            act(Action::DeleteBranch(name.clone())),
        ),
        MenuItem::Separator,
    ]);
    items.extend(visibility_items(tab, &format!("refs/heads/{name}")));
    items.push(MenuItem::entry("Copy branch name", Message::Copy(name)));
    items
}

/// Hide or solo a branch in the graph of all branches.
fn visibility_items(tab: &RepoTab, full: &str) -> Vec<MenuItem> {
    use crate::app::RefVisibility;
    let view = &tab.ref_view;
    let hidden = view.hidden.iter().any(|name| name == full);
    let solo = view.solo.iter().any(|name| name == full);
    let change = |change| Message::RefVisibility(change);
    vec![
        if hidden {
            MenuItem::entry(
                "Show in graph",
                change(RefVisibility::Show(full.to_owned())),
            )
        } else {
            MenuItem::entry(
                "Hide in graph",
                change(RefVisibility::Hide(full.to_owned())),
            )
        },
        if solo {
            MenuItem::entry(
                "Stop soloing",
                change(RefVisibility::Unsolo(full.to_owned())),
            )
        } else {
            MenuItem::entry(
                "Solo in graph",
                change(RefVisibility::Solo(full.to_owned())),
            )
        },
    ]
}

fn ref_items(tab: &RepoTab, target: &RefTarget) -> Vec<MenuItem> {
    let current = tab.current_branch();
    let shown = current.unwrap_or("HEAD");
    match target {
        RefTarget::Local(name) => tab
            .refs
            .local
            .iter()
            .find(|branch| branch.name == *name)
            .map_or_else(Vec::new, |branch| local_ref_items(tab, branch)),
        RefTarget::Remote(name) => {
            let Some(branch) = tab.refs.remote.iter().find(|branch| branch.name == *name) else {
                return Vec::new();
            };
            let local = tab
                .refs
                .local
                .iter()
                .find(|local| local.upstream.as_deref() == Some(name));
            let mut items = vec![MenuItem::entry(
                trf("Checkout {branch} (track)", &[("branch", &branch.branch)]),
                run(Command::Checkout {
                    reference: name.clone(),
                    kind: "remote",
                }),
            )];
            if let Some(local) = local {
                items.push(MenuItem::entry_if(
                    trf("Fast-forward {branch} to here", &[("branch", &local.name)]),
                    local.behind > 0 && local.ahead == 0,
                    run(Command::FastForward {
                        branch: local.name.clone(),
                        target: Some(name.clone()),
                    }),
                ));
            }
            items.extend([
                MenuItem::entry_if(
                    trf(
                        "Merge {name} into {shown}…",
                        &[("name", name), ("shown", shown)],
                    ),
                    current.is_some(),
                    act(Action::Merge(name.clone())),
                ),
                MenuItem::entry_if(
                    trf(
                        "Rebase {shown} onto {name}…",
                        &[("name", name), ("shown", shown)],
                    ),
                    current.is_some(),
                    act(Action::Rebase {
                        onto: name.clone(),
                        label: name.clone(),
                    }),
                ),
                MenuItem::Separator,
                MenuItem::danger(
                    trf("Delete from {remote}…", &[("remote", &branch.remote)]),
                    act(Action::DeleteRemoteBranch(name.clone())),
                ),
                MenuItem::Separator,
            ]);
            items.extend(visibility_items(tab, &format!("refs/remotes/{name}")));
            items.push(MenuItem::entry(
                "Copy branch name",
                Message::Copy(name.clone()),
            ));
            items
        }
        RefTarget::Tag(name) => {
            let sha = tab
                .refs
                .tags
                .iter()
                .find(|tag| tag.name == *name)
                .map_or_else(String::new, |tag| tag.sha.clone());
            vec![
                MenuItem::entry("Checkout tag…", act(Action::CheckoutCommit(sha))),
                MenuItem::entry_if(
                    trf(
                        "Merge {name} into {shown}…",
                        &[("name", name), ("shown", shown)],
                    ),
                    current.is_some(),
                    act(Action::Merge(name.clone())),
                ),
                MenuItem::entry_if(
                    "Push to remote…",
                    !tab.refs.remotes.is_empty(),
                    act(Action::PushTag(name.clone())),
                ),
                MenuItem::danger("Delete…", act(Action::DeleteTag(name.clone()))),
                MenuItem::sub(
                    "Delete from remote",
                    tab.refs
                        .remotes
                        .iter()
                        .map(|remote| {
                            MenuItem::danger(
                                remote.name.clone(),
                                act(Action::DeleteRemoteTag {
                                    remote: remote.name.clone(),
                                    name: name.clone(),
                                }),
                            )
                        })
                        .collect(),
                ),
                MenuItem::entry("Copy tag name", Message::Copy(name.clone())),
            ]
        }
    }
}

/// When a branch or tag was right-clicked its actions come first; other refs on the commit get submenus.
pub fn commit_menu_items(
    tab: &RepoTab,
    commit: &Commit,
    focus: Option<&RefTarget>,
) -> Vec<MenuItem> {
    let mut items = Vec::new();
    if let Some(focus) = focus {
        items.push(MenuItem::Header(ref_title(focus)));
        items.extend(ref_items(tab, focus));
        items.push(MenuItem::Separator);
    }
    let others: Vec<RefTarget> = refs_of(tab, commit)
        .into_iter()
        .filter(|target| Some(target) != focus)
        .collect();
    for target in &others {
        items.push(MenuItem::sub(ref_title(target), ref_items(tab, target)));
    }
    if !others.is_empty() {
        items.push(MenuItem::Separator);
    }
    items.push(MenuItem::Header(trf(
        "Commit {commit}",
        &[("commit", short_sha(&commit.sha))],
    )));
    items.extend(commit_items(tab, commit));
    items
}

fn commit_items(tab: &RepoTab, commit: &Commit) -> Vec<MenuItem> {
    let current = tab.current_branch().unwrap_or("HEAD");
    let head = tab.refs.head.as_deref();
    let is_head = Some(commit.sha.as_str()) == head;
    let sha = commit.sha.clone();
    let has_parent = !commit.parents.is_empty();
    let reset = |label: &str, mode: &'static str| {
        let message = act(Action::ResetTo {
            sha: sha.clone(),
            mode,
        });
        if mode == "hard" {
            MenuItem::danger(label, message)
        } else {
            MenuItem::entry(label, message)
        }
    };
    let mut items = vec![
        MenuItem::entry_if(
            "Checkout commit…",
            !is_head,
            act(Action::CheckoutCommit(sha.clone())),
        ),
        MenuItem::entry(
            "New branch here…",
            act(Action::NewBranch {
                start: Some(sha.clone()),
                label: None,
            }),
        ),
        MenuItem::entry("New tag here…", act(Action::CreateTag(sha.clone()))),
        MenuItem::Separator,
        MenuItem::entry_if(
            trf("Merge into {current}…", &[("current", current)]),
            !is_head,
            act(Action::Merge(sha.clone())),
        ),
        MenuItem::entry_if(
            trf("Rebase {current} onto here…", &[("current", current)]),
            !is_head,
            act(Action::Rebase {
                onto: sha.clone(),
                label: short_sha(&sha).to_owned(),
            }),
        ),
        MenuItem::entry(
            "Interactive rebase from here…",
            Message::OpenRebase(Some(sha.clone())),
        ),
        MenuItem::entry_if(
            trf("Cherry-pick into {current}", &[("current", current)]),
            !is_head,
            act(Action::CherryPick(sha.clone())),
        ),
        MenuItem::entry("Revert commit…", act(Action::Revert(sha.clone()))),
        MenuItem::sub(
            trf("Reset {current} to here", &[("current", current)]),
            vec![
                reset("Soft: keep all changes staged", "soft"),
                reset("Mixed: keep changes, unstaged", "mixed"),
                reset("Keep: discard commits, keep local changes", "keep"),
                reset("Hard: discard all changes", "hard"),
            ],
        ),
    ];
    if is_head && has_parent {
        items.push(MenuItem::entry(
            "Undo last commit (keep changes)",
            act(Action::UndoLast),
        ));
    }
    items.push(MenuItem::sub(
        "Edit commit",
        vec![
            MenuItem::entry("Edit message…", act(Action::EditMessage(sha.clone()))),
            MenuItem::entry_if(
                "Squash into parent…",
                has_parent,
                act(Action::Squash(sha.clone())),
            ),
            MenuItem::danger("Drop commit…", act(Action::DropCommit(sha.clone()))),
            MenuItem::entry_if(
                "Move up (after its child)",
                !is_head,
                act(Action::MoveCommit(sha.clone(), true)),
            ),
            MenuItem::entry_if(
                "Move down (before its parent)",
                has_parent,
                act(Action::MoveCommit(sha.clone(), false)),
            ),
            MenuItem::Separator,
            MenuItem::entry_if(
                "Fixup commit of the staged changes",
                tab.status
                    .as_ref()
                    .is_some_and(|status| !status.staged.is_empty()),
                run(Command::FixupCommit { sha: sha.clone() }),
            ),
        ],
    ));
    let bisect = match &tab.bisect {
        None => vec![MenuItem::entry(
            "Start bisect: this commit is bad…",
            act(Action::BisectStart(sha.clone())),
        )],
        Some(_) => {
            let mark = |verdict| {
                run(Command::BisectMark {
                    verdict,
                    sha: Some(sha.clone()),
                })
            };
            vec![
                MenuItem::entry("Mark as good", mark("good")),
                MenuItem::entry("Mark as bad", mark("bad")),
                MenuItem::entry("Skip (can't be tested)", mark("skip")),
                MenuItem::Separator,
                MenuItem::entry("End bisect", run(Command::BisectReset)),
            ]
        }
    };
    items.push(MenuItem::sub("Bisect", bisect));
    items.push(MenuItem::Separator);
    if let Some(head) = head.filter(|_| !is_head) {
        items.push(MenuItem::entry(
            "Compare with HEAD",
            Message::CompareStart(sha.clone(), head.to_owned()),
        ));
    }
    items.push(MenuItem::entry(
        "Compare with working directory",
        Message::CompareStart(sha.clone(), gitcraft_core::queries::WORKTREE.to_owned()),
    ));
    if let Some(selected) = tab.selected.as_deref().filter(|selected| *selected != sha) {
        items.push(MenuItem::entry(
            trf("Compare with {commit}", &[("commit", short_sha(selected))]),
            Message::CompareStart(sha.clone(), selected.to_owned()),
        ));
    }
    items.push(MenuItem::entry_if(
        "Save as patch…",
        commit.parents.len() <= 1,
        act(Action::SavePatch(sha.clone())),
    ));
    if !is_head {
        items.push(MenuItem::entry(
            "Save commits from here to HEAD as patches…",
            act(Action::SavePatchSeries(sha.clone())),
        ));
    }
    items.push(MenuItem::sub(
        "Copy",
        vec![
            MenuItem::entry("SHA", Message::Copy(sha.clone())),
            MenuItem::entry("Short SHA", Message::Copy(short_sha(&sha).to_owned())),
            MenuItem::entry("Subject", Message::Copy(commit.subject.clone())),
            MenuItem::entry(
                "Author",
                Message::Copy(format!("{} <{}>", commit.author, commit.email)),
            ),
        ],
    ));
    items
}

/// The menu for several picked commits (newest first).
pub fn commits_menu_items(tab: &RepoTab, shas: &[String]) -> Vec<MenuItem> {
    use crate::app::Preset;
    let count = shas.len().to_string();
    let n = |text: &str| trf(text, &[("count", &count)]);
    let has_merge = tab
        .commits
        .iter()
        .any(|commit| shas.contains(&commit.sha) && commit.parents.len() > 1);
    // Oldest first, as they are applied.
    let oldest_first: Vec<String> = shas.iter().rev().cloned().collect();
    vec![
        MenuItem::Header(n("{count} commits")),
        MenuItem::entry_if(
            n("Cherry-pick {count} commits into current"),
            !has_merge,
            run(Command::CherryPickMany {
                shas: oldest_first.clone(),
            }),
        ),
        MenuItem::entry_if(
            "Interactive cherry-pick…",
            !has_merge,
            Message::OpenReplay(shas.to_vec()),
        ),
        MenuItem::Separator,
        MenuItem::entry_if(
            n("Squash {count} commits…"),
            !has_merge,
            Message::PlanCommits(Preset::Squash(shas.to_vec())),
        ),
        MenuItem::entry_if(
            n("Drop {count} commits…"),
            !has_merge,
            Message::PlanCommits(Preset::Drop(shas.to_vec())),
        ),
        MenuItem::Separator,
        MenuItem::entry("Copy SHAs", Message::Copy(oldest_first.join("\n"))),
    ]
}

fn repository_items(tab: &RepoTab) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::entry("Apply patch…", act(Action::ApplyPatch)),
        MenuItem::entry(
            "Save local changes as patch…",
            act(Action::SaveChangesPatch),
        ),
        MenuItem::danger("Clean untracked files…", act(Action::Clean)),
        MenuItem::entry("Reflog (recover lost commits)…", Message::OpenReflog),
        MenuItem::entry("Recover discarded changes…", Message::OpenDiscards),
        MenuItem::entry("Compare commit ranges…", act(Action::RangeDiff)),
        MenuItem::Separator,
        MenuItem::entry_if(
            "Fetch all remotes (incl. tags)",
            !tab.refs.remotes.is_empty(),
            run(Command::Fetch {
                remote: None,
                tags: true,
            }),
        ),
        MenuItem::entry("Add remote…", act(Action::AddRemote)),
    ];
    // A shallow or partial clone can get what it left out.
    if tab.refs.shallow {
        items.extend([
            MenuItem::entry(
                "Fetch 500 more commits",
                run(Command::FetchMore { how: "deepen" }),
            ),
            MenuItem::entry(
                "Fetch the whole history",
                run(Command::FetchMore { how: "unshallow" }),
            ),
        ]);
    }
    if tab.refs.partial {
        items.push(MenuItem::entry(
            "Download everything the partial clone left out",
            run(Command::FetchMore { how: "refetch" }),
        ));
    }
    items.extend([MenuItem::entry(
        "Add worktree…",
        act(Action::AddWorktree(None)),
    )]);
    if !tab.refs.submodules.is_empty() {
        items.push(MenuItem::entry(
            "Update submodules",
            run(Command::UpdateSubmodules),
        ));
    }
    items.extend([
        MenuItem::Separator,
        MenuItem::entry("Refresh", Message::Refresh),
        MenuItem::entry("Copy repository path", Message::Copy(tab.path.clone())),
        MenuItem::entry(reveal_label(), Message::Reveal(tab.path.clone())),
    ]);
    items
}

fn targets(tab: &RepoTab, area: Area, path: &str) -> Vec<String> {
    if tab.picked.0 == area && tab.picked.1.iter().any(|picked| picked == path) {
        tab.picked.1.clone()
    } else {
        vec![path.to_owned()]
    }
}

/// The chosen external tools, by the names the menus show.
struct ToolLabels {
    diff: Option<String>,
    merge: Option<String>,
}

/// "Open in KDiff3" for a file's change, or a way to choose a tool when there is none yet.
fn diff_tool_item(
    tools: &ToolLabels,
    path: &str,
    orig: Option<String>,
    target: ToolTarget,
) -> MenuItem {
    match &tools.diff {
        Some(tool) => MenuItem::entry(
            trf("Open in {tool}", &[("tool", tool)]),
            Message::OpenDiffTool {
                path: path.to_owned(),
                orig,
                target,
            },
        ),
        None => MenuItem::entry("Choose a diff tool…", Message::OpenSettings),
    }
}

fn file_menu_items(
    tab: &RepoTab,
    area: Option<Area>,
    path: &str,
    rev: Option<&str>,
    tools: &ToolLabels,
) -> Vec<MenuItem> {
    let mut items = Vec::new();
    // The file as listed, for a rename's old name and whether it is new.
    let file_in = |area: Area| {
        tab.files_in(area)
            .into_iter()
            .find(|file| file.path == path)
    };
    if let Some(area) = area {
        let paths = targets(tab, area, path);
        // "Stage" for one file, "Stage 3 files" for several.
        let count = paths.len().to_string();
        let label = |one: &str, many: &str| {
            if paths.len() > 1 {
                trf(many, &[("count", &count)])
            } else {
                tr(one).to_owned()
            }
        };
        match area {
            Area::Unstaged => {
                let listed = tab.files_in(area);
                let all_untracked = paths.iter().all(|picked| {
                    listed
                        .iter()
                        .find(|file| file.path == *picked)
                        .is_some_and(|file| file.status == "?")
                });
                items.extend([
                    MenuItem::entry(
                        label("Stage", "Stage {count} files"),
                        run(Command::Stage {
                            paths: paths.clone(),
                        }),
                    ),
                    MenuItem::danger(
                        label("Discard changes…", "Discard changes to {count} files…"),
                        act(Action::Discard(paths.clone())),
                    ),
                    MenuItem::entry(
                        label("Stash…", "Stash {count} files…"),
                        act(Action::StashFiles(paths.clone())),
                    ),
                ]);
                // A new file has nothing to compare with yet.
                if paths.len() == 1
                    && let Some(file) = file_in(area).filter(|file| file.status != "?")
                {
                    items.push(diff_tool_item(
                        tools,
                        path,
                        file.orig_path,
                        ToolTarget::Working { staged: false },
                    ));
                }
                if paths.len() == 1 {
                    let choices = logic::ignore_patterns(path)
                        .into_iter()
                        .map(|(label, pattern)| {
                            MenuItem::entry(
                                label,
                                run(Command::Ignore {
                                    patterns: vec![pattern],
                                }),
                            )
                        })
                        .collect();
                    items.push(MenuItem::sub("Ignore", choices));
                } else if all_untracked {
                    let patterns = paths.iter().map(|picked| format!("/{picked}")).collect();
                    items.push(MenuItem::entry(
                        label("Ignore", "Ignore {count} files"),
                        run(Command::Ignore { patterns }),
                    ));
                }
            }
            Area::Staged => {
                let one = paths.len() == 1;
                items.extend([
                    MenuItem::entry(
                        label("Unstage", "Unstage {count} files"),
                        run(Command::Unstage {
                            paths: paths.clone(),
                        }),
                    ),
                    MenuItem::entry(
                        label("Stash…", "Stash {count} files…"),
                        act(Action::StashFiles(paths.clone())),
                    ),
                ]);
                // A file resolved during a merge, rebase or cherry-pick can be taken up again.
                if tab
                    .status
                    .as_ref()
                    .is_some_and(|status| status.operation.is_some())
                {
                    items.push(MenuItem::entry(
                        "Mark as conflicted again",
                        run(Command::MarkConflicted { paths }),
                    ));
                }
                if one {
                    let orig = file_in(area).and_then(|file| file.orig_path);
                    items.push(diff_tool_item(
                        tools,
                        path,
                        orig,
                        ToolTarget::Working { staged: true },
                    ));
                }
            }
            Area::Conflicts => {
                let resolve = |side: Option<&str>| {
                    act(Action::ConflictResolve {
                        path: path.to_owned(),
                        side: side.map(str::to_owned),
                    })
                };
                if let Some(tool) = &tools.merge {
                    items.push(MenuItem::entry(
                        trf("Resolve in {tool}", &[("tool", tool)]),
                        Message::OpenMergeTool(path.to_owned()),
                    ));
                }
                items.extend([
                    MenuItem::entry(
                        "Resolve in merge tool…",
                        Message::OpenConflict(path.to_owned()),
                    ),
                    MenuItem::entry("Resolve using ours", resolve(Some("ours"))),
                    MenuItem::entry("Resolve using theirs", resolve(Some("theirs"))),
                    MenuItem::entry("Mark as resolved", resolve(None)),
                ]);
            }
        }
        items.push(MenuItem::Separator);
    }
    let full = join_path(&tab.path, path);
    items.extend([
        MenuItem::entry("File history", Message::OpenFileHistory(path.to_owned())),
        MenuItem::entry(
            "History of its folder",
            Message::SetHistoryFilter(Some(gitcraft_core::model::LogFilter {
                path: std::path::Path::new(path)
                    .parent()
                    .map(|dir| dir.to_string_lossy().replace('\\', "/"))
                    .filter(|dir| !dir.is_empty())
                    .unwrap_or_else(|| ".".to_owned()),
                ..Default::default()
            })),
        ),
        MenuItem::entry(
            "Blame",
            Message::OpenBlame(path.to_owned(), rev.map(str::to_owned)),
        ),
    ]);
    // A file of a commit, or of two commits compared.
    if let Some(rev) = rev {
        let comparing = tab.compare.as_ref().filter(|compare| compare.target == rev);
        let (target, orig) = match comparing {
            Some(compare) => (
                ToolTarget::Compare {
                    base: compare.base.clone(),
                    target: compare.target.clone(),
                },
                compare
                    .result
                    .iter()
                    .flat_map(|result| &result.files)
                    .find(|file| file.path == path)
                    .and_then(|file| file.orig_path.clone()),
            ),
            None => {
                let orig = tab
                    .detail
                    .as_ref()
                    .filter(|detail| detail.sha == rev)
                    .and_then(|detail| detail.files.iter().find(|file| file.path == path))
                    .and_then(|file| file.orig_path.clone());
                (
                    ToolTarget::Commit {
                        sha: rev.to_owned(),
                    },
                    orig,
                )
            }
        };
        items.push(diff_tool_item(tools, path, orig, target));
        items.push(MenuItem::entry(
            "Checkout this version of the file…",
            act(Action::CheckoutFile {
                sha: rev.to_owned(),
                path: path.to_owned(),
            }),
        ));
        items.push(MenuItem::entry(
            "Copy file contents",
            Message::CopyFileText(rev.to_owned(), path.to_owned()),
        ));
        let stash = tab
            .stash
            .as_ref()
            .filter(|_| tab.view == View::Stash)
            .and_then(|open| {
                tab.refs
                    .stashes
                    .iter()
                    .find(|stash| stash.name == open.name)
            });
        if let Some(stash) = stash {
            items.push(MenuItem::entry(
                "Take this file from the stash",
                run(Command::StashFile {
                    id: stash.sha.clone(),
                    path: path.to_owned(),
                }),
            ));
        }
    }
    items.extend([
        MenuItem::Separator,
        MenuItem::entry("Open file", Message::OpenFile(full.clone())),
        MenuItem::entry(reveal_label(), Message::Reveal(full.clone())),
        MenuItem::entry("Copy path", Message::Copy(path.to_owned())),
        MenuItem::entry("Copy full path", Message::Copy(full)),
    ]);
    items
}
