//! Runtime configuration of how Claude Code sessions are launched and watched.

use std::time::Duration;

use crate::open_sessions::{PENDING_SPAWN_DEADLINE, RESUME_READY_DEADLINE};

/// The command Delta launches in each tmux session by default.
pub const DEFAULT_SESSION_COMMAND: &str = "claude";

/// How sessions are launched and how long the watchdog waits on a launch.
///
/// Every field has a production default ([`LaunchConfig::default`]), so the
/// composition root only overrides what the environment asks for:
///
/// - [`Self::claude_bin`] lets tests and alternative installs substitute the
///   binary Delta spawns (e.g. a scripted stand-in, or a `claude` outside
///   `PATH`) without changing any spawn logic — the command line built around
///   it is identical.
/// - The two deadlines let a test shrink the launch watchdog from its
///   generous production value so a "launch never came up" path can be
///   exercised in seconds instead of half a minute.
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
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            claude_bin: DEFAULT_SESSION_COMMAND.to_owned(),
            pending_spawn_deadline: PENDING_SPAWN_DEADLINE,
            resume_ready_deadline: RESUME_READY_DEADLINE,
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
    }
}
