//! Runtime configuration of how Claude Code sessions are launched and watched.

use std::time::Duration;

use crate::interactor::session_actor::runtime::{
    ECHO_DEADLINE, PENDING_SPAWN_DEADLINE, RESUME_READY_DEADLINE,
};

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

/// How long the whole launch preparation of a freshly-accepted session may run
/// before it is abandoned.
///
/// The sequence is unbounded from Delta's side: `git fetch origin <branch>` can
/// hang on an unreachable remote or a credential prompt with no timeout of its
/// own, and a session stuck there would sit `spawning` forever — nothing else
/// watches it, because the bind watchdog only starts once a pane exists. This
/// is that backstop. It is set far above any honest preparation (a cold clone
/// of a large repository is minutes at worst) precisely so it never truncates a
/// slow-but-healthy checkout: reaching it means the launch is stuck, and the
/// session is failed with a reason the browser can show.
///
/// Overridable via `DELTA_LAUNCH_PREP_DEADLINE_MS` (see the server's
/// `launch_from_env`), so a test can exercise the give-up path in milliseconds.
pub const LAUNCH_PREP_DEADLINE: Duration = Duration::from_secs(600);

/// How sessions are launched and how long the watchdog waits on a launch.
///
/// Every field has a production default ([`LaunchConfig::default`]), so the
/// composition root only overrides what the environment asks for:
///
/// - [`Self::claude_bin`] lets tests and alternative installs substitute the
///   binary Delta spawns (e.g. a scripted stand-in, or a `claude` outside
///   `PATH`) without changing any spawn logic — the command line built around
///   it is identical.
/// - The deadlines let a test shrink the launch watchdogs (the preparation
///   deadline and the unbound-spawn/resume ones, and the permission decision
///   wait, and the echo watchdog) from their generous production values so a
///   "never finished preparing" / "never came up" / "never decided" / "never
///   echoed" path can be exercised in seconds.
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// The program launched in each tmux session (`claude` by default). Used
    /// verbatim as the command's argv[0], so it may be a bare name resolved via
    /// `PATH` or an absolute path.
    pub claude_bin: String,
    /// How long an accepted session's background launch preparation (worktree
    /// build, trust seed, settings write, agent launch) may run before it is
    /// abandoned and the session failed. Defaults to [`LAUNCH_PREP_DEADLINE`];
    /// see that constant.
    pub launch_prep_deadline: Duration,
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
    /// How long a dispatched send may wait for its echo before the watchdog
    /// gives up on it. Defaults to [`ECHO_DEADLINE`]; see that constant.
    pub echo_deadline: Duration,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            claude_bin: DEFAULT_SESSION_COMMAND.to_owned(),
            launch_prep_deadline: LAUNCH_PREP_DEADLINE,
            pending_spawn_deadline: PENDING_SPAWN_DEADLINE,
            resume_ready_deadline: RESUME_READY_DEADLINE,
            permission_decision_deadline: PERMISSION_DECISION_DEADLINE,
            echo_deadline: ECHO_DEADLINE,
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
        assert_eq!(config.launch_prep_deadline, LAUNCH_PREP_DEADLINE);
        assert_eq!(config.pending_spawn_deadline, PENDING_SPAWN_DEADLINE);
        assert_eq!(config.resume_ready_deadline, RESUME_READY_DEADLINE);
        assert_eq!(
            config.permission_decision_deadline,
            PERMISSION_DECISION_DEADLINE
        );
        assert_eq!(config.echo_deadline, ECHO_DEADLINE);
    }
}
