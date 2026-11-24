//! Operations against real git in throwaway repositories, no mocks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gitcraft_core::model::*;
use gitcraft_core::parsers::Selection;
use gitcraft_core::runner::{Progress, set_editor_exe};
use gitcraft_core::watcher::{self, WatchEvent};
use gitcraft_core::{Error, ops, queries, undo};

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    path: PathBuf,
    repo: String,
}

fn raw_git(cwd: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_EDITOR", "true")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

impl Fixture {
    fn empty() -> Self {
        set_editor_exe(env!("CARGO_BIN_EXE_gitcraft-editor-stub").into());
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let path = root.join("repo");
        std::fs::create_dir(&path).unwrap();
        let fixture = Self {
            repo: path.to_str().unwrap().to_owned(),
            root,
            path,
            _dir: dir,
        };
        fixture.git(&["init", "-q", "-b", "main"]);
        fixture.identity(&fixture.path);
        fixture
    }

    fn with_history() -> Self {
        let fixture = Self::empty();
        fixture.commit("a.txt", "one\ntwo\nthree\n", "first");
        fixture.commit("a.txt", "one\ntwo\nthree\nfour\n", "second");
        fixture
    }

    fn identity(&self, path: &Path) {
        raw_git(path, &["config", "user.name", "Test User"]);
        raw_git(path, &["config", "user.email", "test@example.com"]);
        raw_git(path, &["config", "commit.gpgsign", "false"]);
        // Git for Windows turns this on by default; tests below opt in where it matters.
        raw_git(path, &["config", "core.autocrlf", "false"]);
    }

    fn git(&self, args: &[&str]) -> String {
        String::from_utf8_lossy(&raw_git(&self.path, args)).into_owned()
    }

    fn write(&self, name: &str, content: impl AsRef<[u8]>) {
        write_at(&self.path, name, content);
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path.join(name)).unwrap()
    }

    fn exists(&self, name: &str) -> bool {
        self.path.join(name).exists()
    }

    fn commit(&self, name: &str, content: impl AsRef<[u8]>, message: &str) -> String {
        commit_at(&self.path, name, content, message)
    }

    fn make_conflict(&self) {
        self.git(&["switch", "-q", "-c", "other"]);
        self.commit("a.txt", "one\ntwo\nthree\nTHEIRS\n", "theirs");
        self.git(&["switch", "-q", "main"]);
        self.commit("a.txt", "one\ntwo\nthree\nOURS\n", "ours");
    }
}

fn write_at(root: &Path, name: &str, content: impl AsRef<[u8]>) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn commit_at(root: &Path, name: &str, content: impl AsRef<[u8]>, message: &str) -> String {
    write_at(root, name, content);
    raw_git(root, &["add", "--", name]);
    raw_git(root, &["commit", "-q", "-m", message]);
    String::from_utf8(raw_git(root, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned()
}

fn subjects(commits: &[Commit]) -> Vec<&str> {
    commits
        .iter()
        .map(|commit| commit.subject.as_str())
        .collect()
}

fn paths(files: &[FileChange]) -> Vec<&str> {
    files.iter().map(|file| file.path.as_str()).collect()
}

fn lines_of(selected: &[usize]) -> Option<BTreeSet<usize>> {
    Some(selected.iter().copied().collect())
}

fn whole(hunk: usize) -> Selection {
    BTreeMap::from([(hunk, None)])
}

fn line_index(diff: &FileDiff, hunk: usize, wanted: impl Fn(&DiffLine) -> bool) -> usize {
    diff.hunks[hunk]
        .lines
        .iter()
        .position(wanted)
        .expect("line exists")
}

#[tokio::test]
async fn log_and_detail() {
    let f = Fixture::with_history();
    let data = queries::log(&f.repo, 10, true, None).await.unwrap();
    assert_eq!(subjects(&data.commits), ["second", "first"]);
    let first_ref = &data.commits[0].refs.as_ref().unwrap()[0];
    assert_eq!(
        (
            first_ref.name.as_str(),
            first_ref.kind.as_str(),
            first_ref.current
        ),
        ("main", "local", true)
    );
    assert!(!data.has_more);
    let detail = queries::commit_detail(&f.repo, &data.commits[0].sha)
        .await
        .unwrap();
    assert_eq!(
        detail.files,
        [FileChange {
            status: "M".into(),
            orig_path: None,
            path: "a.txt".into()
        }]
    );
    let diff = queries::commit_diff(&f.repo, &detail.sha, "a.txt", None, 3, false)
        .await
        .unwrap()
        .unwrap();
    let added: Vec<_> = diff.hunks[0]
        .lines
        .iter()
        .filter(|l| l.t == '+')
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(added, ["four"]);
    let root = queries::commit_detail(&f.repo, &data.commits[1].sha)
        .await
        .unwrap();
    assert_eq!(root.files[0].status, "A");
}

#[tokio::test]
async fn empty_repo() {
    let f = Fixture::empty();
    assert!(
        queries::log(&f.repo, 10, true, None)
            .await
            .unwrap()
            .commits
            .is_empty()
    );
    assert_eq!(
        queries::status(&f.repo)
            .await
            .unwrap()
            .branch
            .head
            .as_deref(),
        Some("main")
    );
}

#[tokio::test]
async fn stage_lines_and_commit() {
    let f = Fixture::with_history();
    f.write("a.txt", "ONE\ntwo\nthree\nfour\nfive\n");
    let diff = queries::working_diff(&f.repo, "a.txt", false, 0, false, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(diff.hunks.len(), 2);
    // Stage only the second hunk ("five").
    ops::apply_selection(&f.repo, "a.txt", "stage", &whole(1), 0)
        .await
        .unwrap();
    let staged = queries::working_diff(&f.repo, "a.txt", true, 3, false, None)
        .await
        .unwrap()
        .unwrap();
    let changed: Vec<_> = staged.hunks[0]
        .lines
        .iter()
        .filter(|l| l.t != ' ')
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(changed, ["five"]);

    // Stage just the "+ONE" line of the first hunk, leaving "-one" unstaged.
    let diff = queries::working_diff(&f.repo, "a.txt", false, 3, false, None)
        .await
        .unwrap()
        .unwrap();
    let plus = line_index(&diff, 0, |l| l.t == '+');
    let selection = BTreeMap::from([(0, lines_of(&[plus]))]);
    ops::apply_selection(&f.repo, "a.txt", "stage", &selection, 3)
        .await
        .unwrap();
    assert_eq!(
        f.git(&["show", ":a.txt"]),
        "one\nONE\ntwo\nthree\nfour\nfive\n"
    );

    // Unstage it again.
    let staged = queries::working_diff(&f.repo, "a.txt", true, 3, false, None)
        .await
        .unwrap()
        .unwrap();
    let plus = line_index(&staged, 0, |l| l.t == '+' && l.text == "ONE");
    let selection = BTreeMap::from([(0, lines_of(&[plus]))]);
    ops::apply_selection(&f.repo, "a.txt", "unstage", &selection, 3)
        .await
        .unwrap();
    assert_eq!(f.git(&["show", ":a.txt"]), "one\ntwo\nthree\nfour\nfive\n");

    assert!(
        ops::commit(&f.repo, "add five\n\nbody", false, false)
            .await
            .unwrap()
            .ok
    );
    assert_eq!(
        queries::head_message(&f.repo).await.unwrap(),
        "add five\n\nbody"
    );
    let status = queries::status(&f.repo).await.unwrap();
    assert!(status.staged.is_empty());
    assert_eq!(status.unstaged[0].path, "a.txt");
}

#[tokio::test]
async fn discard_lines_and_files() {
    let f = Fixture::with_history();
    f.write("a.txt", "one\ntwo\nthree\nfour\nextra1\nextra2\n");
    f.write("new.txt", "new\n");
    let diff = queries::working_diff(&f.repo, "a.txt", false, 3, false, None)
        .await
        .unwrap()
        .unwrap();
    let first = line_index(&diff, 0, |l| l.text == "extra1");
    let selection = BTreeMap::from([(0, lines_of(&[first]))]);
    ops::apply_selection(&f.repo, "a.txt", "discard", &selection, 3)
        .await
        .unwrap();
    assert_eq!(f.read("a.txt"), "one\ntwo\nthree\nfour\nextra2\n");
    ops::discard_files(&f.repo, &["a.txt".into(), "new.txt".into()])
        .await
        .unwrap();
    assert!(!f.exists("new.txt"));
    assert!(queries::status(&f.repo).await.unwrap().unstaged.is_empty());
}

#[tokio::test]
async fn discarded_files_can_be_put_back() {
    let f = Fixture::with_history();
    f.commit("b.txt", "bee\n", "third");
    // A staged change, with more on top that is not staged.
    f.write("a.txt", "one\ntwo\nthree\nfour\nstaged\n");
    f.git(&["add", "a.txt"]);
    f.write("a.txt", "one\ntwo\nthree\nfour\nstaged\nnot staged\n");
    f.write("we*ird name.txt", "new\n");
    std::fs::remove_file(f.path.join("b.txt")).unwrap();
    let staged = f.git(&["diff", "--cached"]);
    let list: Vec<String> = vec!["a.txt".into(), "we*ird name.txt".into(), "b.txt".into()];
    ops::discard_files(&f.repo, &list).await.unwrap();
    assert_eq!(f.read("a.txt"), "one\ntwo\nthree\nfour\nstaged\n");
    assert!(!f.exists("we*ird name.txt"));
    assert_eq!(f.read("b.txt"), "bee\n");

    let backups = queries::discard_backups(&f.repo).await.unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].paths, list);
    // The backup commits stay out of the history and out of search.
    let page = queries::log(&f.repo, 100, true, None).await.unwrap();
    assert_eq!(subjects(&page.commits), ["third", "second", "first"]);
    let found = queries::search(&f.repo, "discarded", "message", 100)
        .await
        .unwrap();
    assert!(found.is_empty());

    ops::restore_discard(&f.repo, &backups[0].name)
        .await
        .unwrap();
    assert_eq!(
        f.read("a.txt"),
        "one\ntwo\nthree\nfour\nstaged\nnot staged\n"
    );
    assert_eq!(f.read("we*ird name.txt"), "new\n");
    assert!(!f.exists("b.txt"));
    assert_eq!(
        f.git(&["diff", "--cached"]),
        staged,
        "the index is left alone"
    );
    assert!(queries::discard_backups(&f.repo).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_backup_never_overwrites_later_work() {
    let f = Fixture::with_history();
    f.write("a.txt", "discarded\n");
    ops::discard_files(&f.repo, &["a.txt".into()])
        .await
        .unwrap();
    f.write("a.txt", "written later\n");
    let name = queries::discard_backups(&f.repo).await.unwrap()[0]
        .name
        .clone();
    assert!(matches!(
        ops::restore_discard(&f.repo, &name).await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(f.read("a.txt"), "written later\n");
    ops::delete_discard(&f.repo, &name).await.unwrap();
    assert!(queries::discard_backups(&f.repo).await.unwrap().is_empty());
    assert!(matches!(
        ops::restore_discard(&f.repo, "refs/heads/main").await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn discarded_lines_can_be_put_back() {
    let f = Fixture::with_history();
    f.write("a.txt", "one\ntwo\nthree\nfour\nextra1\nextra2\n");
    let diff = queries::working_diff(&f.repo, "a.txt", false, 3, false, None)
        .await
        .unwrap()
        .unwrap();
    let first = line_index(&diff, 0, |l| l.text == "extra1");
    let selection = BTreeMap::from([(0, lines_of(&[first]))]);
    ops::apply_selection(&f.repo, "a.txt", "discard", &selection, 3)
        .await
        .unwrap();
    assert_eq!(f.read("a.txt"), "one\ntwo\nthree\nfour\nextra2\n");
    // Staging keeps no backup: nothing is lost by it.
    let name = queries::discard_backups(&f.repo).await.unwrap()[0]
        .name
        .clone();
    ops::restore_discard(&f.repo, &name).await.unwrap();
    assert_eq!(f.read("a.txt"), "one\ntwo\nthree\nfour\nextra1\nextra2\n");
}

#[tokio::test]
async fn discarding_before_the_first_commit_keeps_a_backup_too() {
    let f = Fixture::empty();
    f.write("n.txt", "hello\n");
    ops::discard_files(&f.repo, &["n.txt".into()])
        .await
        .unwrap();
    assert!(!f.exists("n.txt"));
    let page = queries::log(&f.repo, 100, true, None).await.unwrap();
    assert!(page.commits.is_empty());
    let name = queries::discard_backups(&f.repo).await.unwrap()[0]
        .name
        .clone();
    ops::restore_discard(&f.repo, &name).await.unwrap();
    assert_eq!(f.read("n.txt"), "hello\n");
}

#[tokio::test]
async fn old_backups_are_let_go() {
    let f = Fixture::with_history();
    let head = f.git(&["rev-parse", "HEAD"]);
    let old = "refs/gitcraft/discards/000000000001-0123456789ab";
    f.git(&["update-ref", old, head.trim()]);
    f.write("a.txt", "changed\n");
    ops::discard_files(&f.repo, &["a.txt".into()])
        .await
        .unwrap();
    let backups = queries::discard_backups(&f.repo).await.unwrap();
    assert_eq!(backups.len(), 1);
    assert_ne!(backups[0].name, old);
}

#[tokio::test]
async fn untracked_diff_and_stage() {
    let f = Fixture::empty();
    f.write("n.txt", "hello\n");
    let diff = queries::working_diff(&f.repo, "n.txt", false, 3, false, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(diff.status, "?");
    let selection = BTreeMap::from([(0, lines_of(&[0]))]);
    assert!(matches!(
        ops::apply_selection(&f.repo, "n.txt", "stage", &selection, 3).await,
        Err(Error::Invalid(_))
    ));
    ops::stage_files(&f.repo, &["n.txt".into()]).await.unwrap();
    let status = queries::status(&f.repo).await.unwrap();
    assert_eq!(
        status.staged[0],
        FileChange {
            path: "n.txt".into(),
            status: "A".into(),
            orig_path: None
        }
    );
    ops::unstage_files(&f.repo, &["n.txt".into()])
        .await
        .unwrap(); // no HEAD yet
    assert!(queries::status(&f.repo).await.unwrap().staged.is_empty());
}

#[tokio::test]
async fn branches_and_refs() {
    let f = Fixture::with_history();
    ops::create_branch(&f.repo, "feature/x", None, true)
        .await
        .unwrap();
    ops::create_tag(&f.repo, "v1", "HEAD", Some("release"))
        .await
        .unwrap();
    let refs = queries::refs(&f.repo).await.unwrap();
    let current: BTreeMap<_, _> = refs
        .local
        .iter()
        .map(|b| (b.name.as_str(), b.current))
        .collect();
    assert_eq!(
        current,
        BTreeMap::from([("feature/x", true), ("main", false)])
    );
    assert_eq!(refs.tags[0].name, "v1");
    assert!(refs.tags[0].annotated);
    ops::checkout(&f.repo, "main", "local").await.unwrap();
    ops::rename_branch(&f.repo, "feature/x", "feature/y")
        .await
        .unwrap();
    ops::delete_branch(&f.repo, "feature/y", false)
        .await
        .unwrap();
    ops::delete_tag(&f.repo, "v1").await.unwrap();
    let refs = queries::refs(&f.repo).await.unwrap();
    assert_eq!(refs.local.len(), 1);
    assert_eq!(refs.local[0].name, "main");
    assert!(refs.tags.is_empty());
}

#[tokio::test]
async fn merge_conflict_resolve_and_continue() {
    let f = Fixture::with_history();
    f.make_conflict();
    let result = ops::merge(&f.repo, "other", "default").await.unwrap();
    assert!(!result.ok && result.conflicts);
    assert!(matches!(result.operation, Some(Operation::Merge { .. })));
    let versions = ops::conflict_versions(&f.repo, "a.txt").await.unwrap();
    assert!(versions.ours.unwrap().ends_with("OURS\n"));
    assert!(versions.theirs.unwrap().ends_with("THEIRS\n"));
    assert!(
        versions
            .chunks
            .iter()
            .any(|chunk| matches!(chunk, ConflictChunk::Conflict { .. }))
    );
    ops::resolve_conflict(
        &f.repo,
        "a.txt",
        Some("one\ntwo\nthree\nOURS\nTHEIRS\n"),
        None,
    )
    .await
    .unwrap();
    assert!(queries::status(&f.repo).await.unwrap().conflicts.is_empty());
    assert!(ops::continue_operation(&f.repo).await.unwrap().ok);
    assert!(queries::operation_state(&f.repo).await.unwrap().is_none());
    let log = queries::log(&f.repo, 10, true, None).await.unwrap();
    assert_eq!(log.commits[0].parents.len(), 2);
}

#[tokio::test]
async fn abort_and_take_side() {
    let f = Fixture::with_history();
    f.make_conflict();
    ops::merge(&f.repo, "other", "default").await.unwrap();
    ops::abort_operation(&f.repo).await.unwrap();
    assert!(queries::operation_state(&f.repo).await.unwrap().is_none());
    ops::merge(&f.repo, "other", "default").await.unwrap();
    ops::resolve_conflict(&f.repo, "a.txt", None, Some("theirs"))
        .await
        .unwrap();
    assert!(f.read("a.txt").ends_with("THEIRS\n"));
}

#[tokio::test]
async fn marking_as_resolved_stages_the_file_as_it_is() {
    let f = Fixture::with_history();
    f.make_conflict();
    ops::merge(&f.repo, "other", "default").await.unwrap();
    f.write("a.txt", "fixed by hand\n");
    ops::resolve_conflict(&f.repo, "a.txt", None, None)
        .await
        .unwrap();
    assert!(queries::status(&f.repo).await.unwrap().conflicts.is_empty());
    assert_eq!(f.git(&["show", ":a.txt"]), "fixed by hand\n");
}

#[tokio::test]
async fn merge_error_is_a_git_error() {
    let f = Fixture::with_history();
    assert!(matches!(
        ops::merge(&f.repo, "does-not-exist", "default").await,
        Err(Error::Git(_))
    ));
}

#[tokio::test]
async fn interactive_rebase_squash_reword_reorder() {
    let f = Fixture::with_history();
    let base = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    let a = f.commit("b.txt", "b\n", "add b");
    let b = f.commit("c.txt", "c\n", "add c");
    let c = f.commit("d.txt", "d\n", "add d");
    let candidates = queries::rebase_candidates(&f.repo, Some(&base))
        .await
        .unwrap();
    let shas: Vec<_> = candidates.iter().map(|commit| commit.sha.clone()).collect();
    assert_eq!(shas, [a.clone(), b.clone(), c.clone()]);
    let step = |sha: &str, action: &str, message: Option<&str>| RebaseStep {
        sha: sha.to_owned(),
        action: action.to_owned(),
        message: message.map(str::to_owned),
    };
    let steps = [
        step(&c, "reword", Some("add d (reworded)")),
        step(&a, "pick", None),
        step(&b, "fixup", None),
    ];
    let result = ops::interactive_rebase(&f.repo, Some(&base), &steps, false)
        .await
        .unwrap();
    assert!(result.ok, "{result:?}");
    let log = queries::log(&f.repo, 10, true, None).await.unwrap();
    assert_eq!(
        subjects(&log.commits),
        ["add b", "add d (reworded)", "second", "first"]
    );
    for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
        assert!(f.exists(name), "{name}");
    }
    assert!(
        !queries::git_dir(&f.repo)
            .await
            .unwrap()
            .join("gitcraft-rebase-messages.json")
            .exists()
    );
}

#[tokio::test]
async fn rebase_with_conflict_then_continue() {
    let f = Fixture::with_history();
    f.make_conflict();
    f.git(&["switch", "-q", "other"]);
    let result = ops::rebase(&f.repo, "main", false).await.unwrap();
    assert!(result.conflicts);
    assert!(matches!(result.operation, Some(Operation::Rebase { .. })));
    ops::resolve_conflict(
        &f.repo,
        "a.txt",
        Some("one\ntwo\nthree\nOURS\nTHEIRS\n"),
        None,
    )
    .await
    .unwrap();
    assert!(ops::continue_operation(&f.repo).await.unwrap().ok);
    let log = queries::log(&f.repo, 3, false, None).await.unwrap();
    assert_eq!(subjects(&log.commits), ["theirs", "ours", "second"]);
}

#[tokio::test]
async fn rebase_autostash() {
    let f = Fixture::with_history();
    f.git(&["switch", "-q", "-c", "feature", "HEAD~1"]);
    f.commit("f.txt", "f\n", "feature work");
    f.write("f.txt", "dirty\n");
    assert!(matches!(
        ops::rebase(&f.repo, "main", false).await,
        Err(Error::Git(_))
    ));
    assert!(ops::rebase(&f.repo, "main", true).await.unwrap().ok);
    let log = queries::log(&f.repo, 3, false, None).await.unwrap();
    assert_eq!(subjects(&log.commits), ["feature work", "second", "first"]);
    assert_eq!(f.read("f.txt"), "dirty\n");
}

#[tokio::test]
async fn stash_cycle() {
    let f = Fixture::with_history();
    f.write("a.txt", "changed\n");
    f.write("u.txt", "untracked\n");
    ops::stash_push(&f.repo, Some("wip"), true, false, false, None)
        .await
        .unwrap();
    assert!(
        queries::refs(&f.repo).await.unwrap().stashes[0]
            .message
            .ends_with("wip")
    );
    let files = queries::stash_files(&f.repo, "stash@{0}").await.unwrap();
    assert_eq!(paths(&files), ["a.txt", "u.txt"]);
    assert!(
        !queries::stash_diff(&f.repo, "stash@{0}", "u.txt", 3)
            .await
            .unwrap()
            .unwrap()
            .hunks
            .is_empty()
    );
    assert!(queries::status(&f.repo).await.unwrap().unstaged.is_empty());
    ops::stash_apply(&f.repo, "stash@{0}", true).await.unwrap();
    assert!(f.exists("u.txt"));
    assert!(queries::refs(&f.repo).await.unwrap().stashes.is_empty());
}

#[tokio::test]
async fn checkout_with_changes_stashes_and_reapplies_them() {
    let f = Fixture::with_history();
    f.git(&["branch", "side"]);
    f.write("a.txt", "one\ntwo\nthree\nfour\nmine\n");
    f.write("staged.txt", "new\n");
    f.git(&["add", "staged.txt"]);
    let result = ops::checkout_stashing(&f.repo, "side", "local", true)
        .await
        .unwrap();
    assert!(result.ok);
    assert_eq!(f.git(&["branch", "--show-current"]).trim(), "side");
    assert!(f.read("a.txt").ends_with("mine\n"), "the change came along");
    let status = queries::status(&f.repo).await.unwrap();
    assert_eq!(paths(&status.staged), ["staged.txt"], "and stays staged");
    assert!(queries::refs(&f.repo).await.unwrap().stashes.is_empty());
}

#[tokio::test]
async fn checkout_with_changes_that_conflict_keeps_the_stash() {
    let f = Fixture::with_history();
    f.git(&["switch", "-q", "-c", "side"]);
    f.commit("a.txt", "side\n", "side changes a");
    f.git(&["switch", "-q", "main"]);
    f.write("a.txt", "mine\n");
    let result = ops::checkout_stashing(&f.repo, "side", "local", true)
        .await
        .unwrap();
    assert!(!result.ok && result.conflicts, "reported as conflicts");
    assert_eq!(f.git(&["branch", "--show-current"]).trim(), "side");
    assert_eq!(
        queries::refs(&f.repo).await.unwrap().stashes.len(),
        1,
        "the changes are still in the stash"
    );
}

#[tokio::test]
async fn checkout_discarding_changes_keeps_them_in_a_stash() {
    let f = Fixture::with_history();
    f.git(&["branch", "side"]);
    f.write("a.txt", "mine\n");
    let result = ops::checkout_stashing(&f.repo, "side", "local", false)
        .await
        .unwrap();
    assert!(result.ok);
    assert_eq!(f.git(&["branch", "--show-current"]).trim(), "side");
    assert_eq!(f.read("a.txt"), "one\ntwo\nthree\nfour\n", "a clean tree");
    let stashes = queries::refs(&f.repo).await.unwrap().stashes;
    assert_eq!(stashes.len(), 1);
    assert!(stashes[0].message.contains("before checking out side"));
}

/// A post-checkout hook that fails, so that git reports an error after it has switched.
fn failing_post_checkout(f: &Fixture) {
    // A global core.hooksPath would send git elsewhere: use this repository's hooks.
    let hooks = f.path.join(".git/hooks");
    f.git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
    let hook = hooks.join("post-checkout");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[tokio::test]
async fn checkout_whose_hook_fails_after_switching_keeps_the_stash() {
    let f = Fixture::with_history();
    f.git(&["branch", "side"]);
    failing_post_checkout(&f);
    f.write("a.txt", "mine\n");
    let result = ops::checkout_stashing(&f.repo, "side", "local", true).await;
    assert!(result.is_err(), "the hook's failure is reported");
    assert_eq!(f.git(&["branch", "--show-current"]).trim(), "side");
    assert_eq!(
        queries::refs(&f.repo).await.unwrap().stashes.len(),
        1,
        "the changes stay in the stash instead of landing on side"
    );
}

#[tokio::test]
async fn a_detached_checkout_whose_hook_fails_keeps_the_stash_too() {
    let f = Fixture::with_history();
    let first = f.git(&["rev-parse", "HEAD~1"]).trim().to_owned();
    let second = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    f.git(&["switch", "-q", "--detach", &first]);
    failing_post_checkout(&f);
    f.write("a.txt", "mine\n");
    let result = ops::checkout_stashing(&f.repo, &second, "commit", true).await;
    assert!(result.is_err());
    assert_eq!(f.git(&["rev-parse", "HEAD"]).trim(), second, "it did move");
    assert_eq!(
        queries::refs(&f.repo).await.unwrap().stashes.len(),
        1,
        "so the changes stay in the stash"
    );
}

#[tokio::test]
async fn checkout_that_fails_puts_the_changes_back() {
    let f = Fixture::with_history();
    f.write("a.txt", "mine\n");
    assert!(
        ops::checkout_stashing(&f.repo, "no-such-branch", "local", true)
            .await
            .is_err()
    );
    assert_eq!(f.read("a.txt"), "mine\n");
    assert!(queries::refs(&f.repo).await.unwrap().stashes.is_empty());
}

/// A "diff tool" that copies the two versions it is given beside the repository.
fn fake_diff_tool(f: &Fixture) {
    let root = f.root.to_str().unwrap().replace('\\', "/");
    let cmd = format!("cp \"$LOCAL\" \"{root}/left\" && cp \"$REMOTE\" \"{root}/right\"");
    f.git(&["config", "difftool.fake.cmd", &cmd]);
}

fn sides(f: &Fixture) -> (String, String) {
    let read = |name: &str| std::fs::read_to_string(f.root.join(name)).unwrap_or_default();
    (read("left"), read("right"))
}

#[tokio::test]
async fn external_diff_tools_are_listed_and_shown_each_version() {
    let f = Fixture::with_history();
    fake_diff_tool(&f);
    let tools = queries::external_tools(&f.repo, false).await.unwrap();
    assert!(tools.iter().any(|tool| tool.name == "fake"), "{tools:?}");

    f.write("a.txt", "one\ntwo\nthree\nfour\nstaged\n");
    f.git(&["add", "a.txt"]);
    f.write("a.txt", "one\ntwo\nthree\nfour\nstaged\nworking\n");
    let open = |target: ToolTarget| {
        let repo = f.repo.clone();
        async move {
            ops::open_diff_tool(&repo, "fake", "a.txt", None, &target)
                .await
                .unwrap()
        }
    };
    open(ToolTarget::Working { staged: false }).await;
    assert_eq!(
        sides(&f),
        (
            "one\ntwo\nthree\nfour\nstaged\n".into(),
            "one\ntwo\nthree\nfour\nstaged\nworking\n".into()
        ),
        "the index against the working tree"
    );
    open(ToolTarget::Working { staged: true }).await;
    assert_eq!(
        sides(&f),
        (
            "one\ntwo\nthree\nfour\n".into(),
            "one\ntwo\nthree\nfour\nstaged\n".into()
        ),
        "HEAD against the index"
    );
    let second = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    open(ToolTarget::Commit { sha: second }).await;
    assert_eq!(
        sides(&f),
        ("one\ntwo\nthree\n".into(), "one\ntwo\nthree\nfour\n".into()),
        "a commit against its parent"
    );
    let first = f.git(&["rev-parse", "HEAD~1"]).trim().to_owned();
    open(ToolTarget::Commit { sha: first }).await;
    assert_eq!(sides(&f).1, "one\ntwo\nthree\n", "a first commit too");
    assert!(matches!(
        ops::open_diff_tool(
            &f.repo,
            "--extcmd=sh",
            "a.txt",
            None,
            &ToolTarget::Working { staged: false }
        )
        .await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn an_external_merge_tool_resolves_the_conflict() {
    let f = Fixture::with_history();
    f.git(&["config", "mergetool.fake.cmd", "cp \"$REMOTE\" \"$MERGED\""]);
    f.git(&["config", "mergetool.fake.trustExitCode", "true"]);
    let tools = queries::external_tools(&f.repo, true).await.unwrap();
    assert!(tools.iter().any(|tool| tool.name == "fake"), "{tools:?}");
    f.make_conflict();
    ops::merge(&f.repo, "other", "default").await.unwrap();
    ops::open_merge_tool(&f.repo, "fake", "a.txt")
        .await
        .unwrap();
    assert!(f.read("a.txt").ends_with("THEIRS\n"));
    assert!(
        queries::status(&f.repo).await.unwrap().conflicts.is_empty(),
        "and marked as resolved"
    );
    assert!(!f.exists("a.txt.orig"), "without a backup copy left behind");
}

#[tokio::test]
async fn cherry_pick_revert_reset() {
    let f = Fixture::with_history();
    f.git(&["switch", "-q", "-c", "side"]);
    let picked = f.commit("s.txt", "s\n", "side commit");
    f.git(&["switch", "-q", "main"]);
    assert!(ops::cherry_pick(&f.repo, &[picked]).await.unwrap().ok);
    assert!(f.exists("s.txt"));
    let head = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    assert!(ops::revert(&f.repo, &head, None).await.unwrap().ok);
    assert!(!f.exists("s.txt"));
    ops::reset(&f.repo, "HEAD~2", "hard").await.unwrap();
    assert_eq!(
        queries::log(&f.repo, 10, false, None)
            .await
            .unwrap()
            .commits[0]
            .subject,
        "second"
    );
}

#[tokio::test]
async fn history_search_blame() {
    let f = Fixture::with_history();
    f.git(&["mv", "a.txt", "renamed.txt"]);
    f.git(&["commit", "-q", "-m", "rename"]);
    let history = queries::file_history(&f.repo, "renamed.txt", 1000)
        .await
        .unwrap();
    assert_eq!(subjects(&history), ["rename", "second", "first"]);
    assert_eq!(history[0].file.as_ref().unwrap().status, "R");
    assert_eq!(history.last().unwrap().file.as_ref().unwrap().path, "a.txt");
    assert_eq!(
        subjects(
            &queries::search(&f.repo, "SEC", "message", 500)
                .await
                .unwrap()
        ),
        ["second"]
    );
    assert_eq!(
        subjects(
            &queries::search(&f.repo, "four", "content", 500)
                .await
                .unwrap()
        ),
        ["second"]
    );
    assert_eq!(
        queries::search(&f.repo, "test user", "author", 500)
            .await
            .unwrap()
            .len(),
        3
    );
    let sha = f.git(&["rev-parse", "HEAD~1"]).trim().to_owned();
    assert_eq!(
        queries::search(&f.repo, &sha[..8], "sha", 500)
            .await
            .unwrap()[0]
            .sha,
        sha
    );
    let blame = queries::blame(&f.repo, "renamed.txt", None).await.unwrap();
    let summaries: Vec<_> = blame.iter().map(|line| line.summary.as_str()).collect();
    assert_eq!(summaries, ["first", "first", "first", "second"]);
    assert_eq!(blame[3].text, "four");
}

#[tokio::test]
async fn remote_fetch_pull_push() {
    let f = Fixture::with_history();
    let remote = f.root.join("remote.git");
    raw_git(
        &f.root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    ops::add_remote(&f.repo, "origin", remote.to_str().unwrap())
        .await
        .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let collect: Progress = Arc::new(move |line| sink.lock().unwrap().push(line));
    let report = Some(&collect);

    assert!(
        ops::push(&f.repo, "origin", Some("main"), true, false, false, report)
            .await
            .unwrap()
            .ok
    );
    let refs = queries::refs(&f.repo).await.unwrap();
    assert_eq!(refs.remote[0].name, "origin/main");
    assert_eq!(refs.local[0].upstream.as_deref(), Some("origin/main"));
    assert!(!seen.lock().unwrap().is_empty(), "push reports progress");

    let clone_dir = PathBuf::from(
        ops::clone(
            f.root.to_str().unwrap(),
            remote.to_str().unwrap(),
            Some("clone"),
            report,
        )
        .await
        .unwrap(),
    );
    f.identity(&clone_dir);
    commit_at(&clone_dir, "x.txt", "x\n", "from clone");
    raw_git(&clone_dir, &["push", "-q"]);

    ops::fetch(&f.repo, None, true, false, report)
        .await
        .unwrap();
    assert_eq!(queries::refs(&f.repo).await.unwrap().local[0].behind, 1);
    assert!(
        ops::pull(&f.repo, None, None, "ff-only", false, report)
            .await
            .unwrap()
            .ok
    );
    assert!(f.exists("x.txt"));

    // Diverged history: pull --rebase replays the local commit on top of the remote one,
    // and autostash keeps uncommitted changes.
    commit_at(&clone_dir, "y.txt", "y\n", "remote work");
    raw_git(&clone_dir, &["push", "-q"]);
    f.commit("z.txt", "z\n", "local work");
    f.write("a.txt", "dirty\n");
    assert!(
        ops::pull(
            &f.repo,
            Some("origin"),
            Some("main"),
            "rebase",
            true,
            report
        )
        .await
        .unwrap()
        .ok
    );
    let log = queries::log(&f.repo, 10, true, None).await.unwrap();
    assert_eq!(subjects(&log.commits[..2]), ["local work", "remote work"]);
    assert!(
        log.commits[..log.commits.len() - 1]
            .iter()
            .all(|commit| commit.parents.len() == 1)
    );
    assert_eq!(f.read("a.txt"), "dirty\n");
}

#[tokio::test]
async fn checkout_file_at_revision() {
    let f = Fixture::with_history();
    let first = f.git(&["rev-parse", "HEAD~1"]).trim().to_owned();
    ops::checkout_file(&f.repo, &first, "a.txt").await.unwrap();
    assert_eq!(f.read("a.txt"), "one\ntwo\nthree\n");
    assert_eq!(
        queries::status(&f.repo).await.unwrap().staged[0].path,
        "a.txt"
    );
}

#[tokio::test]
async fn fast_forward_without_checkout() {
    let f = Fixture::with_history();
    f.git(&["branch", "behind", "HEAD~1"]);
    f.git(&["branch", "--set-upstream-to=main", "behind"]);
    ops::fast_forward(&f.repo, "behind", None).await.unwrap();
    assert_eq!(
        f.git(&["rev-parse", "behind"]),
        f.git(&["rev-parse", "main"])
    );
    // Diverged branches are refused.
    f.git(&["switch", "-q", "-c", "side", "HEAD~1"]);
    f.commit("s.txt", "s\n", "side");
    f.git(&["switch", "-q", "main"]);
    assert!(matches!(
        ops::fast_forward(&f.repo, "side", Some("main")).await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn fast_forward_current_branch() {
    let f = Fixture::with_history();
    f.git(&["switch", "-q", "-c", "old", "HEAD~1"]);
    ops::fast_forward(&f.repo, "old", Some("main"))
        .await
        .unwrap();
    assert_eq!(f.git(&["rev-parse", "HEAD"]), f.git(&["rev-parse", "main"]));
    assert!(f.read("a.txt").ends_with("four\n"));
}

#[tokio::test]
async fn gitignore() {
    let f = Fixture::with_history();
    f.write("build.log", "x");
    f.write(".gitignore", "node_modules/");
    ops::add_to_gitignore(&f.repo, "*.log").await.unwrap();
    ops::add_to_gitignore(&f.repo, "*.log").await.unwrap();
    assert_eq!(f.read(".gitignore"), "node_modules/\n*.log\n");
    assert_eq!(
        paths(&queries::status(&f.repo).await.unwrap().unstaged),
        [".gitignore"]
    );
    // Ignoring a tracked file also removes it from the index.
    ops::add_to_gitignore(&f.repo, "a.txt").await.unwrap();
    let staged = queries::status(&f.repo).await.unwrap().staged;
    assert!(staged.contains(&FileChange {
        path: "a.txt".into(),
        status: "D".into(),
        orig_path: None
    }));
    assert!(matches!(
        ops::add_to_gitignore(&f.repo, "a\nb").await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn clean() {
    let f = Fixture::with_history();
    f.write("junk.txt", "x");
    f.write("tmp/inner.txt", "x");
    let preview = ops::clean(&f.repo, true, false, true).await.unwrap();
    let mut files = preview.files.unwrap();
    files.sort();
    assert_eq!(files, ["junk.txt", "tmp/"]);
    assert!(f.exists("junk.txt"));
    ops::clean(&f.repo, true, false, false).await.unwrap();
    assert!(!f.exists("junk.txt") && !f.exists("tmp"));
}

#[tokio::test]
async fn patch_roundtrip() {
    let f = Fixture::with_history();
    let patch = ops::format_patch(&f.repo, "HEAD").await.unwrap();
    assert!(patch.contains("Subject: [PATCH] second"));
    f.git(&["reset", "-q", "--hard", "HEAD~1"]);
    assert!(ops::apply_patch(&f.repo, &patch, "am").await.unwrap().ok);
    assert_eq!(
        queries::log(&f.repo, 1, false, None).await.unwrap().commits[0].subject,
        "second"
    );
    f.git(&["reset", "-q", "--hard", "HEAD~1"]);
    assert!(ops::apply_patch(&f.repo, &patch, "apply").await.unwrap().ok);
    assert_eq!(
        queries::status(&f.repo).await.unwrap().staged[0].path,
        "a.txt"
    ); // --3way updates the index
}

#[tokio::test]
async fn worktree_add_list_remove() {
    let f = Fixture::with_history();
    let path = f.root.join("wt");
    let path_str = path.to_str().unwrap();
    ops::worktree_add(&f.repo, path_str, None, Some("feature"))
        .await
        .unwrap();
    let refs = queries::refs(&f.repo).await.unwrap();
    assert_eq!(refs.worktrees.len(), 2);
    assert!(refs.worktrees[0].main && refs.worktrees[0].current);
    assert_eq!(refs.worktrees[1].branch.as_deref(), Some("feature"));
    assert!(!refs.worktrees[1].current);
    // Seen from inside the linked worktree, that one is the current one.
    let linked = queries::refs(path_str).await.unwrap();
    assert!(linked.worktrees[1].current && !linked.worktrees[0].current);
    // A branch can only be checked out once.
    assert!(
        ops::worktree_add(
            &f.repo,
            f.root.join("dup").to_str().unwrap(),
            Some("feature"),
            None
        )
        .await
        .is_err()
    );
    ops::worktree_remove(&f.repo, path_str, false)
        .await
        .unwrap();
    assert_eq!(queries::refs(&f.repo).await.unwrap().worktrees.len(), 1);
}

#[tokio::test]
async fn stash_selected_paths() {
    let f = Fixture::with_history();
    f.write("a.txt", "changed\n");
    f.commit("b.txt", "b\n", "add b");
    f.write("b.txt", "changed b\n");
    ops::stash_push(
        &f.repo,
        Some("only a"),
        false,
        false,
        false,
        Some(&["a.txt".to_owned()]),
    )
    .await
    .unwrap();
    assert_eq!(
        paths(&queries::status(&f.repo).await.unwrap().unstaged),
        ["b.txt"]
    );
    assert_eq!(
        paths(&queries::stash_files(&f.repo, "stash@{0}").await.unwrap()),
        ["a.txt"]
    );
}

#[tokio::test]
async fn reflog_and_compare() {
    let f = Fixture::with_history();
    f.git(&["reset", "-q", "--hard", "HEAD~1"]);
    let entries = queries::reflog(&f.repo, "HEAD", 300).await.unwrap();
    assert_eq!(entries[0].selector, "HEAD@{0}");
    assert!(entries[0].action.starts_with("reset"));
    assert_eq!(entries[1].subject, "second");
    let result = queries::compare(&f.repo, "HEAD", &entries[1].sha)
        .await
        .unwrap();
    assert_eq!(
        result.files,
        [FileChange {
            status: "M".into(),
            orig_path: None,
            path: "a.txt".into()
        }]
    );
    assert_eq!((result.base_only, result.target_only), (0, 1));
}

// ------------------------------------------------- deliberate behaviour changes

#[tokio::test]
async fn partial_staging_keeps_non_utf8_bytes_exact() {
    let f = Fixture::empty();
    f.commit(
        "l.txt",
        b"Gr\xfc\xdfe\nzwei\ndrei\nvier\nf\xfcnf\nsechs\nsieben\nacht\nK\xf6ln\n".as_slice(),
        "latin1",
    );
    f.write(
        "l.txt",
        b"Gr\xfc\xdfe\nZWEI\ndrei\nvier\nf\xfcnf\nsechs\nsieben\nacht\nK\xd6LN\n".as_slice(),
    );
    let diff = queries::working_diff(&f.repo, "l.txt", false, 1, false, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(diff.hunks.len(), 2);
    ops::apply_selection(&f.repo, "l.txt", "stage", &whole(1), 1)
        .await
        .unwrap();
    let index = raw_git(&f.path, &["show", ":l.txt"]);
    assert_eq!(
        index,
        b"Gr\xfc\xdfe\nzwei\ndrei\nvier\nf\xfcnf\nsechs\nsieben\nacht\nK\xd6LN\n"
    );
}

#[tokio::test]
async fn partial_staging_works_at_context_zero() {
    let f = Fixture::with_history();
    f.write("a.txt", "one\nTWO\nTWO-B\nthree\n");
    let diff = queries::working_diff(&f.repo, "a.txt", false, 0, false, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(diff.hunks.len(), 2);
    ops::apply_selection(&f.repo, "a.txt", "stage", &whole(0), 0)
        .await
        .unwrap();
    assert_eq!(f.git(&["show", ":a.txt"]), "one\nTWO\nTWO-B\nthree\nfour\n");
    assert_eq!(f.read("a.txt"), "one\nTWO\nTWO-B\nthree\n");
}

#[tokio::test]
async fn values_starting_with_a_dash_are_refused_not_run() {
    let f = Fixture::with_history();
    let head = f.git(&["rev-parse", "HEAD"]);
    assert!(matches!(
        ops::checkout(&f.repo, "--detach", "local").await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        ops::merge(&f.repo, "--abort", "default").await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        ops::reset(&f.repo, "--hard", "mixed").await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        ops::create_branch(&f.repo, "-D", None, false).await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        queries::commit_detail(&f.repo, "--all").await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(f.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
    assert_eq!(f.git(&["rev-parse", "HEAD"]), head);
}

#[tokio::test]
async fn clean_reports_names_with_spaces_intact() {
    let f = Fixture::with_history();
    f.write("two words.txt", "x");
    let real = ops::clean(&f.repo, true, false, false).await.unwrap();
    assert_eq!(real.files.unwrap(), ["two words.txt"]);
}

#[tokio::test]
async fn sha256_repositories_work() {
    let f = Fixture::empty();
    let path = f.root.join("sha256");
    raw_git(
        &f.root,
        &[
            "init",
            "-q",
            "--object-format=sha256",
            "-b",
            "main",
            path.to_str().unwrap(),
        ],
    );
    f.identity(&path);
    commit_at(&path, "a.txt", "one\n", "root");
    let repo = path.to_str().unwrap();
    let log = queries::log(repo, 10, true, None).await.unwrap();
    assert_eq!(log.commits[0].sha.len(), 64);
    let detail = queries::commit_detail(repo, &log.commits[0].sha)
        .await
        .unwrap();
    assert_eq!(paths(&detail.files), ["a.txt"]);
    let diff = queries::commit_diff(repo, &log.commits[0].sha, "a.txt", None, 3, false)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(diff.status, "A");
}

#[tokio::test]
async fn multi_commit_pick_is_seen_between_steps() {
    let f = Fixture::with_history();
    f.git(&["switch", "-q", "-c", "side"]);
    let one = f.commit("a.txt", "side\n", "one");
    let two = f.commit("b.txt", "b\n", "two");
    f.git(&["switch", "-q", "main"]);
    f.commit("a.txt", "main\n", "conflicting");
    assert!(!ops::cherry_pick(&f.repo, &[one, two]).await.unwrap().ok);
    // Resolving and committing by hand clears CHERRY_PICK_HEAD but not the remaining steps.
    f.write("a.txt", "resolved\n");
    f.git(&["add", "a.txt"]);
    f.git(&["commit", "-q", "--no-edit"]);
    assert!(
        !queries::git_dir(&f.repo)
            .await
            .unwrap()
            .join("CHERRY_PICK_HEAD")
            .exists()
    );
    let state = queries::operation_state(&f.repo).await.unwrap();
    assert!(
        matches!(state, Some(Operation::CherryPick { .. })),
        "{state:?}"
    );
    assert!(ops::continue_operation(&f.repo).await.unwrap().ok);
    assert!(queries::operation_state(&f.repo).await.unwrap().is_none());
    assert!(f.exists("b.txt"));
}

#[tokio::test]
async fn remote_passwords_are_masked_and_cannot_be_saved_back() {
    let f = Fixture::with_history();
    f.git(&[
        "remote",
        "add",
        "up",
        "https://me:s3cret@example.invalid/x.git",
    ]);
    let refs = queries::refs(&f.repo).await.unwrap();
    assert_eq!(
        refs.remotes[0].fetch_url,
        "https://me:***@example.invalid/x.git"
    );
    let masked = refs.remotes[0].fetch_url.clone();
    assert!(matches!(
        ops::edit_remote(&f.repo, "up", None, Some(&masked)).await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(
        f.git(&["remote", "get-url", "up"]).trim(),
        "https://me:s3cret@example.invalid/x.git"
    );
    ops::edit_remote(
        &f.repo,
        "up",
        None,
        Some("https://me:new@example.invalid/x.git"),
    )
    .await
    .unwrap();
    assert_eq!(
        f.git(&["remote", "get-url", "up"]).trim(),
        "https://me:new@example.invalid/x.git"
    );
}

#[tokio::test]
async fn editing_a_non_utf8_conflict_is_refused_rather_than_damaging_it() {
    let f = Fixture::with_history();
    f.git(&["switch", "-q", "-c", "other"]);
    f.commit("l.txt", b"K\xf6ln theirs\n".as_slice(), "theirs");
    f.git(&["switch", "-q", "main"]);
    f.commit("l.txt", b"K\xf6ln ours\n".as_slice(), "ours");
    ops::merge(&f.repo, "other", "default").await.unwrap();
    let edit = ops::resolve_conflict(&f.repo, "l.txt", Some("Köln merged\n"), None).await;
    assert!(matches!(edit, Err(Error::Invalid(_))));
    ops::resolve_conflict(&f.repo, "l.txt", None, Some("theirs"))
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(f.path.join("l.txt")).unwrap(),
        b"K\xf6ln theirs\n"
    );
}

#[tokio::test]
async fn subjects_with_control_characters_keep_their_branch_and_reflog_entry() {
    let f = Fixture::with_history();
    f.commit("b.txt", "b\n", "subject with \x0c form feed");
    let refs = queries::refs(&f.repo).await.unwrap();
    assert_eq!(refs.local[0].name, "main");
    let reflog = queries::reflog(&f.repo, "HEAD", 10).await.unwrap();
    assert_eq!(reflog[0].subject, "subject with \x0c form feed");
    assert_eq!(reflog.len(), 3);
}

#[tokio::test]
async fn commit_graph_is_written_once_and_changes_nothing_visible() {
    let f = Fixture::with_history();
    let before = queries::log(&f.repo, 10, true, None).await.unwrap();
    assert!(
        ops::ensure_commit_graph(&f.repo).await.unwrap(),
        "written the first time"
    );
    assert!(
        queries::common_dir(&f.repo)
            .await
            .unwrap()
            .join("objects/info/commit-graph")
            .exists()
    );
    assert!(
        !ops::ensure_commit_graph(&f.repo).await.unwrap(),
        "and left alone after that"
    );
    assert_eq!(queries::log(&f.repo, 10, true, None).await.unwrap(), before);
    // A repository without commits has nothing to index.
    let empty = Fixture::empty();
    assert!(!ops::ensure_commit_graph(&empty.repo).await.unwrap());
}

#[tokio::test]
async fn dropping_a_running_command_kills_it() {
    let f = Fixture::with_history();
    let started = std::time::Instant::now();
    let slow =
        gitcraft_core::runner::git_unchecked(&f.repo, &["-c", "alias.slow=!sleep 30", "slow"]);
    assert!(
        tokio::time::timeout(Duration::from_millis(200), slow)
            .await
            .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

// ------------------------------------- line endings, as on a typical Windows setup

fn crlf_fixture() -> Fixture {
    let f = Fixture::empty();
    f.git(&["config", "core.autocrlf", "true"]);
    f.commit("a.txt", "one\ntwo\nthree\nfour\n", "first");
    f.git(&["rm", "-q", "--cached", "a.txt"]);
    std::fs::remove_file(f.path.join("a.txt")).unwrap();
    f.git(&["checkout", "-q", "HEAD", "--", "a.txt"]);
    assert_eq!(
        f.read("a.txt"),
        "one\r\ntwo\r\nthree\r\nfour\r\n",
        "checked out with CRLF"
    );
    f
}

#[tokio::test]
async fn staging_a_line_works_with_crlf_checkouts() {
    let f = crlf_fixture();
    f.write("a.txt", "one\r\nTWO\r\nthree\r\nfour\r\nfive\r\n");
    let diff = queries::working_diff(&f.repo, "a.txt", false, 3, false, None)
        .await
        .unwrap()
        .unwrap();
    let five = line_index(&diff, 0, |l| l.text.trim_end() == "five");
    let selection = BTreeMap::from([(0, lines_of(&[five]))]);
    ops::apply_selection(&f.repo, "a.txt", "stage", &selection, 3)
        .await
        .unwrap();
    assert_eq!(
        f.git(&["show", ":a.txt"]),
        "one\ntwo\nthree\nfour\nfive\n",
        "the index stays LF"
    );
    assert_eq!(
        f.read("a.txt"),
        "one\r\nTWO\r\nthree\r\nfour\r\nfive\r\n",
        "the file isn't touched"
    );
}

#[tokio::test]
async fn discarding_a_line_keeps_crlf_line_endings() {
    let f = crlf_fixture();
    f.write(
        "a.txt",
        "one\r\ntwo\r\nthree\r\nfour\r\nextra1\r\nextra2\r\n",
    );
    let diff = queries::working_diff(&f.repo, "a.txt", false, 3, false, None)
        .await
        .unwrap()
        .unwrap();
    let first = line_index(&diff, 0, |l| l.text.trim_end() == "extra1");
    let selection = BTreeMap::from([(0, lines_of(&[first]))]);
    ops::apply_selection(&f.repo, "a.txt", "discard", &selection, 3)
        .await
        .unwrap();
    assert_eq!(f.read("a.txt"), "one\r\ntwo\r\nthree\r\nfour\r\nextra2\r\n");
    let name = queries::discard_backups(&f.repo).await.unwrap()[0]
        .name
        .clone();
    ops::restore_discard(&f.repo, &name).await.unwrap();
    assert_eq!(
        f.read("a.txt"),
        "one\r\ntwo\r\nthree\r\nfour\r\nextra1\r\nextra2\r\n"
    );
}

#[tokio::test]
async fn conflict_markers_are_found_in_crlf_files() {
    let f = Fixture::with_history();
    f.make_conflict();
    f.git(&["config", "core.autocrlf", "true"]);
    assert!(!ops::merge(&f.repo, "other", "default").await.unwrap().ok);
    let versions = ops::conflict_versions(&f.repo, "a.txt").await.unwrap();
    assert!(
        versions
            .chunks
            .iter()
            .any(|chunk| matches!(chunk, ConflictChunk::Conflict { .. }))
    );
    ops::resolve_conflict(&f.repo, "a.txt", Some("one\ntwo\nthree\nmerged\n"), None)
        .await
        .unwrap();
    assert_eq!(f.git(&["show", ":a.txt"]), "one\ntwo\nthree\nmerged\n");
    assert!(ops::continue_operation(&f.repo).await.unwrap().ok);
}

// --------------------------------------------------------------------- watcher

async fn next_change(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<WatchEvent>,
) -> Option<WatchEvent> {
    tokio::time::timeout(Duration::from_secs(10), events.recv())
        .await
        .ok()
        .flatten()
}

async fn watching(
    repo: &str,
) -> (
    watcher::Watcher,
    tokio::sync::mpsc::UnboundedReceiver<WatchEvent>,
) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = watcher::watch(repo, move |event| {
        let _ = sender.send(event);
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    (handle, receiver)
}

#[tokio::test]
async fn watcher_reports_worktree_and_git_changes() {
    let f = Fixture::with_history();
    let (_handle, mut events) = watching(&f.repo).await;
    f.write("watched.txt", "x\n");
    assert_eq!(
        next_change(&mut events).await,
        Some(WatchEvent::Changed {
            git: false,
            worktree: true
        })
    );
    tokio::time::sleep(Duration::from_millis(600)).await;
    while events.try_recv().is_ok() {}
    f.git(&["branch", "fresh"]);
    let event = next_change(&mut events).await;
    assert!(
        matches!(event, Some(WatchEvent::Changed { git: true, .. })),
        "{event:?}"
    );
}

#[tokio::test]
async fn watcher_reports_lock_files_in_the_worktree() {
    let f = Fixture::with_history();
    let (_handle, mut events) = watching(&f.repo).await;
    f.write("Cargo.lock", "x\n");
    assert_eq!(
        next_change(&mut events).await,
        Some(WatchEvent::Changed {
            git: false,
            worktree: true
        })
    );
}

#[tokio::test]
async fn watcher_sees_git_changes_of_a_linked_worktree() {
    let f = Fixture::with_history();
    let linked = f.root.join("linked");
    f.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "wt",
        linked.to_str().unwrap(),
    ]);
    let (_handle, mut events) = watching(linked.to_str().unwrap()).await;
    raw_git(&linked, &["branch", "from-worktree"]);
    let event = next_change(&mut events).await;
    assert!(
        matches!(event, Some(WatchEvent::Changed { git: true, .. })),
        "{event:?}"
    );
}

#[tokio::test]
async fn pull_overwrite_replaces_the_branch_and_can_keep_a_backup() {
    let f = Fixture::with_history();
    let remote = f.root.join("overwrite-remote.git");
    raw_git(
        &f.root,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    ops::add_remote(&f.repo, "origin", remote.to_str().unwrap())
        .await
        .unwrap();
    ops::push(&f.repo, "origin", Some("main"), true, false, false, None)
        .await
        .unwrap();
    let clone_dir = PathBuf::from(
        ops::clone(
            f.root.to_str().unwrap(),
            remote.to_str().unwrap(),
            Some("other"),
            None,
        )
        .await
        .unwrap(),
    );
    f.identity(&clone_dir);
    let remote_tip = commit_at(&clone_dir, "r.txt", "remote\n", "remote work");
    raw_git(&clone_dir, &["push", "-q"]);

    // Diverged: a local commit, and an uncommitted change on top.
    let local_tip = f.commit("l.txt", "local\n", "local work");
    f.write("a.txt", "dirty\n");
    let result = ops::pull_overwrite(&f.repo, Some("origin"), Some("main"), true, None)
        .await
        .unwrap();
    assert!(result.ok);
    assert_eq!(f.git(&["rev-parse", "HEAD"]).trim(), remote_tip);
    assert!(
        !f.exists("l.txt"),
        "the local commit is gone from the branch"
    );
    assert_ne!(
        f.read("a.txt"),
        "dirty\n",
        "and so is the uncommitted change"
    );
    let backup = format!("backup/main-{}", &local_tip[..7]);
    assert!(result.message.contains(&backup), "{}", result.message);
    assert_eq!(f.git(&["rev-parse", &backup]).trim(), local_tip);

    // Without a backup, and from the upstream when nothing is named.
    let local_again = f.commit("m.txt", "more\n", "more local work");
    let result = ops::pull_overwrite(&f.repo, None, None, false, None)
        .await
        .unwrap();
    assert!(result.ok);
    assert_eq!(f.git(&["rev-parse", "HEAD"]).trim(), remote_tip);
    let branches = f.git(&["branch", "--list", "backup/*"]);
    assert!(!branches.contains(&local_again[..7]), "no backup was made");

    // A detached HEAD has no branch to overwrite.
    f.git(&["switch", "-q", "--detach"]);
    assert!(matches!(
        ops::pull_overwrite(&f.repo, Some("origin"), Some("main"), false, None).await,
        Err(Error::Invalid(_))
    ));
}

// ------------------------------------------------------------------ undo / redo

/// Runs `op` the way the interface does: state before, the operation, then the record.
macro_rules! undoable {
    ($f:expr, $label:expr, $mode:expr, $op:expr) => {{
        let before = undo::capture(&$f.repo).await.unwrap();
        let result = $op;
        undo::record(&$f.repo, $label, $mode, before).await.unwrap();
        result
    }};
}

fn head_of(f: &Fixture) -> String {
    f.git(&["rev-parse", "HEAD"]).trim().to_owned()
}

#[tokio::test]
async fn a_commit_is_undone_with_its_changes_staged_and_redone() {
    let f = Fixture::with_history();
    let before = head_of(&f);
    f.write("a.txt", "changed\n");
    f.git(&["add", "a.txt"]);
    undoable!(
        f,
        "Commit",
        undo::Mode::Soft,
        ops::commit(&f.repo, "third", false, false).await.unwrap()
    );
    let third = head_of(&f);
    assert_eq!(
        undo::labels(&f.repo).await.unwrap().undo.as_deref(),
        Some("Commit")
    );

    assert_eq!(ops::undo(&f.repo).await.unwrap(), "Commit");
    assert_eq!(head_of(&f), before);
    assert_eq!(f.git(&["diff", "--cached", "--name-only"]), "a.txt\n");
    let labels = undo::labels(&f.repo).await.unwrap();
    assert_eq!(
        (labels.undo, labels.redo.as_deref()),
        (None, Some("Commit"))
    );

    ops::redo(&f.repo).await.unwrap();
    assert_eq!(head_of(&f), third);
    assert!(f.git(&["diff", "--cached", "--name-only"]).is_empty());
    // The commits it needs are pinned, and hidden from the history.
    assert!(
        f.git(&["for-each-ref", "refs/gitcraft/undo/"])
            .contains(&third)
    );
    let page = queries::log(&f.repo, 100, true, None).await.unwrap();
    assert_eq!(subjects(&page.commits), ["third", "second", "first"]);
}

#[tokio::test]
async fn undo_refuses_once_the_branch_moved_on() {
    let f = Fixture::with_history();
    f.write("a.txt", "changed\n");
    f.git(&["add", "a.txt"]);
    undoable!(
        f,
        "Commit",
        undo::Mode::Soft,
        ops::commit(&f.repo, "third", false, false).await.unwrap()
    );
    f.commit("b.txt", "b\n", "made elsewhere");
    let head = head_of(&f);
    assert!(matches!(ops::undo(&f.repo).await, Err(Error::Invalid(_))));
    assert_eq!(head_of(&f), head, "nothing changed");
    assert!(
        undo::labels(&f.repo).await.unwrap().undo.is_some(),
        "still offered"
    );
}

#[tokio::test]
async fn the_first_commit_can_be_undone_too() {
    let f = Fixture::empty();
    f.write("a.txt", "one\n");
    f.git(&["add", "a.txt"]);
    undoable!(
        f,
        "Commit",
        undo::Mode::Soft,
        ops::commit(&f.repo, "first", false, false).await.unwrap()
    );
    ops::undo(&f.repo).await.unwrap();
    assert!(!queries::has_head(&f.repo).await.unwrap());
    assert_eq!(f.git(&["diff", "--cached", "--name-only"]), "a.txt\n");
    ops::redo(&f.repo).await.unwrap();
    assert_eq!(f.git(&["log", "--format=%s"]), "first\n");
}

#[tokio::test]
async fn a_hard_reset_is_undone_only_on_a_clean_tree() {
    let f = Fixture::with_history();
    let second = head_of(&f);
    undoable!(
        f,
        "Reset",
        undo::Mode::Hard,
        ops::reset(&f.repo, "HEAD~1", "hard").await.unwrap()
    );
    f.write("a.txt", "work in progress\n");
    assert!(matches!(ops::undo(&f.repo).await, Err(Error::Invalid(_))));
    assert_eq!(f.read("a.txt"), "work in progress\n");
    f.git(&["checkout", "--", "a.txt"]);
    ops::undo(&f.repo).await.unwrap();
    assert_eq!(head_of(&f), second);
    assert_eq!(f.read("a.txt"), "one\ntwo\nthree\nfour\n");
}

#[tokio::test]
async fn branches_and_tags_come_back_with_their_settings() {
    let f = Fixture::with_history();
    f.git(&["branch", "topic"]);
    f.git(&["config", "branch.topic.description", "my notes"]);
    let topic = f.git(&["rev-parse", "topic"]);
    undoable!(
        f,
        "Delete branch",
        undo::Mode::Keep,
        ops::delete_branch(&f.repo, "topic", true).await.unwrap()
    );
    ops::undo(&f.repo).await.unwrap();
    assert_eq!(f.git(&["rev-parse", "topic"]), topic);
    assert_eq!(f.git(&["config", "branch.topic.description"]), "my notes\n");

    undoable!(
        f,
        "Rename branch",
        undo::Mode::Keep,
        ops::rename_branch(&f.repo, "topic", "renamed")
            .await
            .unwrap()
    );
    ops::undo(&f.repo).await.unwrap();
    assert_eq!(
        f.git(&["branch", "--format=%(refname:short)"]),
        "main\ntopic\n"
    );
    assert_eq!(f.git(&["config", "branch.topic.description"]), "my notes\n");
    ops::redo(&f.repo).await.unwrap();
    assert_eq!(
        f.git(&["branch", "--format=%(refname:short)"]),
        "main\nrenamed\n"
    );
    assert_eq!(
        f.git(&["config", "branch.renamed.description"]),
        "my notes\n"
    );

    undoable!(
        f,
        "New tag",
        undo::Mode::Keep,
        ops::create_tag(&f.repo, "v1", "HEAD", Some("release"))
            .await
            .unwrap()
    );
    ops::undo(&f.repo).await.unwrap();
    assert!(f.git(&["tag"]).is_empty());
    ops::redo(&f.repo).await.unwrap();
    assert_eq!(f.git(&["tag"]), "v1\n");
}

#[tokio::test]
async fn a_checkout_is_undone_and_its_new_branch_removed() {
    let f = Fixture::with_history();
    undoable!(
        f,
        "New branch",
        undo::Mode::Keep,
        ops::create_branch(&f.repo, "feature", None, true)
            .await
            .unwrap()
    );
    assert_eq!(f.git(&["branch", "--show-current"]), "feature\n");
    ops::undo(&f.repo).await.unwrap();
    assert_eq!(f.git(&["branch", "--show-current"]), "main\n");
    assert_eq!(f.git(&["branch", "--format=%(refname:short)"]), "main\n");
    ops::redo(&f.repo).await.unwrap();
    assert_eq!(f.git(&["branch", "--show-current"]), "feature\n");

    let first = f.git(&["rev-parse", "HEAD~1"]).trim().to_owned();
    undoable!(
        f,
        "Checkout",
        undo::Mode::Keep,
        ops::checkout(&f.repo, &first, "commit").await.unwrap()
    );
    assert!(f.git(&["branch", "--show-current"]).trim().is_empty());
    ops::undo(&f.repo).await.unwrap();
    assert_eq!(f.git(&["branch", "--show-current"]), "feature\n");
}

#[tokio::test]
async fn a_merge_is_undone_keeping_unrelated_changes() {
    let f = Fixture::with_history();
    f.git(&["switch", "-q", "-c", "side"]);
    f.commit("side.txt", "side\n", "side work");
    f.git(&["switch", "-q", "main"]);
    f.commit("main.txt", "main\n", "main work");
    let main = head_of(&f);
    undoable!(
        f,
        "Merge",
        undo::Mode::Keep,
        ops::merge(&f.repo, "side", "no-ff").await.unwrap()
    );
    f.write("a.txt", "local edit\n");
    ops::undo(&f.repo).await.unwrap();
    assert_eq!(head_of(&f), main);
    assert_eq!(f.read("a.txt"), "local edit\n");
    assert!(!f.exists("side.txt"));
}

#[tokio::test]
async fn a_discard_is_undone_and_redone() {
    let f = Fixture::with_history();
    f.write("a.txt", "changed\n");
    f.write("new.txt", "new\n");
    let list = vec!["a.txt".to_owned(), "new.txt".to_owned()];
    undoable!(
        f,
        "Discard",
        undo::Mode::Keep,
        ops::discard_files(&f.repo, &list).await.unwrap()
    );
    ops::undo(&f.repo).await.unwrap();
    assert_eq!(f.read("a.txt"), "changed\n");
    assert_eq!(f.read("new.txt"), "new\n");
    assert!(
        queries::discard_backups(&f.repo).await.unwrap().is_empty(),
        "back in place, so not offered for recovery"
    );
    ops::redo(&f.repo).await.unwrap();
    assert_eq!(f.read("a.txt"), "one\ntwo\nthree\nfour\n");
    assert!(!f.exists("new.txt"));
    ops::undo(&f.repo).await.unwrap();
    assert_eq!(f.read("new.txt"), "new\n");
}

#[tokio::test]
async fn nothing_changed_records_nothing_and_new_work_clears_redo() {
    let f = Fixture::with_history();
    undoable!(
        f,
        "Checkout",
        undo::Mode::Keep,
        ops::checkout(&f.repo, "main", "local").await.unwrap()
    );
    assert_eq!(undo::labels(&f.repo).await.unwrap(), UndoLabels::default());
    assert!(matches!(ops::undo(&f.repo).await, Err(Error::Invalid(_))));

    undoable!(
        f,
        "New tag",
        undo::Mode::Keep,
        ops::create_tag(&f.repo, "a", "HEAD", None).await.unwrap()
    );
    ops::undo(&f.repo).await.unwrap();
    undoable!(
        f,
        "New tag",
        undo::Mode::Keep,
        ops::create_tag(&f.repo, "b", "HEAD", None).await.unwrap()
    );
    assert_eq!(undo::labels(&f.repo).await.unwrap().redo, None);
}

#[tokio::test]
async fn the_reflog_of_all_branches_finds_a_branch_moved_elsewhere() {
    let f = Fixture::with_history();
    f.git(&["branch", "topic"]);
    // Moved while main stays checked out: HEAD's reflog never sees it.
    f.git(&["branch", "-f", "topic", "HEAD~1"]);
    let head = queries::reflog(&f.repo, "HEAD", 100).await.unwrap();
    assert!(
        head.iter()
            .all(|entry| !entry.selector.starts_with("topic@"))
    );
    let all = queries::reflog_all(&f.repo, 100).await.unwrap();
    assert!(all.iter().any(|entry| entry.selector.starts_with("topic@")));
    assert!(all.iter().any(|entry| entry.selector.starts_with("HEAD@")));
}

#[tokio::test]
async fn commit_options_sign_off_trailers_skip_hooks_and_stage_all() {
    let f = Fixture::with_history();
    let hooks = f.path.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    f.write("a.txt", "changed\n");
    f.write("new.txt", "new\n");
    let options = CommitOptions {
        sign_off: true,
        no_verify: true,
        stage_all: true,
        trailers: vec!["Co-authored-by: Ada <ada@example.com>".into()],
    };
    ops::commit_with(&f.repo, "third", false, &options)
        .await
        .unwrap();
    let message = f.git(&["log", "-1", "--format=%B"]);
    assert!(
        message.contains("Co-authored-by: Ada <ada@example.com>"),
        "{message}"
    );
    assert!(message.contains("Signed-off-by: Test User <test@example.com>"));
    assert_eq!(
        f.git(&["show", "--name-only", "--format=", "HEAD"]),
        "a.txt\nnew.txt\n"
    );

    #[cfg(unix)]
    {
        f.write("a.txt", "again\n");
        let hooked = CommitOptions {
            stage_all: true,
            ..CommitOptions::default()
        };
        assert!(
            ops::commit_with(&f.repo, "fourth", false, &hooked)
                .await
                .is_err(),
            "the hook runs"
        );
    }
    let bad = CommitOptions {
        trailers: vec!["Two\nlines".into()],
        ..CommitOptions::default()
    };
    assert!(matches!(
        ops::commit_with(&f.repo, "x", true, &bad).await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn stashing_one_hunk_keeps_the_rest_and_applies_back() {
    let f = Fixture::empty();
    let lines: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    f.commit("a.txt", &lines, "first");
    let changed = lines
        .replace("line 2\n", "line 2 changed\n")
        .replace("line 18\n", "line 18 changed\n");
    f.write("a.txt", &changed);
    let diff = queries::working_diff(&f.repo, "a.txt", false, 3, false, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(diff.hunks.len(), 2);
    ops::stash_selection(&f.repo, "a.txt", &whole(1), 3, "the second change")
        .await
        .unwrap();
    assert_eq!(
        f.read("a.txt"),
        lines.replace("line 2\n", "line 2 changed\n")
    );
    assert!(f.git(&["stash", "list"]).contains("the second change"));
    let stashed = f.git(&["stash", "show", "-p", "stash@{0}"]);
    assert!(stashed.contains("+line 18 changed") && !stashed.contains("line 2 changed"));
    // Like any stash, it applies to a file without local changes.
    f.git(&["checkout", "--", "a.txt"]);
    ops::stash_apply(&f.repo, "stash@{0}", true).await.unwrap();
    assert_eq!(
        f.read("a.txt"),
        lines.replace("line 18\n", "line 18 changed\n")
    );
    assert!(
        f.git(&["diff", "--cached"]).is_empty(),
        "nothing comes back staged"
    );

    f.git(&["add", "a.txt"]);
    assert!(matches!(
        ops::stash_selection(&f.repo, "a.txt", &whole(0), 3, "x").await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn lines_of_an_old_commit_are_taken_back_out() {
    let f = Fixture::empty();
    let lines: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    f.commit("a.txt", &lines, "first");
    let changed = lines
        .replace("line 2\n", "line 2 changed\n")
        .replace("line 18\n", "line 18 changed\n");
    let sha = f.commit("a.txt", &changed, "two changes");
    f.commit("b.txt", "later\n", "later work");
    ops::revert_selection(&f.repo, &sha, "a.txt", &whole(1), 3)
        .await
        .unwrap();
    assert_eq!(
        f.read("a.txt"),
        lines.replace("line 2\n", "line 2 changed\n")
    );
    assert_eq!(
        f.git(&["log", "-1", "--format=%s"]),
        "later work\n",
        "nothing is committed"
    );

    // Once the lines moved on, git refuses and the file stays as it is.
    f.write("a.txt", lines.replace("line 2\n", "line 2 rewritten\n"));
    assert!(matches!(
        ops::revert_selection(&f.repo, &sha, "a.txt", &whole(0), 3).await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(
        f.read("a.txt"),
        lines.replace("line 2\n", "line 2 rewritten\n")
    );
    let added = f.commit("c.txt", "new\n", "added");
    assert!(matches!(
        ops::revert_selection(&f.repo, &added, "c.txt", &whole(0), 3).await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn stash_untracked_only_rename_take_one_file_and_find_by_sha() {
    let f = Fixture::with_history();
    f.write("a.txt", "tracked change\n");
    f.write("new.txt", "untracked\n");
    ops::stash_untracked(&f.repo, Some("just the new files"))
        .await
        .unwrap();
    assert!(!f.exists("new.txt"));
    assert_eq!(f.read("a.txt"), "tracked change\n", "tracked changes stay");
    let untracked_sha = f.git(&["rev-parse", "stash@{0}"]).trim().to_owned();

    ops::stash_push(&f.repo, Some("tracked"), false, false, false, None)
        .await
        .unwrap();
    // The first stash is now stash@{1}; its sha still finds it.
    ops::stash_rename(&f.repo, &untracked_sha, "renamed")
        .await
        .unwrap();
    let list = f.git(&["stash", "list", "--format=%gs"]);
    assert_eq!(list.lines().next(), Some("renamed"), "{list}");
    assert_eq!(f.git(&["rev-parse", "stash@{0}"]).trim(), untracked_sha);

    let tracked_sha = f.git(&["rev-parse", "stash@{1}"]).trim().to_owned();
    ops::stash_restore_file(&f.repo, &tracked_sha, "a.txt")
        .await
        .unwrap();
    assert_eq!(f.read("a.txt"), "tracked change\n");
    assert!(
        f.git(&["diff", "--cached"]).is_empty(),
        "the index is left alone"
    );
    assert!(matches!(
        ops::stash_restore_file(&f.repo, &tracked_sha, "a.txt").await,
        Err(Error::Invalid(_))
    ));
    ops::stash_restore_file(&f.repo, &untracked_sha, "new.txt")
        .await
        .unwrap();
    assert_eq!(f.read("new.txt"), "untracked\n");

    ops::stash_drop(&f.repo, &untracked_sha).await.unwrap();
    assert!(matches!(
        ops::stash_drop(&f.repo, &untracked_sha).await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(f.git(&["stash", "list"]).lines().count(), 1);
}

#[tokio::test]
async fn a_fixup_commit_names_its_target() {
    let f = Fixture::with_history();
    let first = f.git(&["rev-parse", "HEAD~1"]).trim().to_owned();
    assert!(
        matches!(
            ops::commit_fixup(&f.repo, &first).await,
            Err(Error::Invalid(_))
        ),
        "nothing staged"
    );
    f.write("b.txt", "fix\n");
    f.git(&["add", "b.txt"]);
    ops::commit_fixup(&f.repo, &first).await.unwrap();
    assert_eq!(f.git(&["log", "-1", "--format=%s"]), "fixup! first\n");
    f.git(&["switch", "-q", "--orphan", "elsewhere"]);
    f.write("c.txt", "c\n");
    f.git(&["add", "c.txt"]);
    assert!(
        matches!(
            ops::commit_fixup(&f.repo, &first).await,
            Err(Error::Invalid(_))
        ),
        "not on this branch"
    );
}
