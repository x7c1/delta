//! How the adapter launches `claude`.

/// How the adapter launches `claude`.
#[derive(Debug, Clone)]
pub struct ClaudeLaunchConfig {
    /// The program launched in each tmux session (`claude` by default). Used
    /// verbatim as `argv[0]`, so it may be a bare name resolved via `PATH` or an
    /// absolute path — matching the core's `LaunchConfig::claude_bin`.
    pub claude_bin: String,
    /// Path to the Delta-owned session settings file passed via `--settings`.
    /// The file is written by the composition root (it is shared across
    /// sessions); the adapter only references its path.
    pub settings_path: String,
}
