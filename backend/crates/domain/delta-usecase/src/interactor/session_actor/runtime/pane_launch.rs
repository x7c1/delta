//! The tail of a pane-backed (Claude) launch.

#[cfg(doc)]
use super::LaunchingSpawn;

/// The tail of a pane-backed (Claude) launch.
#[derive(Debug, Clone)]
pub struct PaneLaunch {
    /// The pane keystrokes will be sent to (`<token>:0.0`) once it exists.
    pub pane: String,
    /// Whether Claude Code's workspace-trust dialog must be pre-accepted for
    /// [`LaunchingSpawn::workdir`] before launching there.
    pub seed_trust: bool,
    /// The full argv the launch runs (`claude --settings … --session-id …`,
    /// the user's launch options, then any first prompt).
    pub command: Vec<String>,
}
