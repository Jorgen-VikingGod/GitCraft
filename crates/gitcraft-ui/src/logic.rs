//! UI decisions that need no toolkit, so they can be tested directly.

use std::collections::BTreeMap;

use gitcraft_core::model::{ConflictChunk, Operation, OperationSides, Remote, Status};

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

/// The order and actions `git rebase --autosquash` would give: `(subject, sha)` oldest
/// first in, `(index into steps, new action)` out. `fixup!`/`amend!` become fixups and
/// `squash!` squashes, each right after the commit it names (by subject or sha prefix) and
/// after earlier ones for the same commit. Those naming nothing in the list stay put.
pub fn autosquash(steps: &[(String, String)]) -> Vec<(usize, Option<&'static str>)> {
    let marker = |subject: &str| -> Option<(&'static str, String)> {
        for (prefix, action) in [
            ("fixup! ", "fixup"),
            ("amend! ", "fixup"),
            ("squash! ", "squash"),
        ] {
            if let Some(rest) = subject.strip_prefix(prefix) {
                return Some((action, rest.to_owned()));
            }
        }
        None
    };
    let mut order: Vec<(usize, Option<&'static str>)> = Vec::new();
    for (index, (subject, _)) in steps.iter().enumerate() {
        if let Some((action, target)) = marker(subject) {
            // A marker on a marker ("fixup! fixup! x") names the same commit.
            let mut target = target;
            while let Some((_, inner)) = marker(&target) {
                target = inner;
            }
            let found = order.iter().position(|(other, _)| {
                let (other_subject, other_sha) = &steps[*other];
                *other != index
                    && (*other_subject == target
                        || (target.len() >= 4 && other_sha.starts_with(&target)))
            });
            if let Some(found) = found {
                let mut at = found + 1;
                while at < order.len() && order[at].1.is_some() {
                    at += 1;
                }
                order.insert(at, (index, Some(action)));
                continue;
            }
        }
        order.push((index, None));
    }
    order
}

/// "a, b, c and 2 more files": at most `shown` names, then how many more.
pub fn list_short(names: &[String], shown: usize) -> String {
    let joined = names
        .iter()
        .take(shown)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    match names.len().saturating_sub(shown) {
        0 => joined,
        rest => trf(
            "{path} and {more}",
            &[("path", &joined), ("more", &plural(rest, "more file"))],
        ),
    }
}

/// Where the history list's columns start, from the right edge in: commit, date, author,
/// changes. `None` for a hidden column; the description ends at `subject_end`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColumnX {
    pub sha: Option<f32>,
    pub date: Option<f32>,
    pub author: Option<f32>,
    pub changes: Option<f32>,
    /// The left edges of the date and author columns, where they are resized.
    pub date_edge: Option<f32>,
    pub author_edge: Option<f32>,
    pub subject_end: f32,
}

pub const SHA_COLUMN: f32 = 76.0;
pub const CHANGES_COLUMN: f32 = 100.0;

pub fn column_x(right: f32, columns: &crate::settings::Columns) -> ColumnX {
    let mut edge = right;
    let mut take = |shown: bool, width: f32, pad: f32| {
        shown.then(|| {
            edge -= width;
            (edge + pad, edge)
        })
    };
    let sha = take(columns.sha, SHA_COLUMN, 4.0);
    let date = take(columns.date, columns.date_width, 8.0);
    let author = take(columns.author, columns.author_width, 8.0);
    let changes = take(columns.changes, CHANGES_COLUMN, 8.0);
    ColumnX {
        sha: sha.map(|(x, _)| x),
        date: date.map(|(x, _)| x),
        author: author.map(|(x, _)| x),
        changes: changes.map(|(x, _)| x),
        date_edge: date.map(|(_, edge)| edge),
        author_edge: author.map(|(_, edge)| edge),
        subject_end: edge,
    }
}

/// What to offer after a push, pull or fetch failed, read from git's message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextStep {
    /// The remote has commits the branch doesn't: pull them, or force-push.
    PullOrForce,
    /// The branch tracks nothing to pull from.
    SetUpstream,
    /// There is no remote to talk to.
    AddRemote,
}

pub fn next_step(error: &str) -> Option<NextStep> {
    if error.contains("[rejected]")
        && (error.contains("fetch first") || error.contains("non-fast-forward"))
    {
        Some(NextStep::PullOrForce)
    } else if error.contains("There is no tracking information for the current branch")
        || error.contains("has no upstream branch")
    {
        Some(NextStep::SetUpstream)
    } else if error.contains("No configured push destination")
        || error.contains("No remote repository specified")
    {
        Some(NextStep::AddRemote)
    } else {
        None
    }
}

/// The repository URL of a `gitcraft://clone?url=…` link, which a web page can use to offer
/// "clone in GitCraft". Only https URLs are taken: a link must not point the clone at a local
/// path or an ssh host it names. The dialog still asks before anything is cloned.
pub fn clone_link(link: &str) -> Option<String> {
    let query = link
        .strip_prefix("gitcraft://clone")?
        .trim_start_matches('/');
    let query = query.strip_prefix('?')?;
    let value = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("url="))?;
    // Percent-decoding, byte by byte; malformed escapes make the link unusable.
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = std::str::from_utf8(bytes.get(index + 1..index + 3)?).ok()?;
                decoded.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    let url = String::from_utf8(decoded).ok()?;
    let safe = url.starts_with("https://")
        && url.len() > "https://".len()
        && !url.chars().any(|c| c.is_whitespace() || c.is_control());
    safe.then_some(url)
}

/// How a git setting is entered in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKind {
    /// Free text, with a hint.
    Text(&'static str),
    /// One of these (value, English label); "" is "not set".
    Choice(&'static [(&'static str, &'static str)]),
}

const YES_NO: &[(&str, &str)] = &[("", "Not set"), ("true", "Yes"), ("false", "No")];

/// The git settings shown in Settings › Git, by key (lower case, as git lists them), with
/// their English label.
pub const GIT_SETTINGS: [(&str, &str, SettingKind); 11] = [
    (
        "user.name",
        "Name",
        SettingKind::Text("Your name, as commits show it"),
    ),
    ("user.email", "Email", SettingKind::Text("you@example.com")),
    (
        "commit.gpgsign",
        "Sign commits",
        SettingKind::Choice(YES_NO),
    ),
    (
        "tag.gpgsign",
        "Sign annotated tags",
        SettingKind::Choice(YES_NO),
    ),
    (
        "gpg.format",
        "Signing with",
        SettingKind::Choice(&[
            ("", "Not set (GPG)"),
            ("openpgp", "GPG"),
            ("ssh", "An SSH key"),
            ("x509", "X.509 (S/MIME)"),
        ]),
    ),
    (
        "user.signingkey",
        "Signing key",
        SettingKind::Text("A GPG key id, or an SSH key file"),
    ),
    (
        "gpg.ssh.allowedsignersfile",
        "Allowed signers file",
        SettingKind::Text("To check SSH signatures"),
    ),
    (
        "core.autocrlf",
        "Line endings",
        SettingKind::Choice(&[
            ("", "Not set"),
            ("true", "CRLF in the working tree (Windows)"),
            ("input", "LF in commits, as written in the working tree"),
            ("false", "Leave them as they are"),
        ]),
    ),
    (
        "init.defaultbranch",
        "First branch of new repositories",
        SettingKind::Text("main"),
    ),
    (
        "rerere.enabled",
        "Remember conflict resolutions (rerere)",
        SettingKind::Choice(YES_NO),
    ),
    (
        "core.hookspath",
        "Hooks folder",
        SettingKind::Text(".git/hooks"),
    ),
];

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

// -------------------------------------------------------------- shortcuts

/// The commands keys can be given to: (id, English label, default keys). Keys are written as
/// craft-keymap chords, where `Cmd` is Ctrl outside macOS. The arrow keys, Escape and the
/// rebase editor's letters are fixed and not listed.
pub const SHORTCUTS: [(&str, &str, &[&str]); 16] = [
    ("view.history", "All commits", &["Cmd+1"]),
    ("view.changes", "Local changes", &["Cmd+2"]),
    ("edit.undo", "Undo the last operation", &["Cmd+Z"]),
    (
        "edit.redo",
        "Redo the last operation",
        &["Cmd+Shift+Z", "Cmd+Y"],
    ),
    ("search", "Search commits", &["Cmd+F"]),
    (
        "files.select_all",
        "Select every file of the list",
        &["Cmd+A"],
    ),
    ("repo.open", "Open repository", &["Cmd+O", "Cmd+T"]),
    ("tab.close", "Close tab", &["Cmd+W"]),
    ("commit", "Commit", &["Cmd+Enter"]),
    (
        "commit.all",
        "Stage everything and commit",
        &["Cmd+Shift+Enter"],
    ),
    ("repo.refresh", "Refresh repository", &["F5", "Cmd+R"]),
    ("settings", "Settings", &["Cmd+Comma"]),
    ("zoom.in", "Zoom in", &["Cmd+Equals", "Cmd+Plus"]),
    ("zoom.out", "Zoom out", &["Cmd+Minus"]),
    ("zoom.reset", "Actual size", &["Cmd+0"]),
    ("window.fullscreen", "Full screen", &["F11"]),
];

/// A command's keys: the user's (an empty list: cleared), else its defaults.
pub fn shortcut_keys(user: &BTreeMap<String, Vec<String>>, id: &str) -> Vec<craft_keymap::Chord> {
    let texts: Vec<&str> = match user.get(id) {
        Some(own) => own.iter().map(String::as_str).collect(),
        None => SHORTCUTS
            .iter()
            .find(|(each, _, _)| *each == id)
            .map_or_else(Vec::new, |(_, _, keys)| keys.to_vec()),
    };
    texts
        .into_iter()
        .filter_map(|text| craft_keymap::Chord::parse(text).ok())
        .collect()
}

/// Gives `id` the key `chord`, in place of its key number `slot` or (`None`) added to its keys,
/// and takes it from any other command that had it. Returns the commands it was taken from.
pub fn assign_shortcut(
    user: &mut BTreeMap<String, Vec<String>>,
    id: &str,
    slot: Option<usize>,
    chord: &craft_keymap::Chord,
) -> Vec<&'static str> {
    let mut taken = Vec::new();
    for (other, _, _) in SHORTCUTS {
        if other == id {
            continue;
        }
        let keys = shortcut_keys(user, other);
        if keys.contains(chord) {
            let kept: Vec<String> = keys
                .iter()
                .filter(|key| *key != chord)
                .map(ToString::to_string)
                .collect();
            user.insert(other.to_owned(), kept);
            taken.push(other);
        }
    }
    let mut keys = shortcut_keys(user, id);
    match slot.filter(|slot| *slot < keys.len()) {
        Some(slot) => keys[slot] = *chord,
        None if !keys.contains(chord) => keys.push(*chord),
        None => {}
    }
    // The same key twice, after replacing one with a key it already has.
    let mut unique: Vec<craft_keymap::Chord> = Vec::new();
    for key in keys {
        if !unique.contains(&key) {
            unique.push(key);
        }
    }
    user.insert(
        id.to_owned(),
        unique.iter().map(ToString::to_string).collect(),
    );
    forget_defaults(user, id);
    taken
}

/// A command whose keys are its defaults again needs no entry of its own.
fn forget_defaults(user: &mut BTreeMap<String, Vec<String>>, id: &str) {
    let defaults: Vec<craft_keymap::Chord> = shortcut_keys(&BTreeMap::new(), id);
    if user.get(id).is_some() && shortcut_keys(user, id) == defaults {
        user.remove(id);
    }
}

/// Whether the branch `full` (`refs/heads/…`) can be hidden from the graph: not the branch
/// checked out, whose commits git shows anyway (only its label would go).
pub fn can_hide(full: &str, current_branch: Option<&str>) -> bool {
    current_branch.is_none_or(|current| full != format!("refs/heads/{current}"))
}

/// What the operation banner calls the operation in progress: "Merging feature into main",
/// "Rebasing feature onto main". Without names (sides unknown) the plain "Merge in progress".
pub fn operation_title(operation: &Operation, sides: Option<&OperationSides>) -> String {
    let named = |value: &str| (!value.is_empty()).then(|| value.to_owned());
    let ours = sides.and_then(|sides| named(&sides.ours));
    let theirs = sides.and_then(|sides| named(&sides.theirs));
    match (operation, ours, theirs) {
        (Operation::Merge { .. }, Some(ours), Some(theirs)) => trf(
            "Merging {theirs} into {ours}",
            &[("theirs", &theirs), ("ours", &ours)],
        ),
        (Operation::Merge { .. }, ..) => tr("Merge in progress").to_owned(),
        (Operation::Rebase { branch, .. }, Some(ours), _) => match branch {
            Some(branch) => trf(
                "Rebasing {branch} onto {ours}",
                &[("branch", branch), ("ours", &ours)],
            ),
            None => trf("Rebasing onto {ours}", &[("ours", &ours)]),
        },
        (Operation::Rebase { .. }, ..) => tr("Rebase in progress").to_owned(),
        (Operation::CherryPick { .. }, Some(ours), Some(theirs)) => trf(
            "Cherry-picking {theirs} onto {ours}",
            &[("theirs", &theirs), ("ours", &ours)],
        ),
        (Operation::CherryPick { .. }, ..) => tr("Cherry-pick in progress").to_owned(),
        (Operation::Revert { .. }, Some(ours), Some(theirs)) => trf(
            "Reverting {theirs} on {ours}",
            &[("theirs", &theirs), ("ours", &ours)],
        ),
        (Operation::Revert { .. }, ..) => tr("Revert in progress").to_owned(),
        (Operation::Am, ..) => tr("Applying patches (git am)").to_owned(),
    }
}

/// What the two sides of a conflict are called, with what they are in this operation: "main
/// · this branch" and "feature · being merged". A rebase swaps the roles git gives them: ours
/// is what the branch goes onto, theirs your own commit.
pub fn side_labels(
    operation: Option<&Operation>,
    sides: Option<&OperationSides>,
) -> (String, String) {
    let named = |value: &str| (!value.is_empty()).then(|| value.to_owned());
    let ours = sides.and_then(|sides| named(&sides.ours));
    let theirs = sides.and_then(|sides| named(&sides.theirs));
    let label = |name: Option<String>, role: &str, fallback: &str| match name {
        Some(name) => trf(role, &[("name", &name)]),
        None => tr(fallback).to_owned(),
    };
    match operation {
        Some(Operation::Rebase { .. }) => (
            label(
                ours,
                "{name} · being rebased onto",
                "Upstream (being rebased onto)",
            ),
            label(
                theirs,
                "{name} · your commit",
                "Your commit (being replayed)",
            ),
        ),
        Some(Operation::CherryPick { .. }) => (
            label(ours, "{name} · this branch", "Ours (HEAD)"),
            label(theirs, "{name} · being picked", "Theirs (incoming)"),
        ),
        Some(Operation::Revert { .. }) => (
            label(ours, "{name} · this branch", "Ours (HEAD)"),
            label(theirs, "{name} · being reverted", "Theirs (incoming)"),
        ),
        Some(Operation::Merge { .. }) => (
            label(ours, "{name} · this branch", "Ours (HEAD)"),
            label(theirs, "{name} · being merged", "Theirs (incoming)"),
        ),
        _ => (
            label(ours, "{name} · this branch", "Ours (HEAD)"),
            tr("Theirs (incoming)").to_owned(),
        ),
    }
}

/// What Continue commits a merge with: the commit box's message, once it has a summary.
/// Other operations commit what they replay, with its own message.
pub fn continue_message(operation: &Operation, summary: &str, body: &str) -> Option<String> {
    (matches!(operation, Operation::Merge { .. }) && !summary.trim().is_empty())
        .then(|| commit_message(summary, body))
}

/// Whether the commit box gives way to a note about the operation in progress: a rebase,
/// cherry-pick, revert or patch commits what it replays when continued. A merge keeps it for
/// its message, and a rebase stopped at an `edit` step for amending.
pub fn commit_box_gives_way(operation: Option<&Operation>) -> bool {
    match operation {
        None | Some(Operation::Merge { .. }) => false,
        Some(Operation::Rebase { editing, .. }) => !editing,
        Some(_) => true,
    }
}

/// What each side did to a conflicted file, from its two status letters (`UU`, `AA`, `DU`…:
/// ours first): "Changed", "Added" or "Deleted"; and in one sentence what that makes.
pub fn conflict_kind(status: &str) -> (&'static str, &'static str, &'static str) {
    let side = |letter: Option<char>| match letter {
        Some('A') => "Added",
        Some('D') => "Deleted",
        _ => "Changed",
    };
    let mut letters = status.chars();
    let (ours, theirs) = (letters.next(), letters.next());
    let summary = match (ours, theirs) {
        (Some('A'), Some('A')) => {
            "Both sides added a file with this name, with different contents."
        }
        (Some('D'), Some('D')) => "Both sides deleted this file.",
        (Some('D'), _) | (_, Some('D')) => "One side changed this file, the other deleted it.",
        (Some('A'), _) | (_, Some('A')) => "One side added this file while the other changed it.",
        _ => "Both sides changed this file in the same places.",
    };
    (side(ours), side(theirs), summary)
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

/// The pattern that would store files like this one in LFS ("*.psd"), unless LFS already
/// tracks exactly that for this file: a pattern counts only when its attributes file's folder
/// holds the file (a root `.gitattributes` holds everything). Files without an extension get
/// none: their own path is rarely what someone wants tracked.
pub fn lfs_track_suggestion(
    path: &str,
    tracked: &[gitcraft_core::model::LfsPattern],
) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let at = name
        .rfind('.')
        .filter(|at| *at > 0 && *at + 1 < name.len())?;
    let pattern = format!("*{}", &name[at..]);
    let covers = |source: &str| {
        let folder = source
            .strip_suffix(".gitattributes")
            .unwrap_or(source)
            .trim_end_matches('/');
        folder.is_empty() || path.starts_with(&format!("{folder}/"))
    };
    (!tracked
        .iter()
        .any(|known| known.pattern == pattern && covers(&known.source)))
    .then_some(pattern)
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
    fn lfs_suggests_the_extension_unless_it_is_tracked() {
        let tracked = |pattern: &str| gitcraft_core::model::LfsPattern {
            pattern: pattern.to_owned(),
            source: ".gitattributes".to_owned(),
            lockable: false,
        };
        assert_eq!(
            lfs_track_suggestion("art/cover.psd", &[]).as_deref(),
            Some("*.psd")
        );
        assert_eq!(
            lfs_track_suggestion("cover.PSD", &[tracked("*.psd")]).as_deref(),
            Some("*.PSD")
        );
        assert_eq!(
            lfs_track_suggestion("art/cover.psd", &[tracked("*.psd")]),
            None
        );
        assert_eq!(lfs_track_suggestion("Makefile", &[]), None);
        assert_eq!(lfs_track_suggestion(".env", &[]), None);
        assert_eq!(lfs_track_suggestion("notes.", &[]), None);
        // A folder's attributes cover only that folder.
        let in_assets = gitcraft_core::model::LfsPattern {
            source: "assets/.gitattributes".to_owned(),
            ..tracked("*.wav")
        };
        assert_eq!(
            lfs_track_suggestion("assets/drums/kick.wav", std::slice::from_ref(&in_assets)),
            None
        );
        assert_eq!(
            lfs_track_suggestion("sounds/kick.wav", std::slice::from_ref(&in_assets)).as_deref(),
            Some("*.wav")
        );
        assert_eq!(
            lfs_track_suggestion("assetsplus/kick.wav", &[in_assets]).as_deref(),
            Some("*.wav"),
            "a folder with a longer name isn't inside it"
        );
    }

    #[test]
    fn every_git_setting_shown_is_one_the_core_changes() {
        for (key, _, _) in GIT_SETTINGS {
            assert!(gitcraft_core::queries::CONFIG_KEYS.contains(&key), "{key}");
        }
        assert_eq!(
            GIT_SETTINGS.len(),
            gitcraft_core::queries::CONFIG_KEYS.len()
        );
    }

    #[test]
    fn clone_links_give_https_urls_only() {
        assert_eq!(
            clone_link("gitcraft://clone?url=https%3A%2F%2Fexample.com%2Fa%2Fb.git").as_deref(),
            Some("https://example.com/a/b.git")
        );
        assert_eq!(
            clone_link("gitcraft://clone/?url=https://example.com/x.git&ref=main").as_deref(),
            Some("https://example.com/x.git")
        );
        assert_eq!(clone_link("gitcraft://clone?url=file%3A%2F%2F%2Fetc"), None);
        assert_eq!(clone_link("gitcraft://clone?url=git%40host%3Ax.git"), None);
        assert_eq!(clone_link("gitcraft://clone?url=https%3A%2F%2Fa%20b"), None);
        assert_eq!(clone_link("gitcraft://clone?url=%zz"), None);
        assert_eq!(clone_link("/home/me/repo"), None);
        assert_eq!(clone_link("gitcraft://other?url=https://x"), None);
    }

    #[test]
    fn failed_pushes_and_pulls_say_what_to_do_next() {
        let rejected =
            " ! [rejected]        HEAD -> main (fetch first)\nerror: failed to push some refs";
        assert_eq!(next_step(rejected), Some(NextStep::PullOrForce));
        assert_eq!(
            next_step(" ! [rejected]        main -> main (non-fast-forward)"),
            Some(NextStep::PullOrForce)
        );
        assert_eq!(
            next_step("There is no tracking information for the current branch.\nPlease specify"),
            Some(NextStep::SetUpstream)
        );
        assert_eq!(
            next_step("fatal: No configured push destination."),
            Some(NextStep::AddRemote)
        );
        assert_eq!(next_step("fatal: Authentication failed"), None);
    }

    #[test]
    fn columns_are_laid_out_from_the_right() {
        let mut columns = crate::settings::Columns::default();
        let x = column_x(1000.0, &columns);
        assert_eq!(x.sha, Some(1000.0 - 76.0 + 4.0));
        assert_eq!(x.date, Some(1000.0 - 76.0 - 120.0 + 8.0));
        assert_eq!(x.author, Some(1000.0 - 76.0 - 120.0 - 150.0 + 8.0));
        assert_eq!(x.changes, None);
        assert_eq!(x.subject_end, 1000.0 - 76.0 - 120.0 - 150.0);
        columns.date = false;
        columns.changes = true;
        let x = column_x(1000.0, &columns);
        assert_eq!(x.date, None);
        assert_eq!(x.author, Some(1000.0 - 76.0 - 150.0 + 8.0));
        assert_eq!(x.subject_end, 1000.0 - 76.0 - 150.0 - 100.0);
    }

    #[test]
    fn autosquash_moves_fixups_after_their_commit() {
        let steps: Vec<(String, String)> = [
            ("add a", "aaaa1111"),
            ("add b", "bbbb2222"),
            ("fixup! add a", "cccc3333"),
            ("squash! aaaa1111", "dddd4444"),
            ("fixup! nothing here", "eeee5555"),
        ]
        .map(|(s, h)| (s.to_owned(), h.to_owned()))
        .to_vec();
        assert_eq!(
            autosquash(&steps),
            [
                (0, None),
                (2, Some("fixup")),
                (3, Some("squash")),
                (1, None),
                (4, None)
            ]
        );
    }

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
    fn operations_and_their_sides_are_named() {
        let sides = OperationSides {
            ours: "main".into(),
            theirs: "feature/alerts".into(),
        };
        let merge = Operation::Merge { head: "abc".into() };
        assert_eq!(
            operation_title(&merge, Some(&sides)),
            "Merging feature/alerts into main"
        );
        assert_eq!(operation_title(&merge, None), "Merge in progress");
        assert_eq!(
            side_labels(Some(&merge), Some(&sides)),
            (
                "main · this branch".to_owned(),
                "feature/alerts · being merged".to_owned()
            )
        );
        let rebase = Operation::Rebase {
            branch: Some("feature/alerts".into()),
            onto: Some("abc".into()),
            step: Some(1),
            total: Some(2),
            stopped_sha: None,
            editing: false,
        };
        let replaying = OperationSides {
            ours: "main".into(),
            theirs: "1234abc Warn earlier".into(),
        };
        assert_eq!(
            operation_title(&rebase, Some(&replaying)),
            "Rebasing feature/alerts onto main"
        );
        assert_eq!(
            side_labels(Some(&rebase), Some(&replaying)),
            (
                "main · being rebased onto".to_owned(),
                "1234abc Warn earlier · your commit".to_owned()
            )
        );
        assert_eq!(
            side_labels(Some(&rebase), None).1,
            "Your commit (being replayed)"
        );
    }

    #[test]
    fn a_merge_continues_with_the_commit_box_and_other_operations_without_it() {
        let merge = Operation::Merge { head: "abc".into() };
        assert_eq!(
            continue_message(&merge, "Merge feature", "Why.").as_deref(),
            Some("Merge feature\n\nWhy.\n")
        );
        assert_eq!(continue_message(&merge, "  ", "body only"), None);
        let rebase = |editing| Operation::Rebase {
            branch: None,
            onto: None,
            step: None,
            total: None,
            stopped_sha: None,
            editing,
        };
        assert_eq!(continue_message(&rebase(false), "typed", ""), None);
        assert!(!commit_box_gives_way(None));
        assert!(!commit_box_gives_way(Some(&merge)));
        assert!(commit_box_gives_way(Some(&rebase(false))));
        assert!(
            !commit_box_gives_way(Some(&rebase(true))),
            "an edit stop amends"
        );
        assert!(commit_box_gives_way(Some(&Operation::CherryPick {
            head: "a".into()
        })));
    }

    #[test]
    fn every_default_shortcut_is_a_key_and_none_is_given_twice() {
        let none = BTreeMap::new();
        let mut seen = Vec::new();
        for (id, _, defaults) in SHORTCUTS {
            let keys = shortcut_keys(&none, id);
            assert_eq!(keys.len(), defaults.len(), "{id}: {defaults:?}");
            for key in keys {
                assert!(!seen.contains(&key), "{key} twice");
                seen.push(key);
            }
        }
    }

    #[test]
    fn shortcuts_are_changed_added_cleared_and_taken_from_others() {
        let chord = |text: &str| craft_keymap::Chord::parse(text).unwrap();
        let mut user = BTreeMap::new();
        // Search gets Ctrl+K in place of Ctrl+F.
        assert!(assign_shortcut(&mut user, "search", Some(0), &chord("Cmd+K")).is_empty());
        assert_eq!(shortcut_keys(&user, "search"), [chord("Cmd+K")]);
        // A second key for it, taken from Refresh.
        let taken = assign_shortcut(&mut user, "search", None, &chord("F5"));
        assert_eq!(taken, ["repo.refresh"]);
        assert_eq!(
            shortcut_keys(&user, "search"),
            [chord("Cmd+K"), chord("F5")]
        );
        assert_eq!(shortcut_keys(&user, "repo.refresh"), [chord("Cmd+R")]);
        // Cleared is kept as cleared; defaults again need no entry.
        user.insert("commit".into(), Vec::new());
        assert!(shortcut_keys(&user, "commit").is_empty());
        assign_shortcut(&mut user, "zoom.out", Some(0), &chord("Cmd+Minus"));
        assert!(!user.contains_key("zoom.out"), "back to its default");
    }

    #[test]
    fn the_checked_out_branch_cannot_be_hidden() {
        assert!(!can_hide("refs/heads/main", Some("main")));
        assert!(can_hide("refs/heads/topic", Some("main")));
        assert!(can_hide("refs/remotes/origin/main", Some("main")));
        assert!(
            can_hide("refs/heads/main", None),
            "detached: nothing is checked out"
        );
    }

    #[test]
    fn conflicts_say_what_each_side_did() {
        assert_eq!(conflict_kind("UU").0, "Changed");
        assert!(conflict_kind("UU").2.starts_with("Both sides changed"));
        let (ours, theirs, _) = conflict_kind("DU");
        assert_eq!((ours, theirs), ("Deleted", "Changed"));
        let (ours, theirs, _) = conflict_kind("UD");
        assert_eq!((ours, theirs), ("Changed", "Deleted"));
        assert!(conflict_kind("UD").2.contains("deleted"));
        assert!(conflict_kind("AA").2.starts_with("Both sides added"));
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
