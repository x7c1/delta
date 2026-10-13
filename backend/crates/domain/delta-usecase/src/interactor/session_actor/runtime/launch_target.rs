//! The provider-specific tail of a launch.

use super::{AdapterLaunch, PaneLaunch};

/// The provider-specific tail of a launch: everything the background task still
/// has to do once the (shared) worktree build is done — and the only thing in
/// the launch machinery that branches on the provider.
#[derive(Debug, Clone)]
pub enum LaunchTarget {
    /// Claude: a tmux pane running `claude`, bound by the first hook it fires.
    Pane(PaneLaunch),
    /// An adapter-backed provider (Codex): a `connect` + `thread/start` over
    /// the provider's adapter, bound by the checkpoint the task posts back.
    Adapter(AdapterLaunch),
}
