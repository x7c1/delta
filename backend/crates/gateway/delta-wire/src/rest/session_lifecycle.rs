//! JSON shape of the tmux/process lifecycle after an eager session spawn.

use delta_usecase::SessionLifecycle;
use serde::Serialize;
use ts_rs::TS;

/// JSON shape of the tmux/process lifecycle after an eager session spawn.
///
/// Mirrors the use-case [`SessionLifecycle`] variant-for-variant; see that
/// type for the semantics. This wire twin carries the serialization concerns
/// the domain must not know about: the snake_case variant names and the
/// TypeScript export. `ready` — the session already existed and was reused.
/// `starting` — the session was just created and may still be coming up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "SessionLifecycle")]
pub enum WireSessionLifecycle {
    Ready,
    Starting,
}

impl From<SessionLifecycle> for WireSessionLifecycle {
    fn from(lifecycle: SessionLifecycle) -> Self {
        match lifecycle {
            SessionLifecycle::Ready => WireSessionLifecycle::Ready,
            SessionLifecycle::Starting => WireSessionLifecycle::Starting,
        }
    }
}
