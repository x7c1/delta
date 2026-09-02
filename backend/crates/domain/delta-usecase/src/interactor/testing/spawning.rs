//! The default [`SpawningSession`] fixture the use-case tests spread from.

use delta_model::{AgentProvider, SessionId};

use crate::ports::SpawningSession;

/// A `spawning` insert with no launch context: a Claude session started in
/// `cwd`, with no git snapshot, no user-selected workdir and no originating
/// pull request. Tests name only the fields their assertion depends on:
///
/// ```ignore
/// SpawningSession {
///     repository_display_name: Some("x7c1/delta"),
///     ..spawning_session(&id, "/work")
/// }
/// ```
pub(crate) fn spawning_session<'a>(id: &'a SessionId, cwd: &'a str) -> SpawningSession<'a> {
    SpawningSession {
        id,
        cwd,
        branch_at_launch: None,
        repo_root: None,
        requested_workdir: None,
        repository_display_name: None,
        provider: AgentProvider::Claude,
        pull_request_number: None,
    }
}
