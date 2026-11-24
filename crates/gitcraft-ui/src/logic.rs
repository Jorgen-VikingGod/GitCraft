//! UI decisions that need no toolkit, so they can be tested directly.

use std::collections::BTreeMap;

use gitcraft_core::model::{ConflictChunk, Remote, Status};

use crate::i18n::{tr, trf};

pub fn short_sha(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

/// "1 file", "3 files", in the interface language: the table has a line for each form
/// ("1 file", "{count} files").
pub fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        tr(&format!("1 {word}")).to_owned()
    } else {
        trf(
            &format!("{{count}} {word}s"),
            &[("count", &count.to_string())],
        )
    }
}

/// The commits picked after a Ctrl/Cmd+click (`range` false: the clicked one is added or
/// taken out) or a Shift+click (`range`: everything between the selected commit and the
/// clicked one) on `sha`. `listed` is the history as shown, newest first; the result keeps
/// that order. A first Ctrl+click starts from the selected commit.
pub fn pick_commits(
    listed: &[String],
    picked: &[String],
    selected: Option<&str>,
    sha: &str,
    range: bool,
) -> Vec<String> {
    let position = |wanted: &str| listed.iter().position(|sha| sha == wanted);
    let mut chosen: Vec<String> = if picked.is_empty() {
        selected.map(str::to_owned).into_iter().collect()
    } else {
        picked.to_vec()
    };
    if range {
        let anchor = selected.and_then(position).or_else(|| position(sha));
        if let (Some(anchor), Some(end)) = (anchor, position(sha)) {
            let (from, to) = (anchor.min(end), anchor.max(end));
            chosen = listed[from..=to].to_vec();
        }
    } else if let Some(at) = chosen.iter().position(|picked| picked == sha) {
        chosen.remove(at);
    } else {
        chosen.push(sha.to_owned());
    }
    chosen.sort_by_key(|sha| position(sha).unwrap_or(usize::MAX));
    chosen
}

/// How a signature's state is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Good,
    Warning,
    Bad,
    Muted,
}

/// What a commit's signature state (`%G?`) means, in English, and how it looks.
pub fn signature_label(status: char) -> (&'static str, Tone) {
    match status {
        'G' => ("Good signature", Tone::Good),
        'U' => ("Good signature, key of unknown trust", Tone::Warning),
        'X' => ("Good signature, but it has expired", Tone::Warning),
        'Y' => ("Good signature by an expired key", Tone::Warning),
        'R' => ("Signed by a revoked key", Tone::Bad),
        'B' => ("Bad signature", Tone::Bad),
        _ => ("Signed, but it can't be checked here", Tone::Muted),
    }
}

/// Amending HEAD rewrites a commit others may already have: it is on its upstream when the
/// branch has one and nothing of its own beyond it.
pub fn amend_rewrites_pushed(status: &Status) -> bool {
    let branch = &status.branch;
    branch.upstream.is_some() && branch.ahead == 0 && !branch.detached && branch.oid.is_some()
}

/// One co-author per line, as `Name <mail>`; other lines are left out.
pub fn co_author_trailers(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| is_co_author(line))
        .map(|line| format!("Co-authored-by: {line}"))
        .collect()
}

/// Where "Push after committing" pushes without asking: the upstream, when it is a branch
/// of the same name. `None` asks with the push dialog instead.
pub fn push_target(status: &Status, remotes: &[Remote]) -> Option<(String, String)> {
    let head = status
        .branch
        .head
        .as_deref()
        .filter(|_| !status.branch.detached)?;
    match split_upstream(status.branch.upstream.as_deref(), remotes) {
        (Some(remote), Some(branch)) if branch == head => Some((remote, branch)),
        _ => None,
    }
}

/// Whether a co-author line looks like `Name <mail>`.
pub fn is_co_author(line: &str) -> bool {
    let line = line.trim();
    match (line.find('<'), line.strip_suffix('>')) {
        (Some(open), Some(_)) => open > 0 && line[open + 1..line.len() - 1].contains('@'),
        _ => false,
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
    let mut options = vec![(
        trf("Ignore “{path}”", &[("path", path)]),
        format!("/{path}"),
    )];
    if let Some(at) = name.rfind('.').filter(|at| *at > 0) {
        let extension = &name[at..];
        options.push((
            trf(
                "Ignore all “*{extension}” files",
                &[("extension", extension)],
            ),
            format!("*{extension}"),
        ));
    }
    if let Some(at) = path.rfind('/') {
        let dir = &path[..=at];
        options.push((
            trf("Ignore folder “{dir}”", &[("dir", dir)]),
            format!("/{dir}"),
        ));
    }
    options
}

/// "3 hours ago", "in 1 minute": whole phrases in the table, as other languages change the
/// word with the phrase around it ("vor 3 Stunden").
fn relative(amount: i64, unit: &str, past: bool) -> String {
    let count = amount.to_string();
    match (amount, unit, past) {
        (1, "day", true) => tr("yesterday").to_owned(),
        (1, "day", false) => tr("tomorrow").to_owned(),
        (1, _, true) => tr(&format!("1 {unit} ago")).to_owned(),
        (1, _, false) => tr(&format!("in 1 {unit}")).to_owned(),
        (_, _, true) => trf(&format!("{{count}} {unit}s ago"), &[("count", &count)]),
        (_, _, false) => trf(&format!("in {{count}} {unit}s"), &[("count", &count)]),
    }
}

/// Compact date for lists: relative within a week, otherwise the date.
pub fn short_date_at(time: i64, now: i64) -> String {
    let seconds = time - now;
    let (past, distance) = (seconds <= 0, seconds.abs());
    let rounded = |unit: i64| (distance + unit / 2) / unit;
    if distance < 60 {
        tr("just now").to_owned()
    } else if distance < 3600 {
        relative(rounded(60), "minute", past)
    } else if distance < 86_400 {
        relative(rounded(3600), "hour", past)
    } else if distance < 7 * 86_400 {
        relative(rounded(86_400), "day", past)
    } else {
        local_format(time, "%b %-d, %Y")
    }
}

pub fn short_date(time: i64) -> String {
    short_date_at(time, chrono::Utc::now().timestamp())
}

pub fn full_date(time: i64) -> String {
    local_format(time, "%b %-d, %Y, %-I:%M %p")
}

fn local_format(time: i64, format: &str) -> String {
    chrono::DateTime::from_timestamp(time, 0)
        .map(|moment| {
            moment
                .with_timezone(&chrono::Local)
                .format(format)
                .to_string()
        })
        .unwrap_or_default()
}

/// Stable hue for an author name, for avatar circles.
pub fn name_hue(name: &str) -> u32 {
    let hash = name.encode_utf16().fold(0i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(unit))
    });
    hash.unsigned_abs() % 360
}

pub fn initials(name: &str) -> String {
    let parts: Vec<&str> = name.split_whitespace().collect();
    let first = |part: &str| {
        part.chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
    };
    match &parts[..] {
        [] => "?".to_owned(),
        [only] => first(only),
        [head, .., last] => first(head) + &first(last),
    }
}

/// The corner radius of the fill under the whole window, or `None` when the window is opaque
/// and needs none. `transparent`: the window was made see-through for round corners;
/// `rounded`: they are round right now (not while maximized).
pub fn underlay_corner(transparent: bool, rounded: bool, radius: u8) -> Option<u8> {
    transparent.then_some(if rounded { radius } else { 0 })
}

/// Whether uncommitted changes would get in the way of a checkout: staged or changed tracked
/// files. New untracked files don't count (git leaves them where they are), and neither does a
/// conflict, which git refuses to check out over and which can't be stashed.
pub fn changes_in_the_way(status: &Status) -> bool {
    status.conflicts.is_empty()
        && (!status.staged.is_empty() || status.unstaged.iter().any(|file| file.status != "?"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commits_are_picked_one_by_one_or_as_a_range_in_list_order() {
        let listed: Vec<String> = ["e", "d", "c", "b", "a"].map(String::from).to_vec();
        let picked = pick_commits(&listed, &[], Some("d"), "b", false);
        assert_eq!(picked, ["d", "b"]);
        let picked = pick_commits(&listed, &picked, Some("d"), "e", false);
        assert_eq!(picked, ["e", "d", "b"]);
        let picked = pick_commits(&listed, &picked, Some("d"), "d", false);
        assert_eq!(picked, ["e", "b"], "a second click takes it out");
        assert_eq!(
            pick_commits(&listed, &[], Some("b"), "e", true),
            ["e", "d", "c", "b"]
        );
        assert_eq!(pick_commits(&listed, &[], None, "c", true), ["c"]);
    }

    #[test]
    fn signature_states_read_as_words() {
        assert_eq!(signature_label('G'), ("Good signature", Tone::Good));
        assert_eq!(signature_label('B').1, Tone::Bad);
        assert_eq!(signature_label('U').1, Tone::Warning);
        assert_eq!(signature_label('E').1, Tone::Muted);
        for status in ['G', 'U', 'X', 'Y', 'R', 'B', 'E'] {
            let (label, _) = signature_label(status);
            let de = crate::i18n::I18N.from_code("de").unwrap();
            assert_ne!(
                crate::i18n::I18N.t(de, label),
                label,
                "no translation for {label}"
            );
        }
    }

    #[test]
    fn co_authors_need_a_name_and_an_address() {
        assert!(is_co_author("Ada Lovelace <ada@example.com>"));
        assert!(!is_co_author("<ada@example.com>"));
        assert!(!is_co_author("Ada"));
        assert!(!is_co_author("Ada <nowhere>"));
        assert_eq!(
            co_author_trailers("  Ada <a@b.c>\n\nnope\nBob <b@c.d>"),
            ["Co-authored-by: Ada <a@b.c>", "Co-authored-by: Bob <b@c.d>"]
        );
    }

    #[test]
    fn amending_warns_only_for_a_pushed_head_and_push_after_needs_a_same_named_upstream() {
        use gitcraft_core::model::BranchStatus;
        let remotes = [Remote {
            name: "origin".into(),
            fetch_url: String::new(),
            push_url: String::new(),
        }];
        let status = |upstream: Option<&str>, ahead| Status {
            branch: BranchStatus {
                head: Some("main".into()),
                oid: Some("abc".into()),
                upstream: upstream.map(str::to_owned),
                ahead,
                behind: 0,
                detached: false,
            },
            ..Status::default()
        };
        assert!(amend_rewrites_pushed(&status(Some("origin/main"), 0)));
        assert!(!amend_rewrites_pushed(&status(Some("origin/main"), 1)));
        assert!(!amend_rewrites_pushed(&status(None, 0)));
        assert_eq!(
            push_target(&status(Some("origin/main"), 1), &remotes),
            Some(("origin".into(), "main".into()))
        );
        assert_eq!(
            push_target(&status(Some("origin/other"), 1), &remotes),
            None
        );
        assert_eq!(push_target(&status(None, 1), &remotes), None);
    }

    #[test]
    fn only_tracked_changes_get_in_the_way_of_a_checkout() {
        use gitcraft_core::model::{ConflictFile, FileChange};
        let file = |status: &str| FileChange {
            path: "a".into(),
            status: status.into(),
            orig_path: None,
        };
        let mut status = Status::default();
        assert!(!changes_in_the_way(&status), "a clean tree");
        status.unstaged = vec![file("?")];
        assert!(!changes_in_the_way(&status), "only a new untracked file");
        status.unstaged = vec![file("M")];
        assert!(changes_in_the_way(&status), "a changed file");
        status.unstaged.clear();
        status.staged = vec![file("A")];
        assert!(changes_in_the_way(&status), "a staged file");
        status.conflicts = vec![ConflictFile {
            path: "b".into(),
            status: "UU".into(),
        }];
        assert!(
            !changes_in_the_way(&status),
            "conflicts: git itself says no"
        );
    }

    #[test]
    fn a_transparent_window_is_always_filled_also_when_maximized() {
        assert_eq!(underlay_corner(true, true, 10), Some(10));
        assert_eq!(
            underlay_corner(true, false, 10),
            Some(0),
            "maximized: square, but filled"
        );
        assert_eq!(
            underlay_corner(false, false, 10),
            None,
            "an opaque window needs no fill"
        );
    }

    #[test]
    fn dates_read_like_the_list_shows_them() {
        let now = 1_700_000_000;
        assert_eq!(short_date_at(now - 10, now), "just now");
        assert_eq!(short_date_at(now - 60, now), "1 minute ago");
        assert_eq!(short_date_at(now - 3 * 3600, now), "3 hours ago");
        assert_eq!(short_date_at(now - 86_400, now), "yesterday");
        assert_eq!(short_date_at(now - 3 * 86_400, now), "3 days ago");
        assert_eq!(short_date_at(now + 7200, now), "in 2 hours");
        assert!(short_date_at(now - 30 * 86_400, now).contains("2023"));
    }

    #[test]
    fn avatars_use_initials_and_a_stable_colour() {
        assert_eq!(initials("Juergen Koennemann"), "JK");
        assert_eq!(initials("Ada Byron King"), "AK");
        assert_eq!(initials("cher"), "C");
        assert_eq!(initials("  "), "?");
        assert_eq!(name_hue("Juergen"), name_hue("Juergen"));
        assert!(name_hue("Juergen") < 360);
    }

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
