//! UI decisions that need no toolkit, so they can be tested directly.

use std::collections::BTreeMap;

use gitcraft_core::model::{ConflictChunk, Remote};

pub fn short_sha(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

pub fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        format!("1 {word}")
    } else {
        format!("{count} {word}s")
    }
}

/// Splits `origin/feature/x` into the longest matching remote and the branch.
pub fn split_upstream(
    upstream: Option<&str>,
    remotes: &[Remote],
) -> (Option<String>, Option<String>) {
    let Some(upstream) = upstream else {
        return (None, None);
    };
    let mut names: Vec<&str> = remotes.iter().map(|remote| remote.name.as_str()).collect();
    names.sort_by_key(|name| std::cmp::Reverse(name.len()));
    for name in names {
        if let Some(branch) = upstream
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('/'))
        {
            return (Some(name.to_owned()), Some(branch.to_owned()));
        }
    }
    (None, Some(upstream.to_owned()))
}

// ------------------------------------------------------------- side by side

/// Pairs removed and added runs; context lines appear on both sides.
/// Items are line indices within a hunk.
pub fn to_split(tags: &[char]) -> Vec<(Option<usize>, Option<usize>)> {
    let mut rows = Vec::new();
    let (mut removed, mut added) = (Vec::new(), Vec::new());
    let flush = |rows: &mut Vec<(Option<usize>, Option<usize>)>,
                 removed: &mut Vec<usize>,
                 added: &mut Vec<usize>| {
        for i in 0..removed.len().max(added.len()) {
            rows.push((removed.get(i).copied(), added.get(i).copied()));
        }
        removed.clear();
        added.clear();
    };
    for (index, tag) in tags.iter().enumerate() {
        match tag {
            '-' => removed.push(index),
            '+' => added.push(index),
            _ => {
                flush(&mut rows, &mut removed, &mut added);
                rows.push((Some(index), Some(index)));
            }
        }
    }
    flush(&mut rows, &mut removed, &mut added);
    rows
}

// ----------------------------------------------------------------- conflicts

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Ours,
    Theirs,
    OursTheirs,
    TheirsOurs,
}

/// Rebuilds the file from chunks; unresolved conflicts keep their markers.
pub fn build_result(chunks: &[ConflictChunk], choices: &[Option<Choice>]) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let mut index = 0;
    for chunk in chunks {
        match chunk {
            ConflictChunk::Common { lines: common } => {
                lines.extend(common.iter().map(String::as_str))
            }
            ConflictChunk::Conflict { ours, theirs, .. } => {
                let (ours, theirs): (Vec<&str>, Vec<&str>) = (
                    ours.iter().map(String::as_str).collect(),
                    theirs.iter().map(String::as_str).collect(),
                );
                match choices.get(index).copied().flatten() {
                    Some(Choice::Ours) => lines.extend(ours),
                    Some(Choice::Theirs) => lines.extend(theirs),
                    Some(Choice::OursTheirs) => {
                        lines.extend(ours);
                        lines.extend(theirs);
                    }
                    Some(Choice::TheirsOurs) => {
                        lines.extend(theirs);
                        lines.extend(ours);
                    }
                    None => {
                        lines.push("<<<<<<< ours");
                        lines.extend(ours);
                        lines.push("=======");
                        lines.extend(theirs);
                        lines.push(">>>>>>> theirs");
                    }
                }
                index += 1;
            }
        }
    }
    lines.join("\n")
}

pub fn has_conflict_markers(text: &str) -> bool {
    text.split('\n').any(|line| {
        ["<<<<<<<", ">>>>>>>"].iter().any(|marker| {
            line.strip_prefix(marker)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        })
    })
}

// -------------------------------------------------------- interactive rebase

/// Why a rebase plan can't start. Input: each step's action, and for rewords
/// whether its message is filled in and its body has loaded.
pub fn rebase_problem(steps: &[(&str, bool, bool)]) -> Option<&'static str> {
    let Some(first) = steps.iter().find(|step| step.0 != "drop") else {
        return Some(if steps.is_empty() {
            "There are no commits to rebase."
        } else {
            "All commits are dropped."
        });
    };
    if matches!(first.0, "squash" | "fixup") {
        return Some(
            "The first remaining commit cannot be squashed or fixed up (there is nothing above it).",
        );
    }
    if steps.iter().any(|step| step.0 == "reword" && !step.1) {
        return Some("Reworded commits need a message.");
    }
    if steps.iter().any(|step| step.0 == "reword" && !step.2) {
        return Some("Loading commit message…");
    }
    None
}

// ---------------------------------------------------------------- ref trees

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    /// Identifies a folder for collapsing.
    pub key: String,
    pub label: String,
    pub depth: usize,
    /// The index of the item, or `None` for a folder.
    pub item: Option<usize>,
}

/// Folder rows for `/`-separated names, hiding what's inside collapsed folders.
/// Names must be sorted, which is how git lists refs.
pub fn build_tree(
    prefix: &str,
    names: &[&str],
    is_collapsed: impl Fn(&str) -> bool,
) -> Vec<TreeRow> {
    #[derive(Default)]
    struct Node<'a> {
        folders: BTreeMap<&'a str, Node<'a>>,
        items: Vec<(&'a str, usize)>,
    }
    let mut root = Node::default();
    for (index, name) in names.iter().enumerate() {
        let parts: Vec<&str> = name.split('/').collect();
        let (leaf, folders) = parts.split_last().expect("split yields at least one part");
        let mut node = &mut root;
        for folder in folders {
            node = node.folders.entry(folder).or_default();
        }
        node.items.push((leaf, index));
    }
    fn walk(
        node: &Node<'_>,
        path: &str,
        depth: usize,
        is_collapsed: &impl Fn(&str) -> bool,
        rows: &mut Vec<TreeRow>,
    ) {
        for (name, child) in &node.folders {
            let key = format!("{path}/{name}");
            rows.push(TreeRow {
                key: key.clone(),
                label: (*name).to_owned(),
                depth,
                item: None,
            });
            if !is_collapsed(&key) {
                walk(child, &key, depth + 1, is_collapsed, rows);
            }
        }
        for (label, index) in &node.items {
            rows.push(TreeRow {
                key: format!("{path}/{label}"),
                label: (*label).to_owned(),
                depth,
                item: Some(*index),
            });
        }
    }
    let mut rows = Vec::new();
    walk(&root, prefix, 0, &is_collapsed, &mut rows);
    rows
}

/// What a commit message becomes: the summary, then the body after a blank line.
pub fn commit_message(summary: &str, body: &str) -> String {
    let (summary, body) = (summary.trim(), body.trim());
    if body.is_empty() {
        format!("{summary}\n")
    } else {
        format!("{summary}\n\n{body}\n")
    }
}

/// Splits a message into the commit box's summary and body.
pub fn split_message(message: &str) -> (String, String) {
    let normalised = message.replace("\r\n", "\n");
    let (summary, body) = normalised.split_once('\n').unwrap_or((&normalised, ""));
    (summary.to_owned(), body.trim_start_matches('\n').to_owned())
}

pub fn patch_file_name(sha: &str, subject: &str) -> String {
    let mut slug = String::new();
    for c in subject.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let slug: String = slug.chars().take(50).collect();
    format!(
        "{}-{}.patch",
        short_sha(sha),
        if slug.is_empty() { "commit" } else { &slug }
    )
}

/// Menu entries for ignoring a file: (label, pattern) for the file, its extension and its folder.
pub fn ignore_patterns(path: &str) -> Vec<(String, String)> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let mut options = vec![(format!("Ignore \"{path}\""), format!("/{path}"))];
    if let Some(at) = name.rfind('.').filter(|at| *at > 0) {
        let extension = &name[at..];
        options.push((
            format!("Ignore all \"*{extension}\" files"),
            format!("*{extension}"),
        ));
    }
    if let Some(at) = path.rfind('/') {
        let dir = &path[..=at];
        options.push((format!("Ignore folder \"{dir}\""), format!("/{dir}")));
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(name: &str) -> Remote {
        Remote {
            name: name.into(),
            fetch_url: String::new(),
            push_url: String::new(),
        }
    }

    #[test]
    fn upstream_splits_on_the_longest_remote() {
        let remotes = [remote("origin"), remote("origin/fork")];
        assert_eq!(
            split_upstream(Some("origin/fork/main"), &remotes),
            (Some("origin/fork".into()), Some("main".into()))
        );
        assert_eq!(
            split_upstream(Some("origin/a/b"), &remotes),
            (Some("origin".into()), Some("a/b".into()))
        );
        assert_eq!(
            split_upstream(Some("elsewhere"), &remotes),
            (None, Some("elsewhere".into()))
        );
        assert_eq!(split_upstream(None, &remotes), (None, None));
    }

    #[test]
    fn split_pairs_changes_and_pads_the_shorter_side() {
        let tags = [' ', '-', '-', '+', ' ', '+', '\\'];
        assert_eq!(
            to_split(&tags),
            [
                (Some(0), Some(0)),
                (Some(1), Some(3)),
                (Some(2), None),
                (Some(4), Some(4)),
                (None, Some(5)),
                (Some(6), Some(6)),
            ]
        );
    }

    fn chunks() -> Vec<ConflictChunk> {
        vec![
            ConflictChunk::Common {
                lines: vec!["top".into()],
            },
            ConflictChunk::Conflict {
                ours: vec!["o".into()],
                base: vec![],
                theirs: vec!["t".into()],
            },
            ConflictChunk::Common {
                lines: vec!["end".into(), String::new()],
            },
        ]
    }

    #[test]
    fn result_follows_choices_and_keeps_markers_for_open_conflicts() {
        let build = |choice| build_result(&chunks(), &[choice]);
        assert_eq!(build(Some(Choice::Ours)), "top\no\nend\n");
        assert_eq!(build(Some(Choice::Theirs)), "top\nt\nend\n");
        assert_eq!(build(Some(Choice::OursTheirs)), "top\no\nt\nend\n");
        assert_eq!(build(Some(Choice::TheirsOurs)), "top\nt\no\nend\n");
        let open = build(None);
        assert_eq!(
            open,
            "top\n<<<<<<< ours\no\n=======\nt\n>>>>>>> theirs\nend\n"
        );
        assert!(has_conflict_markers(&open));
        assert!(!has_conflict_markers("top\n<<<<<<<not a marker\nend"));
    }

    #[test]
    fn rebase_plans_are_checked_like_the_editor_says() {
        let pick = ("pick", true, true);
        assert_eq!(rebase_problem(&[pick]), None);
        assert!(rebase_problem(&[]).unwrap().contains("no commits"));
        assert!(
            rebase_problem(&[("drop", true, true)])
                .unwrap()
                .contains("dropped")
        );
        assert!(
            rebase_problem(&[("drop", true, true), ("fixup", true, true)])
                .unwrap()
                .contains("first remaining")
        );
        assert_eq!(rebase_problem(&[pick, ("squash", true, true)]), None);
        assert!(
            rebase_problem(&[("reword", false, true)])
                .unwrap()
                .contains("message")
        );
        assert!(
            rebase_problem(&[("reword", true, false)])
                .unwrap()
                .contains("Loading")
        );
    }

    #[test]
    fn trees_nest_folders_and_hide_collapsed_ones() {
        let names = ["feature/a", "feature/deep/b", "main"];
        let all = build_tree("b", &names, |_| false);
        let shape: Vec<_> = all
            .iter()
            .map(|row| (row.label.as_str(), row.depth, row.item))
            .collect();
        assert_eq!(
            shape,
            [
                ("feature", 0, None),
                ("deep", 1, None),
                ("b", 2, Some(1)),
                ("a", 1, Some(0)),
                ("main", 0, Some(2)),
            ]
        );
        let collapsed = build_tree("b", &names, |key| key == "b/feature");
        let labels: Vec<_> = collapsed.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["feature", "main"]);
    }

    #[test]
    fn messages_round_trip_through_the_commit_box() {
        assert_eq!(commit_message(" hi ", "  "), "hi\n");
        assert_eq!(commit_message("hi", "\nbody\n"), "hi\n\nbody\n");
        assert_eq!(
            split_message("hi\r\n\r\nbody\nmore"),
            ("hi".to_owned(), "body\nmore".to_owned())
        );
        assert_eq!(split_message("hi"), ("hi".to_owned(), String::new()));
    }

    #[test]
    fn patch_names_are_safe() {
        assert_eq!(
            patch_file_name("abcdef1234", "Fix: the BIG bug!"),
            "abcdef1-fix-the-big-bug.patch"
        );
        assert_eq!(patch_file_name("abcdef1234", "!!!"), "abcdef1-commit.patch");
    }

    #[test]
    fn ignore_options_offer_file_extension_and_folder() {
        let options = ignore_patterns("src/a.log");
        let patterns: Vec<_> = options.iter().map(|o| o.1.as_str()).collect();
        assert_eq!(patterns, ["/src/a.log", "*.log", "/src/"]);
        assert_eq!(ignore_patterns(".env").len(), 1);
    }
}
