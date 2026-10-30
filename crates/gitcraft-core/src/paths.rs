//! Path handling: resolving paths that may not exist, and user-typed paths.

use std::path::{Component, Path, PathBuf};

/// Like Python's `Path.resolve()`: absolute with symlinks resolved as far as the
/// path exists, the rest normalised lexically. Never fails on a missing path, and
/// never returns a `\\?\` prefix on Windows.
pub fn resolve(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => {
                resolved.push(other);
                if let Ok(real) = dunce::canonicalize(&resolved) {
                    resolved = real;
                }
            }
        }
    }
    resolved
}

pub fn resolve_str(path: impl AsRef<Path>) -> String {
    resolve(path).to_string_lossy().into_owned()
}

/// Expands `~` and environment variables in a path the user typed, then resolves it.
pub fn expand(path: &str) -> String {
    let expanded = expand_vars(path);
    let home = crate::store::home_dir();
    let full = match expanded.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            home.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(expanded),
    };
    resolve_str(full)
}

/// `$NAME` and `${NAME}`, plus `%NAME%` on Windows. Unknown variables stay as written.
fn expand_vars(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let (name, used) = match after.strip_prefix('{') {
            Some(inner) => match inner.find('}') {
                Some(end) => (&inner[..end], end + 2),
                None => ("", 0),
            },
            None => {
                let end = after
                    .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .unwrap_or(after.len());
                (&after[..end], end)
            }
        };
        match std::env::var(name) {
            Ok(value) if !name.is_empty() => {
                out.push_str(&value);
                rest = &after[used..];
            }
            _ => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    if cfg!(windows) {
        expand_percent(&out)
    } else {
        out
    }
}

fn expand_percent(text: &str) -> String {
    let parts: Vec<&str> = text.split('%').collect();
    let mut out = String::from(parts[0]);
    let mut i = 1;
    while i < parts.len() {
        match (i + 1 < parts.len()).then(|| std::env::var(parts[i])) {
            Some(Ok(value)) => {
                out.push_str(&value);
                out.push_str(parts[i + 1]);
                i += 2;
            }
            _ => {
                out.push('%');
                out.push_str(parts[i]);
                i += 1;
            }
        }
    }
    out
}

/// Whether `path` stays inside `root` once resolved. Returns the resolved target.
pub fn inside(root: &str, path: &str) -> Option<PathBuf> {
    let root = resolve(root);
    let target = resolve(root.join(path));
    (target == root || target.starts_with(&root)).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_missing_paths_lexically() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        assert_eq!(resolve(root.join("a/b/../c/./d")), root.join("a/c/d"));
        assert_eq!(resolve(root.join("missing/..")), root);
    }

    #[test]
    fn refuses_paths_outside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        assert!(inside(root, "a/b.txt").is_some());
        assert!(inside(root, "a/../b.txt").is_some());
        assert!(inside(root, "../b.txt").is_none());
        assert!(inside(root, "a/../../b.txt").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_cannot_lead_outside_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        assert!(inside(dir.path().to_str().unwrap(), "link/secret.txt").is_none());
    }

    #[test]
    fn keeps_unknown_variables() {
        assert_eq!(
            expand_vars("/a/$GITCRAFT_SURELY_UNSET_VAR/b"),
            "/a/$GITCRAFT_SURELY_UNSET_VAR/b"
        );
        assert_eq!(expand_vars("cost$"), "cost$");
    }
}
