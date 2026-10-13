//! The tail of an adapter-backed (Codex) launch.

use delta_model::{AgentProvider, ThreadId};

use crate::agent::LaunchOptionSpec;

/// The tail of an adapter-backed (Codex) launch.
///
/// It carries what the connect/`thread/start` needs (the provider and the
/// user's launch options) plus what the checkpoint handler needs to finish the
/// session off on the actor: the `main` thread the eager row created, and the
/// first prompt's already-written `queued` send row.
#[derive(Debug, Clone)]
pub struct AdapterLaunch {
    /// Which adapter-backed provider to resolve through the factory registry.
    pub provider: AgentProvider,
    /// The user-selected launch options, already resolved to neutral
    /// `(name, value?)` pairs by the accept phase. Rendered for the provider by
    /// the adapter, never here.
    pub launch_options: Vec<LaunchOptionSpec>,
    /// The eager row's `main` thread, which the content source is folded onto
    /// and the first prompt's turn dispatches against.
    pub main_thread_id: ThreadId,
    /// The first prompt's `send` row, written `queued` by the accept phase.
    /// `None` for a prompt-less spawn. Nothing has received it until the
    /// adapter's `turn/start`, so it is promoted and dispatched by the
    /// checkpoint handler.
    pub first_send_id: Option<i64>,
}
