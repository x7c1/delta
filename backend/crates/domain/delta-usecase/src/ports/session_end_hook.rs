//! Payload of a `SessionEnd` hook.

use delta_model::SessionId;

/// Payload of a `SessionEnd` hook.
///
/// Claude Code fires `SessionEnd` when a session terminates (the process exits,
/// the user clears it, etc.). Delta uses it as a precise early failure signal:
/// a launch that ends while its spawn is still unbound never registered, so it
/// is a failed launch the watchdog would otherwise only catch at the deadline.
#[derive(Debug, Clone)]
pub struct SessionEndHook {
    /// The Claude `session_id` the session ran under. For a Delta spawn this is
    /// the id Delta pinned via `--session-id`, so it matches the spawn's
    /// binding key.
    pub session_id: SessionId,
    /// Why the session ended, as reported by Claude Code (e.g. `exit`, `clear`,
    /// `logout`). Carried for observability only; it does not change handling.
    pub reason: Option<String>,
}
