//! The permission reducer: how a provider-neutral permission [`AgentEvent`]
//! moves a session's runtime state and what the browser is told.
//!
//! Permission is the first live runtime path routed through the neutral
//! [`AgentEvent`] stream. The three sites that observe a permission fact — the
//! `PermissionRequest` hook ([`on_permission_request`]), the browser decision
//! ([`decide_permission`]), and the correlated `tool_result` ingested by the
//! transcript sync ([`sync_transcript`]) — each construct the neutral event and
//! funnel it through [`reduce_permission_event`]. This reducer is the single
//! authority for the *core-loop* effect of that event: it mutates the queryable
//! runtime mirror (`pending_permission`) and produces the [`SessionEvent`]
//! broadcast that clears or raises the browser notice.
//!
//! Everything provider-specific stays out of the event and at the call site,
//! where it belongs (the plan's "projection owns correlation"): the permission
//! row I/O, the `tool_use_id` → row correlation the sync performs, the oneshot
//! that unblocks the blocked hook handler, and the request-id → session index
//! the routing layer keeps. By the time an event reaches this reducer it is
//! already clean — a resolved `request_id` and a decision, nothing Claude-shaped.
//!
//! [`on_permission_request`]: super::hooks
//! [`decide_permission`]: super::permission_decision
//! [`sync_transcript`]: super::sync

use delta_model::SessionId;

use crate::agent::AgentEvent;
use crate::interactor::session_actor::runtime::{PendingPermission, SessionRuntime};
use crate::ports::SessionEvent;

/// Apply a provider-neutral permission [`AgentEvent`] to `state`, returning the
/// [`SessionEvent`]s the caller broadcasts.
///
/// Only the two permission variants carry meaning here; every other
/// [`AgentEvent`] is a no-op (returns no events), so this is safe to call on any
/// event as the runtime is progressively routed through the neutral stream.
///
/// - [`AgentEvent::PermissionRequested`] raises the pending dialog mirror and
///   emits [`SessionEvent::PermissionRequested`] — the notice the browser shows
///   Allow/Deny next to.
/// - [`AgentEvent::PermissionResolved`] clears the mirror (both the permission
///   dialog and any question sharing the same disjoint row-id space, exactly as
///   the transcript sync has always cleared both together) and emits
///   [`SessionEvent::PermissionResolved`] — the signal that settles the notice.
pub(in crate::interactor) fn reduce_permission_event(
    state: &mut SessionRuntime,
    session_id: &SessionId,
    event: &AgentEvent,
) -> Vec<SessionEvent> {
    match event {
        AgentEvent::PermissionRequested { request } => {
            let request_id = parse_request_id(&request.request_id);
            // The neutral event carries the tool input as structured JSON; the
            // runtime mirror and the browser notice speak JSON text. Delta's
            // hook boundary already normalises the payload through
            // `serde_json::Value`, so re-serialising here reproduces the exact
            // text the notice showed before permission was routed through the
            // event.
            let tool_input_json = request.input_json.to_string();
            state.set_pending_permission(PendingPermission {
                request_id,
                tool_name: request.tool_name.clone(),
                tool_input_json: tool_input_json.clone(),
            });
            vec![SessionEvent::PermissionRequested {
                session_id: session_id.clone(),
                request_id,
                tool_name: request.tool_name.clone(),
                tool_input_json,
            }]
        }
        AgentEvent::PermissionResolved { request_id, .. } => {
            let request_id = parse_request_id(request_id);
            // Keep the queryable mirror in step with the broadcast. Clearing the
            // question mirror too is safe and matches the sync path: a question
            // row id and a permission row id are disjoint, so a permission
            // resolution can never wipe a live question's state.
            state.resolve_pending_permission(request_id);
            state.resolve_pending_question(request_id);
            vec![SessionEvent::PermissionResolved {
                session_id: session_id.clone(),
                request_id,
            }]
        }
        _ => Vec::new(),
    }
}

/// Resolve the neutral, opaque `request_id` string back to the row id the
/// runtime mirror and [`SessionEvent`] use.
///
/// In v1 the only provider is Claude, whose permission requests are keyed by
/// Delta's own `permission_request` row id: the call sites stringify that `i64`
/// into the neutral event, so parsing it back here is total. A structured
/// provider whose ids are not `i64` row ids would make this a real modelling
/// seam (the runtime mirror and wire event are `i64` today); that is deferred
/// with the rest of the provider-neutral id work and is out of this slice's
/// scope. The `expect` documents the invariant rather than hiding a silent drop.
fn parse_request_id(request_id: &str) -> i64 {
    request_id.parse::<i64>().unwrap_or_else(|_| {
        panic!("permission request_id must be a Delta row id in v1, got {request_id:?}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentPermissionRequest;
    use crate::interactor::session_actor::runtime::{PendingPermission, PendingQuestion};
    use crate::interactor::PermissionDecision;
    use delta_model::ThreadId;

    fn requested(request_id: &str, tool_name: &str, input: serde_json::Value) -> AgentEvent {
        AgentEvent::PermissionRequested {
            request: AgentPermissionRequest {
                request_id: request_id.to_owned(),
                tool_name: tool_name.to_owned(),
                input_json: input,
                tool_use_id: None,
            },
        }
    }

    #[test]
    fn requested_raises_the_mirror_and_emits_the_notice() {
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        let event = requested("7", "Bash", serde_json::json!({ "command": "rm -i x" }));

        let events = reduce_permission_event(&mut state, &session, &event);

        assert_eq!(
            state.live_state().pending_permission,
            Some(PendingPermission {
                request_id: 7,
                tool_name: "Bash".to_owned(),
                tool_input_json: r#"{"command":"rm -i x"}"#.to_owned(),
            }),
            "the dialog is mirrored for the sends envelope"
        );
        assert_eq!(
            events,
            vec![SessionEvent::PermissionRequested {
                session_id: session,
                request_id: 7,
                tool_name: "Bash".to_owned(),
                tool_input_json: r#"{"command":"rm -i x"}"#.to_owned(),
            }]
        );
    }

    #[test]
    fn resolved_clears_the_mirror_and_emits_the_settle() {
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        reduce_permission_event(
            &mut state,
            &session,
            &requested("7", "Bash", serde_json::json!({ "command": "ls" })),
        );

        let events = reduce_permission_event(
            &mut state,
            &session,
            &AgentEvent::PermissionResolved {
                request_id: "7".to_owned(),
                decision: PermissionDecision::Allow,
            },
        );

        assert_eq!(
            state.live_state().pending_permission,
            None,
            "a resolved dialog is no longer mirrored"
        );
        assert_eq!(
            events,
            vec![SessionEvent::PermissionResolved {
                session_id: session,
                request_id: 7,
            }]
        );
    }

    #[test]
    fn resolving_a_disjoint_id_never_wipes_a_live_question() {
        // A question row id and a permission row id are disjoint, so resolving a
        // permission must leave a live question's mirror untouched.
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        let question = PendingQuestion {
            request_id: 42,
            thread_id: ThreadId(1),
            tool_input_json: r#"{"questions":[]}"#.to_owned(),
        };
        state.set_pending_question(question.clone());

        reduce_permission_event(
            &mut state,
            &session,
            &AgentEvent::PermissionResolved {
                request_id: "7".to_owned(),
                decision: PermissionDecision::Deny,
            },
        );

        assert_eq!(
            state.pending_question(),
            Some(&question),
            "a permission resolution keys off its own id, not the question's"
        );
    }

    #[test]
    fn non_permission_events_are_ignored() {
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        let events = reduce_permission_event(
            &mut state,
            &session,
            &AgentEvent::TurnStarted {
                provider_turn_id: None,
            },
        );
        assert!(events.is_empty());
        assert_eq!(state.live_state().pending_permission, None);
    }

    #[test]
    fn a_resolution_uses_the_decision_faithfully_but_the_event_carries_only_the_id() {
        // The settle broadcast carries no allow/deny (the browser already has
        // it); Deny and Allow produce the same SessionEvent shape.
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        let deny = reduce_permission_event(
            &mut state,
            &session,
            &AgentEvent::PermissionResolved {
                request_id: "9".to_owned(),
                decision: PermissionDecision::Deny,
            },
        );
        assert_eq!(
            deny,
            vec![SessionEvent::PermissionResolved {
                session_id: session,
                request_id: 9,
            }]
        );
    }

    #[test]
    #[should_panic(expected = "must be a Delta row id")]
    fn a_non_row_id_is_an_invariant_violation() {
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        reduce_permission_event(
            &mut state,
            &session,
            &AgentEvent::PermissionResolved {
                request_id: "thr_not_an_i64".to_owned(),
                decision: PermissionDecision::Allow,
            },
        );
    }
}
