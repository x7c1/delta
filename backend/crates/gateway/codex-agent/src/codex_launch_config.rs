//! How the connection launches the app-server.

/// How the connection launches the app-server.
#[derive(Debug, Clone)]
pub struct CodexLaunchConfig {
    /// The program launched to obtain an app-server (`codex` by default). Used
    /// verbatim as `argv[0]`, so it may be a bare name resolved via `PATH` or an
    /// absolute path — matching the core's `LaunchConfig::claude_bin`. A test
    /// points this at the fake app-server binary.
    pub codex_bin: String,
    /// Arguments after `argv[0]` (`["app-server"]` by default, so `codex_bin`
    /// alone spawns the server). A test that points `codex_bin` at a fake
    /// clears this.
    pub args: Vec<String>,
    /// Extra environment variables set on the child, on top of the inherited
    /// environment. Empty by default; the server fills it with its
    /// configuration's `child_env` (the desktop app's login-shell `PATH` and
    /// locale), and a test uses it to hand the fake its scenario without
    /// mutating the parent process's (shared) environment.
    pub env: Vec<(String, String)>,
}

impl Default for CodexLaunchConfig {
    fn default() -> Self {
        Self {
            codex_bin: "codex".to_owned(),
            args: vec!["app-server".to_owned()],
            env: Vec::new(),
        }
    }
}
