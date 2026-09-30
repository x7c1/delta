/// Which of the two cwd-relative settings the environment set explicitly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExplicitPaths {
    /// `DELTA_DB_PATH` is set.
    pub database_path: bool,
    /// `DELTA_SESSION_WORKDIR` is set.
    pub session_workdir: bool,
}

impl ExplicitPaths {
    /// Read from the process environment, with the same test the server's
    /// configuration applies (a set, valid-Unicode value counts, even empty).
    pub fn from_env() -> Self {
        Self {
            database_path: std::env::var("DELTA_DB_PATH").is_ok(),
            session_workdir: std::env::var("DELTA_SESSION_WORKDIR").is_ok(),
        }
    }
}
