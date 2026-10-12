use std::path::PathBuf;

/// Where the directives in use came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterSource {
    /// `RUST_LOG`.
    Env,
    /// The filter file at this path.
    File(PathBuf),
    /// Neither: [`DEFAULT_DIRECTIVES`](super::DEFAULT_DIRECTIVES).
    Default,
}
