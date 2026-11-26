//! Thin async wrapper around the git command line. No shell, no terminal, no prompts.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex as StdMutex, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::text::mask_urls;

pub const EDITOR_FLAG: &str = "--gitcraft-editor";
pub const MIN_GIT: (u32, u32) = (2, 35);

/// Receives each progress line of a long operation.
pub type Progress = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct GitError {
    pub args: Vec<String>,
    pub code: i32,
    pub stderr: String,
    pub stdout: String,
}

impl GitError {
    pub fn new(args: &[&str], code: i32, stderr: &str, stdout: &str) -> Self {
        Self {
            args: args.iter().map(|arg| mask_urls(arg)).collect(),
            code,
            stderr: mask_urls(stderr.trim()),
            stdout: mask_urls(stdout),
        }
    }

    pub fn command(&self) -> String {
        format!("git {}", self.args.join(" "))
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.stderr.is_empty() {
            f.write_str(&self.stderr)
        } else if !self.stdout.trim().is_empty() {
            f.write_str(self.stdout.trim())
        } else {
            write!(f, "{} failed with exit code {}", self.command(), self.code)
        }
    }
}

impl std::error::Error for GitError {}

#[derive(Debug, Clone)]
pub struct GitResult {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub err: String,
}

impl GitResult {
    pub fn out(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.stdout)
    }

    /// Standard output and standard error, for showing to the user.
    pub fn output(&self) -> String {
        let out = self.out();
        let parts = [out.trim(), self.err.trim()];
        let joined = parts
            .iter()
            .filter(|p| !p.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        mask_urls(&joined)
    }
}

#[derive(Default, Clone)]
pub struct Options<'a> {
    pub stdin: Option<&'a [u8]>,
    pub env: Vec<(&'a str, String)>,
    pub progress: Option<Progress>,
    /// The call takes long for reasons of its own (an external tool waiting for the user, or
    /// git's shell scripts probing for tools); it doesn't count against [`MAX_JOBS`], or it would
    /// hold up every refresh.
    pub slow: bool,
}

impl Options<'static> {
    pub fn slow() -> Self {
        Self {
            slow: true,
            ..Self::default()
        }
    }
}

/// How many quick git calls run at once. Starting a process is expensive on Windows, and many at
/// once mostly compete for the same disk and virus scanner. `GITCRAFT_GIT_JOBS` overrides it.
const MAX_JOBS: usize = if cfg!(windows) { 6 } else { 16 };

static JOBS: LazyLock<tokio::sync::Semaphore> = LazyLock::new(|| {
    let jobs = std::env::var("GITCRAFT_GIT_JOBS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|jobs| *jobs > 0)
        .unwrap_or(MAX_JOBS);
    tokio::sync::Semaphore::new(jobs)
});

static EDITOR_EXE: OnceLock<PathBuf> = OnceLock::new();

/// The git executable that is started, and what its environment needs on top.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Program {
    path: PathBuf,
    env: Vec<(String, String)>,
}

impl Program {
    fn plain(path: PathBuf) -> Self {
        Self {
            path,
            env: Vec::new(),
        }
    }

    fn command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(&self.path);
        command.envs(self.env.iter().map(|(key, value)| (key, value)));
        command
    }
}

/// The git program chosen in the settings, already resolved; `None` uses `git` on PATH.
static GIT_PROGRAM: LazyLock<RwLock<Option<Program>>> = LazyLock::new(Default::default);
/// `git` on PATH, looked up once rather than by every process start.
static DEFAULT_PROGRAM: LazyLock<Program> = LazyLock::new(|| resolve(Path::new("git")));

/// Uses this git program from now on; `None` (or an empty path) goes back to `git` on PATH.
pub fn set_git_program(program: Option<PathBuf>) {
    *GIT_PROGRAM
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = program
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| resolve(&path));
}

fn program() -> Program {
    GIT_PROGRAM
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .unwrap_or_else(|| DEFAULT_PROGRAM.clone())
}

fn git_program() -> PathBuf {
    program().path
}

/// On Windows, finds `git` on PATH once and starts Git for Windows' real `git.exe` rather than
/// its `cmd\git.exe` launcher, which would start a second process for every call. The launcher's
/// environment (its tool folders first on PATH, `MSYSTEM`) is set instead. Elsewhere the program
/// is left as it is: packaged builds give git a PATH of its own, which must decide.
fn resolve(program: &Path) -> Program {
    #[cfg(windows)]
    {
        let found = if program == Path::new("git") {
            std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join("git.exe"))
                    .find(|candidate| candidate.is_file())
            })
        } else {
            Some(program.to_path_buf())
        };
        match found {
            Some(found) => behind_launcher(&found).unwrap_or_else(|| Program::plain(found)),
            None => Program::plain(program.to_path_buf()),
        }
    }
    #[cfg(not(windows))]
    {
        Program::plain(program.to_path_buf())
    }
}

/// The `git.exe` that Git for Windows' launcher (`cmd\git.exe` or `bin\git.exe`) would start.
#[cfg(any(windows, test))]
fn behind_launcher(launcher: &Path) -> Option<Program> {
    let folder = launcher.parent()?;
    let name = folder.file_name()?.to_str()?.to_ascii_lowercase();
    if name != "cmd" && name != "bin" {
        return None;
    }
    let root = folder.parent()?;
    let usr_bin = root.join("usr").join("bin");
    if !usr_bin.is_dir() {
        return None;
    }
    let (system, bin) = [
        ("MINGW64", "mingw64"),
        ("CLANGARM64", "clangarm64"),
        ("MINGW32", "mingw32"),
    ]
    .into_iter()
    .map(|(system, dir)| (system, root.join(dir).join("bin")))
    .find(|(_, bin)| bin.join("git.exe").is_file())?;
    let git = bin.join("git.exe");
    if git == launcher {
        return None;
    }
    let mut path = vec![bin, usr_bin];
    if let Some(inherited) = std::env::var_os("PATH") {
        path.extend(std::env::split_paths(&inherited));
    }
    let mut env = Vec::new();
    if let Ok(joined) = std::env::join_paths(path) {
        env.push(("PATH".to_owned(), joined.to_string_lossy().into_owned()));
    }
    if std::env::var_os("MSYSTEM").is_none() {
        env.push(("MSYSTEM".to_owned(), system.to_owned()));
    }
    if std::env::var_os("PLINK_PROTOCOL").is_none() {
        env.push(("PLINK_PROTOCOL".to_owned(), "ssh".to_owned()));
    }
    Some(Program { path: git, env })
}
static CHILD_ENV: LazyLock<RwLock<ChildEnv>> = LazyLock::new(Default::default);

/// Changes to the environment git runs in, on top of what this process inherited.
#[derive(Debug, Clone, Default)]
pub struct ChildEnv {
    pub set: Vec<(String, String)>,
    pub remove: Vec<String>,
}

/// Replaces the changes made to git's environment. GUI launches need this: their PATH
/// is often not the login shell's, and an AppImage's library paths must not reach git.
pub fn set_child_env(env: ChildEnv) {
    *CHILD_ENV
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = env;
}

fn apply_child_env(command: &mut Command) {
    let env = CHILD_ENV
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for key in &env.remove {
        command.env_remove(key);
    }
    for (key, value) in &env.set {
        command.env(key, value);
    }
}

/// Overrides the executable used as git's editor. The default is the running
/// executable, which must then call [`crate::editor::run_if_requested`] first thing in `main`.
pub fn set_editor_exe(path: PathBuf) {
    let _ = EDITOR_EXE.set(path);
}

fn quote(path: &str) -> String {
    // Git runs editors through a shell (also on Windows, via its bundled sh),
    // which copes with forward slashes everywhere.
    format!("\"{}\"", path.replace('\\', "/"))
}

pub fn editor_command(extra: &[&str]) -> String {
    let exe = EDITOR_EXE
        .get()
        .cloned()
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("gitcraft"));
    let mut parts = vec![quote(&exe.to_string_lossy()), EDITOR_FLAG.to_owned()];
    parts.extend(extra.iter().map(|part| (*part).to_owned()));
    parts.join(" ")
}

fn prepare(cwd: &str, args: &[&str], options: &Options<'_>) -> Command {
    let program = program();
    let mut command = Command::new(&program.path);
    command.envs(program.env.iter().map(|(key, value)| (key, value)));
    apply_child_env(&mut command);
    command
        .args(["-c", "color.ui=false", "-c", "core.quotepath=false"])
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if options.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .kill_on_drop(true);
    for key in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "LC_ALL"] {
        command.env_remove(key);
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LANGUAGE", "C")
        .env("LC_MESSAGES", "C")
        .env("GIT_EDITOR", editor_command(&["msg"]))
        .env("GIT_SEQUENCE_EDITOR", editor_command(&["todo"]))
        .env("GIT_MERGE_AUTOEDIT", "no")
        .env("GIT_PAGER", "cat");
    for (key, value) in &options.env {
        command.env(key, value);
    }
    detach(&mut command);
    command
}

#[cfg(unix)]
fn detach(command: &mut Command) {
    // Without a controlling terminal ssh/gpg cannot block waiting for a prompt.
    // SAFETY: setsid is async-signal-safe and touches no memory of the parent.
    #[allow(unsafe_code)]
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn detach(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

fn spawn_failure(args: &[&str], error: &std::io::Error) -> GitError {
    GitError::new(args, -1, &format!("could not run git: {error}"), "")
}

/// Git processes running right now, for the trace.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

tokio::task_local! {
    static SPAWNS: Arc<AtomicU64>;
}

/// Runs `future` and counts the git processes it starts, including those of futures it joins.
pub async fn count_spawns<F: Future>(future: F) -> (F::Output, u64) {
    let counter = Arc::new(AtomicU64::new(0));
    let output = SPAWNS.scope(counter.clone(), future).await;
    (output, counter.load(Ordering::Relaxed))
}

enum TraceTo {
    Stderr,
    File(PathBuf),
}

/// `GITCRAFT_TRACE=1` writes every git call with its duration to stderr; any other value is
/// taken as a file to append to (release builds on Windows have no console).
static TRACE: LazyLock<Option<TraceTo>> = LazyLock::new(|| {
    let value = std::env::var_os("GITCRAFT_TRACE")?;
    match value.to_str() {
        Some("" | "0") => None,
        Some("1") => Some(TraceTo::Stderr),
        _ => Some(TraceTo::File(PathBuf::from(value))),
    }
});

fn trace(args: &[&str], running: usize, elapsed: Duration, code: i32) {
    let Some(to) = TRACE.as_ref() else {
        return;
    };
    let line = format!(
        "[git {:>7.1} ms, {running} running, exit {code}] {}\n",
        elapsed.as_secs_f64() * 1000.0,
        mask_urls(&args.join(" "))
    );
    match to {
        TraceTo::Stderr => eprint!("{line}"),
        TraceTo::File(path) => {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = std::io::Write::write_all(&mut file, line.as_bytes());
            }
        }
    }
}

/// Counts a git process from start to end, for the trace.
struct Running;

impl Running {
    fn start() -> (Self, usize) {
        let _ = SPAWNS.try_with(|counter| counter.fetch_add(1, Ordering::Relaxed));
        (Self, RUNNING.fetch_add(1, Ordering::Relaxed) + 1)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Runs git and returns whatever it did, even a non-zero exit.
pub async fn run_unchecked(
    cwd: &str,
    args: &[&str],
    options: Options<'_>,
) -> Result<GitResult, GitError> {
    // Long operations report progress; they and other slow calls don't queue.
    let _permit = if options.progress.is_none() && !options.slow {
        JOBS.acquire().await.ok()
    } else {
        None
    };
    let started = Instant::now();
    let (_running, running) = Running::start();
    let mut child = prepare(cwd, args, &options)
        .spawn()
        .map_err(|e| spawn_failure(args, &e))?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let data = options.stdin;
    let progress = options.progress.clone();

    // All three pipes run together, so a full pipe can never deadlock git.
    let write = async move {
        if let (Some(mut pipe), Some(data)) = (stdin, data) {
            let _ = pipe.write_all(data).await;
        }
    };
    let read_out = async move {
        let mut buffer = Vec::new();
        if let Some(mut pipe) = stdout {
            let _ = pipe.read_to_end(&mut buffer).await;
        }
        buffer
    };
    let read_err = async move {
        let Some(mut pipe) = stderr else {
            return Vec::new();
        };
        match progress {
            Some(progress) => pump_progress(&mut pipe, &progress).await,
            None => {
                let mut buffer = Vec::new();
                let _ = pipe.read_to_end(&mut buffer).await;
                buffer
            }
        }
    };
    let ((), stdout, stderr) = tokio::join!(write, read_out, read_err);
    let status = child.wait().await.map_err(|e| spawn_failure(args, &e))?;
    trace(
        args,
        running,
        started.elapsed(),
        status.code().unwrap_or(-1),
    );
    Ok(GitResult {
        code: status.code().unwrap_or(-1),
        stdout,
        err: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// Runs git and fails on a non-zero exit.
pub async fn run(cwd: &str, args: &[&str], options: Options<'_>) -> Result<GitResult, GitError> {
    let result = run_unchecked(cwd, args, options).await?;
    if result.code != 0 {
        return Err(GitError::new(args, result.code, &result.err, &result.out()));
    }
    Ok(result)
}

pub async fn git(cwd: &str, args: &[&str]) -> Result<GitResult, GitError> {
    run(cwd, args, Options::default()).await
}

pub async fn git_unchecked(cwd: &str, args: &[&str]) -> Result<GitResult, GitError> {
    run_unchecked(cwd, args, Options::default()).await
}

async fn pump_progress<R: tokio::io::AsyncRead + Unpin>(
    stream: &mut R,
    progress: &Progress,
) -> Vec<u8> {
    let mut all = Vec::new();
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    while let Ok(count) = stream.read(&mut chunk).await {
        if count == 0 {
            break;
        }
        all.extend_from_slice(&chunk[..count]);
        buffer.extend_from_slice(&chunk[..count]);
        while let Some(end) = buffer.iter().position(|b| *b == b'\r' || *b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            report_line(&line[..end], progress);
        }
    }
    report_line(&buffer, progress);
    // Keep only the final state of each \r-updated counter, for error reports.
    let lines: Vec<&[u8]> = all
        .split(|b| *b == b'\n')
        .map(|line| line.rsplit(|b| *b == b'\r').next().unwrap_or(line))
        .collect();
    lines.join(&b'\n')
}

fn report_line(line: &[u8], progress: &Progress) {
    let text = String::from_utf8_lossy(line);
    if !text.trim().is_empty() {
        progress(mask_urls(&text));
    }
}

static LOCKS: LazyLock<StdMutex<HashMap<String, Arc<Mutex<()>>>>> = LazyLock::new(Default::default);

/// Serialises mutating operations per repository to avoid index.lock races.
pub fn repo_lock(repo: &str) -> Arc<Mutex<()>> {
    let mut locks = LOCKS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks.entry(repo.to_owned()).or_default().clone()
}

/// What the git program GitCraft runs says about itself, such as "git version 2.55.0", for
/// the About window.
pub async fn git_version_text() -> Result<String, String> {
    let program = program();
    let output = Command::from(program.command())
        .arg("--version")
        .output()
        .await
        .map_err(|_| format!("{} can't be started", program.path.display()))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The installed git's version, or an explanation for the user. Starts git and waits for it,
/// so call it off the UI thread.
pub fn check_git() -> Result<(u32, u32), String> {
    check_program(&program(), &git_program())
}

/// The version of the git program at `program`, or an explanation for the user.
pub fn check_git_at(program: &Path) -> Result<(u32, u32), String> {
    check_program(&resolve(program), program)
}

fn check_program(resolved: &Program, program: &Path) -> Result<(u32, u32), String> {
    let output = resolved.command().arg("--version").output().map_err(|_| {
        if program == Path::new("git") {
            "git was not found on PATH; install git and try again".to_owned()
        } else {
            format!("{} can't be started", program.display())
        }
    })?;
    let text = String::from_utf8_lossy(&output.stdout);
    let version = parse_git_version(&text)
        .ok_or_else(|| format!("could not read the git version from: {text}"))?;
    if version < MIN_GIT {
        return Err(format!(
            "GitCraft needs git {}.{} or newer, found {}.{}",
            MIN_GIT.0, MIN_GIT.1, version.0, version.1
        ));
    }
    Ok(version)
}

fn parse_git_version(text: &str) -> Option<(u32, u32)> {
    let number = text
        .split_whitespace()
        .find(|word| word.starts_with(|c: char| c.is_ascii_digit()))?;
    let mut parts = number.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_git_program_that_isnt_there_is_explained() {
        let missing = Path::new("/nowhere/at/all/git");
        assert!(
            check_git_at(missing)
                .unwrap_err()
                .contains("can't be started")
        );
        assert!(check_git_at(Path::new("git")).is_ok(), "the one on PATH");
    }

    #[test]
    fn git_for_windows_launchers_lead_to_the_real_git() {
        let root = tempfile::tempdir().unwrap();
        let make = |parts: &[&str]| {
            let path = parts
                .iter()
                .fold(root.path().to_path_buf(), |p, s| p.join(s));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
            path
        };
        let launcher = make(&["cmd", "git.exe"]);
        let real = make(&["mingw64", "bin", "git.exe"]);
        assert_eq!(
            behind_launcher(&launcher),
            None,
            "no usr/bin: not Git for Windows"
        );
        std::fs::create_dir_all(root.path().join("usr").join("bin")).unwrap();
        let program = behind_launcher(&launcher).unwrap();
        assert_eq!(program.path, real);
        let path = &program.env.iter().find(|(key, _)| key == "PATH").unwrap().1;
        assert!(path.starts_with(&*real.parent().unwrap().to_string_lossy()));
        assert_eq!(behind_launcher(&real), None, "already the real one");
        assert_eq!(behind_launcher(&make(&["other", "git.exe"])), None);
    }

    #[test]
    fn reads_git_versions() {
        assert_eq!(parse_git_version("git version 2.55.0\n"), Some((2, 55)));
        assert_eq!(
            parse_git_version("git version 2.39.5 (Apple Git-154)"),
            Some((2, 39))
        );
        assert_eq!(
            parse_git_version("git version 2.47.1.windows.2"),
            Some((2, 47))
        );
        assert_eq!(parse_git_version("nonsense"), None);
    }

    #[tokio::test]
    async fn progress_splits_on_carriage_returns_and_keeps_final_counters() {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let sink = seen.clone();
        let progress: Progress = Arc::new(move |line| sink.lock().unwrap().push(line));
        let mut input: &[u8] = b"Counting 1%\rCounting 100%\nDone\n   \npartial";
        let kept = pump_progress(&mut input, &progress).await;
        assert_eq!(
            *seen.lock().unwrap(),
            ["Counting 1%", "Counting 100%", "Done", "partial"]
        );
        assert_eq!(
            String::from_utf8(kept).unwrap(),
            "Counting 100%\nDone\n   \npartial"
        );
    }
}
