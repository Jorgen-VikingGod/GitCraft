//! Non-interactive editor mode, used for GIT_EDITOR and GIT_SEQUENCE_EDITOR.
//!
//! Git runs this executable with `--gitcraft-editor` and appends the file to edit:
//!
//! ```text
//! gitcraft --gitcraft-editor todo [prepared-todo-file] <git-todo-file>
//! gitcraft --gitcraft-editor msg <git-message-file>
//! ```
//!
//! `todo` replaces the rebase todo list with a prepared one (or accepts it as is).
//! `msg` accepts the proposed message, unless an interactive rebase started by
//! GitCraft left a message for the commit currently being reworded or squashed.
//! Errors are ignored and the exit code is always 0: git then keeps what it proposed.

use std::collections::BTreeMap;
use std::path::Path;

use crate::runner::EDITOR_FLAG;

pub const MESSAGES_FILE: &str = "gitcraft-rebase-messages.json";

/// Call first thing in `main`, before any GUI or logging starts. Exits if this
/// process was started as git's editor, and prints nothing.
pub fn run_if_requested() {
    let args: Vec<String> = std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    if args.get(1).map(String::as_str) == Some(EDITOR_FLAG) {
        run(&args[2..]);
        std::process::exit(0);
    }
}

fn run(args: &[String]) {
    let [mode, .., target] = args else { return };
    match mode.as_str() {
        "todo" => {
            if let [_, prepared, _] = args {
                let _ = std::fs::copy(prepared, target);
            }
        }
        "msg" => rewrite_message(Path::new(target)),
        _ => {}
    }
}

fn rewrite_message(target: &Path) {
    let Some(git_dir) = target.parent().map(dunce_or_same) else {
        return;
    };
    let messages = std::fs::read_to_string(git_dir.join(MESSAGES_FILE)).ok();
    let done = std::fs::read(git_dir.join("rebase-merge").join("done")).ok();
    let (Some(messages), Some(done)) = (messages, done) else {
        return;
    };
    let done = String::from_utf8_lossy(&done);
    let last = done
        .lines()
        .rfind(|line| !line.trim().is_empty() && !line.starts_with('#'));
    let Some(sha) = last.and_then(|line| line.split_whitespace().nth(1)) else {
        return;
    };
    let Ok(messages) = serde_json::from_str::<BTreeMap<String, String>>(&messages) else {
        return;
    };
    let chosen = messages
        .iter()
        .find(|(key, _)| key.starts_with(sha) || sha.starts_with(key.as_str()));
    if let Some((_, message)) = chosen {
        let _ = std::fs::write(target, format!("{}\n", message.trim_end_matches('\n')));
    }
}

/// git may pass a relative message path; its parent is then the current directory.
fn dunce_or_same(parent: &Path) -> std::path::PathBuf {
    if parent.as_os_str().is_empty() {
        std::env::current_dir().unwrap_or_default()
    } else {
        parent.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(done: &str, messages: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("rebase-merge")).unwrap();
        std::fs::write(dir.path().join("rebase-merge/done"), done).unwrap();
        std::fs::write(dir.path().join(MESSAGES_FILE), messages).unwrap();
        let target = dir.path().join("COMMIT_EDITMSG");
        std::fs::write(&target, "proposed\n").unwrap();
        (dir, target)
    }

    #[test]
    fn replaces_the_message_of_the_current_step_by_prefix() {
        let (_dir, target) = setup(
            "pick aaaa1\n# comment\nreword bbbb2 subject\n",
            r#"{"bbbb222222":"new text\n\n"}"#,
        );
        rewrite_message(&target);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "new text\n");
    }

    #[test]
    fn keeps_the_proposal_when_no_message_matches() {
        let (_dir, target) = setup("pick aaaa1\n", r#"{"ffff":"other"}"#);
        rewrite_message(&target);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "proposed\n");
    }

    #[test]
    fn keeps_the_proposal_outside_a_gitcraft_rebase() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("COMMIT_EDITMSG");
        std::fs::write(&target, "proposed\n").unwrap();
        rewrite_message(&target);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "proposed\n");
    }

    #[test]
    fn todo_copies_the_prepared_list_only_when_given() {
        let dir = tempfile::tempdir().unwrap();
        let (prepared, target) = (dir.path().join("prepared"), dir.path().join("todo"));
        std::fs::write(&prepared, "drop abc\n").unwrap();
        std::fs::write(&target, "pick abc\n").unwrap();
        let path = |p: &Path| p.to_string_lossy().into_owned();
        run(&["todo".into(), path(&target)]);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "pick abc\n");
        run(&["todo".into(), path(&prepared), path(&target)]);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "drop abc\n");
    }
}
