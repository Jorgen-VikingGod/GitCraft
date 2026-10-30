//! File-system change notifications for one repository.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
    Changed { git: bool, worktree: bool },
    Error(String),
}

/// Watching stops when this is dropped.
pub struct Watcher {
    _watcher: notify::RecommendedWatcher,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Git,
    Worktree,
}

fn git_inner_kind<'a>(
    parts: impl Iterator<Item = &'a std::ffi::OsStr>,
    path: &Path,
) -> Option<Kind> {
    if path
        .extension()
        .is_some_and(|extension| extension == "lock")
    {
        return None;
    }
    let first = parts.into_iter().next();
    if first.is_some_and(|part| IGNORED_GIT_DIRS.iter().any(|ignored| part == *ignored)) {
        return None;
    }
    Some(Kind::Git)
}

fn classify(repo: &Path, git_dirs: &[PathBuf], path: &Path) -> Option<Kind> {
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
    Some(Kind::Worktree)
}

/// Starts watching `repo`, including its real git directories when they live
/// elsewhere (linked worktrees, submodules). `on_event` runs on a background thread.
pub async fn watch(repo: &str, on_event: impl Fn(WatchEvent) + Send + 'static) -> Result<Watcher> {
    let root = paths::resolve(repo);
    let mut git_dirs = vec![paths::resolve(queries::git_dir(repo).await?)];
    let common = paths::resolve(queries::common_dir(repo).await?);
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
    std::thread::spawn(move || batch_events(&receiver, &root, &git_dirs, &on_event));
    Ok(Watcher { _watcher: watcher })
}

fn batch_events(
    receiver: &mpsc::Receiver<notify::Result<Event>>,
    root: &Path,
    git_dirs: &[PathBuf],
    on_event: &impl Fn(WatchEvent),
) {
    while let Ok(first) = receiver.recv() {
        let (mut git, mut worktree) = (false, false);
        let mut note = |event: notify::Result<Event>| -> std::result::Result<(), String> {
            let event = event.map_err(|error| error.to_string())?;
            // Reads are not changes; reacting to them would make every refresh trigger the next.
            if matches!(event.kind, EventKind::Access(_)) {
                return Ok(());
            }
            for path in &event.paths {
                match classify(root, git_dirs, path) {
                    Some(Kind::Git) => git = true,
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
        if git || worktree {
            on_event(WatchEvent::Changed { git, worktree });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(path: &str) -> Option<&'static str> {
        classify(
            Path::new("/r"),
            &[PathBuf::from("/r/.git"), PathBuf::from("/x/main/.git")],
            Path::new(path),
        )
        .map(|kind| match kind {
            Kind::Git => "git",
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
}
