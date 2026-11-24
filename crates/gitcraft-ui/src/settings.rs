//! Settings and remembered layout, in `config.toml` in the config folder, kept by craft-settings
//! (atomic saves, a backup, damaged files moved aside). The `settings.json` earlier versions wrote
//! there is read once, when no `config.toml` exists yet, and left in place.
//!
//! The window's size and place are not here: craft-window keeps them in `window.json`
//! (see `window.rs`).

use std::path::Path;

use craft_settings::{LegacyFile, Location, OpenOptions, SettingsStore};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub general: General,
    pub appearance: Appearance,
    pub diff: Diff,
    pub tools: Tools,
    pub history: History,
    pub pull: Pull,
    pub rebase: Rebase,
    pub commit: Commit,
    pub graph: Graph,
    pub layout: Layout,
    pub session: Session,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    /// A language code such as `de`, or `auto` for the system's language.
    pub language: String,
}

impl Default for General {
    fn default() -> Self {
        Self {
            language: crate::i18n::AUTO.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// "system", "light" or "dark".
    pub theme: String,
    pub zoom: f32,
    /// Let the system draw the window's title bar instead of the app (takes effect at the next
    /// start).
    pub system_title_bar: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "system".to_owned(),
            zoom: 1.0,
            system_title_bar: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Diff {
    pub context: i64,
    pub ignore_whitespace: bool,
    pub split: bool,
    pub syntax_highlight: bool,
}

impl Default for Diff {
    fn default() -> Self {
        Self {
            context: 3,
            ignore_whitespace: false,
            split: false,
            syntax_highlight: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub log_limit: usize,
    pub fetch_on_open: bool,
    /// Keep a commit-graph file in the repository, which makes history load far faster.
    pub commit_graph: bool,
    /// Merge and rebase dialogs say beforehand which files would conflict.
    pub conflict_check: bool,
}

impl Default for History {
    fn default() -> Self {
        Self {
            log_limit: 2000,
            fetch_on_open: false,
            commit_graph: true,
            conflict_check: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pull {
    pub mode: String,
    pub autostash: bool,
    /// Keep a backup branch when a pull overwrites the local branch.
    pub backup: bool,
}

impl Default for Pull {
    fn default() -> Self {
        Self {
            mode: "merge".to_owned(),
            autostash: false,
            backup: true,
        }
    }
}

/// Branches hidden from or soloed in the graph, by repository path.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Graph {
    pub views: std::collections::BTreeMap<String, gitcraft_core::model::RefView>,
}

/// The commit box's options, remembered for the next commit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Commit {
    /// `--signoff`.
    pub sign_off: bool,
    /// `--no-verify`.
    pub skip_hooks: bool,
    /// Pushes the branch to its upstream once the commit is made.
    pub push_after: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rebase {
    pub autostash: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub sidebar_width: f32,
    pub history_height: f32,
    pub details_width: f32,
    pub changes_width: f32,
    /// Sidebar sections the user folded away.
    pub collapsed: Vec<String>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            sidebar_width: 260.0,
            history_height: 340.0,
            details_width: 360.0,
            changes_width: 380.0,
            collapsed: Vec::new(),
        }
    }
}

/// The external programs chosen for diffs and conflicts, by the name git knows them by
/// (`kdiff3`, `bc`, or a name set up as `difftool.<name>.cmd`). Empty: none chosen.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tools {
    pub diff: String,
    pub merge: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// The repositories open in tabs, in order.
    pub tabs: Vec<String>,
    pub active_tab: usize,
}

impl craft_settings::Settings for Settings {
    const APP_ID: &'static str = "gitcraft";
    const SCHEMA_VERSION: u32 = 1;
}

/// `settings.json` as versions up to 0.3 wrote it: one flat object, camelCase keys.
#[derive(Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct OldSettings {
    theme: String,
    diff_context: i64,
    ignore_whitespace: bool,
    diff_split: bool,
    syntax_highlight: bool,
    log_limit: usize,
    fetch_on_open: bool,
    commit_graph: bool,
    zoom: f32,
    sidebar_width: f32,
    history_height: f32,
    details_width: f32,
    changes_width: f32,
    tabs: Vec<String>,
    active_tab: usize,
    pull_mode: String,
    pull_autostash: bool,
    pull_backup: bool,
    rebase_autostash: bool,
    collapsed: Vec<String>,
}

impl Default for OldSettings {
    fn default() -> Self {
        let now = Settings::default();
        Self {
            theme: now.appearance.theme,
            diff_context: now.diff.context,
            ignore_whitespace: now.diff.ignore_whitespace,
            diff_split: now.diff.split,
            syntax_highlight: now.diff.syntax_highlight,
            log_limit: now.history.log_limit,
            fetch_on_open: now.history.fetch_on_open,
            commit_graph: now.history.commit_graph,
            zoom: now.appearance.zoom,
            sidebar_width: now.layout.sidebar_width,
            history_height: now.layout.history_height,
            details_width: now.layout.details_width,
            changes_width: now.layout.changes_width,
            tabs: now.session.tabs,
            active_tab: now.session.active_tab,
            pull_mode: now.pull.mode,
            pull_autostash: now.pull.autostash,
            pull_backup: now.pull.backup,
            rebase_autostash: now.rebase.autostash,
            collapsed: now.layout.collapsed,
        }
    }
}

fn parse_old(bytes: &[u8]) -> Result<Settings, String> {
    let old: OldSettings = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    Ok(Settings {
        general: General::default(),
        commit: Commit::default(),
        graph: Graph::default(),
        appearance: Appearance {
            theme: old.theme,
            zoom: old.zoom,
            system_title_bar: false,
        },
        diff: Diff {
            context: old.diff_context,
            ignore_whitespace: old.ignore_whitespace,
            split: old.diff_split,
            syntax_highlight: old.syntax_highlight,
        },
        history: History {
            log_limit: old.log_limit,
            fetch_on_open: old.fetch_on_open,
            commit_graph: old.commit_graph,
            ..History::default()
        },
        pull: Pull {
            mode: old.pull_mode,
            autostash: old.pull_autostash,
            backup: old.pull_backup,
        },
        rebase: Rebase {
            autostash: old.rebase_autostash,
        },
        layout: Layout {
            sidebar_width: old.sidebar_width,
            history_height: old.history_height,
            details_width: old.details_width,
            changes_width: old.changes_width,
            collapsed: old.collapsed,
        },
        session: Session {
            tabs: old.tabs,
            active_tab: old.active_tab,
        },
        tools: Tools::default(),
    })
}

fn open(dir: &Path) -> SettingsStore<Settings> {
    let legacy = vec![LegacyFile {
        path: dir.join("settings.json"),
        parse: parse_old,
    }];
    let (store, report) =
        SettingsStore::open_with(OpenOptions::new(Location::Dir(dir.to_path_buf())).legacy(legacy));
    if let Some(moved) = &report.broken_moved_to {
        eprintln!(
            "The settings file was damaged and was moved to {}; using the defaults.",
            moved.display()
        );
    }
    for error in &report.section_errors {
        eprintln!(
            "The [{}] part of the settings file couldn't be read and was reset: {}",
            error.section, error.message
        );
    }
    store
}

/// The window size an older version kept in `settings.json`, if there is one: the window opens
/// at that size until craft-window has saved its own `window.json`.
pub fn old_window_size(dir: &Path) -> Option<[f32; 2]> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Old {
        window_width: f32,
        window_height: f32,
    }
    let bytes = std::fs::read(dir.join("settings.json")).ok()?;
    let old: Old = serde_json::from_slice(&bytes).ok()?;
    let size = [old.window_width, old.window_height];
    size.iter()
        .all(|v| v.is_finite() && *v > 0.0)
        .then_some(size)
}

impl Settings {
    /// What was saved last time. A missing or damaged file gives the defaults.
    pub fn load(dir: &Path) -> Self {
        open(dir).get().clone()
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let mut store = open(dir);
        store.set(self.clone());
        store.flush().map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_loads_and_tolerates_damage() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        let mut changed = Settings::default();
        changed.diff.context = 9;
        changed.session.tabs = vec!["/x".into()];
        changed.save(dir.path()).unwrap();
        assert!(dir.path().join("config.toml").is_file());
        assert_eq!(Settings::load(dir.path()), changed);
        std::fs::write(dir.path().join("config.toml"), "not = [toml").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
    }

    #[test]
    fn the_old_window_size_is_found() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(old_window_size(dir.path()), None);
        let old = dir.path().join("settings.json");
        std::fs::write(&old, r#"{"windowWidth":900,"windowHeight":600}"#).unwrap();
        assert_eq!(old_window_size(dir.path()), Some([900.0, 600.0]));
        std::fs::write(&old, r#"{"windowWidth":900}"#).unwrap();
        assert_eq!(
            old_window_size(dir.path()),
            None,
            "only a whole size counts"
        );
    }

    #[test]
    fn the_old_settings_file_is_read_once() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("settings.json");
        std::fs::write(
            &old,
            r#"{"theme":"dark","diffContext":7,"pullMode":"rebase","tabs":["/a"],"windowWidth":900}"#,
        )
        .unwrap();
        let loaded = Settings::load(dir.path());
        assert_eq!(loaded.appearance.theme, "dark");
        assert_eq!(loaded.diff.context, 7);
        assert_eq!(loaded.pull.mode, "rebase");
        assert_eq!(loaded.session.tabs, ["/a"]);
        assert_eq!(
            loaded.history.log_limit, 2000,
            "missing keys keep their defaults"
        );
        assert!(parse_old(b"nonsense").is_err());
        // Once config.toml exists it wins, and the old file stays for a rollback.
        let mut newer = loaded;
        newer.diff.context = 1;
        newer.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).diff.context, 1);
        assert!(old.is_file());
    }
}
