//! Settings and remembered layout, in a JSON file in the config folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub theme: String,
    pub diff_context: i64,
    pub ignore_whitespace: bool,
    pub diff_split: bool,
    pub syntax_highlight: bool,
    pub log_limit: usize,
    pub fetch_on_open: bool,
    /// Keep a commit-graph file in the repository, which makes history load far faster.
    pub commit_graph: bool,
    pub zoom: f32,
    pub window_width: f32,
    pub window_height: f32,
    pub sidebar_width: f32,
    pub history_height: f32,
    pub details_width: f32,
    pub changes_width: f32,
    pub tabs: Vec<String>,
    pub active_tab: usize,
    pub pull_mode: String,
    pub pull_autostash: bool,
    pub rebase_autostash: bool,
    pub collapsed: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".to_owned(),
            diff_context: 3,
            ignore_whitespace: false,
            diff_split: false,
            syntax_highlight: true,
            log_limit: 2000,
            fetch_on_open: false,
            commit_graph: true,
            zoom: 1.0,
            window_width: 1280.0,
            window_height: 800.0,
            sidebar_width: 260.0,
            history_height: 340.0,
            details_width: 360.0,
            changes_width: 380.0,
            tabs: Vec::new(),
            active_tab: 0,
            pull_mode: "merge".to_owned(),
            pull_autostash: false,
            rebase_autostash: false,
            collapsed: Vec::new(),
        }
    }
}

fn file_in(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

impl Settings {
    /// A missing or damaged file gives the defaults.
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(file_in(dir))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join("settings.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(self).expect("plain data serialises"),
        )?;
        std::fs::rename(tmp, file_in(dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_loads_and_tolerates_damage_and_old_files() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        let changed = Settings {
            diff_context: 9,
            tabs: vec!["/x".into()],
            ..Settings::default()
        };
        changed.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()), changed);
        std::fs::write(file_in(dir.path()), "{\"diffContext\": 7}").unwrap();
        let partial = Settings::load(dir.path());
        assert_eq!(partial.diff_context, 7);
        assert_eq!(partial.log_limit, 2000);
        std::fs::write(file_in(dir.path()), "nonsense").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
    }
}
