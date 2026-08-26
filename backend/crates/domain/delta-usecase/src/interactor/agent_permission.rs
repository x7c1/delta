//! The permission reducer: how a provider-neutral permission [`AgentEvent`]
//! moves a session's runtime state and what the browser is told.
//!
//! Permission is the first live runtime path routed through the neutral
//! [`AgentEvent`] stream. Every site that observes a permission fact — the
//! `PermissionRequest` hook ([`on_permission_request`]), the adapter event pump
//! ([`on_agent_event`]), the browser decision ([`decide_permission`]), and the
//! correlated `tool_result` ingested by the transcript sync ([`sync_transcript`])
//! — each construct the neutral event and funnel it through
//! [`reduce_permission_event`]. This reducer is the single authority for the
//! *core-loop* effect of that event: it mutates the queryable runtime mirror
//! (`pending_permissions`, an ordered queue) and produces the [`SessionEvent`]
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
//! [`on_agent_event`]: super::agent_event
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
/// - [`AgentEvent::PermissionRequested`] appends to the pending dialog queue and
///   emits [`SessionEvent::PermissionRequested`] — the notice the browser shows
///   Allow/Deny next to. A request arriving while others are pending queues
///   *behind* them: the head stays put, so the dialog the user is looking at is
///   never swapped out from under them.
/// - [`AgentEvent::PermissionResolved`] removes that request from the queue (and
///   clears any question sharing the same disjoint row-id space, exactly as the
///   transcript sync has always cleared both together) and emits
///   [`SessionEvent::PermissionResolved`] — the signal that settles the notice.
///   When the resolution retires the *head* and the queue still holds requests,
///   the promoted head is re-broadcast as a second
///   [`SessionEvent::PermissionRequested`], so a client that only follows events
///   always has a dialog on screen while requests are pending.
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
            let pending = PendingPermission {
                request_id,
                tool_name: request.tool_name.clone(),
                tool_input_json,
                file_change: request.file_change.clone(),
                grant_root: request.grant_root.clone(),
            };
            state.enqueue_pending_permission(pending.clone());
            vec![permission_requested_event(session_id, &pending)]
        }
        AgentEvent::PermissionResolved { request_id, .. } => {
            let request_id = parse_request_id(request_id);
            // Keep the queryable mirror in step with the broadcast. Clearing the
            // question mirror too is safe and matches the sync path: a question
            // row id and a permission row id are disjoint, so a permission
            // resolution can never wipe a live question's state.
            let promoted = state.resolve_pending_permission(request_id);
            state.resolve_pending_question(request_id);
            let mut events = vec![SessionEvent::PermissionResolved {
                session_id: session_id.clone(),
                request_id,
            }];
            // The answered dialog was the head and more are queued behind it:
            // raise the next one right away. Without this the browser settles the
            // notice it just answered and shows nothing until the user refetches,
            // which reads as "the agent stopped responding" while the provider is
            // in fact still waiting on the queued approvals.
            if let Some(head) = promoted {
                events.push(permission_requested_event(session_id, &head));
            }
            events
        }
        _ => Vec::new(),
    }
}

/// The browser notice for one pending dialog. Shared by the initial raise, the
/// promotion of a new head, and the decision path's defensive promotion, so
/// every client-visible "this dialog needs an answer" signal has one shape.
pub(in crate::interactor) fn permission_requested_event(
    session_id: &SessionId,
    pending: &PendingPermission,
) -> SessionEvent {
    SessionEvent::PermissionRequested {
        session_id: session_id.clone(),
        request_id: pending.request_id,
        tool_name: pending.tool_name.clone(),
        tool_input_json: pending.tool_input_json.clone(),
        file_change: pending.file_change.clone(),
        grant_root: pending.grant_root.clone(),
    }
}

/// Resolve the neutral, opaque `request_id` string back to the row id the
/// runtime mirror and [`SessionEvent`] use.
///
/// Every call site stringifies a Delta `permission_request` row id into the
/// neutral event, so parsing it back here is total. A pane-backed provider
/// (Claude) is keyed by that row id to begin with; an adapter-backed one (Codex)
/// has its own opaque approval token, and the event pump allocates the row and
/// re-expresses the fact under the row id *before* reducing it (see
/// `agent_event::request_agent_permission`), so the provider token never reaches
/// this function. The panic documents that invariant rather than hiding a silent
/// drop.
fn parse_request_id(request_id: &str) -> i64 {
    request_id.parse::<i64>().unwrap_or_else(|_| {
        panic!("permission request_id must be a Delta row id in v1, got {request_id:?}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{
        AgentFileChange, AgentFileChangeDetail, AgentFileChangeKind, AgentPermissionRequest,
    };
    use crate::interactor::session_actor::runtime::{PendingPermission, PendingQuestion};
    use crate::interactor::PermissionDecision;
    use crate::turn::TurnInput;
    use delta_model::ThreadId;

    fn requested(request_id: &str, tool_name: &str, input: serde_json::Value) -> AgentEvent {
        requested_with(request_id, tool_name, input, None, None)
    }

    fn requested_with(
        request_id: &str,
        tool_name: &str,
        input: serde_json::Value,
        file_change: Option<AgentFileChangeDetail>,
        grant_root: Option<String>,
    ) -> AgentEvent {
        AgentEvent::PermissionRequested {
            request: AgentPermissionRequest {
                request_id: request_id.to_owned(),
                tool_name: tool_name.to_owned(),
                input_json: input,
                tool_use_id: None,
                file_change,
                grant_root,
            },
        }
    }

    fn resolved(request_id: &str) -> AgentEvent {
        AgentEvent::PermissionResolved {
            request_id: request_id.to_owned(),
            decision: PermissionDecision::Allow,
        }
    }

    /// The request ids of the pending queue, oldest first.
    fn queue(state: &SessionRuntime) -> Vec<i64> {
        state
            .live_state()
            .pending_permissions
            .iter()
            .map(|p| p.request_id)
            .collect()
    }

    #[test]
    fn requested_raises_the_mirror_and_emits_the_notice() {
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        let event = requested("7", "Bash", serde_json::json!({ "command": "rm -i x" }));

        let events = reduce_permission_event(&mut state, &session, &event);

        assert_eq!(
            state.live_state().pending_permission().cloned(),
            Some(PendingPermission {
                request_id: 7,
                tool_name: "Bash".to_owned(),
                tool_input_json: r#"{"command":"rm -i x"}"#.to_owned(),
                file_change: None,
                grant_root: None,
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
                file_change: None,
                grant_root: None,
            }]
        );
    }

    /// A provider that states what its request would do to files carries that
    /// detail all the way through: onto the queryable mirror (which re-seeds a
    /// reconnecting browser) and onto the notice broadcast. The reducer is a
    /// pass-through here — it neither invents a detail nor drops one — which is
    /// what keeps the two surfaces showing the same card.
    #[test]
    fn a_file_change_detail_reaches_both_the_mirror_and_the_notice() {
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        let detail = AgentFileChangeDetail {
            changes: vec![AgentFileChange {
                path: "src/lib.rs".to_owned(),
                kind: Some(AgentFileChangeKind::Update),
                diff: "@@ -1 +1 @@".to_owned(),
            }],
            reason: Some("write access".to_owned()),
        };
        let event = requested_with(
            "9",
            "file_change",
            serde_json::json!({ "itemId": "fc_1" }),
            Some(detail.clone()),
            None,
        );

        let events = reduce_permission_event(&mut state, &session, &event);

        assert_eq!(
            state
                .live_state()
                .pending_permission()
                .and_then(|pending| pending.file_change.clone()),
            Some(detail.clone()),
            "the envelope's re-seed carries the detail"
        );
        assert_eq!(
            events,
            vec![SessionEvent::PermissionRequested {
                session_id: session,
                request_id: 9,
                tool_name: "file_change".to_owned(),
                tool_input_json: r#"{"itemId":"fc_1"}"#.to_owned(),
                file_change: Some(detail),
                grant_root: None,
            }]
        );
    }

    /// A request that also asks for a write root carries that to both surfaces
    /// **without** a detail to ride on. This is the case the field's placement
    /// exists for: the change set could not be correlated, so the card has only
    /// the input summary — and the broadest thing the dialog grants must still
    /// reach it, or the user allows a whole tree while reading about one item.
    #[test]
    fn a_grant_root_reaches_both_surfaces_without_a_file_change_detail() {
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        let event = requested_with(
            "11",
            "file_change",
            serde_json::json!({ "itemId": "fc_unknown" }),
            None,
            Some("/repo".to_owned()),
        );

        let events = reduce_permission_event(&mut state, &session, &event);

        assert_eq!(
            state
                .live_state()
                .pending_permission()
                .and_then(|pending| pending.grant_root.clone()),
            Some("/repo".to_owned()),
            "the envelope's re-seed states the root too"
        );
        assert_eq!(
            events,
            vec![SessionEvent::PermissionRequested {
                session_id: session,
                request_id: 11,
                tool_name: "file_change".to_owned(),
                tool_input_json: r#"{"itemId":"fc_unknown"}"#.to_owned(),
                file_change: None,
                grant_root: Some("/repo".to_owned()),
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
            state.live_state().pending_permission(),
            None,
            "a resolved dialog is no longer mirrored"
        );
        assert_eq!(
            events,
            vec![SessionEvent::PermissionResolved {
                session_id: session,
                request_id: 7,
            }],
            "an empty queue produces the settle alone — nothing to promote"
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
        assert_eq!(state.live_state().pending_permission(), None);
    }

    #[test]
    fn parallel_requests_queue_in_arrival_order_and_never_overwrite() {
        // The field failure: a Codex turn fanned out 12 escalated tool calls, all
        // 12 approvals arrived before any answer, and a single slot kept only the
        // last — so 11 requests were unanswerable and the turn never finished.
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");

        let mut raised = Vec::new();
        for id in 1..=3 {
            raised.extend(reduce_permission_event(
                &mut state,
                &session,
                &requested(
                    &id.to_string(),
                    "Bash",
                    serde_json::json!({ "command": format!("cat {id}") }),
                ),
            ));
        }

        assert_eq!(queue(&state), vec![1, 2, 3], "FIFO, nothing overwritten");
        assert_eq!(
            state
                .live_state()
                .pending_permission()
                .map(|p| p.request_id),
            Some(1),
            "the head stays the first request the user was shown"
        );
        assert_eq!(
            raised.len(),
            3,
            "every request is broadcast, so a client can build the same queue"
        );
    }

    #[test]
    fn resolving_the_head_promotes_the_next_and_raises_it() {
        // The no-dialog-less invariant: answering the visible dialog must surface
        // the next one without the browser refetching anything.
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        for id in 1..=3 {
            reduce_permission_event(
                &mut state,
                &session,
                &requested(&id.to_string(), "Bash", serde_json::json!({ "n": id })),
            );
        }

        let events = reduce_permission_event(&mut state, &session, &resolved("1"));

        assert_eq!(
            queue(&state),
            vec![2, 3],
            "the answered head left the queue"
        );
        assert_eq!(
            events,
            vec![
                SessionEvent::PermissionResolved {
                    session_id: session.clone(),
                    request_id: 1,
                },
                SessionEvent::PermissionRequested {
                    session_id: session,
                    request_id: 2,
                    tool_name: "Bash".to_owned(),
                    tool_input_json: r#"{"n":2}"#.to_owned(),
                    file_change: None,
                    grant_root: None,
                },
            ],
            "the settle is followed by the promoted head's own notice"
        );
    }

    #[test]
    fn resolving_a_non_head_request_leaves_the_visible_dialog_alone() {
        // A decision can name any pending row (the endpoint is keyed by row id,
        // not by queue position), so a middle entry can resolve first — e.g. the
        // provider withdrew it. Only that entry leaves.
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        for id in 1..=3 {
            reduce_permission_event(
                &mut state,
                &session,
                &requested(&id.to_string(), "Bash", serde_json::json!({ "n": id })),
            );
        }

        let events = reduce_permission_event(&mut state, &session, &resolved("2"));

        assert_eq!(queue(&state), vec![1, 3], "only the named entry left");
        assert_eq!(
            events,
            vec![SessionEvent::PermissionResolved {
                session_id: session,
                request_id: 2,
            }],
            "no promotion: the head never changed, so no new dialog is raised"
        );
    }

    #[test]
    fn a_repeated_request_for_the_same_row_keeps_its_queue_position() {
        // A retried hook or a duplicate provider frame must not queue the same
        // dialog twice (and must not push it behind newer ones).
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        reduce_permission_event(
            &mut state,
            &session,
            &requested("1", "Bash", serde_json::json!({ "n": 1 })),
        );
        reduce_permission_event(
            &mut state,
            &session,
            &requested("2", "Bash", serde_json::json!({ "n": 2 })),
        );
        reduce_permission_event(
            &mut state,
            &session,
            &requested("1", "Bash", serde_json::json!({ "n": 1 })),
        );

        assert_eq!(queue(&state), vec![1, 2], "de-duplicated, order preserved");
    }

    #[test]
    fn resolving_an_unknown_id_changes_nothing() {
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        reduce_permission_event(
            &mut state,
            &session,
            &requested("1", "Bash", serde_json::json!({ "n": 1 })),
        );

        let events = reduce_permission_event(&mut state, &session, &resolved("99"));

        assert_eq!(queue(&state), vec![1], "the live dialog is untouched");
        assert_eq!(
            events,
            vec![SessionEvent::PermissionResolved {
                session_id: session,
                request_id: 99,
            }],
            "the settle still broadcasts (a client may hold a stale notice)"
        );
    }

    #[test]
    fn the_turn_returning_to_idle_clears_the_whole_queue() {
        // A dialog cannot outlive its turn — and that holds for all of them, not
        // just the head. By the time the turn ends the provider has settled or
        // abandoned its requests; Delta only drops the mirror.
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        state.apply_turn(TurnInput::Dispatch { send_id: 1 });
        state.apply_turn(TurnInput::PromptSubmitted { send_id: Some(1) });
        for id in 1..=3 {
            reduce_permission_event(
                &mut state,
                &session,
                &requested(&id.to_string(), "Bash", serde_json::json!({ "n": id })),
            );
        }

        state.apply_turn(TurnInput::Stop);

        assert!(queue(&state).is_empty(), "the queue went with the turn");
        assert!(
            state.is_empty(),
            "nothing pending pins the actor alive after the sweep"
        );
    }

    #[test]
    fn deleting_the_session_clears_the_whole_queue() {
        let mut state = SessionRuntime::default();
        let session = SessionId::from("sess-1");
        for id in 1..=2 {
            reduce_permission_event(
                &mut state,
                &session,
                &requested(&id.to_string(), "Bash", serde_json::json!({ "n": id })),
            );
        }

        state.forget_turn();

        assert!(queue(&state).is_empty(), "the queue went with the session");
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
