//! Drives the real UI against real repositories: messages go in, tasks run to the
//! end, and results are checked with git itself. No mocks.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command as Process;

use egui::{Event, Modifiers, PointerButton, Pos2, Rect, Vec2, vec2};
use futures::StreamExt;
use gitcraft_core::runner::set_editor_exe;
use gitcraft_core::store::RepoStore;
use gitcraft_ui::App;
use gitcraft_ui::actions::{Action, MenuRequest, RefTarget, commit_menu_items};
use gitcraft_ui::app::{Area, Message, Overlay, ToastKind, View};
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
    assert_eq!(app.settings.diff_context, 1, "bad input is ignored");

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
    assert_eq!(saved.diff_context, 1);
    assert!(saved.diff_split);
    assert_eq!(
        saved.tabs,
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
    assert_eq!(app.settings.pull_mode, "rebase");
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
    app.settings.history_height = 1200.0;
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
    assert!(app.settings.commit_graph);
    assert!(f.path.join(".git/objects/info/commit-graph").exists());
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
        app.settings.pull_mode, "merge",
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
