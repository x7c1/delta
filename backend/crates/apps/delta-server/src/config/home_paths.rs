//! Defaults that resolve under `$HOME`.

use std::ffi::OsString;
use std::path::PathBuf;

/// The default data directory: `<platform data dir>/<identifier>`.
///
/// The platform data directory is `~/Library/Application Support` on macOS and
/// `$XDG_DATA_HOME` (else `~/.local/share`) on Linux — what [`dirs::data_dir`]
/// returns, and what Tauri's `app_data_dir()` joins the bundle identifier to,
/// so the desktop app keeps the directory it used before the server chose it.
/// When there is no home directory to resolve it from — only in degenerate
/// environments — fall back to the temp directory so the server still starts,
/// mirroring [`default_worktree_base`].
pub(super) fn default_data_dir(identifier: &str) -> String {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(identifier)
        .to_string_lossy()
        .into_owned()
}

/// The directory a hook-reported `transcript_path` must resolve under.
///
/// Real Claude Code writes transcripts under its config directory's `projects/`
/// subdirectory, so the default is `${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects`.
/// `DELTA_TRANSCRIPT_ROOT` overrides it so the fake harness (and tests), which
/// write transcripts elsewhere, still validate. When `HOME` is unset — only in
/// degenerate environments — fall back to a temp-dir-based path so the server
/// still starts, mirroring [`default_worktree_base`].
pub(super) fn transcript_root(
    root: Option<String>,
    claude_config_dir: Option<String>,
    home: Option<OsString>,
) -> String {
    if let Some(root) = root.filter(|root| !root.is_empty()) {
        return root;
    }
    let config_dir = match claude_config_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home_or_temp(home).join(".claude"),
    };
    config_dir.join("projects").to_string_lossy().into_owned()
}

/// Default base directory for per-session git worktrees: `$HOME/.delta/worktrees`.
///
/// Deliberately outside any repository tree so a worktree does not inherit a
/// surrounding `CLAUDE.md`/settings (which would otherwise trigger Claude Code's
/// blocking external-import prompt at launch). When `HOME` is unset — only in
/// degenerate environments — fall back to a temp-dir-based path so the server
/// still starts; mirrors how the git gateway resolves `~/.claude.json`.
pub(super) fn default_worktree_base(home: Option<OsString>) -> String {
    home_or_temp(home)
        .join(".delta")
        .join("worktrees")
        .to_string_lossy()
        .into_owned()
}

fn home_or_temp(home: Option<OsString>) -> PathBuf {
    match home {
        Some(home) => PathBuf::from(home),
        None => std::env::temp_dir(),
    }
}
