//! Builders for the hooks the interactor tests fire.

use delta_model::SessionId;

use crate::ports::{SessionStartHook, UserPromptSubmitHook};

/// A `SessionStart` hook for an explicit session id and source.
///
/// The `cwd`/`transcript_path` mirror the `submit` builder so a `startup` bind
/// registers the same session row a `UserPromptSubmit` would.
pub(crate) fn session_start(session_id: &str, source: &str) -> SessionStartHook {
    SessionStartHook {
        session_id: SessionId::from(session_id),
        source: source.into(),
        cwd: "/work".into(),
        transcript_path: "/tmp/t.jsonl".into(),
    }
}

pub(crate) fn submit(text: &str) -> UserPromptSubmitHook {
    UserPromptSubmitHook {
        prompt: text.into(),
        session_id: SessionId::from("sess-1"),
        transcript_path: "/tmp/t.jsonl".into(),
        cwd: "/work".into(),
    }
}

/// A submit hook for an explicit session id and transcript path, for the
/// multi-session routing tests.
pub(crate) fn submit_for(
    session_id: &str,
    transcript_path: &str,
    text: &str,
) -> UserPromptSubmitHook {
    UserPromptSubmitHook {
        prompt: text.into(),
        session_id: SessionId::from(session_id),
        transcript_path: transcript_path.into(),
        cwd: "/work".into(),
    }
}

/// A submit hook for an explicit cwd. The cwd no longer drives spawn binding
/// (that is keyed by the Delta-minted session id), so this is used both for
/// external-claude registration tests and for binding tests that pass a spawn's
/// minted session id while exercising an arbitrary cwd.
pub(crate) fn submit_in(
    session_id: &str,
    transcript_path: &str,
    cwd: &str,
    text: &str,
) -> UserPromptSubmitHook {
    UserPromptSubmitHook {
        prompt: text.into(),
        session_id: SessionId::from(session_id),
        transcript_path: transcript_path.into(),
        cwd: cwd.into(),
    }
}
