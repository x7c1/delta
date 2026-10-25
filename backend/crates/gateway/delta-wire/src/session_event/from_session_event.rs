//! The conversion from the domain [`SessionEvent`] to its wire form.

use super::{SessionEvent, WireSessionEvent};
use crate::file_change_detail::WireFileChangeDetail;
use crate::status_snapshot::WireStatusSnapshot;

impl From<SessionEvent> for WireSessionEvent {
    fn from(event: SessionEvent) -> Self {
        match event {
            SessionEvent::SessionRegistered { session_id } => Self::SessionRegistered {
                session_id: session_id.0,
            },
            SessionEvent::SessionOpened { session_id } => Self::SessionOpened {
                session_id: session_id.0,
            },
            SessionEvent::SessionClosed { session_id } => Self::SessionClosed {
                session_id: session_id.0,
            },
            SessionEvent::SessionRemoved { session_id } => Self::SessionRemoved {
                session_id: session_id.0,
            },
            SessionEvent::SendDispatched {
                session_id,
                send_id,
            } => Self::SendDispatched {
                session_id: session_id.0,
                send_id,
            },
            SessionEvent::SendParked {
                session_id,
                send_id,
                text,
            } => Self::SendParked {
                session_id: session_id.0,
                send_id,
                text,
            },
            SessionEvent::TurnStarted {
                session_id,
                send_id,
                thread_id,
                matched_uuid,
            } => Self::TurnStarted {
                session_id: session_id.0,
                send_id,
                thread_id: thread_id.0,
                matched_uuid: matched_uuid.map(|uuid| uuid.0),
            },
            SessionEvent::ExternalInput { session_id, prompt } => Self::ExternalInput {
                session_id: session_id.0,
                prompt,
            },
            SessionEvent::TurnCompleted {
                session_id,
                thread_id,
                stop_reason,
            } => Self::TurnCompleted {
                session_id: session_id.0,
                thread_id: thread_id.map(|id| id.0),
                stop_reason,
            },
            SessionEvent::TurnInterrupted {
                session_id,
                thread_id,
            } => Self::TurnInterrupted {
                session_id: session_id.0,
                thread_id: thread_id.map(|id| id.0),
            },
            SessionEvent::TranscriptUpdated {
                session_id,
                thread_ids,
            } => Self::TranscriptUpdated {
                session_id: session_id.0,
                thread_ids: thread_ids.into_iter().map(|id| id.0).collect(),
            },
            SessionEvent::PermissionRequested {
                session_id,
                request_id,
                tool_name,
                tool_input_json,
                file_change,
                grant_root,
            } => Self::PermissionRequested {
                session_id: session_id.0,
                request_id,
                tool_name,
                tool_input: tool_input_json,
                file_change: file_change.as_ref().map(WireFileChangeDetail::from),
                grant_root,
            },
            SessionEvent::QuestionAsked {
                session_id,
                request_id,
                thread_id,
                tool_input_json,
            } => Self::QuestionAsked {
                session_id: session_id.0,
                request_id,
                thread_id: thread_id.0,
                tool_input: tool_input_json,
            },
            SessionEvent::PermissionResolved {
                session_id,
                request_id,
            } => Self::PermissionResolved {
                session_id: session_id.0,
                request_id,
            },
            SessionEvent::SpawnPaneReady {
                session_id,
                pane_token,
            } => Self::SpawnPaneReady {
                session_id: session_id.0,
                pane_token,
            },
            SessionEvent::SpawnFailed {
                session_id,
                pane_token,
                reason,
                cancelled,
            } => Self::SpawnFailed {
                session_id: session_id.0,
                pane_token,
                reason,
                cancelled,
            },
            SessionEvent::AssistantStreaming {
                session_id,
                thread_id,
                message_id,
                index,
                final_,
                delta,
            } => Self::AssistantStreaming {
                session_id: session_id.0,
                thread_id: thread_id.0,
                message_id,
                index,
                is_final: final_,
                delta,
            },
            SessionEvent::SubagentStarted {
                session_id,
                thread_id,
                tool_use_id,
                subagent_type,
                description,
                background,
            } => Self::SubagentStarted {
                session_id: session_id.0,
                thread_id: thread_id.0,
                tool_use_id,
                subagent_type,
                description,
                background,
            },
            SessionEvent::SubagentFinished {
                session_id,
                tool_use_id,
            } => Self::SubagentFinished {
                session_id: session_id.0,
                tool_use_id,
            },
            SessionEvent::StatusUpdated {
                session_id,
                snapshot,
            } => Self::StatusUpdated {
                session_id: session_id.0,
                snapshot: WireStatusSnapshot::from(snapshot),
            },
            SessionEvent::RepositoryCloneCompleted {
                repo_owner,
                repo_name,
                clone_root,
                destination_path,
            } => Self::RepositoryCloneCompleted {
                repo_owner,
                repo_name,
                clone_root,
                destination_path,
            },
            SessionEvent::RepositoryCloneFailed {
                repo_owner,
                repo_name,
                clone_root,
                destination_path,
                message,
            } => Self::RepositoryCloneFailed {
                repo_owner,
                repo_name,
                clone_root,
                destination_path,
                message,
            },
        }
    }
}
