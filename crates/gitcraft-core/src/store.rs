//! Persistent list of repositories the user has opened.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::model::RepoEntry;

const MAX_REPOS: usize = 50;

pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// `GITCRAFT_CONFIG_DIR`, or the folder earlier versions used on each OS.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("GITCRAFT_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    let base = if cfg!(windows) {
        dirs::data_local_dir()
    } else {
        dirs::config_dir()
    };
    base.unwrap_or_else(|| home_dir().join(".config"))
        .join("gitcraft")
}

#[derive(Debug)]
pub struct RepoStore {
    path: PathBuf,
    repos: Vec<RepoEntry>,
}

impl RepoStore {
    /// The store in the user's config folder.
    pub fn open() -> Self {
        Self::at(config_dir().join("repos.json"))
    }

    pub fn at(path: PathBuf) -> Self {
        let repos = load(&path);
        Self { path, repos }
    }

    /// The folder this store's file lives in.
    pub fn dir(&self) -> PathBuf {
        self.path
            .parent()
            .map_or_else(config_dir, Path::to_path_buf)
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("tmp");
        let body = serde_json::to_string_pretty(&json!({ "repos": self.repos }))
            .expect("plain data serialises");
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &self.path)
    }

    pub fn list(&self) -> Vec<RepoEntry> {
        self.repos
            .iter()
            .map(|repo| RepoEntry {
                exists: Some(Path::new(&repo.path).is_dir()),
                ..repo.clone()
            })
            .collect()
    }

    pub fn add(&mut self, path: &str) -> std::io::Result<RepoEntry> {
        self.repos.retain(|repo| repo.path != path);
        let entry = RepoEntry {
            path: path.to_owned(),
            name: Path::new(path)
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            last_opened: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64),
            exists: None,
        };
        self.repos.insert(0, entry.clone());
        self.repos.truncate(MAX_REPOS);
        self.save()?;
        Ok(entry)
    }

    pub fn remove(&mut self, path: &str) -> std::io::Result<()> {
        self.repos.retain(|repo| repo.path != path);
        self.save()
    }

    pub fn is_known(&self, path: &str) -> bool {
        self.repos.iter().any(|repo| repo.path == path)
    }
}

/// A missing or corrupt file is an empty list; a damaged entry is skipped.
fn load(path: &Path) -> Vec<RepoEntry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(data) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let Some(items) = data.get("repos").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| serde_json::from_value(item.clone()).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, RepoStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = RepoStore::at(dir.path().join("sub/repos.json"));
        (dir, store)
    }

    #[test]
    fn newest_first_without_duplicates_and_survives_reload() {
        let (dir, mut store) = store();
        store.add("/x/one").unwrap();
        store.add("/x/two").unwrap();
        store.add("/x/one").unwrap();
        let reloaded = RepoStore::at(dir.path().join("sub/repos.json"));
        let paths: Vec<_> = reloaded.list().into_iter().map(|r| r.path).collect();
        assert_eq!(paths, ["/x/one", "/x/two"]);
        assert_eq!(reloaded.list()[0].name, "one");
    }

    #[test]
    fn keeps_the_latest_fifty() {
        let (_dir, mut store) = store();
        for i in 0..60 {
            store.add(&format!("/x/{i}")).unwrap();
        }
        assert_eq!(store.list().len(), 50);
        assert!(store.is_known("/x/59"));
        assert!(!store.is_known("/x/9"));
    }

    #[test]
    fn reads_the_file_the_python_version_wrote_and_tolerates_damage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("repos.json");
        std::fs::write(
            &path,
            r#"{"repos":[{"path":"/a","name":"a","lastOpened":5},{"oops":1},7]}"#,
        )
        .unwrap();
        assert_eq!(RepoStore::at(path.clone()).list().len(), 1);
        std::fs::write(&path, "not json").unwrap();
        assert!(RepoStore::at(path).list().is_empty());
    }
}
