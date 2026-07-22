//! Error types. `HtError` covers the runner's own failures (assertions, graph
//! problems); everything else flows through `anyhow`. `Result<T>` is the
//! crate-wide alias used by nearly every function.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum HtError {
    #[error("manifest error: {0}")]
    Manifest(String),

    #[error("dependency cycle detected involving task `{0}`")]
    Cycle(String),

    #[error("task `{0}` needs unknown task `{1}`")]
    UnknownDep(String, String),

    #[error("assertion failed in task `{task}`: {detail}")]
    Assertion { task: String, detail: String },

    #[error("browser error: {0}")]
    #[allow(dead_code)]
    Browser(String),
}

pub type Result<T> = std::result::Result<T, anyhow::Error>;
