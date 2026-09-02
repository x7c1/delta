//! [`SqliteStore`] tests, split along the same aggregate lines as the
//! implementation, plus `schema` for migrations and the startup gate.

mod clone_roots;
mod launch_options;
mod messages;
mod permissions;
mod prompt_templates;
mod schema;
mod sends;
mod sessions;
mod subagents;
mod threads;

use delta_model::{AgentProvider, SessionId};
use delta_usecase::{NewSession, SpawningSession};

fn new_session() -> NewSession {
    NewSession {
        id: "sess-1".into(),
        cwd: "/work".into(),
        transcript_path: "/tmp/t.jsonl".into(),
        branch_at_launch: None,
        repo_root: None,
        repository_display_name: None,
    }
}

fn new_session_with(id: &str) -> NewSession {
    NewSession {
        id: id.into(),
        cwd: "/work".into(),
        transcript_path: format!("/tmp/{id}.jsonl"),
        branch_at_launch: None,
        repo_root: None,
        repository_display_name: None,
    }
}

/// A `spawning` insert with no launch context: a Claude session started in
/// `cwd`, with no git snapshot, no user-selected workdir and no originating
/// pull request. Tests name only the fields their assertion depends on.
fn spawning_session<'a>(id: &'a SessionId, cwd: &'a str) -> SpawningSession<'a> {
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
