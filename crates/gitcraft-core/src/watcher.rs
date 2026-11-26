//! File-system change notifications for one repository.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::{Event, EventKind, RecursiveMode, Watcher as _};

use crate::{Error, Result, paths, queries};

const IGNORED_DIRS: [&str; 7] = [
    "node_modules",
    "__pycache__",
    ".venv",
    ".mypy_cache",
    ".pytest_cache",
    ".idea",
    ".gradle",
];
const IGNORED_GIT_DIRS: [&str; 5] = ["objects", "logs", "lfs", "hooks", "modules"];
/// A batch of changes ends after this much quiet...
const QUIET: Duration = Duration::from_millis(100);
/// ...or at the latest after this long.
const MAX_BATCH: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    /// `git`: refs, HEAD, config or an operation in progress changed, so everything is reloaded.
    /// `index`: only what is staged changed. `worktree`: files that git doesn't ignore changed.
    Changed {
        git: bool,
        index: bool,
        worktree: bool,
    },
    Error(String),
}

/// Watching stops when this is dropped.
pub struct Watcher {
    _watcher: notify::RecommendedWatcher,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Git,
    Index,
    Worktree,
}

fn git_inner_kind<'a>(
    mut parts: impl Iterator<Item = &'a std::ffi::OsStr>,
    path: &Path,
) -> Option<Kind> {
    if path
        .extension()
        .is_some_and(|extension| extension == "lock")
    {
        return None;
    }
    let first = parts.next()?;
    if IGNORED_GIT_DIRS.iter().any(|ignored| first == *ignored) {
        return None;
    }
    let name = first.to_string_lossy();
    if parts.next().is_none() && (name == "index" || name.starts_with("sharedindex.")) {
        return Some(Kind::Index);
    }
    Some(Kind::Git)
}

/// The work tree's ignore rules: each folder's `.gitignore`, read when first needed, then
/// `.git/info/exclude` and the user's `core.excludesFile`.
struct Ignores {
    root: PathBuf,
    folders: HashMap<PathBuf, Option<Gitignore>>,
    global: Gitignore,
}

impl Ignores {
    fn new(root: &Path, common_dir: &Path) -> Self {
        let mut global = GitignoreBuilder::new(root);
        global
            .case_insensitive(cfg!(any(windows, target_os = "macos")))
            .ok();
        let _ = global.add(common_dir.join("info").join("exclude"));
        if let Some(excludes) = ignore::gitignore::gitconfig_excludes_path() {
            let _ = global.add(excludes);
        }
        Self {
            root: root.to_path_buf(),
            folders: HashMap::new(),
            global: global.build().unwrap_or_else(|_| Gitignore::empty()),
        }
    }

    fn folder(&mut self, dir: &Path) -> Option<&Gitignore> {
        self.folders
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                let file = dir.join(".gitignore");
                if !file.is_file() {
                    return None;
                }
                let mut builder = GitignoreBuilder::new(dir);
                builder
                    .case_insensitive(cfg!(any(windows, target_os = "macos")))
                    .ok();
                let _ = builder.add(file);
                builder.build().ok()
            })
            .as_ref()
    }

    /// A `.gitignore` changed: read it again when next needed.
    fn forget(&mut self, path: &Path) {
        if path.file_name().is_some_and(|name| name == ".gitignore")
            && let Some(dir) = path.parent()
        {
            self.folders.remove(dir);
        }
    }

    /// Whether this one path is ignored, by the deepest rule that speaks about it.
    fn matched(&mut self, path: &Path, is_dir: bool) -> bool {
        let root = self.root.clone();
        for dir in path.ancestors().skip(1) {
            if !dir.starts_with(&root) {
                break;
            }
            if let Some(rules) = self.folder(dir) {
                let found = rules.matched(path, is_dir);
                if found.is_ignore() {
                    return true;
                }
                if found.is_whitelist() {
                    return false;
                }
            }
        }
        self.global.matched(path, is_dir).is_ignore()
    }

    /// Whether git ignores `path`: it, or a folder it is in, is ignored.
    fn ignored(&mut self, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        let parts: Vec<_> = relative.iter().collect();
        let mut current = self.root.clone();
        for (index, part) in parts.iter().enumerate() {
            current.push(part);
            let is_dir = index + 1 < parts.len() || current.is_dir();
            if self.matched(&current.clone(), is_dir) {
                return true;
            }
        }
        false
    }
}

fn classify(repo: &Path, git_dirs: &[PathBuf], ignores: &mut Ignores, path: &Path) -> Option<Kind> {
    for git_dir in git_dirs {
        if let Ok(inner) = path.strip_prefix(git_dir) {
            return git_inner_kind(inner.iter(), path);
        }
    }
    let relative = path.strip_prefix(repo).ok()?;
    let mut parts = relative.iter();
    if parts.clone().next().is_some_and(|part| part == ".git") {
        parts.next();
        return git_inner_kind(parts, path);
    }
    if relative
        .iter()
        .any(|part| IGNORED_DIRS.iter().any(|ignored| part == *ignored))
    {
        return None;
    }
    ignores.forget(path);
    if ignores.ignored(path) {
        return None;
    }
    Some(Kind::Worktree)
}

/// Starts watching `repo`, including its real git directories when they live
/// elsewhere (linked worktrees, submodules). `on_event` runs on a background thread.
pub async fn watch(repo: &str, on_event: impl Fn(WatchEvent) + Send + 'static) -> Result<Watcher> {
    let root = paths::resolve(repo);
    let mut git_dirs = vec![paths::resolve(queries::git_dir(repo).await?)];
    let common = paths::resolve(queries::common_dir(repo).await?);
    let ignores = Ignores::new(&root, &common);
    if !git_dirs.contains(&common) {
        git_dirs.push(common);
    }

    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        let _ = sender.send(event);
    })
    .map_err(|error| Error::Invalid(error.to_string()))?;
    let mode = RecursiveMode::Recursive;
    watcher
        .watch(&root, mode)
        .map_err(|error| Error::Invalid(error.to_string()))?;
    for git_dir in git_dirs
        .iter()
        .filter(|git_dir| !git_dir.starts_with(&root))
    {
        watcher
            .watch(git_dir, mode)
            .map_err(|error| Error::Invalid(error.to_string()))?;
    }
    std::thread::spawn(move || batch_events(&receiver, &root, &git_dirs, ignores, &on_event));
    Ok(Watcher { _watcher: watcher })
}

fn batch_events(
    receiver: &mpsc::Receiver<notify::Result<Event>>,
    root: &Path,
    git_dirs: &[PathBuf],
    mut ignores: Ignores,
    on_event: &impl Fn(WatchEvent),
) {
    while let Ok(first) = receiver.recv() {
        let (mut git, mut index, mut worktree) = (false, false, false);
        let mut note = |event: notify::Result<Event>| -> std::result::Result<(), String> {
            let event = event.map_err(|error| error.to_string())?;
            // Reads are not changes; reacting to them would make every refresh trigger the next.
            if matches!(event.kind, EventKind::Access(_)) {
                return Ok(());
            }
            for path in &event.paths {
                match classify(root, git_dirs, &mut ignores, path) {
                    Some(Kind::Git) => git = true,
                    Some(Kind::Index) => index = true,
                    Some(Kind::Worktree) => worktree = true,
                    None => {}
                }
            }
            Ok(())
        };
        if let Err(message) = note(first) {
            on_event(WatchEvent::Error(message));
            return;
        }
        let deadline = Instant::now() + MAX_BATCH;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match receiver.recv_timeout(QUIET.min(left)) {
                Ok(event) => {
                    if let Err(message) = note(event) {
                        on_event(WatchEvent::Error(message));
                        return;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
        if git || index || worktree {
            on_event(WatchEvent::Changed {
                git,
                index,
                worktree,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(path: &str) -> Option<&'static str> {
        let mut ignores = Ignores::new(Path::new("/r"), Path::new("/r/.git"));
        classify(
            Path::new("/r"),
            &[PathBuf::from("/r/.git"), PathBuf::from("/x/main/.git")],
            &mut ignores,
            Path::new(path),
        )
        .map(|kind| match kind {
            Kind::Git => "git",
            Kind::Index => "index",
            Kind::Worktree => "worktree",
        })
    }

    #[test]
    fn sorts_paths_into_git_and_worktree() {
        assert_eq!(kind("/r/src/a.rs"), Some("worktree"));
        assert_eq!(kind("/r/.git/HEAD"), Some("git"));
        assert_eq!(kind("/r/.git/refs/heads/main"), Some("git"));
        assert_eq!(kind("/r/.git/objects/ab/cd"), None);
        assert_eq!(kind("/r/.git/logs/HEAD"), None);
        assert_eq!(kind("/r/node_modules/x/y.js"), None);
        assert_eq!(kind("/outside/file"), None);
    }

    #[test]
    fn the_index_alone_is_its_own_kind() {
        assert_eq!(kind("/r/.git/index"), Some("index"));
        assert_eq!(kind("/r/.git/sharedindex.0123abcd"), Some("index"));
        assert_eq!(kind("/x/main/.git/index"), Some("index"));
        assert_eq!(kind("/r/.git/refs/heads/index"), Some("git"));
    }

    #[test]
    fn lock_files_are_ignored_only_inside_git() {
        assert_eq!(kind("/r/.git/index.lock"), None);
        assert_eq!(kind("/r/Cargo.lock"), Some("worktree"));
        assert_eq!(kind("/r/backend/uv.lock"), Some("worktree"));
    }

    #[test]
    fn a_linked_worktrees_real_git_dir_counts_as_git() {
        assert_eq!(kind("/x/main/.git/refs/heads/wt"), Some("git"));
        assert_eq!(kind("/x/main/.git/objects/ab"), None);
    }

    #[test]
    fn files_git_ignores_are_no_change() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let git_dir = root.join(".git");
        std::fs::create_dir_all(git_dir.join("info")).unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::create_dir_all(root.join("logs/keep")).unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
        std::fs::write(root.join("logs/keep/.gitignore"), "!wanted.log\n").unwrap();
        std::fs::write(git_dir.join("info/exclude"), "scratch.txt\n").unwrap();
        let mut ignores = Ignores::new(root, &git_dir);
        let mut kind = |path: &str| {
            classify(
                root,
                std::slice::from_ref(&git_dir),
                &mut ignores,
                &root.join(path),
            )
        };
        assert_eq!(kind("target/debug/app.exe"), None);
        assert_eq!(kind("target"), None);
        assert_eq!(kind("build.log"), None);
        assert_eq!(kind("scratch.txt"), None);
        assert_eq!(kind("logs/keep/wanted.log"), Some(Kind::Worktree));
        assert_eq!(kind("src/main.rs"), Some(Kind::Worktree));
        assert_eq!(kind(".gitignore"), Some(Kind::Worktree));

        // A changed .gitignore is read again.
        std::fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        assert_eq!(kind(".gitignore"), Some(Kind::Worktree));
        assert_eq!(kind("target/debug/app.exe"), Some(Kind::Worktree));
    }
}
