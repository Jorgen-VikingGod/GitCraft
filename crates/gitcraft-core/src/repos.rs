//! Opening repositories and browsing folders.

use std::path::Path;

use crate::model::{DirEntry, DirListing};
use crate::{Error, Result, paths, queries, store};

/// The repository root for a folder the user chose.
pub async fn locate(path: &str) -> Result<String> {
    let path = paths::expand(path);
    if !Path::new(&path).is_dir() {
        return Err(Error::Validation(format!("Folder does not exist: {path}")));
    }
    queries::toplevel(&path)
        .await
        .map_err(|_| Error::Validation(format!("Not a git repository: {path}")))
}

pub fn list_dir(path: Option<&str>, hidden: bool) -> Result<DirListing> {
    let directory = path.map_or_else(store::home_dir, |path| paths::expand(path).into());
    if !directory.is_dir() {
        return Err(Error::Validation(format!(
            "Not a folder: {}",
            directory.display()
        )));
    }
    let mut children: Vec<_> = std::fs::read_dir(&directory)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    children.sort_by_key(|child: &std::path::PathBuf| {
        child
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    let dirs = children
        .into_iter()
        .filter_map(|child| {
            let name = child.file_name()?.to_string_lossy().into_owned();
            let visible = hidden || !name.starts_with('.');
            (visible && child.is_dir()).then(|| DirEntry {
                name,
                is_repo: child.join(".git").exists(),
                path: child.to_string_lossy().into_owned(),
            })
        })
        .collect();
    Ok(DirListing {
        path: directory.to_string_lossy().into_owned(),
        parent: directory
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned()),
        home: store::home_dir().to_string_lossy().into_owned(),
        roots: roots(),
        is_repo: directory.join(".git").exists(),
        dirs,
    })
}

fn roots() -> Vec<String> {
    if cfg!(windows) {
        ('C'..='Z')
            .map(|drive| format!("{drive}:\\"))
            .filter(|root| Path::new(root).exists())
            .collect()
    } else {
        vec!["/".to_owned()]
    }
}
