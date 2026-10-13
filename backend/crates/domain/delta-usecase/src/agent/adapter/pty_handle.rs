//! A handle to an attachable terminal surface (Claude's tmux pane).

/// A handle to an attachable terminal surface (Claude's tmux pane).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyHandle {
    /// The adapter-local terminal target (Claude's fully-qualified pane, e.g.
    /// `delta-1:0.0`).
    pub target: String,
}
