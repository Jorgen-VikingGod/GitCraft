//! Drives the real UI against real repositories: messages go in, tasks run to the
//! end, and results are checked with git itself. No mocks.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command as Process;

use egui::{Event, Modifiers, PointerButton, Pos2, Rect, Vec2, vec2};
use futures::StreamExt;
use gitcraft_core::model::ToolTarget;
use gitcraft_core::runner::set_editor_exe;
use gitcraft_core::store::RepoStore;
use gitcraft_ui::App;
use gitcraft_ui::actions::{Action, MenuRequest, RefTarget, commit_menu_items};
use gitcraft_ui::app::{Area, CommitOption, Message, Overlay, ToastKind, View};
use gitcraft_ui::command::Command;
use gitcraft_ui::logic::Choice;
use gitcraft_ui::menu::MenuItem;
use gitcraft_ui::theme::Look;

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Process::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_EDITOR", "true")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    repo: String,
    path: PathBuf,
}

impl Fixture {
    fn write(&self, name: &str, content: &str) {
        std::fs::write(self.path.join(name), content).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path.join(name)).unwrap()
    }

    fn commit(&self, file: &str, content: &str, message: &str) {
        self.write(file, content);
        git(&self.path, &["add", "--", file]);
        git(&self.path, &["commit", "-q", "-m", message]);
    }

    fn git(&self, args: &[&str]) -> String {
        git(&self.path, args)
    }

    fn subjects(&self) -> Vec<String> {
        self.git(&["log", "--format=%s"])
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

fn empty_fixture() -> Fixture {
    set_editor_exe(env!("CARGO_BIN_EXE_gitcraft").into());
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let path = root.join("demo-repo");
    std::fs::create_dir(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    for (key, value) in [
        ("user.name", "Test User"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(&path, &["config", key, value]);
    }
    Fixture {
        repo: path.to_str().unwrap().to_owned(),
        root,
        path,
        _dir: dir,
    }
}

fn fixture() -> Fixture {
    let f = empty_fixture();
    f.commit("a.txt", "one\n", "first commit");
    git(&f.path, &["branch", "side"]);
    f.commit("b.txt", "two\n", "second commit\n\nWith a body line");
    git(&f.path, &["switch", "-q", "side"]);
    f.commit("c.txt", "three\n", "work on the side");
    git(&f.path, &["switch", "-q", "main"]);
    git(
        &f.path,
        &[
            "merge",
            "-q",
            "--no-ff",
            "--no-edit",
            "-m",
            "merge side",
            "side",
        ],
    );
    git(&f.path, &["tag", "v1.0"]);
    f
}

/// Sends a message and runs everything it sets off, like the window would.
/// Returns every message seen, including progress lines.
async fn drive(app: &mut App, message: Message) -> Vec<Message> {
    let mut queue = VecDeque::from([message]);
    let mut seen = Vec::new();
    while let Some(next) = queue.pop_front() {
        seen.push(next.clone());
        if let Some(mut stream) = app.update(next).into_stream() {
            while let Some(output) = stream.next().await {
                queue.push_back(output);
            }
        }
    }
    seen
}

/// The real UI, drawn without a window: what is on screen, and what a click would do.
struct Screen {
    ctx: egui::Context,
    size: Vec2,
}

/// One drawn frame.
struct Frame {
    texts: Vec<(String, Rect)>,
    messages: Vec<Message>,
}

impl Frame {
    fn has(&self, wanted: &str) -> bool {
        self.texts.iter().any(|(text, _)| text.contains(wanted))
    }

    fn find(&self, wanted: &str) -> Option<Rect> {
        self.texts
            .iter()
            .find(|(text, _)| text.contains(wanted))
            .map(|(_, rect)| *rect)
    }
}

fn collect(shape: &egui::Shape, into: &mut Vec<(String, Rect)>) {
    match shape {
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, into)),
        egui::Shape::Text(text) => into.push((
            text.galley.text().to_owned(),
            text.galley.rect.translate(text.pos.to_vec2()),
        )),
        _ => {}
    }
}

impl Screen {
    fn new() -> Self {
        Self::sized(vec2(1280.0, 800.0))
    }

    fn sized(size: Vec2) -> Self {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        Self { ctx, size }
    }

    fn frame(&self, app: &App, events: Vec<Event>) -> Frame {
        app.look.apply(&self.ctx);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            events,
            ..egui::RawInput::default()
        };
        let mut messages = Vec::new();
        let output = self
            .ctx
            .run_ui(input, |ui| messages = gitcraft_ui::views::root(ui, app));
        // Nothing uploads textures here; egui insists that someone has looked at them.
        let mut output = output;
        output.textures_delta.clear();
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut texts);
        }
        Frame { texts, messages }
    }

    /// Draws twice, as widgets only react to the pointer once they have been laid out.
    fn show(&self, app: &App) -> Frame {
        self.frame(app, Vec::new());
        self.frame(app, Vec::new())
    }

    /// Clicks the first text containing `wanted` and gives the messages that caused.
    fn click(&self, app: &App, wanted: &str, button: PointerButton) -> Vec<Message> {
        let at = self
            .show(app)
            .find(wanted)
            .unwrap_or_else(|| panic!("{wanted} is on screen"))
            .center();
        let press = |pressed| Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.frame(app, vec![Event::PointerMoved(at)]);
        let mut messages = self.frame(app, vec![press(true)]).messages;
        messages.extend(self.frame(app, vec![press(false)]).messages);
        messages
    }
}

async fn opened(f: &Fixture, dark: bool) -> App {
    let store = RepoStore::at(f.root.join("config/repos.json"));
    let mut app = App::with_look(Look::preset(dark), store);
    drive(&mut app, Message::OpenPath(f.repo.clone())).await;
    app
}

fn sha_of(app: &App, subject: &str) -> String {
    let tab = app.current().unwrap();
    tab.commits
        .iter()
        .find(|commit| commit.subject == subject)
        .unwrap()
        .sha
        .clone()
}

fn labels(items: &[MenuItem]) -> Vec<String> {
    let mut all = Vec::new();
    for item in items {
        all.extend(item.label().map(str::to_owned));
        if let MenuItem::Sub { children, .. } = item {
            all.extend(labels(children));
        }
    }
    all
}

fn find_entry(items: &[MenuItem], wanted: &str) -> Option<Option<Message>> {
    items.iter().find_map(|item| match item {
        MenuItem::Entry { label, message, .. } if label == wanted => Some(message.clone()),
        MenuItem::Sub { children, .. } => find_entry(children, wanted),
        _ => None,
    })
}

fn toast_kinds(app: &App) -> Vec<ToastKind> {
    app.toasts.iter().map(|toast| toast.kind).collect()
}

#[tokio::test]
async fn opening_loads_history_refs_and_selects_head() {
    let f = fixture();
    f.write("dirty.txt", "x\n");
    let app = opened(&f, false).await;
    let tab = app.current().unwrap();
    assert_eq!(tab.commits[0].subject, "merge side");
    assert_eq!(
        tab.selected.as_deref(),
        Some(tab.commits[0].sha.as_str()),
        "HEAD is selected"
    );
    assert_eq!(
        tab.detail.as_ref().unwrap().sha,
        tab.commits[0].sha,
        "and its details are loaded"
    );
    assert!(
        tab.commit_pane.diff.is_some(),
        "and the diff of its first file"
    );

    let screen = Screen::new().show(&app);
    for expected in [
        "merge side",
        "second commit",
        "work on the side",
        "first commit",
        "v1.0",
        "BRANCHES",
        "Uncommitted changes",
        "1 file",
    ] {
        assert!(screen.has(expected), "{expected} is on screen");
    }
}

#[tokio::test]
async fn clicking_a_commit_selects_it_and_shows_its_details() {
    let f = fixture();
    let mut app = opened(&f, false).await;
    let messages = Screen::new().click(&app, "second commit", PointerButton::Primary);
    let select = messages
        .into_iter()
        .find(|message| matches!(message, Message::Select(_)))
        .expect("the click selects");
    drive(&mut app, select).await;
    assert_eq!(
        app.current().unwrap().detail.as_ref().unwrap().subject,
        "second commit"
    );
    let screen = Screen::new().show(&app);
    assert!(screen.has("b.txt"), "the changed file is listed");
    assert!(
        screen.has("Test User") && screen.has("<t@example.com>"),
        "the author is shown"
    );
}

#[tokio::test]
async fn staging_single_lines_then_committing() {
    let f = empty_fixture();
    f.commit("a.txt", "one\ntwo\nthree\n", "first");
    f.write("a.txt", "ONE\ntwo\nthree\nfour\n");
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SetView(View::Changes)).await;
    drive(
        &mut app,
        Message::FilePicked(Area::Unstaged, "a.txt".into()),
    )
    .await;

    // Stage only the "four" line.
    let plus = {
        let pane = &app.current().unwrap().changes_pane;
        let hunk = &pane.diff.as_ref().expect("the diff loaded").hunks[0];
        hunk.lines
            .iter()
            .position(|line| line.text == "four")
            .unwrap()
    };
    drive(
        &mut app,
        Message::DiffLineClicked(gitcraft_ui::app::PaneKey::Changes, 0, plus),
    )
    .await;
    assert_eq!(app.current().unwrap().changes_pane.selected_in(0), [plus]);
    drive(&mut app, Message::HunkAction(0, "stage")).await;
    assert_eq!(f.git(&["show", ":a.txt"]), "one\ntwo\nthree\nfour\n");
    assert_eq!(
        std::fs::read_to_string(f.path.join("a.txt")).unwrap(),
        "ONE\ntwo\nthree\nfour\n"
    );

    let tab = app.current().unwrap();
    assert_eq!(
        tab.status.as_ref().unwrap().staged.len(),
        1,
        "the file shows as staged"
    );
    assert_eq!(
        tab.status.as_ref().unwrap().unstaged.len(),
        1,
        "and as still changed"
    );

    // Commit from the commit box.
    assert!(!tab.can_commit(), "a summary is required");
    drive(&mut app, Message::SummaryInput("add four".into())).await;
    drive(&mut app, Message::CommitNow).await;
    assert_eq!(f.subjects(), ["add four", "first"]);
    let tab = app.current().unwrap();
    assert!(
        tab.summary.is_empty() && !tab.committing,
        "the box is cleared"
    );
    assert_eq!(tab.commits[0].subject, "add four", "history reloaded");
    assert_eq!(toast_kinds(&app), [ToastKind::Success]);
}

#[tokio::test]
async fn discarding_a_hunk_asks_first() {
    let f = empty_fixture();
    f.commit("a.txt", "one\n", "first");
    f.write("a.txt", "changed\n");
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SetView(View::Changes)).await;
    drive(&mut app, Message::HunkAction(0, "discard")).await;
    assert_eq!(
        std::fs::read_to_string(f.path.join("a.txt")).unwrap(),
        "changed\n",
        "nothing happens before the answer"
    );
    assert!(app.form.is_some());
    drive(&mut app, Message::FormCancel).await;
    assert_eq!(
        std::fs::read_to_string(f.path.join("a.txt")).unwrap(),
        "changed\n"
    );
    drive(&mut app, Message::HunkAction(0, "discard")).await;
    drive(&mut app, Message::FormSubmit).await;
    assert_eq!(
        std::fs::read_to_string(f.path.join("a.txt")).unwrap(),
        "one\n"
    );
}

#[tokio::test]
async fn discarded_changes_can_be_recovered() {
    let f = empty_fixture();
    f.commit("a.txt", "one\n", "first");
    f.write("a.txt", "changed\n");
    f.write("new.txt", "brand new\n");
    let mut app = opened(&f, false).await;
    let paths = vec!["a.txt".to_owned(), "new.txt".to_owned()];
    drive(&mut app, Message::Action(Action::Discard(paths))).await;
    drive(&mut app, Message::FormSubmit).await;
    assert_eq!(f.read("a.txt"), "one\n");
    assert!(!f.path.join("new.txt").exists());
    assert_eq!(
        f.subjects(),
        ["first"],
        "the backup is not a commit of the branch"
    );

    drive(&mut app, Message::OpenDiscards).await;
    let Some(Overlay::Discards { entries, .. }) = &app.overlay else {
        panic!("open")
    };
    assert_eq!(entries.len(), 1);
    let clicked = Screen::new().click(&app, "Restore", PointerButton::Primary);
    assert!(!clicked.is_empty(), "the list shows a Restore button");
    for message in clicked {
        drive(&mut app, message).await;
    }
    assert_eq!(f.read("a.txt"), "changed\n");
    assert_eq!(f.read("new.txt"), "brand new\n");
    assert!(app.overlay.is_none(), "the list closes once restored");
    assert_eq!(app.current().unwrap().view, View::Changes);

    // Deleting asks first, then the list shows what is left.
    drive(
        &mut app,
        Message::Action(Action::Discard(vec!["a.txt".into()])),
    )
    .await;
    drive(&mut app, Message::FormSubmit).await;
    drive(&mut app, Message::OpenDiscards).await;
    for message in Screen::new().click(&app, "Delete…", PointerButton::Primary) {
        drive(&mut app, message).await;
    }
    assert!(app.form.is_some());
    drive(&mut app, Message::FormSubmit).await;
    let Some(Overlay::Discards {
        entries, loading, ..
    }) = &app.overlay
    else {
        panic!("still open")
    };
    assert!(!loading && entries.is_empty());
    assert_eq!(f.read("a.txt"), "one\n");
}

#[tokio::test]
async fn pushing_to_a_bare_remote_reports_progress() {
    let f = fixture();
    let remote = f.root.join("remote.git");
    git(
        &f.root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    let mut app = opened(&f, false).await;

    drive(&mut app, Message::Action(Action::Push(None))).await;
    assert_eq!(app.form.as_ref().unwrap().title, "Push");
    let seen = drive(&mut app, Message::FormSubmit).await;
    assert!(
        seen.iter()
            .any(|message| matches!(message, Message::Progress(..))),
        "progress lines arrive"
    );
    assert!(app.busy.is_none(), "busy clears at the end");
    assert_eq!(
        git(&remote, &["rev-parse", "main"]),
        f.git(&["rev-parse", "main"])
    );
    assert_eq!(
        f.git(&["rev-parse", "--abbrev-ref", "main@{upstream}"])
            .trim(),
        "origin/main",
        "tracking was set"
    );
    let tab = app.current().unwrap();
    assert_eq!(
        tab.refs
            .local
            .iter()
            .find(|b| b.name == "main")
            .unwrap()
            .upstream
            .as_deref(),
        Some("origin/main")
    );
}

#[tokio::test]
async fn a_failed_command_shows_an_error_and_keeps_working() {
    let f = fixture();
    let mut app = opened(&f, false).await;
    drive(
        &mut app,
        Message::Run(Command::Merge {
            reference: "does-not-exist".into(),
            mode: "default".into(),
        }),
    )
    .await;
    assert_eq!(toast_kinds(&app), [ToastKind::Error]);
    assert_eq!(app.toasts[0].message, "Merging failed");
    assert!(app.busy.is_none());
    let id = app.toasts[0].id;
    drive(&mut app, Message::DismissToast(id)).await;
    assert!(app.toasts.is_empty());
}

fn conflicting(f: &Fixture) {
    f.commit("c.txt", "base\n", "base");
    f.git(&["switch", "-q", "-c", "other"]);
    f.commit("c.txt", "theirs\n", "their change");
    f.git(&["switch", "-q", "main"]);
    f.commit("c.txt", "ours\n", "our change");
}

#[tokio::test]
async fn resolving_a_merge_conflict_in_the_ui() {
    let f = empty_fixture();
    conflicting(&f);
    let mut app = opened(&f, false).await;

    drive(
        &mut app,
        Message::Run(Command::Merge {
            reference: "other".into(),
            mode: "default".into(),
        }),
    )
    .await;
    let tab = app.current().unwrap();
    assert_eq!(
        tab.view,
        View::Changes,
        "a conflict switches to Local Changes"
    );
    assert_eq!(tab.focus, Some((Area::Conflicts, "c.txt".to_owned())));
    assert_eq!(toast_kinds(&app), [ToastKind::Warning]);
    {
        let screen = Screen::new().show(&app);
        assert!(screen.has("Merge in progress"), "the banner shows");
        assert!(screen.has("Open merge tool"));
    }

    drive(&mut app, Message::OpenConflict("c.txt".into())).await;
    let Some(Overlay::Conflict(state)) = &app.overlay else {
        panic!("the resolver opened")
    };
    assert_eq!(state.conflict_count(), 1);
    assert!(state.result.contains("<<<<<<<"));

    // Saving with markers left asks first.
    drive(&mut app, Message::ConflictSave).await;
    assert_eq!(app.form.as_ref().unwrap().title, "Conflict markers remain");
    drive(&mut app, Message::FormCancel).await;

    drive(&mut app, Message::ConflictChoose(0, Choice::TheirsOurs)).await;
    let Some(Overlay::Conflict(state)) = &app.overlay else {
        panic!("still open")
    };
    assert_eq!(state.result, "theirs\nours\n");
    drive(&mut app, Message::ConflictSave).await;
    assert!(app.overlay.is_none(), "saving closes the resolver");
    assert_eq!(f.git(&["show", ":c.txt"]), "theirs\nours\n");

    let tab = app.current().unwrap();
    assert!(tab.status.as_ref().unwrap().conflicts.is_empty());
    drive(&mut app, Message::Run(Command::Continue)).await;
    assert_eq!(
        f.git(&["log", "-1", "--format=%P"])
            .split_whitespace()
            .count(),
        2,
        "a merge commit was made"
    );
    assert!(
        app.current()
            .unwrap()
            .status
            .as_ref()
            .unwrap()
            .operation
            .is_none()
    );
}

#[tokio::test]
async fn resolving_a_conflict_in_a_crlf_checkout() {
    let f = empty_fixture();
    conflicting(&f);
    f.git(&["config", "core.autocrlf", "true"]);
    let mut app = opened(&f, false).await;
    drive(
        &mut app,
        Message::Run(Command::Merge {
            reference: "other".into(),
            mode: "default".into(),
        }),
    )
    .await;
    drive(&mut app, Message::OpenConflict("c.txt".into())).await;
    let Some(Overlay::Conflict(state)) = &app.overlay else {
        panic!("the resolver opened")
    };
    assert_eq!(
        state.conflict_count(),
        1,
        "markers are found despite the carriage returns"
    );
    drive(&mut app, Message::ConflictChoose(0, Choice::OursTheirs)).await;
    drive(&mut app, Message::ConflictSave).await;
    assert!(app.form.is_none(), "no markers left, so no warning");
    assert!(
        app.overlay.is_none(),
        "saved: {:?}",
        app.toasts.first().map(|t| &t.detail)
    );
    assert_eq!(
        f.git(&["show", ":c.txt"]),
        "ours\ntheirs\n",
        "the index is clean LF"
    );
}

#[tokio::test]
async fn choosing_a_side_asks_before_replacing_hand_edits() {
    let f = empty_fixture();
    conflicting(&f);
    let mut app = opened(&f, false).await;
    drive(
        &mut app,
        Message::Run(Command::Merge {
            reference: "other".into(),
            mode: "default".into(),
        }),
    )
    .await;
    drive(&mut app, Message::OpenConflict("c.txt".into())).await;
    drive(&mut app, Message::ConflictEdit("x".into())).await;
    drive(&mut app, Message::ConflictChoose(0, Choice::Ours)).await;
    assert_eq!(app.form.as_ref().unwrap().title, "Replace edits?");
    drive(&mut app, Message::FormSubmit).await;
    let Some(Overlay::Conflict(state)) = &app.overlay else {
        panic!("still open")
    };
    assert_eq!(state.result, "ours\n");
    assert!(!state.edited);
}

#[tokio::test]
async fn interactive_rebase_with_reword_squash_and_reorder() {
    let f = empty_fixture();
    f.commit("base.txt", "base\n", "base");
    for name in ["one", "two", "three", "four"] {
        f.commit(&format!("{name}.txt"), "x\n", name);
    }
    let mut app = opened(&f, false).await;
    let base = sha_of(&app, "base");
    drive(&mut app, Message::OpenRebase(Some(base))).await;
    let order = |app: &App| {
        let Some(Overlay::Rebase(state)) = &app.overlay else {
            panic!("the editor is open")
        };
        state
            .steps
            .iter()
            .map(|step| step.commit.subject.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&app), ["one", "two", "three", "four"]);

    drive(&mut app, Message::RebaseMove(3, -3)).await; // four, one, two, three
    assert_eq!(order(&app), ["four", "one", "two", "three"]);
    drive(&mut app, Message::RebaseSetAction(0, "reword")).await;
    drive(&mut app, Message::RebaseMessage(0, "four, reworded".into())).await;
    drive(&mut app, Message::RebaseSetAction(2, "squash")).await; // two into one
    drive(&mut app, Message::RebaseSetAction(3, "drop")).await; // three
    drive(&mut app, Message::RebaseAutostash(false)).await;
    drive(&mut app, Message::RebaseStart).await;

    assert!(
        app.overlay.is_none(),
        "a finished rebase closes the editor: {:?}",
        app.toasts.first().map(|t| &t.detail)
    );
    let subjects = f.subjects();
    assert_eq!(
        subjects.first().map(String::as_str),
        Some("one"),
        "{subjects:?}"
    );
    assert_eq!(
        subjects.iter().filter(|s| *s == "four, reworded").count(),
        1,
        "{subjects:?}"
    );
    assert!(
        !subjects.iter().any(|s| s == "two" || s == "three"),
        "{subjects:?}"
    );
    assert!(
        f.path.join("two.txt").exists(),
        "the squashed change stayed"
    );
    assert!(!f.path.join("three.txt").exists(), "the dropped one went");
}

#[tokio::test]
async fn rebase_plans_with_problems_cant_start() {
    let f = fixture();
    let mut app = opened(&f, false).await;
    let base = sha_of(&app, "first commit");
    let before = f.git(&["rev-parse", "HEAD"]);
    drive(&mut app, Message::OpenRebase(Some(base))).await;
    drive(&mut app, Message::RebaseSetAction(0, "squash")).await;
    drive(&mut app, Message::RebaseStart).await;
    assert!(app.overlay.is_some());
    assert_eq!(f.git(&["rev-parse", "HEAD"]), before);
}

#[tokio::test]
async fn forms_collect_values_and_run_their_command() {
    let f = fixture();
    let mut app = opened(&f, false).await;
    drive(
        &mut app,
        Message::Action(Action::NewBranch {
            start: None,
            label: None,
        }),
    )
    .await;
    assert!(
        !app.form.as_ref().unwrap().can_submit(),
        "the name is required"
    );
    drive(&mut app, Message::FormSubmit).await;
    assert!(app.form.is_some(), "so submitting does nothing yet");
    drive(&mut app, Message::FormText(0, "my new  branch".into())).await;
    drive(&mut app, Message::FormSubmit).await;
    assert!(app.form.is_none());
    assert_eq!(
        f.git(&["branch", "--show-current"]).trim(),
        "my-new-branch",
        "spaces become dashes"
    );
    assert_eq!(
        app.current().unwrap().current_branch(),
        Some("my-new-branch")
    );
}

#[tokio::test]
async fn commit_menu_offers_ref_and_commit_actions_and_they_run() {
    let f = fixture();
    let mut app = opened(&f, false).await;
    let head = sha_of(&app, "merge side");
    let tab = app.current().unwrap();
    let commit = tab
        .commits
        .iter()
        .find(|commit| commit.sha == head)
        .unwrap();

    let plain = commit_menu_items(tab, commit, None);
    let names = labels(&plain);
    for expected in [
        "Commit",
        "New branch here…",
        "Interactive rebase from here…",
        "Reset main to here",
        "Hard: discard all changes",
        "Edit message…",
        "SHA",
    ] {
        assert!(
            names.iter().any(|name| name.starts_with(expected)),
            "{expected} in {names:?}"
        );
    }
    assert!(
        names.iter().any(|name| name == "Branch main"),
        "refs on the commit get submenus: {names:?}"
    );
    assert!(names.iter().any(|name| name == "Tag v1.0"), "{names:?}");

    let focused = commit_menu_items(tab, commit, Some(&RefTarget::Local("main".into())));
    assert_eq!(
        focused[0].label(),
        Some("Branch main"),
        "the clicked ref comes first"
    );
    assert!(
        find_entry(&focused, "Checkout main")
            .expect("listed")
            .is_none(),
        "the current branch can't be checked out"
    );

    // The menu itself draws, and a chosen entry does its work.
    drive(
        &mut app,
        Message::OpenMenu(MenuRequest::Commit {
            sha: head.clone(),
            focus: None,
        }),
    )
    .await;
    let entry = find_entry(&app.menu.as_ref().unwrap().items, "New tag here…")
        .unwrap()
        .unwrap();
    {
        assert!(Screen::new().show(&app).has("New tag here…"));
    }
    drive(&mut app, Message::FromMenu(Box::new(entry))).await;
    assert!(app.menu.is_none(), "choosing closes the menu");
    assert_eq!(app.form.as_ref().unwrap().title, "New tag");
    drive(&mut app, Message::FormText(0, "v2.0".into())).await;
    drive(&mut app, Message::FormSubmit).await;
    assert_eq!(f.git(&["rev-parse", "v2.0"]).trim(), head);
}

#[tokio::test]
async fn squash_and_message_editing_from_the_commit_menu() {
    let f = empty_fixture();
    f.commit("a.txt", "1\n", "first");
    f.commit("b.txt", "2\n", "second");
    f.commit("c.txt", "3\n", "third");
    let mut app = opened(&f, false).await;

    let sha = sha_of(&app, "third");
    drive(&mut app, Message::Action(Action::Squash(sha))).await;
    assert_eq!(app.form.as_ref().unwrap().title, "Squash into parent");
    let form = app.form.as_mut().unwrap();
    form.fields[0].kind = gitcraft_ui::form::FieldKind::Select {
        options: Vec::new(),
        value: "fixup".into(),
    };
    drive(&mut app, Message::FormSubmit).await;
    assert_eq!(f.subjects(), ["second", "first"]);

    let sha = sha_of(&app, "second");
    drive(&mut app, Message::Action(Action::EditMessage(sha))).await;
    assert_eq!(app.form.as_ref().unwrap().title, "Edit commit message");
    let form = app.form.as_mut().unwrap();
    form.fields[0].kind = gitcraft_ui::form::FieldKind::Area("second, better".into());
    drive(&mut app, Message::FormSubmit).await;
    assert_eq!(f.subjects(), ["second, better", "first"]);
}

#[tokio::test]
async fn search_and_compare_change_what_the_list_shows() {
    let f = fixture();
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SearchInput("SECOND".into())).await;
    drive(&mut app, Message::SearchSubmit).await;
    let tab = app.current().unwrap();
    assert_eq!(tab.visible().len(), 1);
    assert_eq!(
        tab.detail.as_ref().unwrap().subject,
        "second commit",
        "the first match is selected"
    );
    {
        assert!(
            Screen::new()
                .show(&app)
                .has("1 commit matching “SECOND” (message)")
        );
    }
    drive(&mut app, Message::SearchClear).await;
    assert_eq!(app.current().unwrap().visible().len(), 4);

    let (base, target) = (sha_of(&app, "first commit"), sha_of(&app, "merge side"));
    drive(
        &mut app,
        Message::CompareStart(base.clone(), target.clone()),
    )
    .await;
    let compare = app.current().unwrap().compare.as_ref().unwrap();
    let result = compare.result.as_ref().unwrap();
    assert_eq!((result.base_only, result.target_only), (0, 3));
    assert_eq!(result.files.len(), 2);
    drive(&mut app, Message::CompareSwap).await;
    assert_eq!(
        app.current().unwrap().compare.as_ref().unwrap().base,
        target
    );
    drive(&mut app, Message::CompareClose).await;
    assert!(app.current().unwrap().compare.is_none());
}

#[tokio::test]
async fn branch_filter_narrows_history() {
    let f = fixture();
    f.git(&["switch", "-q", "--orphan", "island"]);
    f.commit("i.txt", "i\n", "elsewhere");
    f.git(&["switch", "-q", "main"]);
    let mut app = opened(&f, false).await;
    assert!(
        app.current()
            .unwrap()
            .commits
            .iter()
            .any(|commit| commit.subject == "elsewhere")
    );
    drive(&mut app, Message::SetBranchFilter(false)).await;
    assert!(
        !app.current()
            .unwrap()
            .commits
            .iter()
            .any(|commit| commit.subject == "elsewhere")
    );
}

#[tokio::test]
async fn worktrees_show_in_the_sidebar_data_and_the_tab_title() {
    let f = fixture();
    let mut app = opened(&f, false).await;
    assert!(app.current().unwrap().worktree_note().is_none());
    assert!(!app.title().contains('['));
    let wt = f.root.join("wt-feature");
    drive(
        &mut app,
        Message::Run(Command::WorktreeAdd {
            path: wt.to_str().unwrap().to_owned(),
            start: None,
            new_branch: Some("feature".into()),
        }),
    )
    .await;
    let tab = app.current().unwrap();
    assert_eq!(tab.refs.worktrees.len(), 2);
    assert_eq!(tab.worktree_note().as_deref(), Some("2 worktrees"));
    assert!(app.title().contains("2 worktrees"));

    drive(&mut app, Message::OpenPath(wt.to_str().unwrap().to_owned())).await;
    let tab = app.current().unwrap();
    assert_eq!(tab.worktree_note().as_deref(), Some("worktree"));
    assert!(app.title().contains("feature, worktree"));
}

#[tokio::test]
async fn stash_view_reflog_blame_and_file_history() {
    let f = fixture();
    f.write("a.txt", "stashed\n");
    f.git(&["stash", "push", "-q", "-m", "saved work"]);
    let mut app = opened(&f, false).await;

    drive(&mut app, Message::OpenStash("stash@{0}".into())).await;
    let tab = app.current().unwrap();
    assert_eq!(tab.view, View::Stash);
    assert_eq!(tab.stash.as_ref().unwrap().files[0].path, "a.txt");
    assert!(tab.commit_pane.diff.is_some());
    drive(
        &mut app,
        Message::Run(Command::StashApply {
            name: "stash@{0}".into(),
            pop: true,
        }),
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(f.path.join("a.txt")).unwrap(),
        "stashed\n"
    );
    assert_eq!(
        app.current().unwrap().view,
        View::History,
        "the stash is gone, so its view closes"
    );

    drive(&mut app, Message::OpenReflog).await;
    let Some(Overlay::Reflog { entries, .. }) = &app.overlay else {
        panic!("open")
    };
    assert!(!entries.is_empty());
    drive(&mut app, Message::ReflogAll(true)).await;
    let Some(Overlay::Reflog { all, entries, .. }) = &app.overlay else {
        panic!("still open")
    };
    assert!(*all);
    assert!(
        entries
            .iter()
            .any(|entry| entry.selector.starts_with("side@")),
        "branches have their own reflogs"
    );
    drive(&mut app, Message::OverlayClose).await;

    drive(&mut app, Message::OpenBlame("b.txt".into(), None)).await;
    let Some(Overlay::Blame { lines, .. }) = &app.overlay else {
        panic!("open")
    };
    assert_eq!(lines[0].summary, "second commit");
    {
        assert!(Screen::new().show(&app).has("two"), "blame shows the file");
    }
    drive(&mut app, Message::OverlayClose).await;

    drive(&mut app, Message::OpenFileHistory("b.txt".into())).await;
    let Some(Overlay::FileHistory { commits, pane, .. }) = &app.overlay else {
        panic!("open")
    };
    assert_eq!(commits[0].subject, "second commit");
    assert!(pane.diff.is_some(), "the newest change is shown");
}

#[tokio::test]
async fn settings_persist_and_change_the_diff() {
    let f = empty_fixture();
    f.commit("a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n", "first");
    f.write("a.txt", "1\n2\n3\n4\nfive\n6\n7\n8\n9\n");
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SetView(View::Changes)).await;
    let lines = |app: &App| {
        app.current()
            .unwrap()
            .changes_pane
            .diff
            .as_ref()
            .unwrap()
            .hunks[0]
            .lines
            .len()
    };
    assert_eq!(lines(&app), 8, "3 lines of context by default");
    drive(&mut app, Message::SetContext("1".into())).await;
    assert_eq!(lines(&app), 4);
    drive(&mut app, Message::SetContext("nonsense".into())).await;
    assert_eq!(app.settings.diff.context, 1, "bad input is ignored");

    drive(&mut app, Message::SetSplit(true)).await;
    assert!(
        app.current()
            .unwrap()
            .changes_pane
            .rows
            .iter()
            .any(|row| matches!(row, gitcraft_ui::app::DiffRow::Split(..)))
    );

    let saved = gitcraft_ui::settings::Settings::load(&f.root.join("config"));
    assert_eq!(saved.diff.context, 1);
    assert!(saved.diff.split);
    assert_eq!(
        saved.session.tabs,
        std::slice::from_ref(&f.repo),
        "open tabs are remembered"
    );
}

#[tokio::test]
async fn pull_and_rebase_remember_the_last_choice() {
    let f = fixture();
    let remote = f.root.join("remote.git");
    git(
        &f.root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    f.git(&["push", "-q", "-u", "origin", "main"]);
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::Action(Action::Pull)).await;
    let form = app.form.as_ref().unwrap();
    assert_eq!(
        form.values().text("branch"),
        "main",
        "the upstream branch is suggested"
    );
    drive(&mut app, Message::FormSelect(2, "rebase".into())).await;
    drive(&mut app, Message::FormSubmit).await;
    assert_eq!(app.settings.pull.mode, "rebase");
    drive(&mut app, Message::Action(Action::Pull)).await;
    assert_eq!(app.form.as_ref().unwrap().values().text("mode"), "rebase");
}

#[tokio::test]
async fn closing_tabs_keeps_the_right_one_active() {
    let first = fixture();
    let second = fixture();
    let mut app = opened(&first, false).await;
    drive(&mut app, Message::OpenPath(second.repo.clone())).await;
    assert_eq!(app.active, 1);
    drive(&mut app, Message::OpenPath(first.repo.clone())).await;
    assert_eq!(app.active, 0, "opening again activates the tab");
    assert_eq!(app.tabs.len(), 2);
    drive(&mut app, Message::Activate(1)).await;
    drive(&mut app, Message::Close(0)).await;
    assert_eq!(app.current().unwrap().path, second.repo);
}

#[tokio::test]
async fn long_lists_draw_every_visible_row_before_any_scrolling() {
    let f = empty_fixture();
    for i in 0..60 {
        f.git(&[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            &format!("commit number {i}"),
        ]);
    }
    let mut app = opened(&f, false).await;
    let tall = vec2(1280.0, 1600.0);
    drive(&mut app, Message::WindowResized(tall)).await;
    app.settings.layout.history_height = 1200.0;
    let screen = Screen::sized(tall);
    assert!(
        screen.show(&app).has("commit number 30"),
        "rows below the first screenful are there"
    );
    // A scroll request far past the end must not leave the list blank.
    app.current().unwrap().scroll_to.set(Some(100_000.0));
    screen.frame(&app, Vec::new());
    assert_eq!(app.current().unwrap().commits.len(), 60);
    let dbg = screen.show(&app);
    assert!(
        dbg.has("commit number 0"),
        "the clamp draws real rows, not empty space"
    );
}

#[tokio::test]
async fn a_stale_scroll_position_cant_blank_a_shorter_diff() {
    let f = empty_fixture();
    f.commit("a.txt", "one\n", "first");
    f.write("a.txt", "changed\n");
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SetView(View::Changes)).await;
    drive(
        &mut app,
        Message::DiffScrolled(
            gitcraft_ui::app::PaneKey::Changes,
            50_000.0,
            vec2(600.0, 300.0),
        ),
    )
    .await;
    app.current()
        .unwrap()
        .changes_pane
        .scroll_to
        .set(Some(50_000.0));
    let screen = Screen::new();
    screen.frame(&app, Vec::new());
    let shown = screen.show(&app);
    assert!(shown.has("changed"), "the diff still shows");
    assert!(shown.has("Stage hunk"), "and so do its buttons");
}

#[tokio::test]
async fn a_tab_in_the_background_is_coloured_again_for_a_new_theme() {
    let f = empty_fixture();
    f.commit("a.rs", "fn main() {}\n", "first");
    f.write("a.rs", "fn main() { let x = 1; }\n");
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SetSyntax(true)).await;
    drive(&mut app, Message::SetTheme("light".into())).await;
    drive(&mut app, Message::SetView(View::Changes)).await;
    let other = f.root.join("other");
    std::fs::create_dir(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    drive(
        &mut app,
        Message::OpenPath(other.to_str().unwrap().to_owned()),
    )
    .await;
    assert_eq!(app.current().unwrap().name, "other");
    drive(&mut app, Message::SetTheme("dark".into())).await;
    let first = &app.tabs[0];
    assert_eq!(first.changes_pane.highlighted_dark, Some(true));
    assert!(first.changes_pane.highlights.is_some());
}

#[tokio::test]
async fn a_diff_is_coloured_again_when_the_theme_turns_dark() {
    let f = empty_fixture();
    f.commit("a.rs", "fn main() {}\n", "first");
    f.write("a.rs", "fn main() { let x = 1; }\n");
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SetSyntax(true)).await;
    drive(&mut app, Message::SetTheme("light".into())).await;
    drive(&mut app, Message::SetView(View::Changes)).await;
    let pane = |app: &App| {
        let pane = &app.current().unwrap().changes_pane;
        (pane.highlighted_dark, pane.highlights.is_some())
    };
    assert_eq!(
        pane(&app),
        (Some(false), true),
        "coloured for the light theme"
    );
    drive(&mut app, Message::SetTheme("dark".into())).await;
    assert_eq!(pane(&app), (Some(true), true), "and again for the dark one");
    drive(&mut app, Message::SetSyntax(false)).await;
    assert_eq!(
        pane(&app),
        (None, false),
        "no colours once highlighting is off"
    );
}

#[tokio::test]
async fn cancelling_stops_network_operations_only() {
    let fetch = Command::Fetch {
        remote: None,
        tags: false,
    };
    assert!(fetch.cancellable());
    assert!(!Command::Abort.cancellable());
    assert!(
        !Command::Rebase {
            onto: "x".into(),
            autostash: false
        }
        .cancellable()
    );
    assert!(
        !Command::Pull {
            remote: None,
            branch: None,
            mode: "rebase".into(),
            autostash: true,
            backup: false
        }
        .cancellable(),
        "a pull can stop half-way through its merge"
    );
}

#[tokio::test]
async fn opening_writes_a_commit_graph_when_enabled() {
    let f = fixture();
    let app = opened(&f, false).await;
    assert!(app.settings.history.commit_graph);
    assert!(f.path.join(".git/objects/info/commit-graph").exists());
}

#[tokio::test]
async fn follow_system_turns_dark_after_starting_in_light_on_a_dark_desktop() {
    let f = fixture();
    let dir = f.root.join("config");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), "[appearance]\ntheme = \"light\"\n").unwrap();
    let (mut app, _) = App::boot_with(None, true, RepoStore::at(dir.join("repos.json")));
    assert!(!app.look.dark, "starts in the chosen light theme");
    drive(&mut app, Message::SetTheme("system".into())).await;
    assert!(app.look.dark, "and follows the dark desktop once asked to");
}

/// Starts the app as `main` does, and runs what that asks for to the end.
async fn booted(repo: Option<String>, store: RepoStore) -> App {
    let (mut app, task) = App::boot_with(repo, true, store);
    let mut first = Vec::new();
    if let Some(mut stream) = task.into_stream() {
        while let Some(message) = stream.next().await {
            first.push(message);
        }
    }
    for message in first {
        drive(&mut app, message).await;
    }
    app
}

#[tokio::test]
async fn saved_tabs_open_in_their_order_with_the_saved_one_in_front() {
    let f = fixture();
    let mut paths = Vec::new();
    for name in ["one", "two", "three", "four"] {
        let repo = f.root.join(name);
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        paths.push(repo.canonicalize().unwrap().to_str().unwrap().to_owned());
    }
    let dir = f.root.join("config");
    std::fs::create_dir_all(&dir).unwrap();
    let tabs = paths
        .iter()
        .map(|path| format!("{path:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(
        dir.join("config.toml"),
        format!("[session]\ntabs = [{tabs}]\nactive_tab = 2\n"),
    )
    .unwrap();
    let store = || RepoStore::at(dir.join("repos.json"));
    let names = |app: &App| {
        app.tabs
            .iter()
            .map(|tab| tab.name.clone())
            .collect::<Vec<_>>()
    };

    // Closing before the saved tabs have all opened keeps the session as it was.
    let (mut early, _) = App::boot_with(None, true, store());
    drive(&mut early, Message::CloseRequested).await;
    let kept = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    assert!(
        kept.contains("four") && kept.contains("active_tab = 2"),
        "{kept}"
    );

    let app = booted(None, store()).await;
    assert_eq!(
        names(&app),
        ["one", "two", "three", "four"],
        "in the saved order"
    );
    assert_eq!(
        app.current().unwrap().name,
        "three",
        "with the saved tab in front"
    );

    // A repository named on the command line comes last and stays in front.
    let extra = f.root.join("extra");
    std::fs::create_dir(&extra).unwrap();
    git(&extra, &["init", "-q", "-b", "main"]);
    let app = booted(Some(extra.to_str().unwrap().to_owned()), store()).await;
    assert_eq!(names(&app), ["one", "two", "three", "four", "extra"]);
    assert_eq!(app.current().unwrap().name, "extra");
}

#[tokio::test]
async fn f11_and_the_view_menu_toggle_full_screen_without_a_title_bar() {
    use gitcraft_ui::app::{Effect, WindowAction};
    let f = fixture();
    let mut app = opened(&f, true).await;
    app.effects.clear();
    drive(&mut app, Message::Key(egui::Key::F11, Modifiers::NONE)).await;
    assert!(
        matches!(
            app.effects.as_slice(),
            [Effect::Window(WindowAction::ToggleFullscreen)]
        ),
        "F11 asks the window to go full screen"
    );

    let menus = app.bar_menus();
    let (_, view) = menus.iter().find(|(title, _)| *title == "View").unwrap();
    let entry = view
        .iter()
        .find(|item| item.label() == Some("Full screen"))
        .expect("the View menu offers full screen");
    let MenuItem::Entry { message, mark, .. } = entry else {
        panic!("an entry");
    };
    assert!(matches!(
        message,
        Some(Message::Window(WindowAction::ToggleFullscreen))
    ));
    assert_eq!(*mark, gitcraft_ui::menu::Mark::Check(false));

    // Once the window is full screen (the shell reads that from it), the app draws no title bar.
    app.frame = gitcraft_ui::window::decide(false);
    let title = "demo-repo - GitCraft";
    let shown = Screen::new().show(&app);
    let with_bar = shown.texts.iter().any(|(text, _)| text == title);
    app.fullscreen = true;
    let shown = Screen::new().show(&app);
    assert!(
        !shown.texts.iter().any(|(text, _)| text == title),
        "no title bar"
    );
    // Where this machine chose the system's title bar there was none to begin with.
    if !matches!(app.frame, craft_window::policy::Frame::Native) {
        assert!(with_bar, "the title bar shows outside full screen");
    }
}

/// A repository on main with a second branch, and a change to a tracked file.
async fn changed_with_a_branch() -> (Fixture, App) {
    let f = empty_fixture();
    f.commit("a.txt", "one\n", "first");
    f.git(&["branch", "side"]);
    f.write("a.txt", "mine\n");
    let app = opened(&f, false).await;
    (f, app)
}

fn checkout_side() -> Message {
    Message::Run(Command::Checkout {
        reference: "side".into(),
        kind: "local",
    })
}

#[tokio::test]
async fn checking_out_with_changes_asks_and_can_bring_them_along() {
    let (f, mut app) = changed_with_a_branch().await;
    drive(&mut app, checkout_side()).await;
    let form = app
        .form
        .as_ref()
        .expect("a checkout with changes asks first");
    assert_eq!(form.title, "Uncommitted changes");
    let answers: Vec<&str> = form
        .choices
        .iter()
        .map(|choice| choice.label.as_str())
        .collect();
    assert_eq!(
        (form.ok.as_str(), answers),
        ("Stash and reapply", vec!["Commit first", "Discard changes"])
    );
    assert_eq!(
        f.git(&["branch", "--show-current"]).trim(),
        "main",
        "nothing ran yet"
    );

    drive(&mut app, Message::FormSubmit).await;
    assert_eq!(f.git(&["branch", "--show-current"]).trim(), "side");
    assert_eq!(f.read("a.txt"), "mine\n", "the change came along");
    assert_eq!(f.git(&["stash", "list"]), "", "and no stash is left over");
}

#[tokio::test]
async fn discarding_before_a_checkout_keeps_the_changes_in_a_stash() {
    let (f, mut app) = changed_with_a_branch().await;
    drive(&mut app, checkout_side()).await;
    drive(&mut app, Message::FormChoice(1)).await;
    assert_eq!(f.git(&["branch", "--show-current"]).trim(), "side");
    assert_eq!(f.read("a.txt"), "one\n");
    assert!(
        f.git(&["stash", "list"])
            .contains("before checking out side")
    );
}

#[tokio::test]
async fn commit_first_checks_out_once_the_commit_is_made() {
    let (f, mut app) = changed_with_a_branch().await;
    drive(&mut app, checkout_side()).await;
    drive(&mut app, Message::FormChoice(0)).await;
    assert!(app.form.is_none());
    assert_eq!(
        app.current().unwrap().view,
        View::Changes,
        "the commit box is shown"
    );
    assert_eq!(f.git(&["branch", "--show-current"]).trim(), "main");

    drive(&mut app, Message::Run(Command::StageAll)).await;
    drive(&mut app, Message::SummaryInput("keep mine".into())).await;
    drive(&mut app, Message::CommitNow).await;
    assert_eq!(
        f.git(&["log", "-1", "--format=%s", "main"]).trim(),
        "keep mine"
    );
    assert_eq!(
        f.git(&["branch", "--show-current"]).trim(),
        "side",
        "then checked out"
    );
    assert!(app.current().unwrap().after_commit.is_none());
}

#[tokio::test]
async fn the_checkout_question_answers_for_its_own_repository() {
    let (f, mut app) = changed_with_a_branch().await;
    let other = f.root.join("other");
    std::fs::create_dir(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    drive(&mut app, checkout_side()).await;
    // Another repository comes to the front before the question is answered.
    drive(
        &mut app,
        Message::OpenPath(other.to_str().unwrap().to_owned()),
    )
    .await;
    assert_eq!(app.current().unwrap().name, "other");
    drive(&mut app, Message::FormChoice(1)).await;
    assert_eq!(
        f.git(&["branch", "--show-current"]).trim(),
        "side",
        "the asking one switched"
    );
    assert_eq!(git(&other, &["branch", "--show-current"]).trim(), "main");
}

#[tokio::test]
async fn commit_first_clears_amend_and_shows_the_asking_repository() {
    let (f, mut app) = changed_with_a_branch().await;
    drive(&mut app, Message::AmendToggled(true)).await;
    drive(&mut app, checkout_side()).await;
    let other = f.root.join("other");
    std::fs::create_dir(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    drive(
        &mut app,
        Message::OpenPath(other.to_str().unwrap().to_owned()),
    )
    .await;
    drive(&mut app, Message::FormChoice(0)).await;
    let tab = app.current().unwrap();
    assert_ne!(tab.name, "other", "back on the repository that asked");
    assert!(!tab.amend, "a new commit, as the question said");
    assert!(tab.after_commit.is_some());
}

#[tokio::test]
async fn a_checkout_with_only_new_files_doesnt_ask() {
    let f = empty_fixture();
    f.commit("a.txt", "one\n", "first");
    f.git(&["branch", "side"]);
    f.write("new.txt", "untracked\n");
    let mut app = opened(&f, false).await;
    drive(&mut app, checkout_side()).await;
    assert!(app.form.is_none());
    assert_eq!(f.git(&["branch", "--show-current"]).trim(), "side");
}

/// A "diff tool" and a "merge tool" set up in the repository's git config: the diff tool
/// copies the two versions it is shown beside the repository, the merge tool takes theirs.
fn fake_tools(f: &Fixture) {
    let root = f.root.to_str().unwrap().replace('\\', "/");
    let diff = format!("cp \"$LOCAL\" \"{root}/left\" && cp \"$REMOTE\" \"{root}/right\"");
    f.git(&["config", "difftool.fake.cmd", &diff]);
    f.git(&["config", "mergetool.fake.cmd", "cp \"$REMOTE\" \"$MERGED\""]);
    f.git(&["config", "mergetool.fake.trustExitCode", "true"]);
}

#[tokio::test]
async fn an_external_diff_tool_is_found_chosen_and_opened() {
    let f = empty_fixture();
    f.commit("a.txt", "one\n", "first");
    f.write("a.txt", "changed\n");
    fake_tools(&f);
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SetView(View::Changes)).await;
    let menu = |app: &mut App| {
        app.update(Message::OpenMenu(MenuRequest::File {
            area: Some(Area::Unstaged),
            path: "a.txt".into(),
            rev: None,
        }));
        labels(&app.menu.take().unwrap().items)
    };
    assert!(
        menu(&mut app).contains(&"Choose a diff tool…".to_owned()),
        "none chosen yet"
    );

    drive(&mut app, Message::OpenSettings).await;
    assert!(app.found_tools.diff.iter().any(|tool| tool.name == "fake"));
    drive(&mut app, Message::SetDiffTool("fake".into())).await;
    let saved = std::fs::read_to_string(f.root.join("config/config.toml")).unwrap_or_default();
    assert!(
        saved.contains("[tools]") && saved.contains("diff = \"fake\""),
        "{saved}"
    );

    assert!(menu(&mut app).contains(&"Open in fake".to_owned()));
    let seen = drive(
        &mut app,
        Message::OpenDiffTool {
            path: "a.txt".into(),
            orig: None,
            target: ToolTarget::Working { staged: false },
        },
    )
    .await;
    assert!(
        seen.iter()
            .any(|message| matches!(message, Message::ToolClosed(_, false, Ok(_))))
    );
    let read = |name: &str| std::fs::read_to_string(f.root.join(name)).unwrap_or_default();
    assert_eq!(
        (read("left"), read("right")),
        ("one\n".into(), "changed\n".into())
    );

    // A file renamed between two compared commits opens with its old name on the left.
    f.git(&["checkout", "-q", "--", "a.txt"]);
    f.git(&["mv", "a.txt", "b.txt"]);
    f.git(&["commit", "-qm", "rename"]);
    drive(&mut app, Message::Refresh).await;
    let (base, target) = (sha_of(&app, "first"), sha_of(&app, "rename"));
    drive(&mut app, Message::CompareStart(base, target.clone())).await;
    app.update(Message::OpenMenu(MenuRequest::File {
        area: None,
        path: "b.txt".into(),
        rev: Some(target),
    }));
    let items = app.menu.take().unwrap().items;
    let Some(Some(open)) = find_entry(&items, "Open in fake") else {
        panic!("no diff tool entry: {:?}", labels(&items));
    };
    assert!(
        matches!(&open, Message::OpenDiffTool { orig: Some(orig), target: ToolTarget::Compare { .. }, .. } if orig == "a.txt"),
        "{open:?}"
    );
    for side in ["left", "right"] {
        let _ = std::fs::remove_file(f.root.join(side));
    }
    drive(&mut app, open).await;
    assert_eq!(
        (read("left"), read("right")),
        ("one\n".into(), "one\n".into())
    );
}

#[tokio::test]
async fn a_conflict_is_resolved_in_the_chosen_merge_tool() {
    let f = empty_fixture();
    f.commit("a.txt", "base\n", "base");
    f.git(&["switch", "-q", "-c", "other"]);
    f.commit("a.txt", "theirs\n", "theirs");
    f.git(&["switch", "-q", "main"]);
    f.commit("a.txt", "ours\n", "ours");
    let _ = Process::new("git")
        .args(["merge", "other"])
        .current_dir(&f.path)
        .output();
    fake_tools(&f);
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SetMergeTool("fake".into())).await;
    app.update(Message::OpenMenu(MenuRequest::File {
        area: Some(Area::Conflicts),
        path: "a.txt".into(),
        rev: None,
    }));
    let items = app.menu.take().unwrap().items;
    assert!(matches!(
        find_entry(&items, "Resolve in fake"),
        Some(Some(Message::OpenMergeTool(_)))
    ));
    drive(&mut app, Message::OpenMergeTool("a.txt".into())).await;
    assert_eq!(f.read("a.txt"), "theirs\n");
    let status = app.current().unwrap().status.as_ref().unwrap();
    assert!(
        status.conflicts.is_empty(),
        "resolved, and the status shows it"
    );
}

#[tokio::test]
async fn start_page_lists_recent_repositories() {
    let f = fixture();
    let mut store = RepoStore::at(f.root.join("config/repos.json"));
    store.add(&f.repo).unwrap();
    let app = App::with_look(Look::preset(false), store);
    let screen = Screen::new().show(&app);
    assert!(screen.has("demo-repo"));
    assert!(screen.has("Open a repository…"));
}

#[tokio::test]
async fn menus_open_submenus_under_the_pointer_and_run_entries() {
    let f = fixture();
    let mut app = opened(&f, false).await;
    let sha = sha_of(&app, "merge side");
    drive(&mut app, Message::CursorMoved(Pos2::new(500.0, 150.0))).await;
    drive(
        &mut app,
        Message::OpenMenu(MenuRequest::Commit { sha, focus: None }),
    )
    .await;
    let screen = Screen::new();
    let shown = screen.show(&app);
    assert!(shown.has("New branch here…"), "the menu is drawn");
    assert!(
        !shown.has("Hard: discard all changes"),
        "submenus start closed"
    );

    let at = shown.find("Reset main to here").unwrap().center();
    screen.frame(&app, vec![Event::PointerMoved(at)]);
    let shown = screen.show(&app);
    assert!(
        shown.has("Hard: discard all changes"),
        "hovering a submenu opens it"
    );

    let child = shown.find("Hard: discard all changes").unwrap().center();
    let press = |pressed| Event::PointerButton {
        pos: child,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    screen.frame(&app, vec![Event::PointerMoved(child)]);
    screen.frame(&app, vec![press(true)]);
    let messages = screen.frame(&app, vec![press(false)]).messages;
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::FromMenu(_))),
        "clicking an entry in a submenu runs it: {messages:?}"
    );
}

#[tokio::test]
async fn ctrl_a_selects_every_file_of_the_focused_list() {
    let f = empty_fixture();
    f.commit("a.txt", "one\n", "first");
    f.commit("b.txt", "two\n", "second");
    f.commit("c.txt", "three\n", "third");
    f.write("a.txt", "changed a\n");
    f.write("b.txt", "changed b\n");
    f.write("c.txt", "changed c\n");
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SetView(View::Changes)).await;
    drive(
        &mut app,
        Message::FilePicked(Area::Unstaged, "b.txt".into()),
    )
    .await;
    drive(&mut app, Message::Key(egui::Key::A, Modifiers::COMMAND)).await;
    let tab = app.current().unwrap();
    assert_eq!(tab.picked.0, Area::Unstaged);
    assert_eq!(
        tab.picked.1,
        ["a.txt", "b.txt", "c.txt"],
        "every unstaged file"
    );
    assert_eq!(
        tab.focus,
        Some((Area::Unstaged, "b.txt".to_owned())),
        "the file shown stays"
    );

    drive(
        &mut app,
        Message::Run(Command::Stage {
            paths: vec!["c.txt".into()],
        }),
    )
    .await;
    drive(&mut app, Message::FilePicked(Area::Staged, "c.txt".into())).await;
    drive(&mut app, Message::Key(egui::Key::A, Modifiers::COMMAND)).await;
    let tab = app.current().unwrap();
    assert_eq!(
        (tab.picked.0, tab.picked.1.as_slice()),
        (Area::Staged, ["c.txt".to_owned()].as_slice()),
        "only the staged list once it has the focus"
    );
}

#[tokio::test]
async fn arrow_keys_move_through_the_changed_files_not_the_commits() {
    let f = empty_fixture();
    f.commit("a.txt", "one\n", "first");
    f.commit("b.txt", "two\n", "second");
    f.write("a.txt", "changed a\n");
    f.write("b.txt", "changed b\n");
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SetView(View::Changes)).await;
    let selected = |app: &App| app.current().unwrap().focus.clone().unwrap().1;
    assert_eq!(selected(&app), "a.txt");
    let commit = app.current().unwrap().selected.clone();

    drive(
        &mut app,
        Message::Key(egui::Key::ArrowDown, Modifiers::NONE),
    )
    .await;
    assert_eq!(selected(&app), "b.txt", "Down selects the next file");
    drive(
        &mut app,
        Message::Key(egui::Key::ArrowDown, Modifiers::NONE),
    )
    .await;
    assert_eq!(selected(&app), "b.txt", "and stops at the end");
    drive(&mut app, Message::Key(egui::Key::ArrowUp, Modifiers::NONE)).await;
    assert_eq!(selected(&app), "a.txt");
    assert_eq!(
        app.current().unwrap().selected,
        commit,
        "the history selection stays put"
    );
}

#[tokio::test]
async fn arrow_keys_follow_the_list_that_was_clicked_last() {
    let f = empty_fixture();
    f.commit("a.txt", "one\n", "first");
    f.write("a.txt", "changed a\n");
    f.write("b.txt", "two\n");
    git(&f.path, &["add", "--", "a.txt", "b.txt"]);
    git(&f.path, &["commit", "-q", "-m", "second"]);
    f.commit("c.txt", "three\n", "third");
    let mut app = opened(&f, false).await;
    let sha = sha_of(&app, "second");
    drive(&mut app, Message::Select(sha.clone())).await;
    let tab = app.current().unwrap();
    let files: Vec<String> = tab
        .detail
        .as_ref()
        .unwrap()
        .files
        .iter()
        .map(|f| f.path.clone())
        .collect();
    assert_eq!(files.len(), 2);

    // Nothing in the file list was clicked yet: the arrows move through commits.
    drive(
        &mut app,
        Message::Key(egui::Key::ArrowDown, Modifiers::NONE),
    )
    .await;
    assert_ne!(
        app.current().unwrap().selected.as_deref(),
        Some(sha.as_str())
    );
    drive(&mut app, Message::Select(sha.clone())).await;

    drive(&mut app, Message::PickFile(files[0].clone())).await;
    drive(
        &mut app,
        Message::Key(egui::Key::ArrowDown, Modifiers::NONE),
    )
    .await;
    let tab = app.current().unwrap();
    assert_eq!(
        tab.selected_file.as_deref(),
        Some(files[1].as_str()),
        "the next file"
    );
    assert_eq!(
        tab.selected.as_deref(),
        Some(sha.as_str()),
        "the same commit"
    );

    // Picking a commit again hands the arrows back to the commit list.
    drive(&mut app, Message::Select(sha.clone())).await;
    drive(
        &mut app,
        Message::Key(egui::Key::ArrowDown, Modifiers::NONE),
    )
    .await;
    assert_ne!(
        app.current().unwrap().selected.as_deref(),
        Some(sha.as_str())
    );
}

#[tokio::test]
async fn the_sides_of_a_split_diff_scroll_sideways_together() {
    let f = empty_fixture();
    let long = "word ".repeat(80);
    f.commit("a.txt", &format!("{long}old\n"), "first");
    f.commit("a.txt", &format!("{long}new\n"), "second");
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SetSplit(true)).await;
    let screen = Screen::new();
    let before = screen.show(&app);
    let sides: Vec<Rect> = before
        .texts
        .iter()
        .filter(|(text, _)| text.starts_with("word word"))
        .map(|(_, rect)| *rect)
        .collect();
    assert_eq!(sides.len(), 2, "both sides show the changed line");
    let (left, right) = (sides[0], sides[1]);
    assert!(left.left() < right.left());

    // Scroll the right side sideways; the left one follows.
    let over = right.left_center() + vec2(40.0, 0.0);
    screen.frame(&app, vec![Event::PointerMoved(over)]);
    screen.frame(
        &app,
        vec![Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(-120.0, 0.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        }],
    );
    let after = screen.show(&app);
    let moved: Vec<Rect> = after
        .texts
        .iter()
        .filter(|(text, _)| text.starts_with("word word"))
        .map(|(_, rect)| *rect)
        .collect();
    assert!(
        moved[0].left() < left.left() - 20.0,
        "the left side moved too: {moved:?} vs {sides:?}"
    );
    assert!(
        moved[1].left() < right.left() - 20.0,
        "and so did the right side"
    );
}

#[tokio::test]
async fn pulling_can_overwrite_the_branch_after_asking_and_keep_a_backup() {
    let f = fixture();
    let remote = f.root.join("remote.git");
    git(
        &f.root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    f.git(&["push", "-q", "-u", "origin", "main"]);
    let other = f.root.join("other");
    git(&f.root, &["clone", "-q", remote.to_str().unwrap(), "other"]);
    for (key, value) in [("user.name", "Other"), ("user.email", "o@example.com")] {
        git(&other, &["config", key, value]);
    }
    std::fs::write(other.join("r.txt"), "remote\n").unwrap();
    git(&other, &["add", "r.txt"]);
    git(&other, &["commit", "-q", "-m", "remote work"]);
    git(&other, &["push", "-q"]);
    let remote_tip = git(&other, &["rev-parse", "HEAD"]).trim().to_owned();
    f.commit("l.txt", "local\n", "local work");
    let local_tip = f.git(&["rev-parse", "HEAD"]).trim().to_owned();

    let mut app = opened(&f, false).await;
    drive(
        &mut app,
        Message::PullChosen {
            remote: Some("origin".into()),
            branch: Some("main".into()),
            mode: "overwrite".into(),
            autostash: false,
            backup: true,
        },
    )
    .await;
    assert_eq!(app.form.as_ref().unwrap().title, "Overwrite local branch?");
    assert_eq!(
        f.git(&["rev-parse", "HEAD"]).trim(),
        local_tip,
        "nothing happens before the answer"
    );
    assert_eq!(
        app.settings.pull.mode, "merge",
        "overwriting isn't remembered as the usual strategy"
    );

    drive(&mut app, Message::FormSubmit).await;
    assert_eq!(f.git(&["rev-parse", "HEAD"]).trim(), remote_tip);
    let backup = format!("backup/main-{}", &local_tip[..7]);
    assert_eq!(f.git(&["rev-parse", &backup]).trim(), local_tip);
    assert!(toast_kinds(&app).contains(&ToastKind::Success));
}

#[tokio::test]
async fn only_the_picked_name_is_the_sidebar_selection() {
    let f = fixture();
    // `main` and the tag v1.0 point at the same commit.
    let mut app = opened(&f, false).await;
    let head = sha_of(&app, "merge side");
    drive(&mut app, Message::ShowRef("t:v1.0".into(), head.clone())).await;
    let tab = app.current().unwrap();
    assert_eq!(tab.selected.as_deref(), Some(head.as_str()));
    assert_eq!(tab.selected_ref.as_deref(), Some("t:v1.0"));

    // Picking a commit in the list leaves no single name as the selection.
    drive(&mut app, Message::Select(head)).await;
    assert_eq!(app.current().unwrap().selected_ref, None);
}

#[tokio::test]
async fn a_dialog_is_as_tall_as_its_content_and_scrolls_only_past_80_percent() {
    let f = fixture();
    let mut app = opened(&f, false).await;
    drive(
        &mut app,
        Message::Action(Action::DeleteBranch("side".into())),
    )
    .await;
    // In a tall window the whole dialog is there, buttons included, below its first line.
    let tall = Screen::sized(vec2(1280.0, 900.0));
    tall.frame(&app, Vec::new());
    let shown = tall.frame(&app, Vec::new());
    let title = shown.find("Delete branch").unwrap().top();
    let button = shown.find("Cancel").unwrap().bottom();
    assert!(button > title + 60.0, "the buttons follow the content");
    // In a window too short for it, the dialog is capped and scrolls: the end is out of view.
    drive(&mut app, Message::WindowResized(vec2(1280.0, 160.0))).await;
    let short = Screen::sized(vec2(1280.0, 160.0));
    short.frame(&app, Vec::new());
    let shown = short.frame(&app, Vec::new());
    assert!(shown.has("Delete branch"), "the start is shown");
    assert!(
        !shown.has("Cancel"),
        "and the buttons are reached by scrolling"
    );
}

#[tokio::test]
async fn settings_fit_a_tall_window_and_scroll_in_a_short_one() {
    let f = fixture();
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::OpenSettings).await;
    let last = "Move through commits or files";

    drive(&mut app, Message::WindowResized(vec2(1280.0, 1600.0))).await;
    let tall = Screen::sized(vec2(1280.0, 1600.0));
    for _ in 0..4 {
        tall.frame(&app, Vec::new());
    }
    assert!(
        tall.frame(&app, Vec::new()).has(last),
        "everything is there when it fits"
    );

    drive(&mut app, Message::WindowResized(vec2(1280.0, 600.0))).await;
    let short = Screen::sized(vec2(1280.0, 600.0));
    for _ in 0..4 {
        short.frame(&app, Vec::new());
    }
    let shown = short.frame(&app, Vec::new());
    assert!(shown.has("Settings"), "the start is shown");
    assert!(!shown.has(last), "and the rest is reached by scrolling");
}

#[tokio::test]
async fn squash_into_parent_is_in_the_edit_commit_submenu_only() {
    let f = fixture();
    let app = opened(&f, false).await;
    let tab = app.current().unwrap();
    let commit = tab
        .commits
        .iter()
        .find(|c| c.subject == "second commit")
        .unwrap();
    let items = commit_menu_items(tab, commit, None);
    assert!(!items.iter().any(|item| matches!(
        item,
        MenuItem::Entry { label, .. } if label.starts_with("Squash into parent")
    )));
    assert!(
        items
            .iter()
            .any(|item| matches!(item, MenuItem::Sub { label, .. } if label == "Edit commit"))
    );
}

#[tokio::test]
async fn file_dialogs_open_repositories_and_save_and_apply_patches() {
    use gitcraft_ui::app::{Effect, Pick};
    let f = fixture();
    let store = RepoStore::at(f.root.join("config/repos.json"));
    let mut app = App::with_look(Look::preset(true), store);

    // The dialog is the shell's to show; the app only asks, and acts on the answer.
    drive(&mut app, Message::OpenDialog).await;
    assert!(matches!(
        app.effects.as_slice(),
        [Effect::Ask(Pick::OpenRepository)]
    ));
    app.effects.clear();
    drive(
        &mut app,
        Message::Picked(Pick::OpenRepository, f.path.clone()),
    )
    .await;
    assert_eq!(app.tabs.len(), 1, "the picked folder opens in a tab");

    // Not the newest commit: that one is a merge, which has no patch.
    let subject = "second commit".to_owned();
    let sha = sha_of(&app, &subject);
    drive(&mut app, Message::Action(Action::SavePatch(sha))).await;
    let Some(Effect::Ask(pick)) = app.effects.pop() else {
        panic!("saving a patch asks where");
    };
    let Pick::SavePatch { name, .. } = &pick else {
        panic!("{pick:?}");
    };
    assert!(name.ends_with(".patch"), "{name}");
    let target = f.root.join("out.patch");
    drive(&mut app, Message::Picked(pick, target.clone())).await;
    let patch = std::fs::read_to_string(&target).unwrap();
    assert!(
        patch.contains(&format!("Subject: [PATCH] {subject}")),
        "{patch}"
    );
    assert!(toast_kinds(&app).contains(&ToastKind::Success));

    drive(&mut app, Message::Action(Action::ApplyPatch)).await;
    assert!(matches!(
        app.effects.pop(),
        Some(Effect::Ask(Pick::ApplyPatch))
    ));
    drive(&mut app, Message::Picked(Pick::ApplyPatch, target)).await;
    let form = app
        .form
        .as_ref()
        .expect("the patch is offered for applying");
    assert_eq!(form.title, "Apply patch");
    assert_eq!(form.message.as_deref(), Some("out.patch"));

    drive(&mut app, Message::DialogFailed("no portal".into())).await;
    assert!(toast_kinds(&app).contains(&ToastKind::Error));
}

#[tokio::test]
async fn the_menu_bar_offers_the_app_and_its_choices_persist() {
    use gitcraft_ui::menu::Mark;
    let f = fixture();
    let mut app = opened(&f, true).await;
    let menus = app.bar_menus();
    let titles: Vec<_> = menus.iter().map(|(title, _)| *title).collect();
    assert_eq!(titles, ["File", "Edit", "Repository", "View", "Help"]);
    let view = &menus[3].1;
    let Some(MenuItem::Sub {
        children: themes, ..
    }) = view.iter().find(|item| item.label() == Some("Theme"))
    else {
        panic!("View has a Theme submenu");
    };
    let chosen: Vec<_> = themes
        .iter()
        .filter_map(|item| match item {
            MenuItem::Entry {
                label,
                mark: Mark::Radio(true),
                ..
            } => Some(label.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(chosen, ["Follow system"]);
    let repository = labels(&menus[2].1);
    assert!(repository.contains(&"Fetch".to_owned()), "{repository:?}");

    // English, so that the other tests running beside this one keep their English text.
    drive(&mut app, Message::SetLanguage("en".into())).await;
    drive(&mut app, Message::ToggleSystemTitleBar).await;
    assert!(toast_kinds(&app).contains(&ToastKind::Info));
    let saved = gitcraft_ui::settings::Settings::load(&f.root.join("config"));
    assert_eq!(saved.general.language, "en");
    assert!(saved.appearance.system_title_bar);
    assert!(f.root.join("config/config.toml").is_file());
}

#[tokio::test]
async fn many_tabs_scroll_the_one_in_front_into_view_and_are_listed_in_the_menu() {
    let f = fixture();
    let mut app = opened(&f, true).await;
    for index in 0..6 {
        let repo = f.root.join(format!("repository-with-a-long-name-{index}"));
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        drive(
            &mut app,
            Message::OpenPath(repo.to_str().unwrap().to_owned()),
        )
        .await;
    }
    assert_eq!(app.tabs.len(), 7);
    let width = 900.0;
    let screen = Screen::sized(vec2(width, 700.0));
    // A few frames: the strip learns its width in the first, then scrolls.
    let label_x = |screen: &Screen, app: &App, name: &str| {
        screen.show(app);
        let shown = screen.show(app);
        let (_, rect) = shown.texts.iter().find(|(text, _)| text == name).unwrap();
        rect.center().x
    };
    let last = "repository-with-a-long-name-5";
    assert!(
        label_x(&screen, &app, last) < width - 60.0,
        "the newest tab is scrolled into view"
    );
    drive(&mut app, Message::Activate(0)).await;
    let first = label_x(&screen, &app, "demo-repo");
    assert!(
        first > 0.0 && first < width,
        "and so is the first one once picked"
    );

    let menus = app.bar_menus();
    let (_, view) = menus.iter().find(|(title, _)| *title == "View").unwrap();
    let Some(MenuItem::Sub { children, .. }) = view.first() else {
        panic!("the View menu starts with the tabs");
    };
    assert_eq!(children.len(), 7);
    let MenuItem::Entry {
        label,
        message,
        mark,
        ..
    } = &children[0]
    else {
        panic!("an entry per tab");
    };
    assert_eq!(label, "demo-repo");
    assert!(matches!(message, Some(Message::Activate(0))));
    assert_eq!(
        *mark,
        gitcraft_ui::menu::Mark::Radio(true),
        "the tab in front is marked"
    );
}

#[tokio::test]
async fn dragging_a_tab_past_its_neighbour_moves_it() {
    let f = fixture();
    let second = f.root.join("second");
    std::fs::create_dir(&second).unwrap();
    git(&second, &["init", "-q", "-b", "main"]);
    let mut app = opened(&f, true).await;
    drive(
        &mut app,
        Message::OpenPath(second.to_str().unwrap().to_owned()),
    )
    .await;
    let names = |app: &App| {
        app.tabs
            .iter()
            .map(|tab| tab.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&app), ["demo-repo", "second"]);

    let screen = Screen::new();
    let shown = screen.show(&app);
    // The tab's own label, not the window title that contains the name too.
    let exactly = |name: &str| {
        let (_, rect) = shown.texts.iter().find(|(text, _)| text == name).unwrap();
        rect.center()
    };
    let from = exactly("demo-repo");
    // Well past the middle of the neighbouring tab, whose label sits at its left.
    let to = exactly("second") + vec2(80.0, 0.0);
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    screen.frame(&app, vec![Event::PointerMoved(from)]);
    screen.frame(&app, vec![button(from, true)]);
    let mut messages = Vec::new();
    for step in 1..=8 {
        let at = from + (to - from) * (step as f32 / 8.0);
        messages.extend(screen.frame(&app, vec![Event::PointerMoved(at)]).messages);
    }
    screen.frame(&app, vec![button(to, false)]);
    let moved = messages
        .into_iter()
        .find(|message| matches!(message, Message::MoveTab(0, 1)))
        .expect("the drag moves the first tab right");
    drive(&mut app, moved).await;
    assert_eq!(names(&app), ["second", "demo-repo"]);
}

#[tokio::test]
async fn edit_undo_and_redo_take_back_a_commit() {
    let f = empty_fixture();
    f.commit("a.txt", "one\n", "first");
    f.write("a.txt", "two\n");
    let mut app = opened(&f, false).await;
    let edit = |app: &App| {
        let menus = app.bar_menus();
        let (_, items) = menus.iter().find(|(name, _)| *name == "Edit").unwrap();
        labels(items)
    };
    assert_eq!(edit(&app), ["Undo", "Redo"], "nothing to undo yet");

    drive(&mut app, Message::Run(Command::StageAll)).await;
    drive(
        &mut app,
        Message::Run(Command::Commit {
            message: "second".into(),
            amend: false,
            options: Default::default(),
        }),
    )
    .await;
    assert_eq!(f.subjects(), ["second", "first"]);
    assert_eq!(edit(&app), ["Undo: Commit", "Redo"]);

    drive(&mut app, Message::Undo).await;
    assert_eq!(f.subjects(), ["first"]);
    assert_eq!(
        f.git(&["diff", "--cached", "--name-only"]),
        "a.txt\n",
        "still staged"
    );
    assert_eq!(edit(&app), ["Undo", "Redo: Commit"]);

    drive(&mut app, Message::Redo).await;
    assert_eq!(f.subjects(), ["second", "first"]);
    drive(&mut app, Message::Key(egui::Key::Z, Modifiers::COMMAND)).await;
    assert_eq!(f.subjects(), ["first"], "Ctrl+Z undoes too");
}

#[tokio::test]
async fn commit_options_sign_off_co_authors_and_push_after() {
    let f = empty_fixture();
    f.commit("a.txt", "one\n", "first");
    let remote = f.root.join("remote.git");
    git(&f.root, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    f.git(&["push", "-q", "-u", "origin", "main"]);
    f.write("a.txt", "two\n");
    f.write("new.txt", "new\n");
    let mut app = opened(&f, false).await;

    drive(&mut app, Message::CommitOption(CommitOption::SignOff, true)).await;
    drive(
        &mut app,
        Message::CommitOption(CommitOption::PushAfter, true),
    )
    .await;
    drive(&mut app, Message::Action(Action::CoAuthors)).await;
    assert!(app.form.is_some());
    // What the form sends on Save.
    drive(&mut app, Message::FormCancel).await;
    drive(
        &mut app,
        Message::CoAuthors("Ada Lovelace <ada@example.com>\nnot an address\n".into()),
    )
    .await;
    drive(&mut app, Message::SummaryInput("second".into())).await;
    // Ctrl+Shift+Enter stages everything, the new file too.
    drive(
        &mut app,
        Message::Key(egui::Key::Enter, Modifiers::COMMAND | Modifiers::SHIFT),
    )
    .await;

    let message = f.git(&["log", "-1", "--format=%B"]);
    assert!(message.starts_with("second\n"), "{message}");
    assert!(message.contains("Co-authored-by: Ada Lovelace <ada@example.com>"));
    assert!(!message.contains("not an address"));
    assert!(message.contains("Signed-off-by: Test User <t@example.com>"));
    assert_eq!(
        f.git(&["show", "--name-only", "--format=", "HEAD"]),
        "a.txt\nnew.txt\n"
    );
    assert_eq!(
        git(&remote, &["rev-parse", "main"]),
        f.git(&["rev-parse", "HEAD"]),
        "pushed after committing"
    );

    // Amending what is already pushed asks first.
    drive(&mut app, Message::AmendToggled(true)).await;
    drive(&mut app, Message::SummaryInput("second, amended".into())).await;
    drive(&mut app, Message::CommitNow).await;
    assert!(app.form.is_some(), "a warning comes first");
    drive(&mut app, Message::FormCancel).await;
    assert_eq!(f.subjects(), ["second", "first"]);
}

#[tokio::test]
async fn a_hunk_goes_into_a_stash_of_its_own() {
    let f = empty_fixture();
    let lines: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    f.commit("a.txt", &lines, "first");
    let changed = lines
        .replace("line 2\n", "line 2 changed\n")
        .replace("line 18\n", "line 18 changed\n");
    f.write("a.txt", &changed);
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::SetView(View::Changes)).await;
    drive(
        &mut app,
        Message::FilePicked(Area::Unstaged, "a.txt".into()),
    )
    .await;
    let clicked = Screen::new().click(&app, "Stash hunk", PointerButton::Primary);
    assert!(!clicked.is_empty());
    for message in clicked {
        drive(&mut app, message).await;
    }
    assert_eq!(f.git(&["stash", "list"]).lines().count(), 1);
    assert!(
        f.git(&["stash", "list"])
            .contains("Selected changes in a.txt")
    );
    assert_eq!(
        app.current().unwrap().refs.stashes.len(),
        1,
        "the sidebar shows it"
    );
    assert_ne!(f.read("a.txt"), changed, "one of the changes left the file");
}

#[tokio::test]
async fn a_hunk_of_a_commit_is_reverted_from_its_diff() {
    let f = empty_fixture();
    let lines: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    f.commit("a.txt", &lines, "first");
    let changed = lines
        .replace("line 2\n", "line 2 changed\n")
        .replace("line 18\n", "line 18 changed\n");
    f.commit("a.txt", &changed, "two changes");
    let mut app = opened(&f, false).await;
    let sha = sha_of(&app, "two changes");
    drive(&mut app, Message::Select(sha)).await;
    drive(&mut app, Message::PickFile("a.txt".into())).await;
    assert!(app.current().unwrap().commit_pane.diff.is_some());
    let clicked = Screen::new().click(&app, "Revert hunk", PointerButton::Primary);
    assert!(!clicked.is_empty());
    for message in clicked {
        drive(&mut app, message).await;
    }
    assert_eq!(
        f.read("a.txt"),
        changed.replace("line 2 changed\n", "line 2\n")
    );
    assert_eq!(f.subjects(), ["two changes", "first"]);
}

#[tokio::test]
async fn stashes_are_renamed_and_untracked_files_stashed_alone() {
    let f = fixture();
    f.write("a.txt", "changed\n");
    f.write("new.txt", "new\n");
    let mut app = opened(&f, false).await;

    // The stash dialog's last option: only the untracked files.
    drive(&mut app, Message::Action(Action::Stash)).await;
    let last = app.form.as_ref().unwrap().fields.len() - 1;
    drive(&mut app, Message::FormCheck(last, true)).await;
    drive(&mut app, Message::FormSubmit).await;
    assert!(!f.path.join("new.txt").exists());
    assert_eq!(f.read("a.txt"), "changed\n");

    drive(
        &mut app,
        Message::OpenMenu(MenuRequest::Stash("stash@{0}".into())),
    )
    .await;
    let menu = app.menu.as_ref().expect("the stash menu").items.clone();
    let Some(Some(rename)) = find_entry(&menu, "Rename…") else {
        panic!("Rename is offered")
    };
    drive(&mut app, rename).await;
    drive(&mut app, Message::FormText(0, "new name".into())).await;
    drive(&mut app, Message::FormSubmit).await;
    assert_eq!(f.git(&["stash", "list", "--format=%gs"]), "new name\n");

    // One file comes back from the stash view's file menu.
    drive(&mut app, Message::OpenStash("stash@{0}".into())).await;
    drive(
        &mut app,
        Message::OpenMenu(MenuRequest::File {
            area: None,
            path: "new.txt".into(),
            rev: Some("stash@{0}".into()),
        }),
    )
    .await;
    let items = app.menu.as_ref().expect("the file menu").items.clone();
    let Some(Some(take)) = find_entry(&items, "Take this file from the stash") else {
        panic!("offered for a stash's file")
    };
    drive(&mut app, take).await;
    assert_eq!(f.read("new.txt"), "new\n");
}

#[tokio::test]
async fn the_commit_menu_makes_a_fixup_commit() {
    let f = fixture();
    f.write("a.txt", "fixed\n");
    f.git(&["add", "a.txt"]);
    let mut app = opened(&f, false).await;
    let sha = sha_of(&app, "first commit");
    let commit = app
        .current()
        .unwrap()
        .commits
        .iter()
        .find(|c| c.sha == sha)
        .unwrap()
        .clone();
    let items = commit_menu_items(app.current().unwrap(), &commit, None);
    let Some(Some(fixup)) = find_entry(&items, "Fixup commit of the staged changes") else {
        panic!("offered with staged changes")
    };
    drive(&mut app, fixup).await;
    assert_eq!(f.subjects()[0], "fixup! first commit");
}

#[tokio::test]
async fn local_changes_are_saved_as_a_patch() {
    use gitcraft_ui::app::Effect;
    let f = fixture();
    f.write("a.txt", "changed\n");
    f.write("new.txt", "new\n");
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::Action(Action::SaveChangesPatch)).await;
    let Some(Effect::Ask(pick)) = app.effects.pop() else {
        panic!("saving a patch asks where");
    };
    let target = f.root.join("changes.patch");
    drive(&mut app, Message::Picked(pick, target.clone())).await;
    let patch = std::fs::read_to_string(&target).unwrap();
    assert!(
        patch.contains("+changed") && patch.contains("+new"),
        "{patch}"
    );
}

#[tokio::test]
async fn a_commits_file_text_is_copied() {
    use gitcraft_ui::app::Effect;
    let f = fixture();
    let mut app = opened(&f, false).await;
    let sha = sha_of(&app, "first commit");
    drive(&mut app, Message::CopyFileText(sha, "a.txt".into())).await;
    assert!(app.effects.contains(&Effect::Copy("one\n".into())));
}

#[tokio::test]
async fn several_commits_are_picked_then_cherry_picked_squashed_or_replayed() {
    let f = fixture();
    f.git(&["switch", "-q", "-c", "topic", "main"]);
    for n in 1..=3 {
        f.commit(&format!("t{n}.txt"), "t\n", &format!("topic {n}"));
    }
    f.git(&["switch", "-q", "main"]);
    let mut app = opened(&f, false).await;
    let (one, three) = (sha_of(&app, "topic 1"), sha_of(&app, "topic 3"));
    drive(&mut app, Message::Select(three.clone())).await;
    app.modifiers = Modifiers::SHIFT;
    drive(&mut app, Message::Select(one.clone())).await;
    app.modifiers = Modifiers::NONE;
    let picked = app.current().unwrap().picked_commits.clone();
    assert_eq!(picked.len(), 3);
    assert!(app.current().unwrap().compare.is_none());

    // Squashing opens the rebase editor with the plan made: here on topic, from main.
    f.git(&["switch", "-q", "topic"]);
    drive(&mut app, Message::Refresh).await;
    drive(
        &mut app,
        Message::PlanCommits(gitcraft_ui::app::Preset::Squash(picked.clone())),
    )
    .await;
    let Some(Overlay::Rebase(state)) = &app.overlay else {
        panic!("the editor opens")
    };
    let plan: Vec<&str> = state.steps.iter().map(|step| step.action).collect();
    assert_eq!(plan, ["pick", "squash", "squash"], "{:?}", state.error);
    drive(&mut app, Message::OverlayClose).await;
    f.git(&["switch", "-q", "main"]);
    drive(&mut app, Message::Refresh).await;

    drive(
        &mut app,
        Message::OpenMenu(MenuRequest::Commits(picked.clone())),
    )
    .await;
    let items = app.menu.as_ref().unwrap().items.clone();
    let Some(Some(pick)) = find_entry(&items, "Cherry-pick 3 commits into current") else {
        panic!("{:?}", labels(&items))
    };
    drive(&mut app, pick).await;
    assert_eq!(&f.subjects()[..3], ["topic 3", "topic 2", "topic 1"]);

    f.git(&["reset", "-q", "--hard", "HEAD~3"]);
    drive(&mut app, Message::Refresh).await;
    drive(&mut app, Message::OpenReplay(picked)).await;
    let Some(Overlay::Rebase(state)) = &app.overlay else {
        panic!("the editor opens for a cherry-pick")
    };
    assert!(state.replay);
    assert_eq!(state.steps[0].commit.subject, "topic 1", "oldest first");
    drive(&mut app, Message::RebaseSetAction(2, "drop")).await;
    drive(&mut app, Message::RebaseStart).await;
    assert_eq!(&f.subjects()[..2], ["topic 2", "topic 1"]);
}

#[tokio::test]
async fn cherry_picking_a_merge_asks_for_the_mainline() {
    let f = fixture();
    let mut app = opened(&f, false).await;
    let merge = sha_of(&app, "merge side");
    drive(&mut app, Message::Action(Action::CherryPick(merge))).await;
    let form = app.form.as_ref().expect("asks which parent");
    assert_eq!(form.fields.len(), 1);
    drive(&mut app, Message::FormCancel).await;
}

#[tokio::test]
async fn the_rebase_editor_autosquashes_moves_and_refuses_a_stale_plan() {
    let f = empty_fixture();
    f.commit("a.txt", "a\n", "add a");
    f.commit("b.txt", "b\n", "add b");
    f.commit("a.txt", "a fixed\n", "fixup! add a");
    let mut app = opened(&f, false).await;
    let first = sha_of(&app, "add a");
    drive(&mut app, Message::OpenRebase(Some(first.clone()))).await;
    drive(&mut app, Message::OverlayClose).await;
    // From the root: every commit is in the plan.
    drive(&mut app, Message::OpenRebase(None)).await;
    drive(&mut app, Message::RebaseAutosquash).await;
    let Some(Overlay::Rebase(state)) = &app.overlay else {
        panic!("open")
    };
    let plan: Vec<(String, &str)> = state
        .steps
        .iter()
        .map(|step| (step.commit.subject.clone(), step.action))
        .collect();
    assert_eq!(
        plan,
        [
            ("add a".to_owned(), "pick"),
            ("fixup! add a".to_owned(), "fixup"),
            ("add b".to_owned(), "pick")
        ]
    );
    // Someone commits meanwhile: the plan no longer fits.
    f.commit("c.txt", "c\n", "meanwhile");
    drive(&mut app, Message::RebaseStart).await;
    assert_eq!(
        f.subjects(),
        ["meanwhile", "fixup! add a", "add b", "add a"]
    );
    assert!(toast_kinds(&app).contains(&ToastKind::Error));
    drive(&mut app, Message::OverlayClose).await;
    drive(&mut app, Message::Refresh).await;

    let b = sha_of(&app, "add b");
    drive(&mut app, Message::Action(Action::MoveCommit(b, false))).await;
    assert_eq!(
        f.subjects(),
        ["meanwhile", "fixup! add a", "add a", "add b"]
    );
}

#[tokio::test]
async fn bisecting_from_the_commit_menu_and_the_bisect_bar() {
    let f = empty_fixture();
    for n in 1..=8 {
        let state = if n >= 6 { "broken" } else { "fine" };
        f.commit(
            "state.txt",
            &format!("{state}\n{n}\n"),
            &format!("commit {n}"),
        );
    }
    let mut app = opened(&f, false).await;
    let (bad, good) = (sha_of(&app, "commit 8"), sha_of(&app, "commit 1"));
    drive(&mut app, Message::Action(Action::BisectStart(bad))).await;
    drive(&mut app, Message::FormText(0, good)).await;
    drive(&mut app, Message::FormSubmit).await;
    assert!(app.current().unwrap().bisect.is_some());
    for _ in 0..6 {
        if app
            .current()
            .unwrap()
            .bisect
            .as_ref()
            .unwrap()
            .first_bad
            .is_some()
        {
            break;
        }
        let button = if f.read("state.txt").starts_with("broken") {
            "Bad"
        } else {
            "Good"
        };
        for message in Screen::new().click(&app, button, PointerButton::Primary) {
            drive(&mut app, message).await;
        }
    }
    let first = app
        .current()
        .unwrap()
        .bisect
        .as_ref()
        .unwrap()
        .first_bad
        .clone();
    assert_eq!(first, Some(sha_of(&app, "commit 6")));
    assert!(Screen::new().show(&app).has("First bad commit"));
    for message in Screen::new().click(&app, "End bisect", PointerButton::Primary) {
        drive(&mut app, message).await;
    }
    assert!(app.current().unwrap().bisect.is_none());
    assert_eq!(f.git(&["branch", "--show-current"]), "main\n");
}

#[tokio::test]
async fn a_commit_is_compared_with_the_working_directory() {
    let f = fixture();
    f.write("a.txt", "edited\n");
    let mut app = opened(&f, false).await;
    let first = sha_of(&app, "first commit");
    drive(
        &mut app,
        Message::CompareStart(first, gitcraft_core::queries::WORKTREE.to_owned()),
    )
    .await;
    let compare = app.current().unwrap().compare.as_ref().unwrap();
    let files: Vec<&str> = compare
        .result
        .as_ref()
        .unwrap()
        .files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert!(
        files.contains(&"a.txt") && files.contains(&"b.txt"),
        "{files:?}"
    );
    assert!(Screen::new().show(&app).has("Working directory"));
    drive(&mut app, Message::CompareSwap).await;
    assert!(
        app.current()
            .unwrap()
            .compare
            .as_ref()
            .unwrap()
            .error
            .is_none()
    );
}

#[tokio::test]
async fn commit_ranges_are_compared_in_their_own_view() {
    let f = fixture();
    f.git(&["switch", "-q", "-c", "topic"]);
    f.commit("t.txt", "t\n", "topic work");
    f.git(&["branch", "topic-old"]);
    f.git(&["commit", "-q", "--amend", "-m", "topic work, reworded"]);
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::Action(Action::RangeDiff)).await;
    drive(&mut app, Message::FormText(0, "main..topic-old".into())).await;
    drive(&mut app, Message::FormText(1, "main..topic".into())).await;
    drive(&mut app, Message::FormSubmit).await;
    let Some(Overlay::RangeDiff { text, error, .. }) = &app.overlay else {
        panic!("open")
    };
    assert!(error.is_none(), "{error:?}");
    assert!(text.contains("topic work, reworded"));
    assert!(Screen::new().show(&app).has("topic work, reworded"));
}

#[tokio::test]
async fn the_merge_dialog_says_which_files_would_conflict() {
    let f = fixture();
    f.git(&["switch", "-q", "-c", "other"]);
    f.commit("a.txt", "theirs\n", "theirs");
    f.git(&["switch", "-q", "main"]);
    f.commit("a.txt", "ours\n", "ours");
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::Action(Action::Merge("other".into()))).await;
    let message = app
        .form
        .as_ref()
        .unwrap()
        .message
        .clone()
        .unwrap_or_default();
    assert!(message.contains("Expect a conflict in a.txt"), "{message}");
}

#[tokio::test]
async fn the_history_is_filtered_and_the_filter_cleared() {
    let f = fixture();
    let mut app = opened(&f, false).await;
    let all = app.current().unwrap().commits.len();
    drive(&mut app, Message::Action(Action::FilterHistory)).await;
    drive(&mut app, Message::FormText(1, "b.txt".into())).await;
    drive(&mut app, Message::FormSubmit).await;
    let tab = app.current().unwrap();
    let subjects: Vec<&str> = tab
        .commits
        .iter()
        .map(|commit| commit.subject.as_str())
        .collect();
    assert_eq!(subjects, ["second commit"]);
    assert!(Screen::new().show(&app).has("Filtered history"));
    for message in Screen::new().click(&app, "Clear filter", PointerButton::Primary) {
        drive(&mut app, message).await;
    }
    assert!(app.current().unwrap().history_filter.is_none());
    assert_eq!(app.current().unwrap().commits.len(), all);
}

#[tokio::test]
async fn a_branch_is_hidden_from_the_graph_and_remembered() {
    use gitcraft_ui::app::RefVisibility;
    let f = fixture();
    let root = f.git(&["rev-list", "--max-parents=0", "HEAD"]);
    f.git(&["switch", "-q", "-c", "topic", root.trim()]);
    f.commit("t.txt", "t\n", "topic work");
    f.git(&["switch", "-q", "main"]);
    let mut app = opened(&f, false).await;
    assert!(
        app.current()
            .unwrap()
            .commits
            .iter()
            .any(|c| c.subject == "topic work")
    );
    drive(
        &mut app,
        Message::RefVisibility(RefVisibility::Hide("refs/heads/topic".into())),
    )
    .await;
    assert!(
        !app.current()
            .unwrap()
            .commits
            .iter()
            .any(|c| c.subject == "topic work")
    );
    assert!(Screen::new().show(&app).has("hidden: topic"));
    let path = app.current().unwrap().path.clone();
    assert!(
        app.settings.graph.views.contains_key(&path),
        "remembered for the repository"
    );
    drive(
        &mut app,
        Message::RefVisibility(RefVisibility::Solo("refs/heads/topic".into())),
    )
    .await;
    let subjects: Vec<&str> = app
        .current()
        .unwrap()
        .commits
        .iter()
        .map(|c| c.subject.as_str())
        .collect();
    assert_eq!(subjects, ["topic work", "first commit"]);
    drive(&mut app, Message::RefVisibility(RefVisibility::ShowAll)).await;
    assert!(!app.settings.graph.views.contains_key(&path));
}

#[tokio::test]
async fn a_stash_shows_in_the_history_and_opens_from_there() {
    let f = fixture();
    f.write("a.txt", "stashed\n");
    f.git(&["stash", "push", "-q", "-m", "work aside"]);
    let mut app = opened(&f, false).await;
    let tab = app.current().unwrap();
    let row = tab
        .commits
        .iter()
        .find(|commit| commit.stash.is_some())
        .expect("a stash row");
    assert!(row.subject.contains("work aside"));
    for message in Screen::new().click(&app, "stash@{0}", PointerButton::Primary) {
        drive(&mut app, message).await;
    }
    assert_eq!(app.current().unwrap().view, View::Stash);
}

#[tokio::test]
async fn history_columns_are_chosen_and_resized() {
    use gitcraft_ui::app::Column;
    let f = fixture();
    let mut app = opened(&f, false).await;
    assert!(Screen::new().show(&app).has("AUTHOR"));
    drive(&mut app, Message::ToggleColumn(Column::Author)).await;
    assert!(!Screen::new().show(&app).has("AUTHOR"));
    drive(&mut app, Message::ToggleColumn(Column::Changes)).await;
    let tab = app.current().unwrap();
    let second = tab
        .commits
        .iter()
        .find(|c| c.subject == "second commit")
        .unwrap();
    assert_eq!(tab.stats.get(&second.sha).map(|stats| stats.added), Some(1));
    assert!(Screen::new().show(&app).has("CHANGES"));
    drive(&mut app, Message::ColumnWidth(Column::Date, 200.0, true)).await;
    assert_eq!(app.settings.columns.date_width, 200.0);
    drive(&mut app, Message::OpenMenu(MenuRequest::Columns)).await;
    let items = app.menu.as_ref().unwrap().items.clone();
    let Some(Some(reset)) = find_entry(&items, "Reset columns") else {
        panic!("{:?}", labels(&items))
    };
    drive(&mut app, reset).await;
    assert_eq!(
        app.settings.columns,
        gitcraft_ui::settings::Columns::default()
    );
}

#[tokio::test]
async fn blame_skips_listed_commits_and_can_be_told_not_to() {
    let f = empty_fixture();
    f.commit("code.txt", "a = 1\n", "write it");
    f.commit("code.txt", "a  =  1\n", "reformat");
    let reformat = f.git(&["rev-parse", "HEAD"]);
    f.write(".git-blame-ignore-revs", &reformat);
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::OpenBlame("code.txt".into(), None)).await;
    let Some(Overlay::Blame { lines, ignored, .. }) = &app.overlay else {
        panic!("open")
    };
    assert_eq!(*ignored, Some(true));
    assert_eq!(lines[0].summary, "write it");
    assert!(
        Screen::new()
            .show(&app)
            .has("Skip commits in .git-blame-ignore-revs")
    );
    drive(&mut app, Message::BlameIgnoreRevs(false)).await;
    let Some(Overlay::Blame { lines, .. }) = &app.overlay else {
        panic!("still open")
    };
    assert_eq!(lines[0].summary, "reformat");
}

#[tokio::test]
async fn repositories_are_fetched_in_the_background_when_due() {
    let f = fixture();
    let remote = f.root.join("remote.git");
    git(
        &f.root,
        &["clone", "-q", "--bare", &f.repo, remote.to_str().unwrap()],
    );
    f.git(&["remote", "add", "origin", remote.to_str().unwrap()]);
    let mut app = opened(&f, false).await;
    drive(&mut app, Message::AutoFetchTick).await;
    assert!(f.git(&["branch", "-r"]).is_empty(), "off by default");

    drive(&mut app, Message::SetAutoFetch(5)).await;
    drive(&mut app, Message::AutoFetchTick).await;
    assert!(
        f.git(&["branch", "-r"]).is_empty(),
        "not due yet: the tab only just opened"
    );

    app.tabs[app.active].last_fetch = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(600))
        .unwrap();
    drive(&mut app, Message::AutoFetchTick).await;
    assert!(f.git(&["branch", "-r"]).contains("origin/main"));
    let tab = app.current().unwrap();
    assert!(!tab.fetching);
    assert!(
        tab.refs
            .remote
            .iter()
            .any(|branch| branch.name == "origin/main"),
        "the sidebar shows what came in"
    );
}
