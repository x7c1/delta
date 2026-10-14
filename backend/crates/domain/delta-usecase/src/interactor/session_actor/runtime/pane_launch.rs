//! The tail of a pane-backed (Claude) launch.

#[cfg(doc)]
use super::LaunchingSpawn;

/// The tail of a pane-backed (Claude) launch.
#[derive(Debug, Clone)]
pub struct PaneLaunch {
    /// The pane keystrokes will be sent to (`<token>:0.0`) once it exists.
    pub pane: String,
    /// Whether [`LaunchingSpawn::workdir`] is a git working tree, and so a
    /// candidate for pre-accepting Claude Code's workspace-trust dialog. The
    /// launch task decides whether it actually is pre-accepted (see the
    /// launch-trust rule in the lifecycle module).
    pub seed_trust: bool,
    /// The full argv the launch runs (`claude --settings … --session-id …`,
    /// the user's launch options, then any first prompt).
    pub command: Vec<String>,
}
