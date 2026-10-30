//! User-facing commands with their dialogs, and all context menus.

use gitcraft_core::model::{Commit, CommitDetail, LocalBranch, Remote};
use gitcraft_core::{ops, queries};
use iced::{Point, Task};

use crate::app::{App, Area, Message, RepoTab, ToastKind, View};
use crate::command::Command;
use crate::form::{Field, Form, Values};
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
    Squash {
        sha: String,
        fixup: bool,
    },
    SquashBase {
        sha: String,
        fixup: bool,
        base: Result<Option<String>, String>,
    },
    DropCommit(String),
    SavePatch(String),
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
    Stash(String),
    Remote(String),
    Repository,
    /// `area` for a file in Local Changes, `rev` for a file in a commit.
    File {
        area: Option<Area>,
        path: String,
        rev: Option<String>,
    },
    Tab(usize),
    BarFile,
    BarView,
    BarHelp,
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
                })
                .field(remote_field(&remotes, up_remote.as_deref()))
                .field(Field::text("branch", "Remote branch", &name))
                .field(Field::select(
                    "mode",
                    "Strategy",
                    &self.settings.pull_mode,
                    options(&[
                        ("merge", "Merge (create a merge commit if needed)"),
                        ("rebase", "Rebase (replay local commits on top)"),
                        ("ff-only", "Fast-forward only"),
                    ]),
                ))
                .field(Field::check("autostash", "Stash and reapply local changes", self.settings.pull_autostash));
                if let Some(branch) = branch {
                    form = form.message(&format!("Pull into {}", branch.name));
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
                .message(&format!("Push {}", branch.name))
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
                .field(Field::check("keepIndex", "Keep staged changes in the working tree", false));
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
                let message = tab.refs.stashes.iter().find(|stash| stash.name == name).map_or(name.clone(), |s| s.message.clone());
                self.ask(Form::confirm(
                    "Drop stash",
                    &format!("Delete \"{message}\"? This cannot be undone."),
                    "Drop",
                    true,
                    run(Command::StashDrop { name }),
                ))
            }
            Action::NewBranch { start, label } => {
                let based = label.or_else(|| start.as_deref().map(|sha| short_sha(sha).to_owned())).unwrap_or(head);
                let form = Form::new("New branch", "Create branch", move |values: &Values| {
                    let name = values.text("name").split_whitespace().collect::<Vec<_>>().join("-");
                    run(Command::CreateBranch { name, start: start.clone(), checkout: values.check("checkout") })
                })
                .message(&format!("Based on {based}"))
                .field(Field::text("name", "Branch name", "").required().hint("feature/my-change"))
                .field(Field::check("checkout", "Check out after creating", true));
                self.ask(form)
            }
            Action::CheckoutCommit(sha) => self.ask(Form::confirm(
                "Checkout commit",
                &format!(
                    "Check out {}? You will be in \"detached HEAD\" state; create a branch to keep new commits.",
                    short_sha(&sha)
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
                .message(&format!("Delete local branch {name}?"))
                .danger()
                .field(Field::check("force", "Force delete, even if it is not merged", false));
                if let Some(upstream) = upstream {
                    form = form.field(Field::check("remote", &format!("Also delete {upstream} on the remote"), false));
                }
                self.ask(form)
            }
            Action::DeleteRemoteBranch(name) => {
                let Some(branch) = tab.refs.remote.iter().find(|branch| branch.name == name).cloned() else {
                    return Task::none();
                };
                self.ask(Form::confirm(
                    "Delete remote branch",
                    &format!("Delete {} from {}?", branch.name, branch.remote),
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
                .message(&format!("Choose the upstream of {name}"))
                .field(Field::select("upstream", "Upstream", &upstream, choices));
                self.ask(form)
            }
            Action::Merge(reference) => {
                let target = reference.clone();
                let form = Form::new("Merge", "Merge", move |values: &Values| {
                    run(Command::Merge { reference: target.clone(), mode: values.text("mode") })
                })
                .message(&format!("Merge {reference} into {head}"))
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
                self.ask(form)
            }
            Action::Rebase { onto, label } => {
                let form = Form::new("Rebase", "Rebase", move |values: &Values| Message::RebaseChosen {
                    onto: onto.clone(),
                    autostash: values.check("autostash"),
                })
                .message(&format!("Rebase {head} onto {label}?"))
                .field(Field::check("autostash", "Stash and reapply local changes", self.settings.rebase_autostash));
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
                    &format!("Create a commit that reverts \"{}\"?", commit.subject),
                    "Revert",
                    false,
                    run(Command::Revert { sha, mainline: None }),
                ))
            }
            Action::Abort => {
                let kind = tab.status.as_ref().and_then(|s| s.operation.as_ref()).map_or("operation", |op| op.command());
                self.ask(Form::confirm(
                    &format!("Abort {kind}"),
                    &format!("Abort the {kind} and restore the previous state?"),
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
                .message(&format!("Tag {}", short_sha(&target)))
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
                let mut form = Form::new("Delete tag", "Delete", move |values: &Values| {
                    run(Command::DeleteTag {
                        name: tag.clone(),
                        remotes: if values.check("remote") { names.clone() } else { Vec::new() },
                    })
                })
                .message(&format!("Delete tag {name}?"))
                .danger();
                if has_remotes {
                    form = form.field(Field::check("remote", "Also delete from all remotes", false));
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
                    run(Command::EditRemote { name: name.clone(), new_name: values.text("newName"), url: values.text("url") })
                })
                .field(Field::text("newName", "Name", &remote.name).required())
                .field(Field::text("url", "URL", &remote.fetch_url).required());
                self.ask(form)
            }
            Action::RemoveRemote(name) => self.ask(Form::confirm(
                "Remove remote",
                &format!("Remove remote {name} and its remote-tracking branches?"),
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
                    &format!(
                        "Reset {head} to {} \"{subject}\" and discard ALL uncommitted changes? This cannot be undone.",
                        short_sha(&sha)
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
                .message(&format!("{}: commits after it are rewritten.", short_sha(&sha)))
                .field(Field::area("message", "Message", &message).required());
                self.ask(form)
            }
            Action::Squash { sha, fixup } => {
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
                        (sha, fixup, base)
                    },
                    |(sha, fixup, base)| act(Action::SquashBase { sha, fixup, base }),
                )
            }
            Action::SquashBase { sha, fixup, base } => {
                let base = match base {
                    Ok(base) => base,
                    Err(error) => return self.fail("Could not edit the commit", error),
                };
                let subject = self.commit_by_sha(&sha).map_or_else(String::new, |commit| commit.subject.clone());
                let verb = if fixup { "Fixup" } else { "Squash" };
                let how = if fixup { ", discarding its message" } else { ", combining both messages" };
                self.ask(Form::confirm(
                    &format!("{verb} into parent"),
                    &format!("{verb} \"{subject}\" into its parent commit{how}?"),
                    verb,
                    false,
                    run(Command::QuickRebase { sha, base, action: if fixup { "fixup" } else { "squash" }, message: None }),
                ))
            }
            Action::DropCommit(sha) => {
                let Some(commit) = self.commit_by_sha(&sha).cloned() else { return Task::none() };
                if commit.parents.len() > 1 {
                    return self.warn("Merge commits cannot be edited this way");
                }
                self.ask(Form::confirm(
                    "Drop commit",
                    &format!("Remove \"{}\" from the branch history?", commit.subject),
                    "Drop",
                    true,
                    run(Command::QuickRebase { base: commit.parents.first().cloned(), sha, action: "drop", message: None }),
                ))
            }
            Action::SavePatch(sha) => {
                let repo = tab.path.clone();
                let subject = self.commit_by_sha(&sha).map_or_else(String::new, |commit| commit.subject.clone());
                let name = logic::patch_file_name(&sha, &subject);
                Task::perform(
                    async move {
                        let patch = ops::format_patch(&repo, &sha).await.map_err(|error| error.to_string())?;
                        let Some(file) = rfd::AsyncFileDialog::new().set_file_name(name).save_file().await else {
                            return Ok(None);
                        };
                        std::fs::write(file.path(), patch).map_err(|error| error.to_string())?;
                        Ok(Some(file.path().to_string_lossy().into_owned()))
                    },
                    |result| act(Action::PatchSaved(result)),
                )
            }
            Action::PatchSaved(result) => {
                match result {
                    Ok(Some(path)) => self.toast(ToastKind::Success, format!("Saved {path}"), None),
                    Ok(None) => {}
                    Err(error) => self.toast(ToastKind::Error, "Could not create patch", Some(error)),
                }
                Task::none()
            }
            Action::ApplyPatch => Task::perform(
                async {
                    let file = rfd::AsyncFileDialog::new()
                        .add_filter("Patches", &["patch", "diff", "eml", "txt"])
                        .pick_file()
                        .await?;
                    let bytes = std::fs::read(file.path()).ok()?;
                    Some((file.file_name(), String::from_utf8_lossy(&bytes).into_owned()))
                },
                |picked| act(Action::PatchPicked(picked)),
            ),
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
                    list.push_str(&format!("\n… and {} more", files.len() - 25));
                }
                self.ask(Form::confirm(
                    &format!("Delete {} item(s)?", files.len()),
                    &list,
                    "Delete",
                    true,
                    run(Command::Clean { directories, ignored }),
                ))
            }
            Action::CheckoutFile { sha, path } => self.ask(Form::confirm(
                "Checkout file",
                &format!(
                    "Replace {path} in the working tree and index with its version from {}? Local changes to it are lost.",
                    short_sha(&sha)
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
                    &format!("Discard all changes to {what}? This cannot be undone."),
                    "Discard",
                    true,
                    run(Command::Discard { paths }),
                ))
            }
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
                            })
                            .map_err(|error| error.to_string())
                    },
                    move |result| Message::MenuResolved(request.clone(), at, result),
                )
            }
            MenuRequest::Stash(name) => {
                let name = name.clone();
                self.menu_at_cursor(vec![
                    MenuItem::entry(
                        "Apply",
                        run(Command::StashApply {
                            name: name.clone(),
                            pop: false,
                        }),
                    ),
                    MenuItem::entry(
                        "Pop (apply and drop)",
                        run(Command::StashApply {
                            name: name.clone(),
                            pop: true,
                        }),
                    ),
                    MenuItem::entry(
                        "New branch from stash…",
                        act(Action::NewBranch {
                            start: Some(format!("{name}^")),
                            label: Some(name.clone()),
                        }),
                    ),
                    MenuItem::danger("Drop…", act(Action::DropStash(name))),
                ]);
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
                        format!("Fetch {name}"),
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
                let items = file_menu_items(tab, *area, path, rev.as_deref());
                self.menu_at_cursor(items);
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
            MenuRequest::BarFile => {
                let recent: Vec<MenuItem> = self
                    .recent
                    .iter()
                    .take(15)
                    .map(|repo| {
                        MenuItem::entry(repo.name.clone(), Message::OpenPath(repo.path.clone()))
                    })
                    .collect();
                let mut items = vec![
                    MenuItem::entry("Open repository…", Message::OpenDialog),
                    MenuItem::entry("Clone repository…", act(Action::Clone)),
                    MenuItem::entry("New repository…", act(Action::Init)),
                ];
                if !recent.is_empty() {
                    items.push(MenuItem::sub("Recent", recent));
                }
                items.extend([
                    MenuItem::Separator,
                    MenuItem::entry("Settings…", Message::OpenSettings),
                    MenuItem::Separator,
                    MenuItem::entry("Quit", Message::Quit),
                ]);
                self.menu_at_cursor(items);
                Task::none()
            }
            MenuRequest::BarView => {
                let has_repo = self.current().is_some();
                self.menu_at_cursor(vec![
                    MenuItem::entry_if("All commits", has_repo, Message::SetView(View::History)),
                    MenuItem::entry_if("Local changes", has_repo, Message::SetView(View::Changes)),
                    MenuItem::entry_if("Refresh", has_repo, Message::Refresh),
                    MenuItem::Separator,
                    MenuItem::entry("Zoom in", Message::Zoom(1.1)),
                    MenuItem::entry("Zoom out", Message::Zoom(1.0 / 1.1)),
                    MenuItem::entry("Actual size", Message::Zoom(0.0)),
                ]);
                Task::none()
            }
            MenuRequest::BarHelp => {
                let folder = gitcraft_core::store::config_dir()
                    .to_string_lossy()
                    .into_owned();
                self.menu_at_cursor(vec![MenuItem::entry(
                    "Show data folder",
                    Message::OpenFile(folder),
                )]);
                Task::none()
            }
        }
    }

    pub fn show_commit_menu(&mut self, request: MenuRequest, commit: &Commit, at: Point) {
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
        RefTarget::Local(name) => format!("Branch {name}"),
        RefTarget::Remote(name) => name.clone(),
        RefTarget::Tag(name) => format!("Tag {name}"),
    }
}

fn local_ref_items(tab: &RepoTab, branch: &LocalBranch) -> Vec<MenuItem> {
    let current = tab.current_branch();
    let shown = current.unwrap_or("HEAD");
    let name = branch.name.clone();
    let other = !branch.current && current.is_some();
    let mut items = vec![
        MenuItem::entry_if(
            format!("Checkout {name}"),
            !branch.current,
            run(Command::Checkout {
                reference: name.clone(),
                kind: "local",
            }),
        ),
        MenuItem::entry_if(
            format!("Merge {name} into {shown}…"),
            other,
            act(Action::Merge(name.clone())),
        ),
        MenuItem::entry_if(
            format!("Rebase {shown} onto {name}…"),
            other,
            act(Action::Rebase {
                onto: name.clone(),
                label: name.clone(),
            }),
        ),
    ];
    if let Some(upstream) = &branch.upstream {
        items.push(MenuItem::entry_if(
            format!("Fast-forward to {upstream}"),
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
        MenuItem::entry("Copy branch name", Message::Copy(name)),
    ]);
    items
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
                format!("Checkout {} (track)", branch.branch),
                run(Command::Checkout {
                    reference: name.clone(),
                    kind: "remote",
                }),
            )];
            if let Some(local) = local {
                items.push(MenuItem::entry_if(
                    format!("Fast-forward {} to here", local.name),
                    local.behind > 0 && local.ahead == 0,
                    run(Command::FastForward {
                        branch: local.name.clone(),
                        target: Some(name.clone()),
                    }),
                ));
            }
            items.extend([
                MenuItem::entry_if(
                    format!("Merge {name} into {shown}…"),
                    current.is_some(),
                    act(Action::Merge(name.clone())),
                ),
                MenuItem::entry_if(
                    format!("Rebase {shown} onto {name}…"),
                    current.is_some(),
                    act(Action::Rebase {
                        onto: name.clone(),
                        label: name.clone(),
                    }),
                ),
                MenuItem::Separator,
                MenuItem::danger(
                    format!("Delete from {}…", branch.remote),
                    act(Action::DeleteRemoteBranch(name.clone())),
                ),
                MenuItem::entry("Copy branch name", Message::Copy(name.clone())),
            ]);
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
                    format!("Merge {name} into {shown}…"),
                    current.is_some(),
                    act(Action::Merge(name.clone())),
                ),
                MenuItem::entry_if(
                    "Push to remote…",
                    !tab.refs.remotes.is_empty(),
                    act(Action::PushTag(name.clone())),
                ),
                MenuItem::danger("Delete…", act(Action::DeleteTag(name.clone()))),
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
    items.push(MenuItem::Header(format!(
        "Commit {}",
        short_sha(&commit.sha)
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
            format!("Merge into {current}…"),
            !is_head,
            act(Action::Merge(sha.clone())),
        ),
        MenuItem::entry_if(
            format!("Rebase {current} onto here…"),
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
            format!("Cherry-pick into {current}"),
            !is_head,
            run(Command::CherryPick { sha: sha.clone() }),
        ),
        MenuItem::entry("Revert commit…", act(Action::Revert(sha.clone()))),
        MenuItem::sub(
            format!("Reset {current} to here"),
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
                act(Action::Squash {
                    sha: sha.clone(),
                    fixup: false,
                }),
            ),
            MenuItem::entry_if(
                "Fixup into parent (discard message)…",
                has_parent,
                act(Action::Squash {
                    sha: sha.clone(),
                    fixup: true,
                }),
            ),
            MenuItem::danger("Drop commit…", act(Action::DropCommit(sha.clone()))),
        ],
    ));
    items.push(MenuItem::Separator);
    if let Some(head) = head.filter(|_| !is_head) {
        items.push(MenuItem::entry(
            "Compare with HEAD",
            Message::CompareStart(sha.clone(), head.to_owned()),
        ));
    }
    if let Some(selected) = tab.selected.as_deref().filter(|selected| *selected != sha) {
        items.push(MenuItem::entry(
            format!("Compare with {}", short_sha(selected)),
            Message::CompareStart(sha.clone(), selected.to_owned()),
        ));
    }
    items.push(MenuItem::entry_if(
        "Save as patch…",
        commit.parents.len() <= 1,
        act(Action::SavePatch(sha.clone())),
    ));
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

fn repository_items(tab: &RepoTab) -> Vec<MenuItem> {
    let mut items = vec![
        MenuItem::entry("Apply patch…", act(Action::ApplyPatch)),
        MenuItem::danger("Clean untracked files…", act(Action::Clean)),
        MenuItem::entry("Reflog (recover lost commits)…", Message::OpenReflog),
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

fn file_menu_items(
    tab: &RepoTab,
    area: Option<Area>,
    path: &str,
    rev: Option<&str>,
) -> Vec<MenuItem> {
    let mut items = Vec::new();
    if let Some(area) = area {
        let paths = targets(tab, area, path);
        let suffix = if paths.len() > 1 {
            format!(" {} files", paths.len())
        } else {
            String::new()
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
                        format!("Stage{suffix}"),
                        run(Command::Stage {
                            paths: paths.clone(),
                        }),
                    ),
                    MenuItem::danger(
                        format!("Discard changes{suffix}…"),
                        act(Action::Discard(paths.clone())),
                    ),
                    MenuItem::entry(
                        format!("Stash{suffix}…"),
                        act(Action::StashFiles(paths.clone())),
                    ),
                ]);
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
                        format!("Ignore{suffix}"),
                        run(Command::Ignore { patterns }),
                    ));
                }
            }
            Area::Staged => items.extend([
                MenuItem::entry(
                    format!("Unstage{suffix}"),
                    run(Command::Unstage {
                        paths: paths.clone(),
                    }),
                ),
                MenuItem::entry(format!("Stash{suffix}…"), act(Action::StashFiles(paths))),
            ]),
            Area::Conflicts => {
                let resolve = |side: Option<&str>| {
                    act(Action::ConflictResolve {
                        path: path.to_owned(),
                        side: side.map(str::to_owned),
                    })
                };
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
            "Blame",
            Message::OpenBlame(path.to_owned(), rev.map(str::to_owned)),
        ),
    ]);
    if let Some(rev) = rev {
        items.push(MenuItem::entry(
            "Checkout this version of the file…",
            act(Action::CheckoutFile {
                sha: rev.to_owned(),
                path: path.to_owned(),
            }),
        ));
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
