//! Git layer of GitCraft. Runs the system `git` and parses its machine-readable output.

pub mod editor;
pub mod graph;
pub mod model;
pub mod ops;
pub mod parsers;
pub mod paths;
pub mod queries;
pub mod repos;
pub mod runner;
pub mod store;
pub mod text;
pub mod watcher;

pub use runner::GitError;

#[derive(Debug, Clone)]
pub enum Error {
    /// A git command failed.
    Git(GitError),
    /// The request can't be carried out; the message is for the user.
    Invalid(String),
    /// A typed path or similar input is wrong; the message is for the user.
    Validation(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Git(error) => error.fmt(f),
            Self::Invalid(message) | Self::Validation(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<GitError> for Error {
    fn from(error: GitError) -> Self {
        Self::Git(error)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Invalid(error.to_string())
    }
}

/// Refuses values git would read as an option rather than a ref or a name.
pub fn check_ref(value: &str) -> Result<&str> {
    if value.starts_with('-') {
        return Err(Error::Invalid(format!("Invalid name: {value}")));
    }
    Ok(value)
}
