//! Application state, messages and the update loop.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gitcraft_core::model::*;
use gitcraft_core::parsers::Selection;
use gitcraft_core::runner::Progress;
use gitcraft_core::store::RepoStore;
use gitcraft_core::watcher::{self, WatchEvent};
use gitcraft_core::{Error, ops, queries, repos};
use iced::futures::{SinkExt, Stream};
use iced::widget::{Id, operation, scrollable, text_editor};
use iced::{Element, Point, Size, Subscription, Task, Theme, keyboard, theme, window};

use crate::actions::{Action, MenuRequest};
use crate::command::Command;
use crate::form::{FieldKind, Form};
use crate::highlight::{self, Highlights};
use crate::logic::{self, Choice, plural, short_sha};
use crate::menu::{Menu, MenuItem};
use crate::settings::Settings;
use crate::theme::Look;
use crate::views;

/// Height of one commit row; lists are virtualised on this.
pub const ROW_HEIGHT: f32 = 26.0;
pub const DIFF_ROW_HEIGHT: f32 = 20.0;
pub const BLAME_ROW_HEIGHT: f32 = 20.0;
pub const REBASE_ROW_HEIGHT: f32 = 36.0;
const SEARCH_LIMIT: usize = 500;
const FILE_HISTORY_LIMIT: usize = 1000;
const REFLOG_LIMIT: usize = 500;

pub type Answer<T> = Result<T, String>;

fn answer<T>(result: Result<T, Error>) -> Answer<T> {
    result.map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    History,
    Changes,
    Stash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Unstaged,
    Staged,
    Conflicts,
}

/// Which diff a message is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKey {
    Changes,
    Commit,
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffRow {
    Hunk(usize),
    Line(usize, usize),
    Split(usize, Option<usize>, Option<usize>),
}

/// A virtualised diff.
pub struct DiffPane {
    pub diff: Option<FileDiff>,
    pub rows: Vec<DiffRow>,
    pub highlights: Option<Arc<Highlights>>,
    pub loading: bool,
    pub token: u64,
    pub offset: f32,
    pub viewport: Size,
    pub scroll: Id,
    /// Row tops for a width and row count: see the diff view.
    pub layout: RefCell<(f32, usize, Vec<f32>)>,
    pub selection: BTreeSet<(usize, usize)>,
    pub anchor: Option<(usize, usize)>,
}

impl Default for DiffPane {
    fn default() -> Self {
        Self {
            diff: None,
            rows: Vec::new(),
            highlights: None,
            loading: false,
            token: 0,
            offset: 0.0,
            viewport: Size::new(800.0, 400.0),
            scroll: Id::unique(),
            layout: RefCell::new((-1.0, 0, Vec::new())),
            selection: BTreeSet::new(),
            anchor: None,
        }
    }
}

impl DiffPane {
    fn clear(&mut self) {
        self.token += 1;
        self.diff = None;
        self.rows.clear();
        self.layout.get_mut().0 = -1.0;
        self.highlights = None;
        self.loading = false;
        self.selection.clear();
        self.anchor = None;
    }

    pub fn rebuild_rows(&mut self, split: bool) {
        self.rows.clear();
        self.layout.get_mut().0 = -1.0;
        let Some(diff) = &self.diff else { return };
        for (index, hunk) in diff.hunks.iter().enumerate() {
            self.rows.push(DiffRow::Hunk(index));
            if split {
                let tags: Vec<char> = hunk.lines.iter().map(|line| line.t).collect();
                self.rows.extend(
                    logic::to_split(&tags)
                        .into_iter()
                        .map(|(left, right)| DiffRow::Split(index, left, right)),
                );
            } else {
                self.rows
                    .extend((0..hunk.lines.len()).map(|line| DiffRow::Line(index, line)));
            }
        }
    }

    pub fn selected_in(&self, hunk: usize) -> Vec<usize> {
        self.selection
            .iter()
            .filter(|(h, _)| *h == hunk)
            .map(|(_, line)| *line)
            .collect()
    }
}

pub enum DiffRequest {
    Working {
        path: String,
        staged: bool,
        orig: Option<String>,
    },
    Commit {
        sha: String,
        path: String,
        orig: Option<String>,
    },
    Compare {
        base: String,
        target: String,
        path: String,
    },
    Stash {
        name: String,
        path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Message,
    Author,
    Sha,
    Content,
}

impl SearchMode {
    pub const ALL: [Self; 4] = [Self::Message, Self::Author, Self::Sha, Self::Content];
    pub fn key(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Author => "author",
            Self::Sha => "sha",
            Self::Content => "content",
        }
    }
}

impl std::fmt::Display for SearchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Message => "Message",
            Self::Author => "Author",
            Self::Sha => "SHA",
            Self::Content => "Content",
        })
    }
}

pub struct SearchState {
    pub query: String,
    pub mode: SearchMode,
    pub results: Vec<Commit>,
    pub loading: bool,
}

pub struct CompareState {
    pub base: String,
    pub target: String,
    pub result: Option<Comparison>,
    pub error: Option<String>,
    pub selected: Option<String>,
}

pub struct StashState {
    pub name: String,
    pub files: Vec<FileChange>,
    pub selected: Option<String>,
}

/// One open repository.
pub struct RepoTab {
    pub path: String,
    pub name: String,
    pub view: View,
    pub status: Option<Status>,
    pub refs: Refs,
    pub commits: Vec<Commit>,
    pub has_more: bool,
    pub log_limit: usize,
    pub loading: bool,
    pub all_branches: bool,
    /// The most lanes any commit needs; the graph column is this wide for every row.
    pub graph_lanes: u32,
    pub selected: Option<String>,
    pub detail: Option<CommitDetail>,
    pub detail_error: Option<String>,
    pub selected_file: Option<String>,
    pub commit_pane: DiffPane,
    pub message: text_editor::Content,
    pub offset: f32,
    pub viewport: f32,
    pub list: Id,
    pub search: Option<SearchState>,
    pub search_input: String,
    pub search_mode: SearchMode,
    pub compare: Option<CompareState>,
    pub stash: Option<StashState>,
    pub focus: Option<(Area, String)>,
    pub picked: (Area, Vec<String>),
    pub pick_anchor: Option<String>,
    pub changes_pane: DiffPane,
    pub summary: String,
    pub body: text_editor::Content,
    pub amend: bool,
    pub committing: bool,
    pub filter: String,
}

impl RepoTab {
    fn new(path: String, log_limit: usize) -> Self {
        let name = std::path::Path::new(&path)
            .file_name()
            .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
        Self {
            path,
            name,
            view: View::History,
            status: None,
            refs: Refs::default(),
            commits: Vec::new(),
            has_more: false,
            log_limit,
            loading: true,
            all_branches: true,
            graph_lanes: 1,
            selected: None,
            detail: None,
            detail_error: None,
            selected_file: None,
            commit_pane: DiffPane::default(),
            message: text_editor::Content::new(),
            offset: 0.0,
            viewport: 300.0,
            list: Id::unique(),
            search: None,
            search_input: String::new(),
            search_mode: SearchMode::Message,
            compare: None,
            stash: None,
            focus: None,
            picked: (Area::Unstaged, Vec::new()),
            pick_anchor: None,
            changes_pane: DiffPane::default(),
            summary: String::new(),
            body: text_editor::Content::new(),
            amend: false,
            committing: false,
            filter: String::new(),
        }
    }

    /// Distinct files with any change.
    pub fn change_count(&self) -> usize {
        let Some(status) = &self.status else { return 0 };
        let mut paths: HashSet<&str> = HashSet::new();
        paths.extend(status.staged.iter().map(|file| file.path.as_str()));
        paths.extend(status.unstaged.iter().map(|file| file.path.as_str()));
        paths.extend(status.conflicts.iter().map(|file| file.path.as_str()));
        paths.len()
    }

    pub fn current_branch(&self) -> Option<&str> {
        self.status
            .as_ref()
            .and_then(|status| status.branch.head.as_deref())
    }

    pub fn visible(&self) -> &[Commit] {
        self.search
            .as_ref()
            .map_or(&self.commits, |search| &search.results)
    }

    pub fn files_in(&self, area: Area) -> Vec<FileChange> {
        let Some(status) = &self.status else {
            return Vec::new();
        };
        match area {
            Area::Unstaged => status.unstaged.clone(),
            Area::Staged => status.staged.clone(),
            Area::Conflicts => status
                .conflicts
                .iter()
                .map(|file| FileChange {
                    path: file.path.clone(),
                    status: "U".into(),
                    orig_path: None,
                })
                .collect(),
        }
    }

    pub fn can_commit(&self) -> bool {
        let Some(status) = &self.status else {
            return false;
        };
        if self.summary.trim().is_empty() || !status.conflicts.is_empty() || self.committing {
            return false;
        }
        self.amend
            || !status.staged.is_empty()
            || matches!(status.operation, Some(Operation::Merge { .. }))
    }

    fn pane(&mut self, key: PaneKey) -> Option<&mut DiffPane> {
        match key {
            PaneKey::Changes => Some(&mut self.changes_pane),
            PaneKey::Commit => Some(&mut self.commit_pane),
            PaneKey::Overlay => None,
        }
    }
}

pub struct ConflictState {
    pub path: String,
    pub data: Option<ConflictVersions>,
    pub error: Option<String>,
    pub choices: Vec<Option<Choice>>,
    pub result: text_editor::Content,
    pub edited: bool,
}

impl ConflictState {
    pub fn conflict_count(&self) -> usize {
        self.choices.len()
    }
    pub fn unresolved(&self) -> usize {
        self.choices
            .iter()
            .filter(|choice| choice.is_none())
            .count()
    }
}

pub struct RebaseStepState {
    pub commit: Commit,
    pub action: &'static str,
    pub message: String,
    /// The message body, loaded when the commit is reworded.
    pub body: Option<String>,
}

pub struct RebaseState {
    pub base: Option<String>,
    pub base_label: String,
    pub steps: Vec<RebaseStepState>,
    pub original: Vec<Commit>,
    pub loading: bool,
    pub error: Option<String>,
    pub autostash: bool,
    pub focused: usize,
    pub drag: Option<(usize, f32)>,
}

impl RebaseState {
    pub fn problem(&self) -> Option<&'static str> {
        let steps: Vec<(&str, bool, bool)> = self
            .steps
            .iter()
            .map(|step| {
                (
                    step.action,
                    !step.message.trim().is_empty(),
                    step.body.is_some(),
                )
            })
            .collect();
        logic::rebase_problem(&steps)
    }
    pub fn unchanged(&self) -> bool {
        self.steps.len() == self.original.len()
            && self
                .steps
                .iter()
                .zip(&self.original)
                .all(|(step, commit)| step.action == "pick" && step.commit.sha == commit.sha)
    }
    fn reset(&mut self) {
        self.steps = self
            .original
            .iter()
            .map(|commit| RebaseStepState {
                commit: commit.clone(),
                action: "pick",
                message: commit.subject.clone(),
                body: None,
            })
            .collect();
    }
    fn move_step(&mut self, from: usize, to: usize) {
        if from < self.steps.len() && to < self.steps.len() {
            let step = self.steps.remove(from);
            self.steps.insert(to, step);
            self.focused = to;
        }
    }
}

pub enum Overlay {
    Settings,
    Reflog {
        entries: Vec<ReflogEntry>,
        loading: bool,
        error: Option<String>,
    },
    Blame {
        path: String,
        rev: Option<String>,
        lines: Vec<BlameLine>,
        loading: bool,
        error: Option<String>,
        offset: f32,
        viewport: f32,
        hovered: Option<String>,
    },
    FileHistory {
        path: String,
        commits: Vec<Commit>,
        selected: Option<String>,
        pane: Box<DiffPane>,
        loading: bool,
        error: Option<String>,
    },
    Conflict(Box<ConflictState>),
    Rebase(Box<RebaseState>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub message: String,
    pub detail: Option<String>,
    expires: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Splitter {
    Sidebar,
    History,
    Details,
    Changes,
}

pub struct App {
    pub look: Look,
    pub settings: Settings,
    pub tabs: Vec<RepoTab>,
    pub active: usize,
    pub recent: Vec<RepoEntry>,
    pub toasts: Vec<Toast>,
    pub form: Option<Form>,
    pub menu: Option<Menu>,
    pub overlay: Option<Overlay>,
    pub busy: Option<(u64, String)>,
    /// Stops the running operation, when it's one that can safely stop.
    pub cancel: Option<iced::task::Handle>,
    pub progress_line: String,
    pub cursor: Point,
    pub window: Size,
    pub modifiers: keyboard::Modifiers,
    pub collapsed: HashSet<String>,
    pub search_box: Id,
    drag: Option<(Splitter, Point, f32)>,
    next_id: u64,
    pub(crate) config_dir: PathBuf,
    pub(crate) store: RepoStore,
}

#[derive(Debug, Clone)]
pub enum Message {
    OpenDialog,
    Picked(Option<PathBuf>),
    OpenPath(String),
    Located(Answer<String>),
    Forget(String),
    Activate(usize),
    Close(usize),
    MoveTab(usize, isize),
    Refresh,
    SetView(View),
    StatusLoaded(String, Answer<Status>),
    RefsLoaded(String, Answer<Refs>),
    LogLoaded(String, Answer<(Vec<Commit>, bool)>),
    LoadMore,
    SetBranchFilter(bool),
    Select(String),
    ShowCommit(String),
    ActivateCommit(String),
    DetailLoaded(String, String, Answer<CommitDetail>),
    MessageAction(text_editor::Action),
    PickFile(String),
    Scrolled(f32, f32),
    DiffLoaded(String, PaneKey, u64, Answer<Option<FileDiff>>),
    HighlightsLoaded(String, PaneKey, u64, Option<Arc<Highlights>>),
    DiffScrolled(PaneKey, f32, Size),
    DiffLineClicked(PaneKey, usize, usize),
    HunkAction(usize, &'static str),
    ToggleSplit,
    SearchInput(String),
    SearchModeChanged(SearchMode),
    SearchSubmit,
    SearchClear,
    SearchFocus,
    SearchDone(String, String, Answer<Vec<Commit>>),
    CompareStart(String, String),
    CompareDone(String, Answer<Comparison>),
    CompareSwap,
    CompareClose,
    OpenStash(String),
    StashFilesLoaded(String, String, Answer<Vec<FileChange>>),
    FilePicked(Area, String),
    FileActivated(Area, String),
    SummaryInput(String),
    BodyAction(text_editor::Action),
    AmendToggled(bool),
    HeadMessageLoaded(String, Answer<String>),
    TemplateLoaded(String, Answer<String>),
    CommitNow,
    FilterInput(String),
    ToggleCollapsed(String),
    Run(Command),
    PullChosen {
        remote: Option<String>,
        branch: Option<String>,
        mode: String,
        autostash: bool,
    },
    RebaseChosen {
        onto: String,
        autostash: bool,
    },
    Progress(u64, String),
    Cancel,
    Done(String, u64, Command, Answer<OpResult>),
    Action(Action),
    OpenMenu(MenuRequest),
    MenuResolved(MenuRequest, Point, Answer<Commit>),
    MenuHover(Option<usize>),
    CloseMenu,
    FromMenu(Box<Message>),
    FormText(usize, String),
    FormArea(usize, text_editor::Action),
    FormCheck(usize, bool),
    FormSelect(usize, String),
    FormSubmit,
    FormCancel,
    OverlayClose,
    OpenReflog,
    ReflogLoaded(Answer<Vec<ReflogEntry>>),
    OpenBlame(String, Option<String>),
    BlameLoaded(Answer<Vec<BlameLine>>),
    BlameScrolled(f32, f32),
    BlameHover(String),
    OpenFileHistory(String),
    FileHistoryLoaded(Answer<Vec<Commit>>),
    FileHistorySelect(String),
    OpenConflict(String),
    ConflictLoaded(Answer<ConflictVersions>),
    ConflictChoose(usize, Choice),
    ConflictChooseAll(Choice),
    ConflictEdit(text_editor::Action),
    ConflictSave,
    OpenRebase(Option<String>),
    RebaseLoaded(Answer<Vec<Commit>>),
    RebaseSetAction(usize, &'static str),
    RebaseMessage(usize, String),
    RebaseBodyLoaded(String, Answer<String>),
    RebaseMove(usize, isize),
    RebaseFocus(usize),
    RebaseDragStart(usize),
    RebaseAutostash(bool),
    RebaseReset,
    RebaseStart,
    OpenSettings,
    SetTheme(String),
    SetSplit(bool),
    SetContext(String),
    SetIgnoreWhitespace(bool),
    SetSyntax(bool),
    SetLogLimit(String),
    SetFetchOnOpen(bool),
    SetCommitGraph(bool),
    Zoom(f32),
    ThemeChanged,
    Copy(String),
    Reveal(String),
    OpenFile(String),
    Instance(String),
    Watched(String, WatchEvent),
    Key(keyboard::Key, keyboard::Modifiers),
    ModifiersChanged(keyboard::Modifiers),
    CursorMoved(Point),
    PointerReleased,
    SplitterPressed(Splitter),
    WindowResized(Size),
    CloseRequested(window::Id),
    Tick,
    DismissToast(u64),
    Quit,
}

impl App {
    pub fn boot(repo: Option<String>, look: Look) -> (Self, Task<Message>) {
        let store = RepoStore::open();
        let settings = Settings::load(&store.dir());
        let mut app = Self::new(look, store, settings);
        if let Err(problem) = gitcraft_core::runner::check_git() {
            app.toast(ToastKind::Error, problem, None);
        }
        let mut tasks: Vec<Task<Message>> = app
            .settings
            .tabs
            .clone()
            .into_iter()
            .map(|path| Task::done(Message::OpenPath(path)))
            .collect();
        if let Some(path) = repo {
            tasks.push(Task::done(Message::OpenPath(path)));
        }
        (app, Task::batch(tasks))
    }

    /// An app with a fixed look and a store of the caller's, without saved tabs.
    pub fn with_look(look: Look, store: RepoStore) -> Self {
        let settings = Settings::load(&store.dir());
        Self::new(look, store, settings)
    }

    fn new(look: Look, store: RepoStore, settings: Settings) -> Self {
        let recent = store.list();
        Self {
            look,
            collapsed: settings.collapsed.iter().cloned().collect(),
            window: Size::new(settings.window_width, settings.window_height),
            settings,
            tabs: Vec::new(),
            active: 0,
            recent,
            toasts: Vec::new(),
            form: None,
            menu: None,
            overlay: None,
            busy: None,
            cancel: None,
            progress_line: String::new(),
            cursor: Point::ORIGIN,
            modifiers: keyboard::Modifiers::empty(),
            search_box: Id::unique(),
            drag: None,
            next_id: 1,
            config_dir: store.dir(),
            store,
        }
    }

    pub fn title(&self) -> String {
        match self.tabs.get(self.active) {
            Some(tab) => format!("{} - GitCraft", tab.name),
            None => "GitCraft".to_owned(),
        }
    }

    pub fn theme(&self) -> Theme {
        self.look.theme.clone()
    }

    pub fn style(&self, _theme: &Theme) -> theme::Style {
        theme::Style {
            background_color: self.look.c.bg,
            text_color: self.look.c.text,
        }
    }

    pub fn scale(&self) -> f32 {
        self.settings.zoom
    }

    pub fn current(&self) -> Option<&RepoTab> {
        self.tabs.get(self.active)
    }

    pub(crate) fn current_mut(&mut self) -> Option<&mut RepoTab> {
        self.tabs.get_mut(self.active)
    }

    fn tab_mut(&mut self, path: &str) -> Option<&mut RepoTab> {
        self.tabs.iter_mut().find(|tab| tab.path == path)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut parts: Vec<Subscription<Message>> = self
            .tabs
            .iter()
            .map(|tab| Subscription::run_with(tab.path.clone(), watch_stream))
            .collect();
        parts.push(keyboard::listen().filter_map(|event| match event {
            keyboard::Event::KeyPressed { key, modifiers, .. } => {
                Some(Message::Key(key, modifiers))
            }
            keyboard::Event::ModifiersChanged(modifiers) => {
                Some(Message::ModifiersChanged(modifiers))
            }
            keyboard::Event::KeyReleased { .. } => None,
        }));
        // Global, because dialogs capture mouse events on their way to the page below.
        parts.push(iced::event::listen_with(|event, _, _| match event {
            iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                Some(Message::CursorMoved(position))
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(Message::PointerReleased)
            }
            _ => None,
        }));
        parts.push(window::resize_events().map(|(_, size)| Message::WindowResized(size)));
        parts.push(window::close_requests().map(Message::CloseRequested));
        parts.push(native_theme_iced::theme_changes().map(|_| Message::ThemeChanged));
        parts.push(Subscription::run(crate::instance::listen).map(Message::Instance));
        if !self.toasts.is_empty() {
            parts.push(iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick));
        }
        Subscription::batch(parts)
    }

    pub fn view(&self) -> Element<'_, Message> {
        views::root(self)
    }

    pub fn toast(&mut self, kind: ToastKind, message: impl Into<String>, detail: Option<String>) {
        let seconds = match kind {
            ToastKind::Error => 12,
            ToastKind::Warning => 8,
            _ => 4,
        };
        let id = self.next_id;
        self.next_id += 1;
        self.toasts.push(Toast {
            id,
            kind,
            message: message.into(),
            detail,
            expires: Instant::now() + Duration::from_secs(seconds),
        });
    }

    fn save_settings(&mut self) {
        self.settings.tabs = self.tabs.iter().map(|tab| tab.path.clone()).collect();
        self.settings.active_tab = self.active;
        self.settings.collapsed = self.collapsed.iter().cloned().collect();
        self.settings.collapsed.sort();
        if let Err(error) = self.settings.save(&self.config_dir) {
            self.toast(
                ToastKind::Error,
                format!("Could not save settings: {error}"),
                None,
            );
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenDialog => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Open repository")
                        .pick_folder()
                        .await
                        .map(|folder| folder.path().to_path_buf())
                },
                Message::Picked,
            ),
            Message::Picked(Some(folder)) => {
                Task::done(Message::OpenPath(folder.to_string_lossy().into_owned()))
            }
            Message::Picked(None) => Task::none(),
            Message::OpenPath(path) | Message::Instance(path) => Task::perform(
                async move { answer(repos::locate(&path).await) },
                Message::Located,
            ),
            Message::Located(Ok(path)) => self.opened(path),
            Message::Located(Err(error)) => {
                self.toast(ToastKind::Error, error, None);
                Task::none()
            }
            Message::Forget(path) => {
                if let Err(error) = self.store.remove(&path) {
                    self.toast(ToastKind::Error, error.to_string(), None);
                }
                self.recent = self.store.list();
                Task::none()
            }
            Message::Activate(index) => {
                if index >= self.tabs.len() {
                    return Task::none();
                }
                self.active = index;
                self.save_settings();
                let tab = &self.tabs[index];
                Task::batch([
                    scroll_to(tab, tab.offset),
                    scroll_pane(&tab.commit_pane, tab.commit_pane.offset),
                    scroll_pane(&tab.changes_pane, tab.changes_pane.offset),
                ])
            }
            Message::Close(index) => {
                if index < self.tabs.len() {
                    self.tabs.remove(index);
                    if index < self.active {
                        self.active -= 1;
                    }
                    self.active = self.active.min(self.tabs.len().saturating_sub(1));
                    self.save_settings();
                }
                Task::none()
            }
            Message::MoveTab(index, delta) => {
                let target = index.saturating_add_signed(delta);
                if target < self.tabs.len() && index < self.tabs.len() && target != index {
                    let active_path = self.current().map(|tab| tab.path.clone());
                    let tab = self.tabs.remove(index);
                    self.tabs.insert(target, tab);
                    self.active = self
                        .tabs
                        .iter()
                        .position(|tab| Some(&tab.path) == active_path.as_ref())
                        .unwrap_or(0);
                    self.save_settings();
                }
                Task::none()
            }
            Message::Refresh => match self.current() {
                Some(tab) => load_all(tab.path.clone(), tab.log_limit, tab.all_branches),
                None => Task::none(),
            },
            Message::SetView(view) => {
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                tab.view = view;
                if view == View::Changes {
                    self.reload_changes()
                } else {
                    Task::none()
                }
            }
            Message::StatusLoaded(path, result) => self.status_loaded(&path, result),
            Message::RefsLoaded(path, result) => {
                match result {
                    Ok(refs) => {
                        if let Some(tab) = self.tab_mut(&path) {
                            if let Some(stash) = &tab.stash {
                                let name = stash.name.clone();
                                if !refs.stashes.iter().any(|entry| entry.name == name) {
                                    tab.stash = None;
                                    if tab.view == View::Stash {
                                        tab.view = View::History;
                                    }
                                }
                            }
                            tab.refs = refs;
                        }
                    }
                    Err(error) => self.toast(ToastKind::Error, error, None),
                }
                Task::none()
            }
            Message::LogLoaded(path, result) => self.log_loaded(&path, result),
            Message::LoadMore => {
                let step = self.settings.log_limit;
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                tab.log_limit += step;
                tab.loading = true;
                load_log(tab.path.clone(), tab.log_limit, tab.all_branches)
            }
            Message::SetBranchFilter(all) => {
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                tab.all_branches = all;
                tab.loading = true;
                tab.offset = 0.0;
                Task::batch([
                    scroll_to(tab, 0.0),
                    load_log(tab.path.clone(), tab.log_limit, all),
                ])
            }
            Message::Select(sha) => {
                // Ctrl/Cmd+click compares the selected commit with the clicked one.
                if self.modifiers.command() {
                    let selected = self.current().and_then(|tab| tab.selected.clone());
                    if let Some(selected) = selected.filter(|selected| *selected != sha) {
                        return Task::done(Message::CompareStart(selected, sha));
                    }
                }
                if let Some(tab) = self.current_mut() {
                    tab.compare = None;
                }
                self.select(sha)
            }
            Message::ShowCommit(sha) => {
                self.overlay = None;
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                tab.view = View::History;
                tab.compare = None;
                tab.search = None;
                let index = tab.commits.iter().position(|commit| commit.sha == sha);
                let reveal = index.map_or_else(Task::none, |index| self.reveal(index));
                Task::batch([self.select(sha), reveal])
            }
            Message::ActivateCommit(sha) => {
                let Some(tab) = self.current() else {
                    return Task::none();
                };
                let branch = tab
                    .visible()
                    .iter()
                    .find(|commit| commit.sha == sha)
                    .and_then(|commit| commit.refs.as_ref())
                    .and_then(|refs| {
                        refs.iter()
                            .find(|label| label.kind == "local" && !label.current)
                    });
                match branch {
                    Some(branch) => {
                        let command = Command::Checkout {
                            reference: branch.name.clone(),
                            kind: "local",
                        };
                        Task::done(Message::Run(command))
                    }
                    None => Task::none(),
                }
            }
            Message::DetailLoaded(path, sha, result) => self.detail_loaded(&path, &sha, result),
            Message::MessageAction(action) => {
                // Selectable and copyable, but not editable.
                if !action.is_edit()
                    && let Some(tab) = self.current_mut()
                {
                    tab.message.perform(action);
                }
                Task::none()
            }
            Message::PickFile(path) => self.pick_file(path),
            Message::Scrolled(offset, height) => {
                if let Some(tab) = self.current_mut() {
                    tab.offset = offset;
                    tab.viewport = height;
                }
                Task::none()
            }
            Message::DiffLoaded(repo, key, token, result) => {
                self.diff_loaded(&repo, key, token, result)
            }
            Message::HighlightsLoaded(repo, key, token, highlights) => {
                let pane = self.pane_mut(&repo, key);
                if let Some(pane) = pane.filter(|pane| pane.token == token) {
                    pane.highlights = highlights;
                }
                Task::none()
            }
            Message::DiffScrolled(key, offset, viewport) => {
                let repo = self
                    .current()
                    .map(|tab| tab.path.clone())
                    .unwrap_or_default();
                if let Some(pane) = self.pane_mut(&repo, key) {
                    pane.offset = offset;
                    pane.viewport = viewport;
                }
                Task::none()
            }
            Message::DiffLineClicked(key, hunk, line) => {
                let shift = self.modifiers.shift();
                let repo = self
                    .current()
                    .map(|tab| tab.path.clone())
                    .unwrap_or_default();
                if let Some(pane) = self.pane_mut(&repo, key) {
                    click_line(pane, hunk, line, shift);
                }
                Task::none()
            }
            Message::HunkAction(hunk, mode) => self.hunk_action(hunk, mode),
            Message::ToggleSplit => {
                let split = !self.settings.diff_split;
                self.set_split(split);
                Task::none()
            }
            Message::SearchInput(text) => {
                if let Some(tab) = self.current_mut() {
                    tab.search_input = text;
                }
                Task::none()
            }
            Message::SearchModeChanged(mode) => {
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                tab.search_mode = mode;
                if tab.search_input.trim().is_empty() {
                    Task::none()
                } else {
                    self.run_search()
                }
            }
            Message::SearchSubmit => self.run_search(),
            Message::SearchClear => {
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                let had_results = tab.search.take().is_some();
                tab.search_input.clear();
                if !had_results {
                    return Task::none();
                }
                tab.offset = 0.0;
                scroll_to(tab, 0.0)
            }
            Message::SearchFocus => {
                let id = self.search_box.clone();
                Task::batch([operation::focus(id.clone()), operation::select_all(id)])
            }
            Message::SearchDone(repo, query, result) => self.search_done(&repo, &query, result),
            Message::CompareStart(base, target) => self.compare_start(base, target),
            Message::CompareDone(repo, result) => self.compare_done(&repo, result),
            Message::CompareSwap => {
                let Some(compare) = self.current_mut().and_then(|tab| tab.compare.take()) else {
                    return Task::none();
                };
                self.compare_start(compare.target, compare.base)
            }
            Message::CompareClose => {
                if let Some(tab) = self.current_mut() {
                    tab.compare = None;
                }
                self.reload_commit_pane()
            }
            Message::OpenStash(name) => self.open_stash(name),
            Message::StashFilesLoaded(repo, name, result) => {
                self.stash_files_loaded(&repo, &name, result)
            }
            Message::FilePicked(area, path) => self.file_picked(area, path),
            Message::FileActivated(area, path) => match area {
                Area::Unstaged => Task::done(Message::Run(Command::Stage { paths: vec![path] })),
                Area::Staged => Task::done(Message::Run(Command::Unstage { paths: vec![path] })),
                Area::Conflicts => Task::done(Message::OpenConflict(path)),
            },
            Message::SummaryInput(text) => {
                if let Some(tab) = self.current_mut() {
                    tab.summary = text;
                }
                Task::none()
            }
            Message::BodyAction(action) => {
                if let Some(tab) = self.current_mut() {
                    tab.body.perform(action);
                }
                Task::none()
            }
            Message::AmendToggled(amend) => {
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                tab.amend = amend;
                if !amend || !tab.summary.is_empty() || !tab.body.text().trim().is_empty() {
                    return Task::none();
                }
                let path = tab.path.clone();
                Task::perform(
                    async move {
                        let result = answer(queries::head_message(&path).await);
                        (path, result)
                    },
                    |(path, result)| Message::HeadMessageLoaded(path, result),
                )
            }
            Message::HeadMessageLoaded(repo, result) | Message::TemplateLoaded(repo, result) => {
                match result {
                    Ok(message) => {
                        if let Some(tab) = self.tab_mut(&repo)
                            && tab.summary.is_empty()
                            && tab.body.text().trim().is_empty()
                        {
                            let (summary, body) = logic::split_message(&message);
                            tab.summary = summary;
                            tab.body = text_editor::Content::with_text(&body);
                        }
                    }
                    Err(error) => self.toast(ToastKind::Error, error, None),
                }
                Task::none()
            }
            Message::CommitNow => {
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                if !tab.can_commit() {
                    return Task::none();
                }
                tab.committing = true;
                let message = logic::commit_message(&tab.summary, &tab.body.text());
                let amend = tab.amend;
                Task::done(Message::Run(Command::Commit { message, amend }))
            }
            Message::FilterInput(text) => {
                if let Some(tab) = self.current_mut() {
                    tab.filter = text;
                }
                Task::none()
            }
            Message::ToggleCollapsed(key) => {
                if !self.collapsed.remove(&key) {
                    self.collapsed.insert(key);
                }
                self.save_settings();
                Task::none()
            }
            Message::Run(command) => self.run_command(command),
            Message::PullChosen {
                remote,
                branch,
                mode,
                autostash,
            } => {
                self.settings.pull_mode = mode.clone();
                self.settings.pull_autostash = autostash;
                self.save_settings();
                self.run_command(Command::Pull {
                    remote,
                    branch,
                    mode,
                    autostash,
                })
            }
            Message::RebaseChosen { onto, autostash } => {
                self.settings.rebase_autostash = autostash;
                self.save_settings();
                self.run_command(Command::Rebase { onto, autostash })
            }
            Message::Progress(id, line) => {
                if self.busy.as_ref().is_some_and(|(busy, _)| *busy == id) {
                    self.progress_line = line;
                }
                Task::none()
            }
            Message::Cancel => {
                // Dropping the running task kills its git process.
                if let Some(handle) = self.cancel.take() {
                    handle.abort();
                    self.busy = None;
                    self.progress_line.clear();
                    self.toast(ToastKind::Info, "Cancelled", None);
                    return Task::done(Message::Refresh);
                }
                Task::none()
            }
            Message::Done(repo, id, command, result) => {
                self.command_done(repo, id, command, result)
            }
            Message::Action(action) => self.perform(action),
            Message::OpenMenu(request) => self.open_menu(request),
            Message::MenuResolved(request, at, result) => match result {
                Ok(commit) => {
                    self.show_commit_menu(request, &commit, at);
                    Task::none()
                }
                Err(error) => {
                    self.toast(ToastKind::Error, "Commit not found", Some(error));
                    Task::none()
                }
            },
            Message::MenuHover(index) => {
                if let Some(menu) = &mut self.menu {
                    menu.open_sub = index;
                }
                Task::none()
            }
            Message::CloseMenu => {
                self.menu = None;
                Task::none()
            }
            Message::FromMenu(inner) => {
                self.menu = None;
                self.update(*inner)
            }
            Message::FormText(index, text) => {
                if let Some(FieldKind::Text(value)) = self
                    .form
                    .as_mut()
                    .and_then(|form| form.fields.get_mut(index))
                    .map(|field| &mut field.kind)
                {
                    *value = text;
                }
                Task::none()
            }
            Message::FormArea(index, action) => {
                if let Some(FieldKind::Area(content)) = self
                    .form
                    .as_mut()
                    .and_then(|form| form.fields.get_mut(index))
                    .map(|field| &mut field.kind)
                {
                    content.perform(action);
                }
                Task::none()
            }
            Message::FormCheck(index, checked) => {
                if let Some(FieldKind::Check(value)) = self
                    .form
                    .as_mut()
                    .and_then(|form| form.fields.get_mut(index))
                    .map(|field| &mut field.kind)
                {
                    *value = checked;
                }
                Task::none()
            }
            Message::FormSelect(index, chosen) => {
                if let Some(FieldKind::Select { value, .. }) = self
                    .form
                    .as_mut()
                    .and_then(|form| form.fields.get_mut(index))
                    .map(|field| &mut field.kind)
                {
                    *value = chosen;
                }
                Task::none()
            }
            Message::FormSubmit => {
                let Some(form) = self.form.take_if(|form| form.can_submit()) else {
                    return Task::none();
                };
                let next = (form.on_ok)(&form.values());
                self.update(next)
            }
            Message::FormCancel => {
                self.form = None;
                Task::none()
            }
            Message::OverlayClose => {
                self.overlay = None;
                Task::none()
            }
            Message::OpenReflog => self.open_reflog(),
            Message::ReflogLoaded(result) => {
                if let Some(Overlay::Reflog {
                    entries,
                    loading,
                    error,
                }) = &mut self.overlay
                {
                    *loading = false;
                    match result {
                        Ok(list) => *entries = list,
                        Err(message) => *error = Some(message),
                    }
                }
                Task::none()
            }
            Message::OpenBlame(path, rev) => self.open_blame(path, rev),
            Message::BlameLoaded(result) => {
                if let Some(Overlay::Blame {
                    lines,
                    loading,
                    error,
                    ..
                }) = &mut self.overlay
                {
                    *loading = false;
                    match result {
                        Ok(list) => *lines = list,
                        Err(message) => *error = Some(message),
                    }
                }
                Task::none()
            }
            Message::BlameScrolled(y, height) => {
                if let Some(Overlay::Blame {
                    offset, viewport, ..
                }) = &mut self.overlay
                {
                    *offset = y;
                    *viewport = height;
                }
                Task::none()
            }
            Message::BlameHover(sha) => {
                if let Some(Overlay::Blame { hovered, .. }) = &mut self.overlay {
                    *hovered = Some(sha);
                }
                Task::none()
            }
            Message::OpenFileHistory(path) => self.open_file_history(path),
            Message::FileHistoryLoaded(result) => self.file_history_loaded(result),
            Message::FileHistorySelect(sha) => self.file_history_select(sha),
            Message::OpenConflict(path) => self.open_conflict(path),
            Message::ConflictLoaded(result) => {
                if let Some(Overlay::Conflict(state)) = &mut self.overlay {
                    match result {
                        Ok(data) => {
                            let count = data
                                .chunks
                                .iter()
                                .filter(|chunk| matches!(chunk, ConflictChunk::Conflict { .. }))
                                .count();
                            state.choices = vec![None; count];
                            state.result = text_editor::Content::with_text(
                                data.working.as_deref().unwrap_or(""),
                            );
                            state.data = Some(data);
                        }
                        Err(message) => state.error = Some(message),
                    }
                }
                Task::none()
            }
            Message::ConflictChoose(index, choice) => {
                self.conflict_choose(Some(index), choice, false)
            }
            Message::ConflictChooseAll(choice) => self.conflict_choose(None, choice, false),
            Message::ConflictEdit(action) => {
                if let Some(Overlay::Conflict(state)) = &mut self.overlay {
                    state.edited |= action.is_edit();
                    state.result.perform(action);
                }
                Task::none()
            }
            Message::ConflictSave => self.conflict_save(),
            Message::OpenRebase(base) => self.open_rebase(base),
            Message::RebaseLoaded(result) => {
                if let Some(Overlay::Rebase(state)) = &mut self.overlay {
                    state.loading = false;
                    match result {
                        Ok(commits) => {
                            state.original = commits;
                            state.reset();
                        }
                        Err(message) => state.error = Some(message),
                    }
                }
                Task::none()
            }
            Message::RebaseSetAction(index, action) => self.rebase_set_action(index, action),
            Message::RebaseMessage(index, text) => {
                if let Some(Overlay::Rebase(state)) = &mut self.overlay
                    && let Some(step) = state.steps.get_mut(index)
                {
                    step.message = text;
                    state.focused = index;
                }
                Task::none()
            }
            Message::RebaseBodyLoaded(sha, result) => {
                match result {
                    Ok(body) => {
                        if let Some(Overlay::Rebase(state)) = &mut self.overlay {
                            for step in state.steps.iter_mut().filter(|step| step.commit.sha == sha)
                            {
                                step.body = Some(body.clone());
                            }
                        }
                    }
                    Err(error) => self.toast(
                        ToastKind::Error,
                        "Could not load the commit message",
                        Some(error),
                    ),
                }
                Task::none()
            }
            Message::RebaseMove(index, delta) => {
                if let Some(Overlay::Rebase(state)) = &mut self.overlay {
                    state.move_step(index, index.saturating_add_signed(delta));
                }
                Task::none()
            }
            Message::RebaseFocus(index) => {
                if let Some(Overlay::Rebase(state)) = &mut self.overlay {
                    state.focused = index;
                }
                Task::none()
            }
            Message::RebaseDragStart(index) => {
                let y = self.cursor.y;
                if let Some(Overlay::Rebase(state)) = &mut self.overlay {
                    state.drag = Some((index, y));
                    state.focused = index;
                }
                Task::none()
            }
            Message::RebaseAutostash(value) => {
                if let Some(Overlay::Rebase(state)) = &mut self.overlay {
                    state.autostash = value;
                }
                Task::none()
            }
            Message::RebaseReset => {
                if let Some(Overlay::Rebase(state)) = &mut self.overlay {
                    state.reset();
                }
                Task::none()
            }
            Message::RebaseStart => self.rebase_start(),
            Message::OpenSettings => {
                self.overlay = Some(Overlay::Settings);
                Task::none()
            }
            Message::SetTheme(choice) => {
                self.settings.theme = choice;
                self.look = Look::for_setting(&self.settings.theme);
                self.save_settings();
                Task::none()
            }
            Message::ThemeChanged => {
                native_theme_iced::native_theme::detect::invalidate_caches();
                self.look = Look::for_setting(&self.settings.theme);
                Task::none()
            }
            Message::SetSplit(split) => {
                self.set_split(split);
                Task::none()
            }
            Message::SetContext(text) => {
                self.settings.diff_context = text
                    .trim()
                    .parse::<i64>()
                    .map_or(self.settings.diff_context, |n| n.clamp(0, 100));
                self.save_settings();
                self.reload_diffs()
            }
            Message::SetIgnoreWhitespace(value) => {
                self.settings.ignore_whitespace = value;
                self.save_settings();
                self.reload_diffs()
            }
            Message::SetSyntax(value) => {
                self.settings.syntax_highlight = value;
                self.save_settings();
                self.reload_diffs()
            }
            Message::SetLogLimit(text) => {
                self.settings.log_limit = text
                    .trim()
                    .parse::<usize>()
                    .map_or(self.settings.log_limit, |n| n.clamp(100, 100_000));
                self.save_settings();
                Task::none()
            }
            Message::SetCommitGraph(value) => {
                self.settings.commit_graph = value;
                self.save_settings();
                Task::none()
            }
            Message::SetFetchOnOpen(value) => {
                self.settings.fetch_on_open = value;
                self.save_settings();
                Task::none()
            }
            Message::Zoom(factor) => {
                self.settings.zoom = if factor == 0.0 {
                    1.0
                } else {
                    (self.settings.zoom * factor).clamp(0.5, 3.0)
                };
                self.save_settings();
                Task::none()
            }
            Message::Copy(text) => {
                self.toast(ToastKind::Success, "Copied to clipboard", None);
                iced::clipboard::write(text)
            }
            Message::Reveal(path) => {
                if let Err(error) = opener::reveal(&path) {
                    self.toast(
                        ToastKind::Error,
                        "Could not show the file",
                        Some(error.to_string()),
                    );
                }
                Task::none()
            }
            Message::OpenFile(path) => {
                if let Err(error) = opener::open(&path) {
                    self.toast(
                        ToastKind::Error,
                        "Could not open the file",
                        Some(error.to_string()),
                    );
                }
                Task::none()
            }
            Message::Watched(path, WatchEvent::Changed { git, worktree: _ }) => {
                let found = self
                    .tabs
                    .iter()
                    .find(|tab| tab.path == path)
                    .map(|tab| (tab.log_limit, tab.all_branches));
                let Some((limit, all)) = found else {
                    return Task::none();
                };
                if git {
                    load_all(path, limit, all)
                } else {
                    load_status(path)
                }
            }
            Message::Watched(path, WatchEvent::Error(error)) => {
                self.toast(
                    ToastKind::Warning,
                    format!("Live refresh stopped for {path}"),
                    Some(error),
                );
                Task::none()
            }
            Message::Key(key, modifiers) => self.key(&key, modifiers),
            Message::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
                Task::none()
            }
            Message::CursorMoved(position) => {
                self.cursor = position;
                if let Some((splitter, start, size)) = self.drag {
                    self.drag_splitter(splitter, start, size, position);
                }
                Task::none()
            }
            Message::PointerReleased => {
                if self.drag.take().is_some() {
                    self.save_settings();
                }
                let y = self.cursor.y;
                if let Some(Overlay::Rebase(state)) = &mut self.overlay
                    && let Some((from, start)) = state.drag.take()
                {
                    let rows = ((y - start) / REBASE_ROW_HEIGHT).round() as isize;
                    let last = state.steps.len().saturating_sub(1) as isize;
                    let to = (from as isize + rows).clamp(0, last) as usize;
                    state.move_step(from, to);
                }
                Task::none()
            }
            Message::SplitterPressed(splitter) => {
                let size = match splitter {
                    Splitter::Sidebar => self.settings.sidebar_width,
                    Splitter::History => self.settings.history_height,
                    Splitter::Details => self.settings.details_width,
                    Splitter::Changes => self.settings.changes_width,
                };
                self.drag = Some((splitter, self.cursor, size));
                Task::none()
            }
            Message::WindowResized(size) => {
                self.window = size;
                self.settings.window_width = size.width;
                self.settings.window_height = size.height;
                Task::none()
            }
            Message::CloseRequested(id) => {
                self.save_settings();
                window::close(id)
            }
            Message::Tick => {
                let now = Instant::now();
                self.toasts.retain(|toast| toast.expires > now);
                Task::none()
            }
            Message::DismissToast(id) => {
                self.toasts.retain(|toast| toast.id != id);
                Task::none()
            }
            Message::Quit => {
                self.save_settings();
                iced::exit()
            }
        }
    }

    fn opened(&mut self, path: String) -> Task<Message> {
        if let Err(error) = self.store.add(&path) {
            self.toast(
                ToastKind::Error,
                format!("Could not save the recent list: {error}"),
                None,
            );
        }
        self.recent = self.store.list();
        if let Some(index) = self.tabs.iter().position(|tab| tab.path == path) {
            self.active = index;
            self.save_settings();
            return Task::none();
        }
        let limit = self.settings.log_limit;
        let mut tab = RepoTab::new(path.clone(), limit);
        tab.viewport = self.history_list_height();
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.save_settings();
        let template = {
            let repo = path.clone();
            Task::perform(
                async move {
                    let result = answer(queries::commit_template(&repo).await);
                    (repo, result)
                },
                |(repo, result)| Message::TemplateLoaded(repo, result),
            )
        };
        let mut tasks = vec![load_all(path, limit, true), template];
        if self.settings.fetch_on_open {
            tasks.push(Task::done(Message::Run(Command::Fetch {
                remote: None,
                tags: false,
            })));
        }
        if self.settings.commit_graph {
            let repo = self.tabs[self.active].path.clone();
            // Best effort, and it only makes history faster: a failure changes nothing.
            tasks.push(Task::future(async move {
                let _ = ops::ensure_commit_graph(&repo).await;
                Message::Tick
            }));
        }
        Task::batch(tasks)
    }

    fn pane_mut(&mut self, repo: &str, key: PaneKey) -> Option<&mut DiffPane> {
        if key == PaneKey::Overlay {
            return match &mut self.overlay {
                Some(Overlay::FileHistory { pane, .. }) => Some(pane),
                _ => None,
            };
        }
        self.tab_mut(repo).and_then(|tab| tab.pane(key))
    }

    fn set_split(&mut self, split: bool) {
        self.settings.diff_split = split;
        for tab in &mut self.tabs {
            tab.changes_pane.rebuild_rows(split);
            tab.commit_pane.rebuild_rows(split);
        }
        if let Some(Overlay::FileHistory { pane, .. }) = &mut self.overlay {
            pane.rebuild_rows(split);
        }
        self.save_settings();
    }

    /// Roughly the commit list's height: the top pane without its header rows.
    fn history_list_height(&self) -> f32 {
        (self.settings.history_height - 60.0).max(ROW_HEIGHT)
    }

    fn drag_splitter(&mut self, splitter: Splitter, start: Point, size: f32, position: Point) {
        let (dx, dy) = (position.x - start.x, position.y - start.y);
        match splitter {
            Splitter::Sidebar => self.settings.sidebar_width = (size + dx).clamp(160.0, 600.0),
            Splitter::History => {
                self.settings.history_height = (size + dy).clamp(120.0, 2000.0);
                let height = self.history_list_height();
                self.tabs.iter_mut().for_each(|tab| tab.viewport = height);
            }
            Splitter::Details => self.settings.details_width = (size + dx).clamp(220.0, 900.0),
            Splitter::Changes => self.settings.changes_width = (size + dx).clamp(260.0, 900.0),
        }
    }

    // ------------------------------------------------------------ loading

    fn status_loaded(&mut self, path: &str, result: Answer<Status>) -> Task<Message> {
        match result {
            Ok(status) => {
                let Some(tab) = self.tab_mut(path) else {
                    return Task::none();
                };
                tab.status = Some(status);
                ensure_focus(tab);
                if tab.view == View::Changes && self.current().is_some_and(|tab| tab.path == path) {
                    return self.reload_changes();
                }
            }
            Err(error) => self.toast(ToastKind::Error, error, None),
        }
        Task::none()
    }

    fn log_loaded(&mut self, path: &str, result: Answer<(Vec<Commit>, bool)>) -> Task<Message> {
        let (commits, has_more) = match result {
            Ok(page) => page,
            Err(error) => {
                if let Some(tab) = self.tab_mut(path) {
                    tab.loading = false;
                }
                self.toast(ToastKind::Error, error, None);
                return Task::none();
            }
        };
        let is_current = self.current().is_some_and(|tab| tab.path == path);
        let Some(tab) = self.tab_mut(path) else {
            return Task::none();
        };
        tab.graph_lanes = commits
            .iter()
            .filter_map(|commit| commit.graph.as_ref().map(|graph| graph.width))
            .max()
            .unwrap_or(1);
        tab.commits = commits;
        tab.has_more = has_more;
        tab.loading = false;
        let still_there = tab
            .selected
            .as_ref()
            .is_some_and(|sha| tab.commits.iter().any(|commit| commit.sha == *sha));
        if !still_there && tab.search.is_none() {
            let head = tab.refs.head.clone();
            let first = tab
                .commits
                .iter()
                .find(|commit| Some(&commit.sha) == head.as_ref())
                .or(tab.commits.first())
                .map(|commit| commit.sha.clone());
            tab.selected = None;
            tab.detail = None;
            tab.message = text_editor::Content::new();
            if let Some(first) = first.filter(|_| is_current) {
                return self.select(first);
            }
        }
        Task::none()
    }

    fn select(&mut self, sha: String) -> Task<Message> {
        let Some(tab) = self.current_mut() else {
            return Task::none();
        };
        tab.selected = Some(sha.clone());
        tab.detail_error = None;
        let path = tab.path.clone();
        Task::perform(
            async move {
                let result = answer(queries::commit_detail(&path, &sha).await);
                (path, sha, result)
            },
            |(path, sha, result)| Message::DetailLoaded(path, sha, result),
        )
    }

    fn detail_loaded(
        &mut self,
        path: &str,
        sha: &str,
        result: Answer<CommitDetail>,
    ) -> Task<Message> {
        let Some(tab) = self.tab_mut(path) else {
            return Task::none();
        };
        if tab.selected.as_deref() != Some(sha) {
            return Task::none();
        }
        match result {
            Ok(detail) => {
                let keep = tab
                    .selected_file
                    .as_ref()
                    .filter(|file| detail.files.iter().any(|f| f.path == **file))
                    .cloned();
                tab.selected_file =
                    keep.or_else(|| detail.files.first().map(|file| file.path.clone()));
                tab.message = text_editor::Content::with_text(&detail.message);
                tab.detail = Some(detail);
                self.reload_commit_pane()
            }
            Err(error) => {
                tab.detail = None;
                tab.detail_error = Some(error);
                tab.commit_pane.clear();
                Task::none()
            }
        }
    }

    fn pick_file(&mut self, path: String) -> Task<Message> {
        let Some(tab) = self.current_mut() else {
            return Task::none();
        };
        if let Some(compare) = tab.compare.as_mut() {
            compare.selected = Some(path);
        } else if tab.view == View::Stash {
            if let Some(stash) = tab.stash.as_mut() {
                stash.selected = Some(path);
            }
        } else {
            tab.selected_file = Some(path);
        }
        self.reload_commit_pane()
    }

    /// Loads the diff shown under the history or in the stash view.
    fn reload_commit_pane(&mut self) -> Task<Message> {
        let Some(tab) = self.current() else {
            return Task::none();
        };
        let request = if let Some(compare) = &tab.compare {
            compare.selected.as_ref().map(|path| DiffRequest::Compare {
                base: compare.base.clone(),
                target: compare.target.clone(),
                path: path.clone(),
            })
        } else if tab.view == View::Stash {
            tab.stash.as_ref().and_then(|stash| {
                stash.selected.as_ref().map(|path| DiffRequest::Stash {
                    name: stash.name.clone(),
                    path: path.clone(),
                })
            })
        } else {
            tab.detail.as_ref().and_then(|detail| {
                let file = tab
                    .selected_file
                    .as_ref()
                    .and_then(|path| detail.files.iter().find(|file| file.path == *path))?;
                Some(DiffRequest::Commit {
                    sha: detail.sha.clone(),
                    path: file.path.clone(),
                    orig: file.orig_path.clone(),
                })
            })
        };
        match request {
            Some(request) => self.load_diff(PaneKey::Commit, request),
            None => {
                if let Some(tab) = self.current_mut() {
                    tab.commit_pane.clear();
                }
                Task::none()
            }
        }
    }

    pub(crate) fn reload_changes(&mut self) -> Task<Message> {
        let Some(tab) = self.current() else {
            return Task::none();
        };
        let request = match &tab.focus {
            Some((Area::Conflicts, _)) | None => None,
            Some((area, path)) => {
                let staged = *area == Area::Staged;
                let file = tab
                    .files_in(*area)
                    .into_iter()
                    .find(|file| file.path == *path);
                file.map(|file| DiffRequest::Working {
                    path: file.path,
                    staged,
                    orig: if staged { file.orig_path } else { None },
                })
            }
        };
        match request {
            Some(request) => self.load_diff(PaneKey::Changes, request),
            None => {
                if let Some(tab) = self.current_mut() {
                    tab.changes_pane.clear();
                }
                Task::none()
            }
        }
    }

    fn reload_diffs(&mut self) -> Task<Message> {
        Task::batch([self.reload_commit_pane(), self.reload_changes()])
    }

    fn load_diff(&mut self, key: PaneKey, request: DiffRequest) -> Task<Message> {
        let (context, ignore_ws) = (self.settings.diff_context, self.settings.ignore_whitespace);
        let Some(repo) = self.current().map(|tab| tab.path.clone()) else {
            return Task::none();
        };
        let Some(pane) = self.pane_mut(&repo, key) else {
            return Task::none();
        };
        pane.token += 1;
        pane.loading = pane.diff.is_none();
        let token = pane.token;
        Task::perform(
            async move {
                let result = match request {
                    DiffRequest::Working { path, staged, orig } => {
                        queries::working_diff(
                            &repo,
                            &path,
                            staged,
                            context,
                            ignore_ws,
                            orig.as_deref(),
                        )
                        .await
                    }
                    DiffRequest::Commit { sha, path, orig } => {
                        queries::commit_diff(
                            &repo,
                            &sha,
                            &path,
                            orig.as_deref(),
                            context,
                            ignore_ws,
                        )
                        .await
                    }
                    DiffRequest::Compare { base, target, path } => {
                        queries::range_diff(&repo, &base, &target, &path, context).await
                    }
                    DiffRequest::Stash { name, path } => {
                        queries::stash_diff(&repo, &name, &path, context).await
                    }
                };
                (repo, answer(result))
            },
            move |(repo, result)| Message::DiffLoaded(repo, key, token, result),
        )
    }

    fn diff_loaded(
        &mut self,
        repo: &str,
        key: PaneKey,
        token: u64,
        result: Answer<Option<FileDiff>>,
    ) -> Task<Message> {
        let split = self.settings.diff_split;
        let syntax = self.settings.syntax_highlight;
        let dark = self.look.dark;
        let mut failure = None;
        let Some(pane) = self.pane_mut(repo, key).filter(|pane| pane.token == token) else {
            return Task::none();
        };
        pane.loading = false;
        let mut task = Task::none();
        match result {
            Ok(diff) => {
                // Same content keeps the line selection and scroll position.
                if diff != pane.diff {
                    pane.diff = diff;
                    pane.selection.clear();
                    pane.anchor = None;
                    pane.highlights = None;
                    pane.offset = 0.0;
                    pane.rebuild_rows(split);
                    task = scroll_pane(pane, 0.0);
                    if syntax && let Some(diff) = pane.diff.clone() {
                        let repo = repo.to_owned();
                        let colours = Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    highlight::highlight(&diff, dark)
                                })
                                .await
                                .ok()
                                .flatten()
                            },
                            move |highlights| {
                                Message::HighlightsLoaded(repo.clone(), key, token, highlights)
                            },
                        );
                        task = Task::batch([task, colours]);
                    }
                }
            }
            Err(error) => {
                pane.clear();
                failure = Some(error);
            }
        }
        if let Some(error) = failure {
            self.toast(ToastKind::Error, "Could not load diff", Some(error));
        }
        task
    }

    fn hunk_action(&mut self, hunk: usize, mode: &'static str) -> Task<Message> {
        let context = self.settings.diff_context;
        let Some(tab) = self.current() else {
            return Task::none();
        };
        let Some((_, path)) = &tab.focus else {
            return Task::none();
        };
        let lines = tab.changes_pane.selected_in(hunk);
        let count = lines.len();
        let chosen = (!lines.is_empty()).then(|| lines.into_iter().collect());
        let selection: Selection = BTreeMap::from([(hunk, chosen)]);
        let command = Command::Selection {
            path: path.clone(),
            mode,
            selection,
            context,
        };
        if mode == "discard" {
            let what = if count > 0 {
                format!("{} selected", plural(count, "line"))
            } else {
                "this hunk".to_owned()
            };
            self.form = Some(Form::confirm(
                "Discard changes",
                &format!("Discard {what}? This cannot be undone."),
                "Discard",
                true,
                Message::Run(command),
            ));
            return Task::none();
        }
        Task::done(Message::Run(command))
    }

    // ------------------------------------------------------------- search

    fn run_search(&mut self) -> Task<Message> {
        let Some(tab) = self.current_mut() else {
            return Task::none();
        };
        let query = tab.search_input.trim().to_owned();
        tab.view = View::History;
        if query.is_empty() {
            tab.search = None;
            return Task::none();
        }
        let mode = tab.search_mode;
        let results = tab
            .search
            .take()
            .map(|search| search.results)
            .unwrap_or_default();
        tab.search = Some(SearchState {
            query: query.clone(),
            mode,
            results,
            loading: true,
        });
        let path = tab.path.clone();
        Task::perform(
            async move {
                let result = answer(queries::search(&path, &query, mode.key(), SEARCH_LIMIT).await);
                (path, query, result)
            },
            |(path, query, result)| Message::SearchDone(path, query, result),
        )
    }

    fn search_done(
        &mut self,
        repo: &str,
        query: &str,
        result: Answer<Vec<Commit>>,
    ) -> Task<Message> {
        let Some(tab) = self.tab_mut(repo) else {
            return Task::none();
        };
        let Some(search) = tab.search.as_mut().filter(|search| search.query == query) else {
            return Task::none();
        };
        search.loading = false;
        match result {
            Ok(results) => {
                let first = results.first().map(|commit| commit.sha.clone());
                search.results = results;
                tab.offset = 0.0;
                let top = scroll_to(tab, 0.0);
                Task::batch([top, first.map_or_else(Task::none, |sha| self.select(sha))])
            }
            Err(error) => {
                search.results.clear();
                self.toast(ToastKind::Error, "Search failed", Some(error));
                Task::none()
            }
        }
    }

    // ------------------------------------------------------------ compare

    fn compare_start(&mut self, base: String, target: String) -> Task<Message> {
        let Some(tab) = self.current_mut() else {
            return Task::none();
        };
        tab.search = None;
        tab.view = View::History;
        tab.compare = Some(CompareState {
            base: base.clone(),
            target: target.clone(),
            result: None,
            error: None,
            selected: None,
        });
        tab.commit_pane.clear();
        let path = tab.path.clone();
        Task::perform(
            async move {
                let result = answer(queries::compare(&path, &base, &target).await);
                (path, result)
            },
            |(path, result)| Message::CompareDone(path, result),
        )
    }

    fn compare_done(&mut self, repo: &str, result: Answer<Comparison>) -> Task<Message> {
        let Some(compare) = self.tab_mut(repo).and_then(|tab| tab.compare.as_mut()) else {
            return Task::none();
        };
        match result {
            Ok(result) => {
                compare.selected = result.files.first().map(|file| file.path.clone());
                compare.result = Some(result);
                self.reload_commit_pane()
            }
            Err(error) => {
                compare.error = Some(error);
                Task::none()
            }
        }
    }

    // -------------------------------------------------------------- stash

    fn open_stash(&mut self, name: String) -> Task<Message> {
        let Some(tab) = self.current_mut() else {
            return Task::none();
        };
        tab.view = View::Stash;
        tab.stash = Some(StashState {
            name: name.clone(),
            files: Vec::new(),
            selected: None,
        });
        tab.commit_pane.clear();
        let path = tab.path.clone();
        Task::perform(
            async move {
                let result = answer(queries::stash_files(&path, &name).await);
                (path, name, result)
            },
            |(path, name, result)| Message::StashFilesLoaded(path, name, result),
        )
    }

    fn stash_files_loaded(
        &mut self,
        repo: &str,
        name: &str,
        result: Answer<Vec<FileChange>>,
    ) -> Task<Message> {
        let Some(stash) = self
            .tab_mut(repo)
            .and_then(|tab| tab.stash.as_mut())
            .filter(|stash| stash.name == name)
        else {
            return Task::none();
        };
        match result {
            Ok(files) => {
                stash.selected = files.first().map(|file| file.path.clone());
                stash.files = files;
                self.reload_commit_pane()
            }
            Err(error) => {
                self.toast(ToastKind::Error, "Could not load stash", Some(error));
                Task::none()
            }
        }
    }

    // ------------------------------------------------------------ changes

    fn file_picked(&mut self, area: Area, path: String) -> Task<Message> {
        let modifiers = self.modifiers;
        let Some(tab) = self.current_mut() else {
            return Task::none();
        };
        pick_file_in(tab, area, path, modifiers);
        self.reload_changes()
    }

    // ----------------------------------------------------------- commands

    fn run_command(&mut self, command: Command) -> Task<Message> {
        let Some(repo) = self.current().map(|tab| tab.path.clone()) else {
            return Task::none();
        };
        self.run_in(repo, command)
    }

    pub(crate) fn run_in(&mut self, repo: String, command: Command) -> Task<Message> {
        let id = self.next_id;
        self.next_id += 1;
        self.busy = Some((id, command.label()));
        self.progress_line.clear();
        let cancellable = command.cancellable();
        let task = Task::stream(iced::stream::channel(64, async move |mut output| {
            let (sender, mut lines) = tokio::sync::mpsc::unbounded_channel();
            let progress: Progress = Arc::new(move |line| {
                let _ = sender.send(line);
            });
            let result = {
                let work = command.clone().run(&repo, Some(progress));
                tokio::pin!(work);
                loop {
                    tokio::select! {
                        result = &mut work => break result,
                        Some(line) = lines.recv() => {
                            let _ = output.send(Message::Progress(id, line)).await;
                        }
                    }
                }
            };
            let _ = output
                .send(Message::Done(repo, id, command, answer(result)))
                .await;
        }));
        let (task, handle) = task.abortable();
        self.cancel = cancellable.then_some(handle);
        task
    }

    fn command_done(
        &mut self,
        repo: String,
        id: u64,
        command: Command,
        result: Answer<OpResult>,
    ) -> Task<Message> {
        if self.busy.as_ref().is_some_and(|(busy, _)| *busy == id) {
            self.busy = None;
            self.cancel = None;
            self.progress_line.clear();
        }
        let label = command.label();
        let mut succeeded = false;
        match result {
            Ok(result) if !result.ok => {
                let message = if result.conflicts {
                    "Stopped because of conflicts. Resolve them in Local Changes."
                } else {
                    "Operation paused"
                };
                self.toast(ToastKind::Warning, message, Some(result.message));
                if let Some(tab) = self.tab_mut(&repo) {
                    tab.view = View::Changes;
                }
            }
            Ok(_) => {
                succeeded = true;
                if let Some(message) = command.success() {
                    self.toast(ToastKind::Success, message, None);
                }
            }
            Err(error) => self.toast(ToastKind::Error, format!("{label} failed"), Some(error)),
        }
        match &command {
            Command::Commit { .. } => {
                if let Some(tab) = self.tab_mut(&repo) {
                    tab.committing = false;
                    if succeeded {
                        tab.summary.clear();
                        tab.body = text_editor::Content::new();
                        tab.amend = false;
                    }
                }
            }
            Command::Resolve { .. } | Command::InteractiveRebase { .. } => {
                if matches!(
                    self.overlay,
                    Some(Overlay::Conflict(_) | Overlay::Rebase(_))
                ) && succeeded
                {
                    self.overlay = None;
                }
            }
            Command::Merge { mode, .. } if mode == "squash" && succeeded => {
                if let Some(tab) = self.tab_mut(&repo) {
                    tab.view = View::Changes;
                }
            }
            Command::Reset { undo: true, .. } if succeeded => {
                if let Some(tab) = self.tab_mut(&repo) {
                    tab.view = View::Changes;
                }
            }
            _ => {}
        }
        let found = self
            .tabs
            .iter()
            .find(|tab| tab.path == repo)
            .map(|tab| (tab.log_limit, tab.all_branches));
        let Some((limit, all)) = found else {
            return Task::none();
        };
        if command.status_only() {
            load_status(repo)
        } else {
            load_all(repo, limit, all)
        }
    }

    // ----------------------------------------------------------- overlays

    fn open_reflog(&mut self) -> Task<Message> {
        let Some(path) = self.current().map(|tab| tab.path.clone()) else {
            return Task::none();
        };
        self.overlay = Some(Overlay::Reflog {
            entries: Vec::new(),
            loading: true,
            error: None,
        });
        Task::perform(
            async move { answer(queries::reflog(&path, "HEAD", REFLOG_LIMIT).await) },
            Message::ReflogLoaded,
        )
    }

    fn open_blame(&mut self, path: String, rev: Option<String>) -> Task<Message> {
        let Some(repo) = self.current().map(|tab| tab.path.clone()) else {
            return Task::none();
        };
        self.overlay = Some(Overlay::Blame {
            path: path.clone(),
            rev: rev.clone(),
            lines: Vec::new(),
            loading: true,
            error: None,
            offset: 0.0,
            viewport: 600.0,
            hovered: None,
        });
        Task::perform(
            async move { answer(queries::blame(&repo, &path, rev.as_deref()).await) },
            Message::BlameLoaded,
        )
    }

    fn open_file_history(&mut self, path: String) -> Task<Message> {
        let Some(repo) = self.current().map(|tab| tab.path.clone()) else {
            return Task::none();
        };
        self.overlay = Some(Overlay::FileHistory {
            path: path.clone(),
            commits: Vec::new(),
            selected: None,
            pane: Box::default(),
            loading: true,
            error: None,
        });
        Task::perform(
            async move { answer(queries::file_history(&repo, &path, FILE_HISTORY_LIMIT).await) },
            Message::FileHistoryLoaded,
        )
    }

    fn file_history_loaded(&mut self, result: Answer<Vec<Commit>>) -> Task<Message> {
        let Some(Overlay::FileHistory {
            commits,
            loading,
            error,
            ..
        }) = &mut self.overlay
        else {
            return Task::none();
        };
        *loading = false;
        match result {
            Ok(list) => {
                let first = list.first().map(|commit| commit.sha.clone());
                *commits = list;
                first.map_or_else(Task::none, |sha| self.file_history_select(sha))
            }
            Err(message) => {
                *error = Some(message);
                Task::none()
            }
        }
    }

    fn file_history_select(&mut self, sha: String) -> Task<Message> {
        let Some(Overlay::FileHistory {
            path,
            commits,
            selected,
            ..
        }) = &mut self.overlay
        else {
            return Task::none();
        };
        *selected = Some(sha.clone());
        let file = commits
            .iter()
            .find(|commit| commit.sha == sha)
            .and_then(|commit| commit.file.clone());
        let request = DiffRequest::Commit {
            sha,
            path: file
                .as_ref()
                .map_or_else(|| path.clone(), |file| file.path.clone()),
            orig: file.and_then(|file| file.orig_path),
        };
        self.load_diff(PaneKey::Overlay, request)
    }

    fn open_conflict(&mut self, path: String) -> Task<Message> {
        let Some(repo) = self.current().map(|tab| tab.path.clone()) else {
            return Task::none();
        };
        self.overlay = Some(Overlay::Conflict(Box::new(ConflictState {
            path: path.clone(),
            data: None,
            error: None,
            choices: Vec::new(),
            result: text_editor::Content::new(),
            edited: false,
        })));
        Task::perform(
            async move { answer(ops::conflict_versions(&repo, &path).await) },
            Message::ConflictLoaded,
        )
    }

    /// Applies a side to one conflict (or all); hand edits are only overwritten after asking.
    pub(crate) fn conflict_choose(
        &mut self,
        index: Option<usize>,
        choice: Choice,
        confirmed: bool,
    ) -> Task<Message> {
        let Some(Overlay::Conflict(state)) = &mut self.overlay else {
            return Task::none();
        };
        if state.edited && !confirmed {
            let next = Message::Action(Action::ConflictOverwrite(index, choice));
            self.form = Some(Form::confirm(
                "Replace edits?",
                "Choosing a side rebuilds the result and discards your manual edits.",
                "Rebuild",
                false,
                next,
            ));
            return Task::none();
        }
        match index {
            Some(index) => {
                if let Some(slot) = state.choices.get_mut(index) {
                    *slot = if *slot == Some(choice) {
                        None
                    } else {
                        Some(choice)
                    };
                }
            }
            None => state
                .choices
                .iter_mut()
                .for_each(|slot| *slot = Some(choice)),
        }
        if let Some(data) = &state.data {
            state.result =
                text_editor::Content::with_text(&logic::build_result(&data.chunks, &state.choices));
        }
        state.edited = false;
        Task::none()
    }

    fn conflict_save(&mut self) -> Task<Message> {
        let Some(Overlay::Conflict(state)) = &self.overlay else {
            return Task::none();
        };
        let content = state.result.text();
        let command = Command::Resolve {
            path: state.path.clone(),
            content: Some(content.clone()),
            side: None,
        };
        if logic::has_conflict_markers(&content) {
            self.form = Some(Form::confirm(
                "Conflict markers remain",
                "The result still contains conflict markers. Save and mark the file as resolved anyway?",
                "Save anyway",
                true,
                Message::Run(command),
            ));
            return Task::none();
        }
        Task::done(Message::Run(command))
    }

    fn open_rebase(&mut self, base: Option<String>) -> Task<Message> {
        let Some(tab) = self.current() else {
            return Task::none();
        };
        let repo = tab.path.clone();
        let base_label = match &base {
            None => "the root commit".to_owned(),
            Some(base) => match tab.commits.iter().find(|commit| commit.sha == *base) {
                Some(commit) => format!("{} “{}”", short_sha(base), commit.subject),
                None => short_sha(base).to_owned(),
            },
        };
        self.overlay = Some(Overlay::Rebase(Box::new(RebaseState {
            base: base.clone(),
            base_label,
            steps: Vec::new(),
            original: Vec::new(),
            loading: true,
            error: None,
            autostash: true,
            focused: 0,
            drag: None,
        })));
        Task::perform(
            async move { answer(queries::rebase_candidates(&repo, base.as_deref()).await) },
            Message::RebaseLoaded,
        )
    }

    fn rebase_set_action(&mut self, index: usize, action: &'static str) -> Task<Message> {
        let repo = self
            .current()
            .map(|tab| tab.path.clone())
            .unwrap_or_default();
        let Some(Overlay::Rebase(state)) = &mut self.overlay else {
            return Task::none();
        };
        let Some(step) = state.steps.get_mut(index) else {
            return Task::none();
        };
        step.action = action;
        state.focused = index;
        if action != "reword" || step.body.is_some() {
            return Task::none();
        }
        let sha = step.commit.sha.clone();
        Task::perform(
            async move {
                let result = queries::commit_detail(&repo, &sha).await.map(|detail| {
                    let (_, body) = logic::split_message(&detail.message);
                    body
                });
                (sha, answer(result))
            },
            |(sha, result)| Message::RebaseBodyLoaded(sha, result),
        )
    }

    fn rebase_start(&mut self) -> Task<Message> {
        let Some(Overlay::Rebase(state)) = &self.overlay else {
            return Task::none();
        };
        if state.problem().is_some() {
            return Task::none();
        }
        let steps = state
            .steps
            .iter()
            .map(|step| RebaseStep {
                sha: step.commit.sha.clone(),
                action: step.action.to_owned(),
                // The edited summary replaces the first line; the original body is kept.
                message: (step.action == "reword" && step.message.trim() != step.commit.subject)
                    .then(
                        || match step.body.as_deref().filter(|body| !body.is_empty()) {
                            Some(body) => format!("{}\n\n{body}", step.message.trim()),
                            None => step.message.trim().to_owned(),
                        },
                    ),
            })
            .collect();
        let command = Command::InteractiveRebase {
            base: state.base.clone(),
            steps,
            autostash: state.autostash,
        };
        Task::done(Message::Run(command))
    }

    // ----------------------------------------------------------- keyboard

    /// Scrolls just far enough to bring a row into view.
    fn reveal(&mut self, index: usize) -> Task<Message> {
        let Some(tab) = self.current_mut() else {
            return Task::none();
        };
        let top = index as f32 * ROW_HEIGHT;
        let target = if top < tab.offset {
            top
        } else if top + ROW_HEIGHT > tab.offset + tab.viewport {
            top + ROW_HEIGHT - tab.viewport
        } else {
            return Task::none();
        };
        tab.offset = target;
        scroll_to(tab, target)
    }

    fn key(&mut self, key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Task<Message> {
        use keyboard::Key::{Character, Named};
        use keyboard::key::Named as N;
        if matches!(key.as_ref(), Named(N::Escape)) {
            if self.menu.take().is_some() {
                return Task::none();
            }
            if self.form.take().is_some() {
                return Task::none();
            }
            if self.overlay.take().is_some() {
                return Task::none();
            }
            return Task::done(Message::SearchClear);
        }
        if self.form.is_some() {
            return Task::none();
        }
        if let Some(Overlay::Rebase(state)) = &self.overlay {
            let focused = state.focused;
            let editing = state
                .steps
                .get(focused)
                .is_some_and(|step| step.action == "reword");
            return match key.as_ref() {
                Named(N::ArrowUp) if modifiers.alt() => {
                    Task::done(Message::RebaseMove(focused, -1))
                }
                Named(N::ArrowDown) if modifiers.alt() => {
                    Task::done(Message::RebaseMove(focused, 1))
                }
                Named(N::ArrowUp) => Task::done(Message::RebaseFocus(focused.saturating_sub(1))),
                Named(N::ArrowDown) => Task::done(Message::RebaseFocus(
                    (focused + 1).min(state.steps.len().saturating_sub(1)),
                )),
                Character(letter) if !editing && !modifiers.command() => {
                    let action = match letter {
                        "p" => "pick",
                        "r" => "reword",
                        "e" => "edit",
                        "s" => "squash",
                        "f" => "fixup",
                        "d" => "drop",
                        _ => return Task::none(),
                    };
                    Task::done(Message::RebaseSetAction(focused, action))
                }
                _ => Task::none(),
            };
        }
        match key.as_ref() {
            Named(N::F5) => return Task::done(Message::Refresh),
            Character("r") if modifiers.command() => return Task::done(Message::Refresh),
            Character("o" | "t") if modifiers.command() => return Task::done(Message::OpenDialog),
            Character("w") if modifiers.command() => {
                return Task::done(Message::Close(self.active));
            }
            Character(",") if modifiers.command() => return Task::done(Message::OpenSettings),
            Character("f") if modifiers.command() => return Task::done(Message::SearchFocus),
            Character("1") if modifiers.command() => {
                return Task::done(Message::SetView(View::History));
            }
            Character("2") if modifiers.command() => {
                return Task::done(Message::SetView(View::Changes));
            }
            Character("=" | "+") if modifiers.command() => return Task::done(Message::Zoom(1.1)),
            Character("-") if modifiers.command() => return Task::done(Message::Zoom(1.0 / 1.1)),
            Character("0") if modifiers.command() => return Task::done(Message::Zoom(0.0)),
            Named(N::Enter) if modifiers.command() => return Task::done(Message::CommitNow),
            _ => {}
        }
        if self.overlay.is_some() {
            return Task::none();
        }
        let Some(tab) = self.current().filter(|tab| tab.view == View::History) else {
            return Task::none();
        };
        let commits = tab.visible();
        if commits.is_empty() {
            return Task::none();
        }
        let last = commits.len() - 1;
        let page = ((tab.viewport / ROW_HEIGHT) as usize).max(2) - 1;
        let current = tab
            .selected
            .as_ref()
            .and_then(|sha| commits.iter().position(|commit| commit.sha == *sha));
        let target = match key.as_ref() {
            Named(N::ArrowDown) => current.map_or(0, |i| (i + 1).min(last)),
            Named(N::ArrowUp) => current.map_or(0, |i| i.saturating_sub(1)),
            Named(N::PageDown) => current.map_or(0, |i| (i + page).min(last)),
            Named(N::PageUp) => current.map_or(0, |i| i.saturating_sub(page)),
            Named(N::Home) => 0,
            Named(N::End) => last,
            _ => return Task::none(),
        };
        let sha = commits[target].sha.clone();
        let reveal = self.reveal(target);
        Task::batch([self.select(sha), reveal])
    }

    pub(crate) fn menu_at_cursor(&mut self, items: Vec<MenuItem>) {
        self.menu = Some(Menu::new(self.cursor, items));
    }
}

/// Keeps the focused file valid when status changes, for example after staging it.
fn ensure_focus(tab: &mut RepoTab) {
    let contains = |tab: &RepoTab, area: Area, path: &str| {
        tab.files_in(area).iter().any(|file| file.path == path)
    };
    if let Some((area, path)) = &tab.focus
        && contains(tab, *area, path)
    {
        return;
    }
    let mut next = None;
    if let Some((area, path)) = tab.focus.clone() {
        let same_list = tab
            .files_in(area)
            .first()
            .map(|file| (area, file.path.clone()));
        let moved = [Area::Unstaged, Area::Staged, Area::Conflicts]
            .into_iter()
            .find(|other| *other != area && contains(tab, *other, &path));
        next = match moved {
            Some(other) => same_list.or(Some((other, path))),
            None => same_list,
        };
    }
    let next = next
        .or_else(|| {
            tab.files_in(Area::Conflicts)
                .first()
                .map(|file| (Area::Conflicts, file.path.clone()))
        })
        .or_else(|| {
            tab.files_in(Area::Unstaged)
                .first()
                .map(|file| (Area::Unstaged, file.path.clone()))
        })
        .or_else(|| {
            tab.files_in(Area::Staged)
                .first()
                .map(|file| (Area::Staged, file.path.clone()))
        });
    tab.picked = match &next {
        Some((area, path)) => (*area, vec![path.clone()]),
        None => (Area::Unstaged, Vec::new()),
    };
    tab.focus = next;
}

/// Ctrl toggles, Shift selects a range from the anchor, a plain click selects one file.
fn pick_file_in(tab: &mut RepoTab, area: Area, path: String, modifiers: keyboard::Modifiers) {
    let list: Vec<String> = tab
        .files_in(area)
        .into_iter()
        .map(|file| file.path)
        .collect();
    let same_area = tab.picked.0 == area;
    let paths = if modifiers.command() && same_area {
        let mut paths = tab.picked.1.clone();
        if let Some(at) = paths.iter().position(|picked| *picked == path) {
            paths.remove(at);
        } else {
            paths.push(path.clone());
        }
        paths
    } else if let Some((from, to)) = tab
        .pick_anchor
        .as_ref()
        .filter(|_| modifiers.shift() && same_area)
        .and_then(|anchor| {
            Some((
                list.iter().position(|p| p == anchor)?,
                list.iter().position(|p| *p == path)?,
            ))
        })
    {
        list[from.min(to)..=from.max(to)].to_vec()
    } else {
        tab.pick_anchor = Some(path.clone());
        vec![path.clone()]
    };
    tab.picked = (area, paths);
    tab.focus = Some((area, path));
}

/// A click toggles a changed line; Shift+click selects the range from the last click.
fn click_line(pane: &mut DiffPane, hunk: usize, line: usize, shift: bool) {
    let Some(lines) = pane
        .diff
        .as_ref()
        .and_then(|diff| diff.hunks.get(hunk))
        .map(|hunk| &hunk.lines)
    else {
        return;
    };
    let is_change = |index: usize| {
        lines
            .get(index)
            .is_some_and(|line| matches!(line.t, '+' | '-'))
    };
    if !is_change(line) {
        return;
    }
    if let Some((anchor_hunk, anchor)) = pane
        .anchor
        .filter(|(anchor_hunk, _)| shift && *anchor_hunk == hunk)
    {
        let range = anchor.min(line)..=anchor.max(line);
        let chosen: Vec<usize> = range.filter(|index| is_change(*index)).collect();
        pane.selection
            .extend(chosen.into_iter().map(|index| (anchor_hunk, index)));
        return;
    }
    if !pane.selection.remove(&(hunk, line)) {
        pane.selection.insert((hunk, line));
    }
    pane.anchor = Some((hunk, line));
}

fn scroll_pane(pane: &DiffPane, offset: f32) -> Task<Message> {
    operation::scroll_to(
        pane.scroll.clone(),
        scrollable::AbsoluteOffset {
            x: Some(0.0),
            y: Some(offset),
        },
    )
}

fn scroll_to(tab: &RepoTab, offset: f32) -> Task<Message> {
    operation::scroll_to(
        tab.list.clone(),
        scrollable::AbsoluteOffset {
            x: None,
            y: Some(offset),
        },
    )
}

fn load_status(path: String) -> Task<Message> {
    Task::perform(
        async move {
            let result = answer(queries::status(&path).await);
            (path, result)
        },
        |(path, result)| Message::StatusLoaded(path, result),
    )
}

fn load_refs(path: String) -> Task<Message> {
    Task::perform(
        async move {
            let result = answer(queries::refs(&path).await);
            (path, result)
        },
        |(path, result)| Message::RefsLoaded(path, result),
    )
}

fn load_log(path: String, limit: usize, all: bool) -> Task<Message> {
    Task::perform(
        async move {
            let result = answer(queries::log(&path, limit, all, None).await)
                .map(|page| (page.commits, page.has_more));
            (path, result)
        },
        |(path, result)| Message::LogLoaded(path, result),
    )
}

fn load_all(path: String, limit: usize, all: bool) -> Task<Message> {
    Task::batch([
        load_status(path.clone()),
        load_refs(path.clone()),
        load_log(path, limit, all),
    ])
}

#[allow(clippy::ptr_arg)] // Subscription::run_with needs exactly fn(&String).
fn watch_stream(path: &String) -> impl Stream<Item = Message> + use<> {
    let repo = path.clone();
    iced::stream::channel(16, async move |mut output| {
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        let handle = watcher::watch(&repo, move |event| {
            let _ = sender.send(event);
        })
        .await;
        match handle {
            Ok(_handle) => {
                while let Some(event) = events.recv().await {
                    if output
                        .send(Message::Watched(repo.clone(), event))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
            Err(error) => {
                let event = WatchEvent::Error(error.to_string());
                let _ = output.send(Message::Watched(repo.clone(), event)).await;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane_with(lines: &str) -> DiffPane {
        let text = format!("diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,9 +1,9 @@\n{lines}");
        DiffPane {
            diff: gitcraft_core::parsers::parse_diff(text.as_bytes()).pop(),
            ..DiffPane::default()
        }
    }

    #[test]
    fn clicking_toggles_changed_lines_only() {
        let mut pane = pane_with(" ctx\n-old\n+new\n");
        click_line(&mut pane, 0, 0, false);
        assert!(pane.selection.is_empty(), "context can't be selected");
        click_line(&mut pane, 0, 1, false);
        assert_eq!(pane.selected_in(0), [1]);
        click_line(&mut pane, 0, 1, false);
        assert!(pane.selection.is_empty());
    }

    #[test]
    fn shift_click_selects_changes_in_the_range_and_skips_context() {
        let mut pane = pane_with("-a\n ctx\n-b\n+c\n");
        click_line(&mut pane, 0, 0, false);
        click_line(&mut pane, 0, 3, true);
        assert_eq!(pane.selected_in(0), [0, 2, 3]);
    }

    fn file(path: &str) -> FileChange {
        FileChange {
            path: path.into(),
            status: "M".into(),
            orig_path: None,
        }
    }

    fn tab_with(unstaged: &[&str], staged: &[&str]) -> RepoTab {
        let mut tab = RepoTab::new("/r".into(), 10);
        tab.status = Some(Status {
            unstaged: unstaged.iter().map(|p| file(p)).collect(),
            staged: staged.iter().map(|p| file(p)).collect(),
            ..Status::default()
        });
        tab
    }

    #[test]
    fn focus_moves_on_when_the_focused_file_leaves() {
        let mut tab = tab_with(&["a", "b"], &[]);
        ensure_focus(&mut tab);
        assert_eq!(tab.focus, Some((Area::Unstaged, "a".to_owned())));
        // Staging a: it moved, and the unstaged list still has work, so stay there.
        tab.status = tab_with(&["b"], &["a"]).status;
        ensure_focus(&mut tab);
        assert_eq!(tab.focus, Some((Area::Unstaged, "b".to_owned())));
        // Staging the last one: follow it.
        tab.status = tab_with(&[], &["a", "b"]).status;
        ensure_focus(&mut tab);
        assert_eq!(tab.focus, Some((Area::Staged, "b".to_owned())));
        tab.status = tab_with(&[], &[]).status;
        ensure_focus(&mut tab);
        assert_eq!(tab.focus, None);
    }

    #[test]
    fn picking_supports_ctrl_and_shift() {
        let mut tab = tab_with(&["a", "b", "c", "d"], &[]);
        pick_file_in(
            &mut tab,
            Area::Unstaged,
            "b".into(),
            keyboard::Modifiers::empty(),
        );
        assert_eq!(tab.picked.1, ["b"]);
        pick_file_in(
            &mut tab,
            Area::Unstaged,
            "d".into(),
            keyboard::Modifiers::SHIFT,
        );
        assert_eq!(tab.picked.1, ["b", "c", "d"]);
        pick_file_in(
            &mut tab,
            Area::Unstaged,
            "c".into(),
            keyboard::Modifiers::COMMAND,
        );
        assert_eq!(tab.picked.1, ["b", "d"]);
        assert_eq!(tab.focus, Some((Area::Unstaged, "c".to_owned())));
    }

    #[test]
    fn rebase_steps_move_and_reset() {
        let commit = |sha: &str| Commit {
            sha: sha.into(),
            parents: vec![],
            author: String::new(),
            email: String::new(),
            time: 0,
            refs: None,
            subject: sha.into(),
            graph: None,
            file: None,
        };
        let mut state = RebaseState {
            base: None,
            base_label: String::new(),
            steps: Vec::new(),
            original: vec![commit("a"), commit("b"), commit("c")],
            loading: false,
            error: None,
            autostash: true,
            focused: 0,
            drag: None,
        };
        state.reset();
        assert!(state.unchanged());
        state.move_step(0, 2);
        assert_eq!(state.focused, 2);
        assert_eq!(state.steps[2].commit.sha, "a");
        assert!(!state.unchanged());
        state.move_step(0, 9);
        assert_eq!(state.steps[0].commit.sha, "b");
        state.reset();
        assert!(state.unchanged());
    }
}
