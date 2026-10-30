//! Thin async wrapper around the git command line. No shell, no terminal, no prompts.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex as StdMutex, OnceLock, RwLock};
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
}

static EDITOR_EXE: OnceLock<PathBuf> = OnceLock::new();
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
    let mut command = Command::new("git");
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

/// Runs git and returns whatever it did, even a non-zero exit.
pub async fn run_unchecked(
    cwd: &str,
    args: &[&str],
    options: Options<'_>,
) -> Result<GitResult, GitError> {
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

/// The installed git's version, or an explanation for the user.
pub fn check_git() -> Result<(u32, u32), String> {
    let output = std::process::Command::new("git")
        .arg("--version")
        .output()
        .map_err(|_| "git was not found on PATH; install git and try again".to_owned())?;
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
