//! Runtime configuration of how Claude Code sessions are launched and watched.

use std::time::Duration;

use crate::open_sessions::{PENDING_SPAWN_DEADLINE, RESUME_READY_DEADLINE};

/// The command Delta launches in each tmux session by default.
pub const DEFAULT_SESSION_COMMAND: &str = "claude";

/// How long a `PermissionRequest` hook response may block waiting for a
/// browser decision before falling back to the interactive TUI prompt.
///
/// Generous, like the launch deadlines: the user may be reading the notice, so
/// the wait must comfortably outlast a human think. It is kept under Claude
/// Code's own hook timeout (60s by default) so the fallback is always Delta's
/// deliberate empty passthrough — the TUI prompt appears exactly as it would
/// have without the hook — rather than Claude abandoning the hook mid-wait.
pub const PERMISSION_DECISION_DEADLINE: Duration = Duration::from_secs(50);

/// How sessions are launched and how long the watchdog waits on a launch.
///
/// Every field has a production default ([`LaunchConfig::default`]), so the
/// composition root only overrides what the environment asks for:
///
/// - [`Self::claude_bin`] lets tests and alternative installs substitute the
///   binary Delta spawns (e.g. a scripted stand-in, or a `claude` outside
///   `PATH`) without changing any spawn logic — the command line built around
///   it is identical.
/// - The deadlines let a test shrink the launch watchdog (and the permission
///   decision wait) from their generous production values so a "never came
///   up" / "never decided" path can be exercised in seconds.
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// The program launched in each tmux session (`claude` by default). Used
    /// verbatim as the command's argv[0], so it may be a bare name resolved via
    /// `PATH` or an absolute path.
    pub claude_bin: String,
    /// How long a fresh spawn may sit unbound before the watchdog reaps it.
    /// Defaults to [`PENDING_SPAWN_DEADLINE`]; see that constant for why the
    /// production value is deliberately generous.
    pub pending_spawn_deadline: Duration,
    /// How long a resumed session may sit not-ready before the watchdog fails
    /// it. Defaults to [`RESUME_READY_DEADLINE`]; see that constant.
    pub resume_ready_deadline: Duration,
    /// How long the `PermissionRequest` hook response blocks waiting for a
    /// browser decision before falling back to the TUI prompt. Defaults to
    /// [`PERMISSION_DECISION_DEADLINE`]; see that constant.
    pub permission_decision_deadline: Duration,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            claude_bin: DEFAULT_SESSION_COMMAND.to_owned(),
            pending_spawn_deadline: PENDING_SPAWN_DEADLINE,
            resume_ready_deadline: RESUME_READY_DEADLINE,
            permission_decision_deadline: PERMISSION_DECISION_DEADLINE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_the_production_constants() {
        let config = LaunchConfig::default();
        assert_eq!(config.claude_bin, "claude");
        assert_eq!(config.pending_spawn_deadline, PENDING_SPAWN_DEADLINE);
        assert_eq!(config.resume_ready_deadline, RESUME_READY_DEADLINE);
        assert_eq!(
            config.permission_decision_deadline,
            PERMISSION_DECISION_DEADLINE
        );
    }
}
