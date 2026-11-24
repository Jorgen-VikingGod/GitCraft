//! Application state, messages and the update loop.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Key, Modifiers, Pos2, Vec2};
use gitcraft_core::model::*;
use gitcraft_core::parsers::Selection;
use gitcraft_core::runner::Progress;
use gitcraft_core::store::RepoStore;
use gitcraft_core::watcher::{self, WatchEvent};
use gitcraft_core::{Error, ops, queries, repos, undo};

use crate::actions::{Action, MenuRequest};
use crate::command::Command;
use crate::form::{FieldKind, Form};
use crate::highlight::{self, Highlights};
use crate::i18n::{tr, trf};
use crate::logic::{self, Choice, plural, short_sha};
use crate::menu::{Menu, MenuItem};
use crate::settings::Settings;
use crate::task::{Handle, Task};
use crate::theme::Look;

/// Height of one commit row; lists are virtualised on this.
pub const ROW_HEIGHT: f32 = 26.0;
pub const DIFF_ROW_HEIGHT: f32 = 20.0;
pub const BLAME_ROW_HEIGHT: f32 = 20.0;
pub const REBASE_ROW_HEIGHT: f32 = 36.0;
const SEARCH_LIMIT: usize = 500;
const FILE_HISTORY_LIMIT: usize = 1000;
const REFLOG_LIMIT: usize = 500;

pub type Answer<T> = Result<T, String>;

/// A list of file paths, the one that is current, and how to pick another.
type FileList = (Vec<String>, Option<String>, Box<dyn Fn(String) -> Message>);

pub(crate) fn answer<T>(result: Result<T, Error>) -> Answer<T> {
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
    /// Whether the colours asked for were made for the dark theme; `None` when none were.
    pub highlighted_dark: Option<bool>,
    pub loading: bool,
    pub token: u64,
    pub offset: f32,
    pub viewport: Vec2,
    /// A scroll position the view should jump to the next time it is drawn.
    pub scroll_to: Cell<Option<f32>>,
    /// Row tops for a width and row count: see the diff view.
    pub layout: RefCell<(f32, usize, Vec<f32>)>,
    /// The widest line in columns, once known: the diff scrolls sideways by this much.
    pub max_cols: Cell<Option<usize>>,
    /// Where the sides of a split diff are scrolled to sideways; they move together.
    pub hscroll: Cell<f32>,
    pub selection: BTreeSet<(usize, usize)>,
    pub anchor: Option<(usize, usize)>,
}

impl Default for DiffPane {
    fn default() -> Self {
        Self {
            diff: None,
            rows: Vec::new(),
            highlights: None,
            highlighted_dark: None,
            loading: false,
            token: 0,
            offset: 0.0,
            viewport: Vec2::new(800.0, 400.0),
            scroll_to: Cell::new(None),
            layout: RefCell::new((-1.0, 0, Vec::new())),
            max_cols: Cell::new(None),
            hscroll: Cell::new(0.0),
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
        self.max_cols.set(None);
        self.hscroll.set(0.0);
        self.highlights = None;
        self.highlighted_dark = None;
        self.loading = false;
        self.selection.clear();
        self.anchor = None;
    }

    pub fn rebuild_rows(&mut self, split: bool) {
        self.rows.clear();
        self.layout.get_mut().0 = -1.0;
        self.max_cols.set(None);
        self.hscroll.set(0.0);
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
        f.write_str(tr(match self {
            Self::Message => "Message",
            Self::Author => "Author",
            Self::Sha => "SHA",
            Self::Content => "Content",
        }))
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
    /// The branch or tag picked in the sidebar ("b:name", "r:origin/name", "t:name"), which
    /// is the one marked as selected there; other names for the commit are only related.
    pub selected_ref: Option<String>,
    pub detail: Option<CommitDetail>,
    pub detail_error: Option<String>,
    pub selected_file: Option<String>,
    /// The keyboard moves through the commit's files rather than the commits: set by picking
    /// a file, cleared by picking a commit.
    pub files_focused: bool,
    pub commit_pane: DiffPane,
    pub offset: f32,
    pub viewport: f32,
    /// A scroll position the history list should jump to the next time it is drawn.
    pub scroll_to: Cell<Option<f32>>,
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
    pub body: String,
    pub amend: bool,
    pub committing: bool,
    /// "Commit first" in the checkout dialog: the checkout to run once the commit succeeds.
    pub after_commit: Option<Command>,
    pub filter: String,
    /// Commits picked with Ctrl/Shift+click, newest first as listed; empty when only
    /// `selected` is.
    pub picked_commits: Vec<String>,
    /// Co-authors for the next commits, one `Name <mail>` per line.
    pub co_authors: String,
    /// What Edit › Undo and Redo would do.
    pub undo: UndoLabels,
    /// The bisect in progress.
    pub bisect: Option<Bisect>,
    /// What the history is narrowed to; `None` shows everything.
    pub history_filter: Option<LogFilter>,
    /// Branches hidden from the graph, or the only ones shown.
    pub ref_view: RefView,
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
            selected_ref: None,
            detail: None,
            detail_error: None,
            selected_file: None,
            files_focused: false,
            commit_pane: DiffPane::default(),
            offset: 0.0,
            viewport: 300.0,
            scroll_to: Cell::new(None),
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
            body: String::new(),
            amend: false,
            committing: false,
            after_commit: None,
            filter: String::new(),
            undo: UndoLabels::default(),
            bisect: None,
            history_filter: None,
            ref_view: RefView::default(),
            co_authors: String::new(),
            picked_commits: Vec::new(),
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

    /// What the tab says about worktrees: "worktree" in a linked one, "N worktrees" in the
    /// main one while others exist. `None` for a repository with a single working tree.
    pub fn worktree_note(&self) -> Option<String> {
        let trees = &self.refs.worktrees;
        let current = trees.iter().find(|tree| tree.current)?;
        if !current.main {
            Some(tr("worktree").to_owned())
        } else if trees.len() > 1 {
            Some(trf(
                "{count} worktrees",
                &[("count", &trees.len().to_string())],
            ))
        } else {
            None
        }
    }

    /// What the history list loads.
    pub fn log_query(&self) -> LogQuery {
        LogQuery {
            limit: self.log_limit,
            all: self.all_branches,
            filter: self.history_filter.clone().unwrap_or_default(),
            view: self.ref_view.clone(),
        }
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

    /// Ctrl+Shift+Enter stages everything first, so any change will do.
    pub fn can_commit_all(&self) -> bool {
        let Some(status) = &self.status else {
            return false;
        };
        let changes = !status.staged.is_empty() || !status.unstaged.is_empty();
        !self.summary.trim().is_empty()
            && status.conflicts.is_empty()
            && !self.committing
            && (changes || self.amend)
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
    pub result: String,
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

#[derive(Debug, Clone)]
pub struct RebaseStepState {
    pub commit: Commit,
    pub action: &'static str,
    /// A shell command to run after this step.
    pub exec: Option<String>,
    pub message: String,
    /// The message body, loaded when the commit is reworded.
    pub body: Option<String>,
}

/// A plan the rebase editor starts from instead of picking everything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preset {
    /// These commits are dropped.
    Drop(Vec<String>),
    /// These commits are squashed into the oldest of them.
    Squash(Vec<String>),
}

pub struct RebaseState {
    /// HEAD when the plan was made: the rebase refuses to start once it has moved.
    pub head: Option<String>,
    /// Replays `original` on top of HEAD (an interactive cherry-pick) instead of rewriting
    /// the branch since `base`.
    pub replay: bool,
    pub preset: Option<Preset>,
    pub base: Option<String>,
    pub base_label: String,
    pub steps: Vec<RebaseStepState>,
    pub original: Vec<Commit>,
    pub loading: bool,
    pub error: Option<String>,
    pub autostash: bool,
    pub focused: usize,
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
            && self.steps.iter().zip(&self.original).all(|(step, commit)| {
                step.action == "pick" && step.commit.sha == commit.sha && step.exec.is_none()
            })
    }

    /// Moves `fixup!`, `squash!` and `amend!` commits right after the commit they name and
    /// gives them that action, as `git rebase --autosquash` would.
    pub fn autosquash(&mut self) {
        let subjects: Vec<(String, String)> = self
            .steps
            .iter()
            .map(|step| (step.commit.subject.clone(), step.commit.sha.clone()))
            .collect();
        let plan = crate::logic::autosquash(&subjects);
        let old = std::mem::take(&mut self.steps);
        self.steps = plan
            .into_iter()
            .map(|(index, action)| {
                let mut step = old[index].clone();
                if let Some(action) = action {
                    step.action = action;
                }
                step
            })
            .collect();
    }
    /// Applies the preset after the commits are loaded. False when a commit of the preset is
    /// not among them (it isn't on the branch since the base).
    pub fn apply_preset(&mut self) -> bool {
        let Some(preset) = &self.preset else {
            return true;
        };
        let (Preset::Drop(shas) | Preset::Squash(shas)) = preset;
        let found = |sha: &String| self.steps.iter().any(|step| step.commit.sha == *sha);
        if !shas.iter().all(found) {
            return false;
        }
        match preset {
            Preset::Drop(shas) => {
                for step in self
                    .steps
                    .iter_mut()
                    .filter(|step| shas.contains(&step.commit.sha))
                {
                    step.action = "drop";
                }
            }
            Preset::Squash(shas) => {
                // The oldest stays a pick; the others move right after it as squashes.
                let Some(anchor) = self
                    .steps
                    .iter()
                    .position(|step| shas.contains(&step.commit.sha))
                else {
                    return true;
                };
                let anchor_sha = self.steps[anchor].commit.sha.clone();
                let mut moved = Vec::new();
                self.steps.retain(|step| {
                    let take = step.commit.sha != anchor_sha && shas.contains(&step.commit.sha);
                    if take {
                        moved.push(step.clone());
                    }
                    !take
                });
                let at = self
                    .steps
                    .iter()
                    .position(|step| step.commit.sha == anchor_sha)
                    .map_or(0, |index| index + 1);
                for (offset, mut step) in moved.into_iter().enumerate() {
                    step.action = "squash";
                    self.steps.insert(at + offset, step);
                }
            }
        }
        true
    }

    fn reset(&mut self) {
        self.steps = self
            .original
            .iter()
            .map(|commit| RebaseStepState {
                commit: commit.clone(),
                action: "pick",
                exec: None,
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
        /// Every branch's reflog rather than only HEAD's.
        all: bool,
        entries: Vec<ReflogEntry>,
        loading: bool,
        error: Option<String>,
    },
    Discards {
        entries: Vec<DiscardBackup>,
        loading: bool,
        error: Option<String>,
    },
    Blame {
        path: String,
        rev: Option<String>,
        lines: Vec<BlameLine>,
        loading: bool,
        error: Option<String>,
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
    /// `git range-diff old new`.
    RangeDiff {
        old: String,
        new: String,
        text: String,
        loading: bool,
        error: Option<String>,
    },
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

/// What `update` asks the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Copy(String),
    Quit,
    Window(WindowAction),
    /// Show a file dialog; the shell answers with [`Message::Picked`] or
    /// [`Message::DialogFailed`].
    Ask(Pick),
}

/// What a file dialog was opened for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    OpenRepository,
    SavePatch {
        repo: String,
        of: PatchOf,
        name: String,
    },
    ApplyPatch,
}

/// What a saved patch holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchOf {
    Commit(String),
    /// Every commit from this one up to HEAD.
    Series(String),
    /// The uncommitted changes.
    Changes,
}

impl Pick {
    /// The dialog to show for it (craft-dialogs: it never blocks the window).
    pub fn dialog(&self) -> craft_dialogs::FileDialog {
        use craft_dialogs::FileDialog;
        match self {
            Self::OpenRepository => FileDialog::folder().title(tr("Open repository")),
            Self::SavePatch { name, .. } => FileDialog::save()
                .title(tr("Save patch"))
                .file_name(name.clone()),
            Self::ApplyPatch => FileDialog::open()
                .title(tr("Apply patch"))
                .filter(tr("Patches"), &["patch", "diff", "eml", "txt"]),
        }
    }
}

/// What the app's own title bar asks of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    Minimize,
    ToggleMaximize,
    /// F11: the window fills the screen, without a title bar.
    ToggleFullscreen,
    /// Start moving the window with the pointer.
    Drag,
    /// Start resizing from an edge or corner.
    Resize(egui::ResizeDirection),
}

/// External tools git can start: for diffs, and for conflicts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoundTools {
    pub diff: Vec<ExternalTool>,
    pub merge: Vec<ExternalTool>,
}

impl FoundTools {
    /// How to call a tool in the interface: its own name if git knows it, else as written.
    pub fn label(&self, merge: bool, name: &str) -> String {
        let list = if merge { &self.merge } else { &self.diff };
        list.iter()
            .find(|tool| tool.name == name)
            .map_or_else(|| name.to_owned(), |tool| tool.label.clone())
    }
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
    pub cancel: Option<Handle>,
    pub progress_line: String,
    pub cursor: Pos2,
    pub window: Vec2,
    pub modifiers: Modifiers,
    /// Who draws the title bar and window edges (craft-window decides, before the window opens).
    pub frame: craft_window::policy::Frame,
    pub maximized: bool,
    pub fullscreen: bool,
    /// The desktop is set to dark; decides the look while the theme setting is "system".
    pub system_dark: bool,
    pub collapsed: HashSet<String>,
    /// Set when the search box should take the keyboard; the view clears it.
    pub focus_search: Cell<bool>,
    /// The saved tabs still to open at start-up, one after another so that they keep their
    /// order; `true` marks the one to bring to the front once all are open.
    restoring: VecDeque<(String, bool)>,
    /// The restore request on its way: whether it is the front tab.
    restore_waiting: Option<bool>,
    /// The front tab once it has opened, as git names its folder.
    restore_front: Option<String>,
    /// The diff and the merge tools git can start, as found when GitCraft started or the
    /// settings were opened.
    pub found_tools: FoundTools,
    /// Things only the shell can do, such as the clipboard; it drains them after each update.
    pub effects: Vec<Effect>,
    /// The merge or rebase target whose dialog waits for a conflict check.
    pub conflict_preview: Option<String>,
    next_id: u64,
    pub(crate) config_dir: PathBuf,
    pub(crate) store: RepoStore,
}

#[derive(Debug, Clone)]
pub enum Message {
    OpenDialog,
    /// A file dialog was answered with this path.
    Picked(Pick, PathBuf),
    /// No file dialog could be shown; the text says why.
    DialogFailed(String),
    OpenPath(String),
    Located(Answer<String>),
    /// A saved tab located at start-up (see `App::boot`).
    Restored(Answer<String>),
    Forget(String),
    Activate(usize),
    Close(usize),
    MoveTab(usize, isize),
    Refresh,
    SetView(View),
    StatusLoaded(String, Answer<Status>),
    RefsLoaded(String, Answer<Refs>),
    UndoLoaded(String, Answer<UndoLabels>),
    BisectLoaded(String, Answer<Option<Bisect>>),
    Undo,
    Redo,
    LogLoaded(String, Answer<(Vec<Commit>, bool)>),
    LoadMore,
    SetBranchFilter(bool),
    Select(String),
    ShowCommit(String),
    /// Show the commit a branch or tag points at, and remember which name was picked.
    ShowRef(String, String),
    ActivateCommit(String),
    DetailLoaded(String, String, Answer<CommitDetail>),
    PickFile(String),
    Scrolled(f32, f32),
    DiffLoaded(String, PaneKey, u64, Answer<Option<FileDiff>>),
    /// Colours for a diff: its repository, pane, request, and whether they are for the dark theme.
    HighlightsLoaded(String, PaneKey, u64, bool, Option<Arc<Highlights>>),
    DiffScrolled(PaneKey, f32, Vec2),
    DiffLineClicked(PaneKey, usize, usize),
    HunkAction(usize, &'static str),
    /// Reverts a hunk (or its selected lines) of the commit shown in the details.
    HunkRevert(usize),
    ToggleSplit,
    SearchInput(String),
    SearchModeChanged(SearchMode),
    SearchSubmit,
    SearchClear,
    SearchFocus,
    SearchDone(String, String, Answer<Vec<Commit>>),
    CompareStart(String, String),
    /// Hides a branch from the graph, solos it, or shows everything again.
    RefVisibility(RefVisibility),
    /// Narrows the history; `None` or an empty filter shows it all again.
    SetHistoryFilter(Option<LogFilter>),
    /// Opens the rebase editor with these commits dropped or squashed.
    PlanCommits(Preset),
    /// Opens the rebase editor to replay these commits on top of HEAD.
    OpenReplay(Vec<String>),
    CompareDone(String, Answer<Comparison>),
    CompareSwap,
    CompareClose,
    OpenStash(String),
    StashFilesLoaded(String, String, Answer<Vec<FileChange>>),
    FilePicked(Area, String),
    /// Ctrl+A in Local Changes: every file of the list that has the focus.
    SelectAllFiles,
    FileActivated(Area, String),
    SummaryInput(String),
    BodyInput(String),
    AmendToggled(bool),
    HeadMessageLoaded(String, Answer<String>),
    TemplateLoaded(String, Answer<String>),
    CommitNow,
    /// Ctrl+Shift+Enter: stages everything, then commits.
    CommitAll,
    /// After the warning about amending a commit that is already pushed.
    CommitConfirmed {
        stage_all: bool,
    },
    CommitOption(CommitOption, bool),
    CoAuthors(String),
    FilterInput(String),
    ToggleCollapsed(String),
    Run(Command),
    /// Run a command in this repository, whichever tab is in front; nothing if it was closed.
    RunIn(String, Command),
    PullChosen {
        remote: Option<String>,
        branch: Option<String>,
        mode: String,
        autostash: bool,
        backup: bool,
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
    MenuResolved(MenuRequest, Pos2, Answer<Commit>),
    MenuHover(Option<usize>),
    CloseMenu,
    FromMenu(Box<Message>),
    FormText(usize, String),
    FormArea(usize, String),
    FormCheck(usize, bool),
    FormSelect(usize, String),
    FormSubmit,
    FormCancel,
    /// One of a form's own answers (see [`crate::form::Form::choice`]).
    FormChoice(usize),
    /// Show the commit box of this repository, and run this checkout once the commit succeeds.
    CommitFirst(String, Command),
    /// Don't run the checkout waiting for the commit after all.
    CommitFirstCancel,
    OverlayClose,
    OpenReflog,
    /// Shows the reflogs of every branch (true) or only HEAD's.
    ReflogAll(bool),
    ReflogLoaded(Answer<Vec<ReflogEntry>>),
    OpenDiscards,
    /// Compares two ranges of commits: (old, new).
    RangeDiff(String, String),
    RangeDiffLoaded(Answer<String>),
    DiscardsLoaded(Answer<Vec<DiscardBackup>>),
    OpenBlame(String, Option<String>),
    BlameLoaded(Answer<Vec<BlameLine>>),
    BlameHover(String),
    OpenFileHistory(String),
    FileHistoryLoaded(Answer<Vec<Commit>>),
    FileHistorySelect(String),
    OpenConflict(String),
    ConflictLoaded(Answer<ConflictVersions>),
    ConflictChoose(usize, Choice),
    ConflictChooseAll(Choice),
    ConflictEdit(String),
    ConflictSave,
    OpenRebase(Option<String>),
    RebaseLoaded(Answer<Vec<Commit>>),
    RebaseSetAction(usize, &'static str),
    RebaseMessage(usize, String),
    RebaseBodyLoaded(String, Answer<String>),
    RebaseMove(usize, isize),
    RebaseFocus(usize),
    RebaseAutostash(bool),
    RebaseReset,
    RebaseAutosquash,
    /// A command to run after step `index`; empty removes it.
    RebaseExec(usize, String),
    RebaseStart,
    OpenSettings,
    /// The external tools git can start: diff tools, then merge tools.
    ToolsFound(FoundTools),
    /// The external diff tool to use, by git's name for it; empty for none.
    SetDiffTool(String),
    SetMergeTool(String),
    /// Show a file's change in the chosen diff tool.
    OpenDiffTool {
        path: String,
        orig: Option<String>,
        target: ToolTarget,
    },
    /// Resolve a conflicted file in the chosen merge tool.
    OpenMergeTool(String),
    /// An external tool was closed: the repository, whether it was the merge tool, and how
    /// it went.
    ToolClosed(String, bool, Answer<OpResult>),
    SetTheme(String),
    /// A language code, or `auto` for the system's.
    SetLanguage(String),
    /// The system draws the title bar from the next start on, or no longer does.
    ToggleSystemTitleBar,
    SetSplit(bool),
    SetContext(String),
    SetIgnoreWhitespace(bool),
    SetSyntax(bool),
    SetLogLimit(String),
    SetFetchOnOpen(bool),
    SetConflictCheck(bool),
    /// The files a merge with this target would conflict in (see `queries::conflict_check`).
    ConflictPreview(String, Answer<Option<Vec<String>>>),
    SetCommitGraph(bool),
    Zoom(f32),
    /// The desktop switched between light and dark.
    SystemTheme(bool),
    Copy(String),
    /// Copies a file's text as it is in a commit: (rev, path).
    CopyFileText(String, String),
    FileTextLoaded(Answer<String>),
    Reveal(String),
    OpenFile(String),
    Instance(String),
    Watched(String, WatchEvent),
    Key(Key, Modifiers),
    ModifiersChanged(Modifiers),
    CursorMoved(Pos2),
    /// A splitter was dragged by this many points.
    Resize(Splitter, f32),
    ResizeDone,
    WindowResized(Vec2),
    CloseRequested,
    Tick,
    DismissToast(u64),
    Quit,
    Window(WindowAction),
}

impl App {
    /// `system_dark`: the desktop is set to dark.
    pub fn boot(repo: Option<String>, system_dark: bool) -> (Self, Task<Message>) {
        Self::boot_with(repo, system_dark, RepoStore::open())
    }

    /// [`App::boot`] with a store of the caller's.
    pub fn boot_with(
        repo: Option<String>,
        system_dark: bool,
        store: RepoStore,
    ) -> (Self, Task<Message>) {
        let settings = Settings::load(&store.dir());
        let look = Look::for_setting(&settings.appearance.theme, system_dark);
        let mut app = Self::new(look, store, settings);
        // What the desktop is set to, not what is shown: with "Light" chosen on a dark desktop,
        // "Follow system" must still turn dark later.
        app.system_dark = system_dark;
        if let Err(problem) = gitcraft_core::runner::check_git() {
            app.toast(ToastKind::Error, problem, None);
        }
        let session = &app.settings.session;
        // A repository named on the command line opens last and stays in front.
        let front = repo.is_none().then_some(session.active_tab);
        let tools = find_tools(app.config_dir.to_string_lossy().into_owned());
        app.restoring = session
            .tabs
            .iter()
            .enumerate()
            .map(|(index, path)| (path.clone(), Some(index) == front))
            .chain(repo.map(|path| (path, false)))
            .collect();
        let task = app.restore_next();
        (app, Task::batch([task, tools]))
    }

    /// Start-up is still opening the saved tabs: the saved session is left as it is until then.
    fn restoring(&self) -> bool {
        !self.restoring.is_empty() || self.restore_waiting.is_some()
    }

    /// Opens the next saved tab, or, when all are open, brings the saved front tab forward and
    /// saves the session as it now is.
    fn restore_next(&mut self) -> Task<Message> {
        if let Some((path, front)) = self.restoring.pop_front() {
            self.restore_waiting = Some(front);
            return Task::perform(
                async move { answer(repos::locate(&path).await) },
                Message::Restored,
            );
        }
        let was_restoring = self.restore_waiting.take().is_some();
        if let Some(front) = self.restore_front.take()
            && let Some(index) = self.tabs.iter().position(|tab| tab.path == front)
        {
            self.active = index;
        }
        if was_restoring {
            self.save_settings();
        }
        Task::none()
    }

    /// An app with a fixed look and a store of the caller's, without saved tabs.
    pub fn with_look(look: Look, store: RepoStore) -> Self {
        let settings = Settings::load(&store.dir());
        Self::new(look, store, settings)
    }

    fn new(look: Look, store: RepoStore, settings: Settings) -> Self {
        let recent = store.list();
        let system_dark = look.dark;
        Self {
            look,
            collapsed: settings.layout.collapsed.iter().cloned().collect(),
            window: Vec2::new(1280.0, 800.0),
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
            cursor: Pos2::ZERO,
            modifiers: Modifiers::NONE,
            frame: craft_window::policy::Frame::Native,
            system_dark,
            maximized: false,
            fullscreen: false,
            focus_search: Cell::new(false),
            restoring: VecDeque::new(),
            restore_waiting: None,
            restore_front: None,
            found_tools: FoundTools::default(),
            effects: Vec::new(),
            conflict_preview: None,
            next_id: 1,
            config_dir: store.dir(),
            store,
        }
    }

    pub fn title(&self) -> String {
        match self.tabs.get(self.active) {
            Some(tab) => match (tab.current_branch(), tab.worktree_note()) {
                (Some(branch), Some(note)) => format!("{} [{branch}, {note}] - GitCraft", tab.name),
                (None, Some(note)) => format!("{} [{note}] - GitCraft", tab.name),
                _ => format!("{} - GitCraft", tab.name),
            },
            None => "GitCraft".to_owned(),
        }
    }

    pub fn scale(&self) -> f32 {
        self.settings.appearance.zoom
    }

    /// The app draws the title bar row (its own window buttons, or room for the traffic lights).
    pub fn draws_title_bar(&self) -> bool {
        !matches!(self.frame, craft_window::policy::Frame::Native)
    }

    /// The window has round corners the app paints right now.
    pub fn rounded(&self) -> bool {
        crate::window::rounded_for(&self.frame) && !self.maximized && !self.fullscreen
    }

    /// Reads the theme setting again, keeping what was measured on screen. Diffs are coloured
    /// again when it turns light or dark.
    fn restyle(&mut self) -> Task<Message> {
        let (char_width, was_dark) = (self.look.char_width, self.look.dark);
        self.look = Look::for_setting(&self.settings.appearance.theme, self.system_dark);
        self.look.char_width = char_width;
        if self.look.dark == was_dark {
            Task::none()
        } else {
            self.recolour_all()
        }
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
            message: crate::i18n::tr(&message.into()).to_owned(),
            detail,
            expires: Instant::now() + Duration::from_secs(seconds),
        });
    }

    fn save_settings(&mut self) {
        // Quitting half-way through start-up keeps the saved tabs that weren't opened yet.
        if !self.restoring() {
            self.settings.session.tabs = self.tabs.iter().map(|tab| tab.path.clone()).collect();
            self.settings.session.active_tab = self.active;
        }
        self.settings.layout.collapsed = self.collapsed.iter().cloned().collect();
        self.settings.layout.collapsed.sort();
        if let Err(error) = self.settings.save(&self.config_dir) {
            self.toast(
                ToastKind::Error,
                trf("Could not save settings: {error}", &[("error", &error)]),
                None,
            );
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenDialog => {
                self.effects.push(Effect::Ask(Pick::OpenRepository));
                Task::none()
            }
            Message::Picked(pick, path) => self.picked(pick, path),
            Message::DialogFailed(why) => {
                self.toast(
                    ToastKind::Error,
                    "The file dialog couldn't be shown",
                    Some(why),
                );
                Task::none()
            }
            Message::OpenPath(path) | Message::Instance(path) => Task::perform(
                async move { answer(repos::locate(&path).await) },
                Message::Located,
            ),
            Message::Located(Ok(path)) => self.opened(path),
            Message::Located(Err(error)) => {
                self.toast(ToastKind::Error, error, None);
                Task::none()
            }
            Message::Restored(result) => {
                let front = self.restore_waiting == Some(true);
                let opened = match result {
                    Ok(path) => {
                        if front {
                            self.restore_front = Some(path.clone());
                        }
                        self.opened(path)
                    }
                    Err(error) => {
                        self.toast(ToastKind::Error, error, None);
                        Task::none()
                    }
                };
                Task::batch([opened, self.restore_next()])
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
                Task::none()
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
                Some(tab) => load_all(tab.path.clone(), tab.log_query()),
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
            Message::BisectLoaded(path, result) => {
                if let (Some(tab), Ok(bisect)) = (self.tab_mut(&path), result) {
                    tab.bisect = bisect;
                }
                Task::none()
            }
            Message::UndoLoaded(path, result) => {
                if let (Some(tab), Ok(labels)) = (self.tab_mut(&path), result) {
                    tab.undo = labels;
                }
                Task::none()
            }
            Message::Undo | Message::Redo => {
                let back = matches!(message, Message::Undo);
                let Some(tab) = self.current() else {
                    return Task::none();
                };
                let label = if back { &tab.undo.undo } else { &tab.undo.redo };
                match (label.clone(), self.busy.is_none()) {
                    (Some(label), true) if back => self.run_command(Command::Undo { label }),
                    (Some(label), true) => self.run_command(Command::Redo { label }),
                    _ => Task::none(),
                }
            }
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
                let step = self.settings.history.log_limit;
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                tab.log_limit += step;
                tab.loading = true;
                load_log(tab.path.clone(), tab.log_query())
            }
            Message::SetBranchFilter(all) => {
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                tab.all_branches = all;
                tab.loading = true;
                scroll_to(tab, 0.0);
                load_log(tab.path.clone(), tab.log_query())
            }
            Message::Select(sha) => {
                let (command, shift) = (self.modifiers.command, self.modifiers.shift);
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                if command || shift {
                    let listed: Vec<String> = tab
                        .visible()
                        .iter()
                        .map(|commit| commit.sha.clone())
                        .collect();
                    tab.picked_commits = logic::pick_commits(
                        &listed,
                        &tab.picked_commits,
                        tab.selected.as_deref(),
                        &sha,
                        shift,
                    );
                    // Two commits picked with Ctrl/Cmd+click are compared, as before.
                    if command
                        && let [first, second] = &tab.picked_commits[..]
                        && let Some(selected) = tab.selected.clone()
                    {
                        let other = if *first == selected { second } else { first };
                        return Task::done(Message::CompareStart(selected, other.clone()));
                    }
                    tab.compare = None;
                    return Task::none();
                }
                tab.picked_commits.clear();
                tab.compare = None;
                self.select(sha)
            }
            Message::PlanCommits(preset) => self.plan_commits(preset),
            Message::RefVisibility(change) => {
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                let view = &mut tab.ref_view;
                match change {
                    RefVisibility::Hide(name) => {
                        view.solo.retain(|other| *other != name);
                        if !view.hidden.contains(&name) {
                            view.hidden.push(name);
                        }
                    }
                    RefVisibility::Show(name) => view.hidden.retain(|other| *other != name),
                    RefVisibility::Solo(name) => {
                        view.hidden.retain(|other| *other != name);
                        if !view.solo.contains(&name) {
                            view.solo.push(name);
                        }
                    }
                    RefVisibility::Unsolo(name) => view.solo.retain(|other| *other != name),
                    RefVisibility::ShowAll => *view = RefView::default(),
                }
                let (path, view) = (tab.path.clone(), tab.ref_view.clone());
                tab.loading = true;
                let reload = load_log(path.clone(), tab.log_query());
                if view.is_empty() {
                    self.settings.graph.views.remove(&path);
                } else {
                    self.settings.graph.views.insert(path, view);
                }
                self.save_settings();
                reload
            }
            Message::SetHistoryFilter(filter) => {
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                tab.history_filter = filter.filter(|filter| !filter.is_empty());
                tab.view = View::History;
                tab.search = None;
                tab.loading = true;
                scroll_to(tab, 0.0);
                load_log(tab.path.clone(), tab.log_query())
            }
            Message::OpenReplay(shas) => self.open_replay(shas),
            Message::ShowCommit(sha) => {
                self.overlay = None;
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                tab.view = View::History;
                tab.compare = None;
                tab.search = None;
                let index = tab.commits.iter().position(|commit| commit.sha == sha);
                if let Some(index) = index {
                    self.reveal(index);
                }
                self.select(sha)
            }
            Message::ShowRef(key, sha) => {
                let task = self.update(Message::ShowCommit(sha));
                if let Some(tab) = self.current_mut() {
                    tab.selected_ref = Some(key);
                }
                task
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
            Message::HighlightsLoaded(repo, key, token, dark, highlights) => {
                // Only colours for the diff as it is now, for the theme as it is now, and while
                // highlighting is still on, count; anything older is dropped.
                let current = dark == self.look.dark && self.settings.diff.syntax_highlight;
                let pane = self.pane_mut(&repo, key);
                if let Some(pane) = pane.filter(|pane| pane.token == token && current) {
                    pane.highlights = highlights;
                    pane.highlighted_dark = Some(dark);
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
                let shift = self.modifiers.shift;
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
            Message::HunkRevert(hunk) => {
                let context = self.settings.diff.context;
                let Some(tab) = self.current() else {
                    return Task::none();
                };
                let (Some(detail), Some(path)) = (&tab.detail, &tab.selected_file) else {
                    return Task::none();
                };
                let lines = tab.commit_pane.selected_in(hunk);
                let chosen = (!lines.is_empty()).then(|| lines.into_iter().collect());
                Task::done(Message::Run(Command::RevertSelection {
                    sha: detail.sha.clone(),
                    path: path.clone(),
                    selection: BTreeMap::from([(hunk, chosen)]),
                    context,
                }))
            }
            Message::ToggleSplit => {
                let split = !self.settings.diff.split;
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
                scroll_to(tab, 0.0);
                Task::none()
            }
            Message::SearchFocus => {
                self.focus_search.set(true);
                Task::none()
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
            Message::SelectAllFiles => {
                let picked = self
                    .current_mut()
                    .filter(|tab| tab.view == View::Changes)
                    .is_some_and(pick_all_in);
                if picked {
                    self.reload_changes()
                } else {
                    Task::none()
                }
            }
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
            Message::BodyInput(text) => {
                if let Some(tab) = self.current_mut() {
                    tab.body = text;
                }
                Task::none()
            }
            Message::AmendToggled(amend) => {
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                tab.amend = amend;
                if !amend || !tab.summary.is_empty() || !tab.body.trim().is_empty() {
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
                            && tab.body.trim().is_empty()
                        {
                            let (summary, body) = logic::split_message(&message);
                            tab.summary = summary;
                            tab.body = body;
                        }
                    }
                    Err(error) => self.toast(ToastKind::Error, error, None),
                }
                Task::none()
            }
            Message::CommitNow | Message::CommitAll => {
                let stage_all = matches!(message, Message::CommitAll);
                let Some(tab) = self.current() else {
                    return Task::none();
                };
                let ready = if stage_all {
                    tab.can_commit_all()
                } else {
                    tab.can_commit()
                };
                if !ready {
                    return Task::none();
                }
                if tab.amend
                    && tab
                        .status
                        .as_ref()
                        .is_some_and(logic::amend_rewrites_pushed)
                {
                    let upstream = tab
                        .status
                        .as_ref()
                        .and_then(|status| status.branch.upstream.clone())
                        .unwrap_or_default();
                    self.form = Some(Form::confirm(
                        "Amend a pushed commit",
                        &trf(
                            "The last commit is already on {upstream}. Amending it rewrites history others may have, and the branch then needs a force push.",
                            &[("upstream", &upstream)],
                        ),
                        "Amend",
                        true,
                        Message::CommitConfirmed { stage_all },
                    ));
                    return Task::none();
                }
                Task::done(Message::CommitConfirmed { stage_all })
            }
            Message::CommitConfirmed { stage_all } => {
                let prefs = self.settings.commit.clone();
                let Some(tab) = self.current_mut() else {
                    return Task::none();
                };
                tab.committing = true;
                let message = logic::commit_message(&tab.summary, &tab.body);
                let options = CommitOptions {
                    sign_off: prefs.sign_off,
                    no_verify: prefs.skip_hooks,
                    stage_all,
                    trailers: logic::co_author_trailers(&tab.co_authors),
                };
                let amend = tab.amend;
                Task::done(Message::Run(Command::Commit {
                    message,
                    amend,
                    options,
                }))
            }
            Message::CommitOption(option, on) => {
                let prefs = &mut self.settings.commit;
                match option {
                    CommitOption::SignOff => prefs.sign_off = on,
                    CommitOption::SkipHooks => prefs.skip_hooks = on,
                    CommitOption::PushAfter => prefs.push_after = on,
                }
                self.save_settings();
                Task::none()
            }
            Message::CoAuthors(text) => {
                if let Some(tab) = self.current_mut() {
                    tab.co_authors = text;
                }
                Task::none()
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
            Message::RunIn(repo, command) => {
                if self.tabs.iter().any(|tab| tab.path == repo) {
                    self.run_in(repo, command)
                } else {
                    Task::none()
                }
            }
            Message::PullChosen {
                remote,
                branch,
                mode,
                autostash,
                backup,
            } => {
                // Overwriting is never remembered as the usual strategy.
                if mode != "overwrite" {
                    self.settings.pull.mode = mode.clone();
                }
                self.settings.pull.autostash = autostash;
                self.settings.pull.backup = backup;
                self.save_settings();
                if mode == "overwrite" {
                    let source = match (&remote, &branch) {
                        (Some(remote), Some(branch)) => format!("{remote}/{branch}"),
                        _ => tr("its upstream").to_owned(),
                    };
                    let kept = if backup {
                        tr("The current state is kept as a backup branch first.")
                    } else {
                        tr("There is no backup: commits only on this branch are lost.")
                    };
                    let command = Command::Pull {
                        remote,
                        branch,
                        mode,
                        autostash,
                        backup,
                    };
                    self.form = Some(Form::confirm(
                        "Overwrite local branch?",
                        &format!(
                            "{} {kept}",
                            trf(
                                "The current branch will be reset to {source}. Local commits and uncommitted changes are discarded.",
                                &[("source", &source)],
                            )
                        ),
                        "Overwrite",
                        true,
                        Message::Run(command),
                    ));
                    return Task::none();
                }
                self.run_command(Command::Pull {
                    remote,
                    branch,
                    mode,
                    autostash,
                    backup,
                })
            }
            Message::RebaseChosen { onto, autostash } => {
                self.settings.rebase.autostash = autostash;
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
            Message::FormArea(index, text) => {
                if let Some(FieldKind::Area(value)) = self
                    .form
                    .as_mut()
                    .and_then(|form| form.fields.get_mut(index))
                    .map(|field| &mut field.kind)
                {
                    *value = text;
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
            Message::CommitFirst(repo, checkout) => {
                let Some(index) = self.tabs.iter().position(|tab| tab.path == repo) else {
                    return Task::none();
                };
                self.active = index;
                let tab = &mut self.tabs[index];
                tab.after_commit = Some(checkout);
                // The note about the checkout takes the place of "Amend last commit": a new
                // commit is what the dialog promised.
                tab.amend = false;
                tab.view = View::Changes;
                self.reload_changes()
            }
            Message::CommitFirstCancel => {
                if let Some(tab) = self.current_mut() {
                    tab.after_commit = None;
                }
                Task::none()
            }
            Message::FormChoice(index) => {
                let Some(form) = self.form.take() else {
                    return Task::none();
                };
                match form.choices.into_iter().nth(index) {
                    Some(choice) => self.update(choice.message),
                    None => Task::none(),
                }
            }
            Message::FormCancel => {
                self.form = None;
                Task::none()
            }
            Message::OverlayClose => {
                self.overlay = None;
                Task::none()
            }
            Message::OpenReflog => self.open_reflog(false),
            Message::ReflogAll(all) => self.open_reflog(all),
            Message::ReflogLoaded(result) => {
                if let Some(Overlay::Reflog {
                    entries,
                    loading,
                    error,
                    ..
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
            Message::OpenDiscards => self.open_discards(),
            Message::RangeDiff(old, new) => {
                let Some(repo) = self.current().map(|tab| tab.path.clone()) else {
                    return Task::none();
                };
                self.overlay = Some(Overlay::RangeDiff {
                    old: old.clone(),
                    new: new.clone(),
                    text: String::new(),
                    loading: true,
                    error: None,
                });
                Task::perform(
                    async move { answer(queries::range_diff_ranges(&repo, &old, &new).await) },
                    Message::RangeDiffLoaded,
                )
            }
            Message::RangeDiffLoaded(result) => {
                if let Some(Overlay::RangeDiff {
                    text,
                    loading,
                    error,
                    ..
                }) = &mut self.overlay
                {
                    *loading = false;
                    match result {
                        Ok(found) => *text = found,
                        Err(message) => *error = Some(message),
                    }
                }
                Task::none()
            }
            Message::DiscardsLoaded(result) => {
                if let Some(Overlay::Discards {
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
                            state.result = data.working.clone().unwrap_or_default();
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
            Message::ConflictEdit(text) => {
                if let Some(Overlay::Conflict(state)) = &mut self.overlay {
                    state.edited |= state.result != text;
                    state.result = text;
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
                            if !state.apply_preset() {
                                state.error = Some(
                                    tr("Only commits of the current branch can be squashed or dropped together; merge commits can't be in between.")
                                        .to_owned(),
                                );
                            }
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
            Message::RebaseAutosquash => {
                if let Some(Overlay::Rebase(state)) = &mut self.overlay {
                    state.autosquash();
                }
                Task::none()
            }
            Message::RebaseExec(index, command) => {
                if let Some(Overlay::Rebase(state)) = &mut self.overlay
                    && let Some(step) = state.steps.get_mut(index)
                {
                    let command = command.trim().to_owned();
                    step.exec = (!command.is_empty()).then_some(command);
                    state.focused = index;
                }
                Task::none()
            }
            Message::RebaseStart => self.rebase_start(),
            Message::OpenSettings => {
                self.overlay = Some(Overlay::Settings);
                // Looked for again: a tool may have been installed since, and the repository
                // may set up tools of its own.
                let cwd = self.current().map_or_else(
                    || self.config_dir.to_string_lossy().into_owned(),
                    |tab| tab.path.clone(),
                );
                find_tools(cwd)
            }
            Message::ToolsFound(found) => {
                self.found_tools = found;
                Task::none()
            }
            Message::SetDiffTool(name) => {
                self.settings.tools.diff = name;
                self.save_settings();
                Task::none()
            }
            Message::SetMergeTool(name) => {
                self.settings.tools.merge = name;
                self.save_settings();
                Task::none()
            }
            Message::OpenDiffTool { path, orig, target } => {
                let tool = self.settings.tools.diff.clone();
                let Some(repo) = self.current().map(|tab| tab.path.clone()) else {
                    return Task::none();
                };
                if tool.is_empty() {
                    return Task::none();
                }
                let label = self.found_tools.label(false, &tool);
                self.toast(
                    ToastKind::Info,
                    trf("Opening {tool}…", &[("tool", &label)]),
                    None,
                );
                // In the background and without the busy state: the tool stays open as long
                // as the user likes, and GitCraft goes on working meanwhile.
                Task::perform(
                    async move {
                        let result =
                            ops::open_diff_tool(&repo, &tool, &path, orig.as_deref(), &target)
                                .await;
                        (repo, answer(result))
                    },
                    |(repo, result)| Message::ToolClosed(repo, false, result),
                )
            }
            Message::OpenMergeTool(path) => {
                let tool = self.settings.tools.merge.clone();
                let Some(repo) = self.current().map(|tab| tab.path.clone()) else {
                    return Task::none();
                };
                if tool.is_empty() {
                    return Task::none();
                }
                let label = self.found_tools.label(true, &tool);
                self.toast(
                    ToastKind::Info,
                    trf("Opening {tool}…", &[("tool", &label)]),
                    None,
                );
                Task::perform(
                    async move {
                        let result = ops::open_merge_tool(&repo, &tool, &path).await;
                        (repo, answer(result))
                    },
                    |(repo, result)| Message::ToolClosed(repo, true, result),
                )
            }
            Message::ToolClosed(repo, merge, result) => {
                if let Err(error) = result {
                    let tool = if merge {
                        &self.settings.tools.merge
                    } else {
                        &self.settings.tools.diff
                    };
                    let label = self.found_tools.label(merge, tool);
                    self.toast(
                        ToastKind::Error,
                        trf("{tool} could not be opened", &[("tool", &label)]),
                        Some(error),
                    );
                }
                // A merge tool may have resolved the file.
                if merge && self.tabs.iter().any(|tab| tab.path == repo) {
                    load_status(repo)
                } else {
                    Task::none()
                }
            }
            Message::SetLanguage(code) => {
                crate::i18n::set_language(&code);
                self.settings.general.language = code;
                self.save_settings();
                Task::none()
            }
            Message::ToggleSystemTitleBar => {
                let system = &mut self.settings.appearance.system_title_bar;
                *system = !*system;
                self.save_settings();
                self.toast(
                    ToastKind::Info,
                    "The title bar changes when GitCraft starts again.",
                    None,
                );
                Task::none()
            }
            Message::SetTheme(choice) => {
                self.settings.appearance.theme = choice;
                self.save_settings();
                self.restyle()
            }
            Message::SystemTheme(dark) => {
                self.system_dark = dark;
                self.restyle()
            }
            Message::SetSplit(split) => {
                self.set_split(split);
                Task::none()
            }
            Message::SetContext(text) => {
                self.settings.diff.context = text
                    .trim()
                    .parse::<i64>()
                    .map_or(self.settings.diff.context, |n| n.clamp(0, 100));
                self.save_settings();
                self.reload_diffs()
            }
            Message::SetIgnoreWhitespace(value) => {
                self.settings.diff.ignore_whitespace = value;
                self.save_settings();
                self.reload_diffs()
            }
            Message::SetSyntax(value) => {
                self.settings.diff.syntax_highlight = value;
                self.save_settings();
                self.recolour_all()
            }
            Message::SetLogLimit(text) => {
                self.settings.history.log_limit = text
                    .trim()
                    .parse::<usize>()
                    .map_or(self.settings.history.log_limit, |n| n.clamp(100, 100_000));
                self.save_settings();
                Task::none()
            }
            Message::SetCommitGraph(value) => {
                self.settings.history.commit_graph = value;
                self.save_settings();
                Task::none()
            }
            Message::SetConflictCheck(value) => {
                self.settings.history.conflict_check = value;
                self.save_settings();
                Task::none()
            }
            Message::ConflictPreview(target, result) => {
                // Only into the dialog it was asked for.
                if self.conflict_preview.as_deref() == Some(target.as_str())
                    && let Some(form) = &mut self.form
                    && let Ok(Some(files)) = result
                {
                    let note = match &files[..] {
                        [] => tr("No conflicts expected.").to_owned(),
                        [one] => trf("Expect a conflict in {path}.", &[("path", one)]),
                        many => trf(
                            "Expect conflicts in {files}: {names}",
                            &[
                                ("files", &plural(many.len(), "file")),
                                ("names", &crate::logic::list_short(many, 5)),
                            ],
                        ),
                    };
                    let message = form.message.take().unwrap_or_default();
                    form.message = Some(format!("{message}\n{note}").trim().to_owned());
                }
                Task::none()
            }
            Message::SetFetchOnOpen(value) => {
                self.settings.history.fetch_on_open = value;
                self.save_settings();
                Task::none()
            }
            Message::Zoom(factor) => {
                self.settings.appearance.zoom = if factor == 0.0 {
                    1.0
                } else {
                    (self.settings.appearance.zoom * factor).clamp(0.5, 3.0)
                };
                self.save_settings();
                Task::none()
            }
            Message::Copy(text) => {
                self.toast(ToastKind::Success, "Copied to clipboard", None);
                self.effects.push(Effect::Copy(text));
                Task::none()
            }
            Message::CopyFileText(rev, path) => {
                let Some(repo) = self.current().map(|tab| tab.path.clone()) else {
                    return Task::none();
                };
                Task::perform(
                    async move { answer(queries::file_text(&repo, &rev, &path).await) },
                    Message::FileTextLoaded,
                )
            }
            Message::FileTextLoaded(result) => match result {
                Ok(text) => Task::done(Message::Copy(text)),
                Err(error) => {
                    self.toast(ToastKind::Error, "Could not copy the file", Some(error));
                    Task::none()
                }
            },
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
                    .map(RepoTab::log_query);
                let Some(query) = found else {
                    return Task::none();
                };
                if git {
                    load_all(path, query)
                } else {
                    load_status(path)
                }
            }
            Message::Watched(path, WatchEvent::Error(error)) => {
                self.toast(
                    ToastKind::Warning,
                    trf("Live refresh stopped for {path}", &[("path", &path)]),
                    Some(error),
                );
                Task::none()
            }
            Message::Key(key, modifiers) => self.key(key, modifiers),
            Message::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
                Task::none()
            }
            Message::CursorMoved(position) => {
                self.cursor = position;
                Task::none()
            }
            Message::Resize(splitter, delta) => {
                self.resize(splitter, delta);
                Task::none()
            }
            Message::ResizeDone => {
                self.save_settings();
                Task::none()
            }
            Message::WindowResized(size) => {
                self.window = size;
                Task::none()
            }
            Message::CloseRequested => {
                self.save_settings();
                Task::none()
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
            Message::Window(action) => {
                self.effects.push(Effect::Window(action));
                Task::none()
            }
            Message::Quit => {
                self.save_settings();
                self.effects.push(Effect::Quit);
                Task::none()
            }
        }
    }

    fn opened(&mut self, path: String) -> Task<Message> {
        if let Err(error) = self.store.add(&path) {
            self.toast(
                ToastKind::Error,
                trf(
                    "Could not save the recent list: {error}",
                    &[("error", &error.to_string())],
                ),
                None,
            );
        }
        self.recent = self.store.list();
        if let Some(index) = self.tabs.iter().position(|tab| tab.path == path) {
            self.active = index;
            self.save_settings();
            return Task::none();
        }
        let limit = self.settings.history.log_limit;
        let mut tab = RepoTab::new(path.clone(), limit);
        tab.viewport = self.history_list_height();
        tab.ref_view = self
            .settings
            .graph
            .views
            .get(&path)
            .cloned()
            .unwrap_or_default();
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
        let query = LogQuery {
            limit,
            all: true,
            filter: LogFilter::default(),
            view: self
                .settings
                .graph
                .views
                .get(&path)
                .cloned()
                .unwrap_or_default(),
        };
        let mut tasks = vec![load_all(path, query), template];
        if self.settings.history.fetch_on_open {
            tasks.push(Task::done(Message::Run(Command::Fetch {
                remote: None,
                tags: false,
            })));
        }
        if self.settings.history.commit_graph {
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
        self.settings.diff.split = split;
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
        (self.settings.layout.history_height - 60.0).max(ROW_HEIGHT)
    }

    fn resize(&mut self, splitter: Splitter, delta: f32) {
        let settings = &mut self.settings;
        match splitter {
            Splitter::Sidebar => {
                settings.layout.sidebar_width =
                    (settings.layout.sidebar_width + delta).clamp(160.0, 600.0);
            }
            Splitter::History => {
                settings.layout.history_height =
                    (settings.layout.history_height + delta).clamp(120.0, 2000.0);
                let height = self.history_list_height();
                self.tabs.iter_mut().for_each(|tab| tab.viewport = height);
            }
            Splitter::Details => {
                settings.layout.details_width =
                    (settings.layout.details_width + delta).clamp(220.0, 900.0);
            }
            Splitter::Changes => {
                settings.layout.changes_width =
                    (settings.layout.changes_width + delta).clamp(260.0, 900.0);
            }
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
        tab.selected_ref = None;
        tab.files_focused = false;
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
        tab.files_focused = true;
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
        let (context, ignore_ws) = (
            self.settings.diff.context,
            self.settings.diff.ignore_whitespace,
        );
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
        let split = self.settings.diff.split;
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
                    pane.highlighted_dark = None;
                    pane.offset = 0.0;
                    pane.rebuild_rows(split);
                    pane.scroll_to.set(Some(0.0));
                }
                task = self.colour(repo, key);
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

    /// Colours a pane's diff for the current theme, unless it already has them. Colours are made
    /// for one theme: they are made again after it changes, and dropped when highlighting is off.
    fn colour(&mut self, repo: &str, key: PaneKey) -> Task<Message> {
        let syntax = self.settings.diff.syntax_highlight;
        let dark = self.look.dark;
        let Some(pane) = self.pane_mut(repo, key) else {
            return Task::none();
        };
        if !syntax {
            pane.highlights = None;
            pane.highlighted_dark = None;
            return Task::none();
        }
        let Some(diff) = pane
            .diff
            .clone()
            .filter(|_| pane.highlighted_dark != Some(dark))
        else {
            return Task::none();
        };
        let (repo, token) = (repo.to_owned(), pane.token);
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || highlight::highlight(&diff, dark))
                    .await
                    .ok()
                    .flatten()
            },
            move |highlights| Message::HighlightsLoaded(repo.clone(), key, token, dark, highlights),
        )
    }

    /// After the theme turned light or dark: every open diff, in every tab and in the file
    /// history, is coloured again. The old colours are dropped at once, as they don't fit.
    fn recolour_all(&mut self) -> Task<Message> {
        let mut panes: Vec<(String, PaneKey)> = self
            .tabs
            .iter()
            .flat_map(|tab| {
                [
                    (tab.path.clone(), PaneKey::Changes),
                    (tab.path.clone(), PaneKey::Commit),
                ]
            })
            .collect();
        if let Some(repo) = self.current().map(|tab| tab.path.clone()) {
            panes.push((repo, PaneKey::Overlay));
        }
        let tasks: Vec<Task<Message>> = panes
            .into_iter()
            .map(|(repo, key)| {
                if let Some(pane) = self.pane_mut(&repo, key) {
                    pane.highlights = None;
                    pane.highlighted_dark = None;
                }
                self.colour(&repo, key)
            })
            .collect();
        Task::batch(tasks)
    }

    fn hunk_action(&mut self, hunk: usize, mode: &'static str) -> Task<Message> {
        let context = self.settings.diff.context;
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
        if mode == "stash" {
            return Task::done(Message::Run(Command::StashSelection {
                path: path.clone(),
                selection,
                context,
            }));
        }
        let command = Command::Selection {
            path: path.clone(),
            mode,
            selection,
            context,
        };
        if mode == "discard" {
            let what = if count > 0 {
                trf("{lines} selected", &[("lines", &plural(count, "line"))])
            } else {
                tr("this hunk").to_owned()
            };
            self.form = Some(Form::confirm(
                "Discard changes",
                &trf(
                    "Discard {what}? A backup is kept: Repository › Recover discarded changes.",
                    &[("what", &what)],
                ),
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
                scroll_to(tab, 0.0);
                first.map_or_else(Task::none, |sha| self.select(sha))
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

    /// What the user chose in a file dialog.
    fn picked(&mut self, pick: Pick, path: PathBuf) -> Task<Message> {
        let act = |action| Message::Action(crate::actions::Action::PatchSaved(action));
        match pick {
            Pick::OpenRepository => {
                Task::done(Message::OpenPath(path.to_string_lossy().into_owned()))
            }
            Pick::SavePatch { repo, of, .. } => Task::perform(
                async move {
                    let patch = match of {
                        PatchOf::Commit(sha) => gitcraft_core::ops::format_patch(&repo, &sha).await,
                        PatchOf::Series(sha) => {
                            gitcraft_core::ops::format_patch_series(&repo, &sha).await
                        }
                        PatchOf::Changes => gitcraft_core::ops::working_patch(&repo).await,
                    }
                    .map_err(|error| error.to_string())?;
                    craft_atomic::atomic_write(&path, patch.as_bytes())
                        .map_err(|error| error.to_string())?;
                    Ok(Some(path.to_string_lossy().into_owned()))
                },
                act,
            ),
            // Read in the background: the file may sit on a slow drive.
            Pick::ApplyPatch => Task::perform(
                async move {
                    let bytes = tokio::task::spawn_blocking({
                        let path = path.clone();
                        move || std::fs::read(path)
                    })
                    .await
                    .ok()?
                    .ok()?;
                    let name = path
                        .file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                    Some((name, String::from_utf8_lossy(&bytes).into_owned()))
                },
                |picked| Message::Action(crate::actions::Action::PatchPicked(picked)),
            ),
        }
    }

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
        let Some(tab) = self.current() else {
            return Task::none();
        };
        // A checkout with uncommitted changes in the way first asks what to do with them.
        if let Command::Checkout { reference, kind } = &command
            && tab.status.as_ref().is_some_and(logic::changes_in_the_way)
        {
            self.form = Some(checkout_with_changes(&tab.path, reference, kind));
            return Task::none();
        }
        let repo = tab.path.clone();
        self.run_in(repo, command)
    }

    pub(crate) fn run_in(&mut self, repo: String, command: Command) -> Task<Message> {
        let id = self.next_id;
        self.next_id += 1;
        self.busy = Some((id, command.label()));
        self.progress_line.clear();
        let cancellable = command.cancellable();
        let undoable = command.undo_kind();
        let (sender, lines) = tokio::sync::mpsc::unbounded_channel();
        let progress: Progress = Arc::new(move |line| {
            let _ = sender.send(line);
        });
        let work = Box::pin(async move {
            // What the command changes is kept so Edit › Undo can take it back. Undo is a
            // convenience: when keeping it fails, the command itself still counts.
            let before = match undoable {
                Some(_) => undo::capture(&repo).await.ok(),
                None => None,
            };
            let result = command.clone().run(&repo, Some(progress)).await;
            if let (Some((label, mode)), Some(before), Ok(done)) = (undoable, before, &result)
                && done.ok
            {
                let _ = undo::record(&repo, label, mode, before).await;
            }
            (repo, command, result)
        });
        type Work = std::pin::Pin<
            Box<
                dyn std::future::Future<Output = (String, Command, Result<OpResult, Error>)> + Send,
            >,
        >;
        // Progress lines come first, then the result; dropping the stream kills the process.
        let task = Task::stream(futures::stream::unfold(
            Some((work as Work, lines)),
            move |state| async move {
                let (mut work, mut lines) = state?;
                tokio::select! {
                    (repo, command, result) = &mut work => {
                        Some((Message::Done(repo, id, command, answer(result)), None))
                    }
                    Some(line) = lines.recv() => {
                        Some((Message::Progress(id, line), Some((work, lines))))
                    }
                }
            },
        ));
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
            Err(error) => self.toast(
                ToastKind::Error,
                trf("{label} failed", &[("label", &label)]),
                Some(error),
            ),
        }
        let mut then = None;
        let mut push: Option<Task<Message>> = None;
        let mut refresh_list = false;
        match &command {
            Command::Commit { .. } => {
                let push_after = self.settings.commit.push_after;
                if let Some(tab) = self.tab_mut(&repo) {
                    tab.committing = false;
                    if succeeded {
                        tab.summary.clear();
                        tab.body.clear();
                        tab.amend = false;
                        then = tab.after_commit.take();
                        // A checkout waiting for this commit goes first; the push can wait.
                        if then.is_none() && push_after {
                            push = Some(push_after_commit(tab));
                        }
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
            Command::RestoreDiscard { .. } if succeeded => {
                if matches!(self.overlay, Some(Overlay::Discards { .. })) {
                    self.overlay = None;
                }
                if let Some(tab) = self.tab_mut(&repo) {
                    tab.view = View::Changes;
                }
            }
            Command::DeleteDiscard { .. }
                if succeeded && matches!(self.overlay, Some(Overlay::Discards { .. })) =>
            {
                refresh_list = true;
            }
            _ => {}
        }
        let found = self
            .tabs
            .iter()
            .find(|tab| tab.path == repo)
            .map(RepoTab::log_query);
        let Some(query) = found else {
            return Task::none();
        };
        // The checkout waiting for this commit goes straight to git: what is left uncommitted
        // was left on purpose, so it isn't asked about again.
        let then = then.map_or_else(Task::none, |checkout| self.run_in(repo.clone(), checkout));
        let reload = if command.status_only() {
            load_status(repo)
        } else {
            load_all(repo, query)
        };
        let list = if refresh_list {
            self.open_discards()
        } else {
            Task::none()
        };
        Task::batch([reload, then, list, push.unwrap_or_else(Task::none)])
    }

    // ----------------------------------------------------------- overlays

    fn open_reflog(&mut self, all: bool) -> Task<Message> {
        let Some(path) = self.current().map(|tab| tab.path.clone()) else {
            return Task::none();
        };
        self.overlay = Some(Overlay::Reflog {
            all,
            entries: Vec::new(),
            loading: true,
            error: None,
        });
        Task::perform(
            async move {
                let entries = if all {
                    queries::reflog_all(&path, REFLOG_LIMIT).await
                } else {
                    queries::reflog(&path, "HEAD", REFLOG_LIMIT).await
                };
                answer(entries)
            },
            Message::ReflogLoaded,
        )
    }

    fn open_discards(&mut self) -> Task<Message> {
        let Some(path) = self.current().map(|tab| tab.path.clone()) else {
            return Task::none();
        };
        self.overlay = Some(Overlay::Discards {
            entries: Vec::new(),
            loading: true,
            error: None,
        });
        Task::perform(
            async move { answer(queries::discard_backups(&path).await) },
            Message::DiscardsLoaded,
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
            result: String::new(),
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
            state.result = logic::build_result(&data.chunks, &state.choices);
        }
        state.edited = false;
        Task::none()
    }

    fn conflict_save(&mut self) -> Task<Message> {
        let Some(Overlay::Conflict(state)) = &self.overlay else {
            return Task::none();
        };
        let content = state.result.clone();
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
        let head = tab
            .status
            .as_ref()
            .and_then(|status| status.branch.oid.clone());
        let base_label = match &base {
            None => tr("the root commit").to_owned(),
            Some(base) => match tab.commits.iter().find(|commit| commit.sha == *base) {
                Some(commit) => format!("{} “{}”", short_sha(base), commit.subject),
                None => short_sha(base).to_owned(),
            },
        };
        self.overlay = Some(Overlay::Rebase(Box::new(RebaseState {
            head,
            replay: false,
            preset: None,
            base: base.clone(),
            base_label,
            steps: Vec::new(),
            original: Vec::new(),
            loading: true,
            error: None,
            autostash: true,
            focused: 0,
        })));
        Task::perform(
            async move { answer(queries::rebase_candidates(&repo, base.as_deref()).await) },
            Message::RebaseLoaded,
        )
    }

    /// The rebase editor with several picked commits squashed or dropped, from the parent
    /// of the oldest of them.
    fn plan_commits(&mut self, preset: Preset) -> Task<Message> {
        let Some(tab) = self.current() else {
            return Task::none();
        };
        let (Preset::Drop(shas) | Preset::Squash(shas)) = &preset;
        // Newest first as listed, so the oldest is the last one found.
        let oldest = tab
            .commits
            .iter()
            .rev()
            .find(|commit| shas.contains(&commit.sha));
        let Some(oldest) = oldest else {
            return Task::none();
        };
        let base = oldest.parents.first().cloned();
        let task = self.open_rebase(base);
        if let Some(Overlay::Rebase(state)) = &mut self.overlay {
            state.preset = Some(preset);
        }
        task
    }

    /// The rebase editor in cherry-pick mode: the commits, oldest first, onto HEAD.
    fn open_replay(&mut self, shas: Vec<String>) -> Task<Message> {
        let Some(tab) = self.current() else {
            return Task::none();
        };
        let original: Vec<Commit> = tab
            .commits
            .iter()
            .rev()
            .filter(|commit| shas.contains(&commit.sha))
            .cloned()
            .collect();
        if original.is_empty() {
            return Task::none();
        }
        let mut state = RebaseState {
            head: tab
                .status
                .as_ref()
                .and_then(|status| status.branch.oid.clone()),
            replay: true,
            preset: None,
            base: None,
            base_label: "HEAD".to_owned(),
            steps: Vec::new(),
            original,
            loading: false,
            error: None,
            autostash: true,
            focused: 0,
        };
        state.reset();
        self.overlay = Some(Overlay::Rebase(Box::new(state)));
        Task::none()
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
                exec: step.exec.clone(),
            })
            .collect();
        let command = Command::InteractiveRebase {
            expected_head: state.head.clone(),
            base: state.base.clone(),
            steps,
            autostash: state.autostash,
            replay: state.replay,
        };
        Task::done(Message::Run(command))
    }

    // ----------------------------------------------------------- keyboard

    /// Scrolls just far enough to bring a row into view.
    fn reveal(&mut self, index: usize) {
        let Some(tab) = self.current_mut() else {
            return;
        };
        let top = index as f32 * ROW_HEIGHT;
        let target = if top < tab.offset {
            top
        } else if top + ROW_HEIGHT > tab.offset + tab.viewport {
            top + ROW_HEIGHT - tab.viewport
        } else {
            return;
        };
        scroll_to(tab, target);
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) -> Task<Message> {
        if key == Key::Escape {
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
            return match key {
                Key::ArrowUp if modifiers.alt => Task::done(Message::RebaseMove(focused, -1)),
                Key::ArrowDown if modifiers.alt => Task::done(Message::RebaseMove(focused, 1)),
                Key::ArrowUp => Task::done(Message::RebaseFocus(focused.saturating_sub(1))),
                Key::ArrowDown => Task::done(Message::RebaseFocus(
                    (focused + 1).min(state.steps.len().saturating_sub(1)),
                )),
                letter if !editing && !modifiers.command => {
                    let action = match letter {
                        Key::P => "pick",
                        Key::R => "reword",
                        Key::E => "edit",
                        Key::S => "squash",
                        Key::F => "fixup",
                        Key::D => "drop",
                        _ => return Task::none(),
                    };
                    Task::done(Message::RebaseSetAction(focused, action))
                }
                _ => Task::none(),
            };
        }
        let command = modifiers.command;
        match key {
            Key::F5 => return Task::done(Message::Refresh),
            Key::F11 => return Task::done(Message::Window(WindowAction::ToggleFullscreen)),
            Key::R if command => return Task::done(Message::Refresh),
            Key::O | Key::T if command => return Task::done(Message::OpenDialog),
            Key::W if command => return Task::done(Message::Close(self.active)),
            Key::Comma if command => return Task::done(Message::OpenSettings),
            Key::F if command => return Task::done(Message::SearchFocus),
            Key::Num1 if command => return Task::done(Message::SetView(View::History)),
            Key::Num2 if command => return Task::done(Message::SetView(View::Changes)),
            Key::Equals | Key::Plus if command => return Task::done(Message::Zoom(1.1)),
            Key::Minus if command => return Task::done(Message::Zoom(1.0 / 1.1)),
            Key::Num0 if command => return Task::done(Message::Zoom(0.0)),
            Key::Enter if command && modifiers.shift => return Task::done(Message::CommitAll),
            Key::Enter if command => return Task::done(Message::CommitNow),
            Key::A if command => return Task::done(Message::SelectAllFiles),
            Key::Z if command && modifiers.shift => return Task::done(Message::Redo),
            Key::Z if command => return Task::done(Message::Undo),
            Key::Y if command => return Task::done(Message::Redo),
            _ => {}
        }
        if self.overlay.is_some() {
            return Task::none();
        }
        if let Some(task) = self.key_in_file_list(key) {
            return task;
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
        let target = match key {
            Key::ArrowDown => current.map_or(0, |i| (i + 1).min(last)),
            Key::ArrowUp => current.map_or(0, |i| i.saturating_sub(1)),
            Key::PageDown => current.map_or(0, |i| (i + page).min(last)),
            Key::PageUp => current.map_or(0, |i| i.saturating_sub(page)),
            Key::Home => 0,
            Key::End => last,
            _ => return Task::none(),
        };
        let sha = commits[target].sha.clone();
        self.reveal(target);
        self.select(sha)
    }

    /// Up and Down move through the file list of the view that has one (Local Changes
    /// or a stash), rather than through the commits.
    fn key_in_file_list(&mut self, key: Key) -> Option<Task<Message>> {
        let step: isize = match key {
            Key::ArrowUp => -1,
            Key::ArrowDown => 1,
            _ => return None,
        };
        let tab = self.current()?;
        let (paths, current, pick): FileList = match tab.view {
            View::Changes => {
                let (area, path) = tab.focus.clone()?;
                let paths = tab
                    .files_in(area)
                    .into_iter()
                    .map(|file| file.path)
                    .collect();
                (
                    paths,
                    Some(path),
                    Box::new(move |path| Message::FilePicked(area, path)),
                )
            }
            View::Stash => {
                let stash = tab.stash.as_ref()?;
                let paths = stash.files.iter().map(|file| file.path.clone()).collect();
                (paths, stash.selected.clone(), Box::new(Message::PickFile))
            }
            View::History if tab.files_focused => {
                let (paths, current) = match &tab.compare {
                    Some(compare) => (
                        compare.result.as_ref().map_or_else(Vec::new, |result| {
                            result.files.iter().map(|file| file.path.clone()).collect()
                        }),
                        compare.selected.clone(),
                    ),
                    None => (
                        tab.detail.as_ref().map_or_else(Vec::new, |detail| {
                            detail.files.iter().map(|file| file.path.clone()).collect()
                        }),
                        tab.selected_file.clone(),
                    ),
                };
                (paths, current, Box::new(Message::PickFile))
            }
            View::History => return None,
        };
        let at = current.and_then(|current| paths.iter().position(|path| *path == current));
        let target = match at {
            Some(at) => at
                .saturating_add_signed(step)
                .min(paths.len().saturating_sub(1)),
            None => 0,
        };
        Some(match paths.get(target) {
            Some(path) if at != Some(target) => Task::done(pick(path.clone())),
            _ => Task::none(),
        })
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

/// Asks git which diff and merge tools it can start from `cwd`.
fn find_tools(cwd: String) -> Task<Message> {
    Task::perform(
        async move {
            let (diff, merge) = tokio::join!(
                queries::external_tools(&cwd, false),
                queries::external_tools(&cwd, true),
            );
            FoundTools {
                diff: diff.unwrap_or_default(),
                merge: merge.unwrap_or_default(),
            }
        },
        Message::ToolsFound,
    )
}

/// What to do with uncommitted changes before checking out `reference`.
/// Every answer is bound to `repo`, the repository the question was asked for, even if another
/// tab is in front by the time it is answered.
fn checkout_with_changes(repo: &str, reference: &str, kind: &'static str) -> crate::form::Form {
    let name = if kind == "commit" {
        logic::short_sha(reference)
    } else {
        reference
    };
    let checkout = Command::Checkout {
        reference: reference.to_owned(),
        kind,
    };
    let stashing = |reapply| {
        Message::RunIn(
            repo.to_owned(),
            Command::CheckoutStashing {
                reference: reference.to_owned(),
                kind,
                reapply,
            },
        )
    };
    let mut form = crate::form::Form::confirm(
        "Uncommitted changes",
        "",
        "Stash and reapply",
        false,
        stashing(true),
    )
    .choice(
        "Commit first",
        false,
        Message::CommitFirst(repo.to_owned(), checkout),
    )
    .choice("Discard changes", true, stashing(false));
    let name = [("name", name)];
    let paragraphs = [
        trf("There are uncommitted changes. What should happen to them before {name} is checked out?", &name),
        trf("Stash and reapply puts them aside and brings them back on {name}. Where they clash with it, they stay in the stash and the clashes show in Local Changes.", &name),
        trf("Commit first takes you to the commit box and checks out {name} once you have committed.", &name),
        tr("Discard changes clears them away into a stash, from where they can still be brought back.").to_owned(),
    ];
    form.message = Some(paragraphs.join("\n\n"));
    form
}

/// Picks every file of the list with the focus (the unstaged files when none has it), keeping
/// the focused file in view. False when that list is empty.
fn pick_all_in(tab: &mut RepoTab) -> bool {
    let area = match &tab.focus {
        Some((area, _)) => *area,
        None if !tab.files_in(Area::Unstaged).is_empty() => Area::Unstaged,
        None => Area::Staged,
    };
    let paths: Vec<String> = tab
        .files_in(area)
        .into_iter()
        .map(|file| file.path)
        .collect();
    let Some(first) = paths.first().cloned() else {
        return false;
    };
    let focus = match &tab.focus {
        Some((at, path)) if *at == area && paths.contains(path) => path.clone(),
        _ => first.clone(),
    };
    tab.pick_anchor = Some(first);
    tab.picked = (area, paths);
    tab.focus = Some((area, focus));
    true
}

/// Ctrl toggles, Shift selects a range from the anchor, a plain click selects one file.
fn pick_file_in(tab: &mut RepoTab, area: Area, path: String, modifiers: Modifiers) {
    let list: Vec<String> = tab
        .files_in(area)
        .into_iter()
        .map(|file| file.path)
        .collect();
    let same_area = tab.picked.0 == area;
    let paths = if modifiers.command && same_area {
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
        .filter(|_| modifiers.shift && same_area)
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

/// Asks the history list to jump to a position the next time it is drawn.
fn scroll_to(tab: &mut RepoTab, offset: f32) {
    tab.offset = offset;
    tab.scroll_to.set(Some(offset));
}

/// A change to which branches the graph shows (full ref names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefVisibility {
    Hide(String),
    Show(String),
    Solo(String),
    Unsolo(String),
    ShowAll,
}

/// The commit box's options menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitOption {
    SignOff,
    SkipHooks,
    PushAfter,
}

/// "Push after committing": straight to the upstream when it has the branch's name,
/// otherwise the push dialog asks where.
fn push_after_commit(tab: &RepoTab) -> Task<Message> {
    let target = tab
        .status
        .as_ref()
        .and_then(|status| logic::push_target(status, &tab.refs.remotes));
    match target {
        Some((remote, branch)) => Task::done(Message::Run(Command::Push {
            remote,
            branch,
            set_upstream: false,
            force: false,
            tags: false,
        })),
        None => Task::done(Message::Action(crate::actions::Action::Push(None))),
    }
}

fn load_undo(path: String) -> Task<Message> {
    Task::perform(
        async move {
            let result = answer(undo::labels(&path).await);
            (path, result)
        },
        |(path, result)| Message::UndoLoaded(path, result),
    )
}

fn load_status(path: String) -> Task<Message> {
    let undo = load_undo(path.clone());
    let bisect = {
        let path = path.clone();
        Task::perform(
            async move {
                let result = answer(queries::bisect_state(&path).await);
                (path, result)
            },
            |(path, result)| Message::BisectLoaded(path, result),
        )
    };
    let status = Task::perform(
        async move {
            let result = answer(queries::status(&path).await);
            (path, result)
        },
        |(path, result)| Message::StatusLoaded(path, result),
    );
    Task::batch([status, undo, bisect])
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

/// How much of the history to load, from which branches, narrowed how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogQuery {
    pub limit: usize,
    pub all: bool,
    pub filter: LogFilter,
    pub view: RefView,
}

fn load_log(path: String, query: LogQuery) -> Task<Message> {
    Task::perform(
        async move {
            let LogQuery {
                limit,
                all,
                filter,
                view,
            } = query;
            let result =
                answer(queries::log_filtered(&path, limit, all, None, &filter, &view).await)
                    .map(|page| (page.commits, page.has_more));
            (path, result)
        },
        |(path, result)| Message::LogLoaded(path, result),
    )
}

fn load_all(path: String, query: LogQuery) -> Task<Message> {
    Task::batch([
        load_status(path.clone()),
        load_refs(path.clone()),
        load_log(path, query),
    ])
}

/// Reports changes in a repository until the future is dropped. A failure to start
/// watching is reported once.
pub async fn watch(repo: String, send: impl Fn(Message) -> bool) {
    let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
    let handle = watcher::watch(&repo, move |event| {
        let _ = sender.send(event);
    })
    .await;
    match handle {
        Ok(_handle) => {
            while let Some(event) = events.recv().await {
                if !send(Message::Watched(repo.clone(), event)) {
                    break;
                }
            }
        }
        Err(error) => {
            let event = WatchEvent::Error(error.to_string());
            send(Message::Watched(repo, event));
        }
    }
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
        pick_file_in(&mut tab, Area::Unstaged, "b".into(), Modifiers::NONE);
        assert_eq!(tab.picked.1, ["b"]);
        pick_file_in(&mut tab, Area::Unstaged, "d".into(), Modifiers::SHIFT);
        assert_eq!(tab.picked.1, ["b", "c", "d"]);
        pick_file_in(&mut tab, Area::Unstaged, "c".into(), Modifiers::COMMAND);
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
            head: None,
            replay: false,
            preset: None,
            base: None,
            base_label: String::new(),
            steps: Vec::new(),
            original: vec![commit("a"), commit("b"), commit("c")],
            loading: false,
            error: None,
            autostash: true,
            focused: 0,
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

    #[test]
    fn presets_drop_or_squash_the_picked_commits() {
        let commit = |sha: &str| Commit {
            sha: sha.into(),
            parents: Vec::new(),
            author: String::new(),
            email: String::new(),
            time: 0,
            refs: None,
            subject: sha.into(),
            graph: None,
            file: None,
        };
        let state = |preset| {
            let mut state = RebaseState {
                head: None,
                replay: false,
                preset: Some(preset),
                base: None,
                base_label: String::new(),
                steps: Vec::new(),
                original: ["a", "b", "c", "d"].map(commit).to_vec(),
                loading: false,
                error: None,
                autostash: true,
                focused: 0,
            };
            state.reset();
            let found = state.apply_preset();
            (found, state)
        };
        let plan = |state: &RebaseState| -> Vec<String> {
            state
                .steps
                .iter()
                .map(|step| format!("{} {}", step.action, step.commit.sha))
                .collect()
        };
        let (found, dropped) = state(Preset::Drop(vec!["b".into(), "d".into()]));
        assert!(found);
        assert_eq!(plan(&dropped), ["pick a", "drop b", "pick c", "drop d"]);
        let (_, squashed) = state(Preset::Squash(vec!["d".into(), "b".into()]));
        assert_eq!(plan(&squashed), ["pick a", "pick b", "squash d", "pick c"]);
        let (found, _) = state(Preset::Squash(vec!["x".into()]));
        assert!(!found, "a commit that isn't on the branch");
    }
}
