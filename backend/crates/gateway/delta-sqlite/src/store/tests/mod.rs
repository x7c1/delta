//! [`SqliteStore`] tests, split along the same aggregate lines as the
//! implementation, plus `schema` for migrations and the startup gate.

mod clone_roots;
mod launch_options;
mod messages;
mod permissions;
mod schema;
mod sends;
mod sessions;
mod subagents;
mod threads;

use delta_usecase::NewSession;

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
