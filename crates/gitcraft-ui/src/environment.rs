//! Prepares the environment git runs in, for launches that didn't come from a terminal.

use gitcraft_core::runner::{ChildEnv, set_child_env};

/// Library settings an AppImage sets for itself, which would break git, ssh and gpg.
const APPIMAGE_VARIABLES: [&str; 6] = [
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
];
const COMMON_DIRS: [&str; 4] = ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin", "/bin"];
const MARK: &str = "__GITCRAFT_PATH__";

fn merge_paths(shell: Option<&str>, inherited: &str) -> String {
    let mut merged: Vec<&str> = Vec::new();
    let sources = shell
        .into_iter()
        .chain([inherited])
        .flat_map(|list| list.split(':'));
    for dir in sources.chain(COMMON_DIRS) {
        if !dir.is_empty() && !merged.contains(&dir) {
            merged.push(dir);
        }
    }
    merged.join(":")
}

fn extract_path(output: &str) -> Option<&str> {
    let (_, rest) = output.split_once(MARK)?;
    let (path, _) = rest.split_once(MARK)?;
    Some(path)
}

/// Removes AppImage settings right away, and looks up the login shell's PATH in
/// the background, as a slow shell profile must not delay the window.
#[cfg(unix)]
pub fn prepare() {
    let remove: Vec<String> = if std::env::var_os("APPIMAGE").is_some() {
        APPIMAGE_VARIABLES
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    } else {
        Vec::new()
    };
    let inherited = std::env::var("PATH").unwrap_or_default();
    set_child_env(ChildEnv {
        set: vec![("PATH".to_owned(), merge_paths(None, &inherited))],
        remove: remove.clone(),
    });
    std::thread::spawn(move || {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
        let script = format!("printf '{MARK}%s{MARK}' \"$PATH\"");
        let mut command = std::process::Command::new(shell);
        command
            .args(["-ilc", &script])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        for name in &remove {
            command.env_remove(name);
        }
        let Ok(output) = command.output() else { return };
        let text = String::from_utf8_lossy(&output.stdout);
        if let Some(path) = extract_path(&text) {
            set_child_env(ChildEnv {
                set: vec![("PATH".to_owned(), merge_paths(Some(path), &inherited))],
                remove,
            });
        }
    });
}

#[cfg(not(unix))]
pub fn prepare() {
    let _ = (APPIMAGE_VARIABLES, COMMON_DIRS, MARK);
    let _ = (
        merge_paths as fn(Option<&str>, &str) -> String,
        extract_path as fn(&str) -> Option<&str>,
    );
    set_child_env(ChildEnv::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_path_comes_first_without_duplicates() {
        let merged = merge_paths(Some("/home/me/bin:/usr/bin"), "/usr/bin:/snap/bin");
        assert_eq!(
            merged,
            "/home/me/bin:/usr/bin:/snap/bin:/usr/local/bin:/opt/homebrew/bin:/bin"
        );
    }

    #[test]
    fn finds_the_path_among_profile_noise() {
        let output = format!("Welcome!\n{MARK}/a:/b{MARK}\nbye");
        assert_eq!(extract_path(&output), Some("/a:/b"));
        assert_eq!(extract_path("nothing"), None);
    }
}
