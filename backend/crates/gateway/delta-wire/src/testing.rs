//! Domain fixtures shared by the wire tests.
//!
//! Each builder returns a record with fixed values, so a test spells out only
//! the fields it varies (with struct update syntax or by assigning to the
//! returned value) and a field added to a domain type is filled in here once.

use delta_model::{AgentProvider, Session, SessionId, SessionStatus, Thread, ThreadId};
use delta_usecase::{SessionListing, TurnState};

/// The id every fixture record belongs to.
const SESSION_ID: &str = "sess-1";

/// The timestamp every fixture record was created at.
const CREATED_AT: &str = "2026-01-01T00:00:00Z";

/// An active Claude session `sess-1` in `/work`, created at [`CREATED_AT`],
/// with every optional field unset.
pub(crate) fn session() -> Session {
    Session {
        id: SessionId::from(SESSION_ID),
        cwd: "/work".into(),
        transcript_path: None,
        title: None,
        status: SessionStatus::Active,
        created_at: CREATED_AT.into(),
        branch_at_launch: None,
        repo_root: None,
        requested_workdir: None,
        repository_display_name: None,
        provider: AgentProvider::Claude,
        provider_session_id: None,
        provider_thread_id: None,
        pull_request_number: None,
        failure_reason: None,
        closed_reason: None,
    }
}

/// The trunk thread of [`session`]: id 1, titled `main`, created at
/// [`CREATED_AT`], with no parent, root message or activity yet.
pub(crate) fn main_thread() -> Thread {
    Thread {
        id: ThreadId(1),
        session_id: SessionId::from(SESSION_ID),
        title: "main".into(),
        parent_thread_id: None,
        root_message_uuid: None,
        created_at: CREATED_AT.into(),
        last_activity_at: None,
    }
}

/// [`session`] as an open, idle list row whose only thread is
/// [`main_thread`], with no activity and no running subagents.
pub(crate) fn listing() -> SessionListing {
    SessionListing {
        session: session(),
        open: true,
        pane_starting: false,
        hooks_unreachable: false,
        main_thread_id: ThreadId(1),
        threads: vec![main_thread()],
        last_activity_at: None,
        turn: TurnState::Idle,
        in_progress_thread: None,
        running_subagents: Vec::new(),
    }
}
