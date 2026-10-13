//! Maps a provider-neutral [`AgentEvent`] onto the turn machine's input.

use crate::agent::{AgentEvent, TurnStatus};

use super::TurnInput;

/// Map a provider-neutral [`AgentEvent`] onto the [`TurnInput`] that drives the
/// turn state machine, for the unambiguous turn-end facts only.
///
/// This is the first, isolated step of routing the runtime through the neutral
/// [`AgentEvent`] stream: it proves the turn-end mapping in a pure function,
/// without changing how the live runtime feeds the FSM (hooks and transcript
/// ingestion still drive it exactly as before). Only the three terminal
/// [`AgentEvent::TurnCompleted`] statuses have a 1:1 turn-end equivalent today:
///
/// - [`TurnStatus::Completed`] → [`TurnInput::Stop`] — the honest turn-end
///   signal the `Stop` hook produces.
/// - [`TurnStatus::Interrupted`] → [`TurnInput::Interrupt`] — the user aborted
///   the in-flight turn (no `Stop` fires).
/// - [`TurnStatus::Failed`] → [`TurnInput::Stop`] — a turn that ended on an
///   error still genuinely ended, so it takes the same honest turn-end input
///   (and orphan-send disposition) as a normal completion.
///
/// Every other [`AgentEvent`] variant returns `None`: those facts are consumed
/// by later steps and carry no turn-end meaning here.
pub fn turn_input_for_agent_event(event: &AgentEvent) -> Option<TurnInput> {
    match event {
        AgentEvent::TurnCompleted { status } => Some(match status {
            TurnStatus::Completed | TurnStatus::Failed => TurnInput::Stop,
            TurnStatus::Interrupted => TurnInput::Interrupt,
        }),
        _ => None,
    }
}

/// FSM behaviour driven by synthetic, provider-neutral [`AgentEvent`] sequences.
///
/// These prove the neutral turn-end mapping in isolation: build an
/// [`AgentEvent`], map it with [`turn_input_for_agent_event`], apply the result
/// to the FSM, and assert the outcome equals feeding the equivalent
/// [`TurnInput`] directly. They deliberately use no provider-specific detail
/// (opaque ids are arbitrary placeholders).
#[cfg(test)]
mod agent_event_mapping_tests {
    use super::TurnStatus as St;
    use super::*;
    use crate::agent::{AgentPermissionRequest, SessionEndReason};
    use crate::interactor::PermissionDecision;
    use crate::turn::transition;
    use crate::turn::TurnState as S;
    use serde_json::json;

    /// The turn-end statuses map onto exactly the three turn-end inputs.
    #[test]
    fn turn_completed_statuses_map_to_the_turn_end_inputs() {
        assert_eq!(
            turn_input_for_agent_event(&AgentEvent::TurnCompleted {
                status: St::Completed
            }),
            Some(TurnInput::Stop),
        );
        assert_eq!(
            turn_input_for_agent_event(&AgentEvent::TurnCompleted {
                status: St::Interrupted
            }),
            Some(TurnInput::Interrupt),
        );
        // A failed turn still genuinely ended: same honest turn-end input as a
        // normal completion (mirrors the API-error abort path today).
        assert_eq!(
            turn_input_for_agent_event(&AgentEvent::TurnCompleted { status: St::Failed }),
            Some(TurnInput::Stop),
        );
    }

    /// Every non-turn-end variant maps to `None`: they carry no turn-end fact
    /// and are handled by later steps, not here.
    #[test]
    fn non_turn_end_events_map_to_none() {
        let others = [
            AgentEvent::SessionStarted {
                provider_session_id: "sid".to_owned(),
            },
            AgentEvent::SessionEnded {
                reason: SessionEndReason::Closed,
            },
            AgentEvent::SessionEnded {
                reason: SessionEndReason::ProcessExited,
            },
            AgentEvent::SessionEnded {
                reason: SessionEndReason::Failed,
            },
            AgentEvent::TurnStarted {
                provider_turn_id: None,
            },
            AgentEvent::TurnStarted {
                provider_turn_id: Some("turn".to_owned()),
            },
            AgentEvent::UserPromptAccepted {
                provider_message_id: None,
                text: "hi".to_owned(),
                at_ms: None,
            },
            AgentEvent::AssistantDelta {
                provider_item_id: "item".to_owned(),
                text: "frag".to_owned(),
            },
            AgentEvent::AssistantMessage {
                provider_item_id: "item".to_owned(),
                text: "msg".to_owned(),
                at_ms: None,
            },
            AgentEvent::ThinkingDelta {
                provider_item_id: "item".to_owned(),
                text: "frag".to_owned(),
            },
            AgentEvent::ThinkingMessage {
                provider_item_id: "item".to_owned(),
                text: "reasoning".to_owned(),
                at_ms: None,
            },
            AgentEvent::ToolStarted {
                provider_item_id: "item".to_owned(),
                name: "tool".to_owned(),
                input_json: json!({}),
                at_ms: None,
            },
            AgentEvent::ToolCompleted {
                provider_item_id: "item".to_owned(),
                output_json: json!({}),
                at_ms: None,
            },
            AgentEvent::PermissionRequested {
                request: AgentPermissionRequest {
                    request_id: "req".to_owned(),
                    tool_name: "tool".to_owned(),
                    input_json: json!({}),
                    tool_use_id: None,
                    file_change: None,
                    grant_root: None,
                },
            },
            AgentEvent::PermissionResolved {
                request_id: "req".to_owned(),
                decision: PermissionDecision::Allow,
            },
            AgentEvent::UnsupportedInteraction {
                method: "custom/method".to_owned(),
                detail_json: json!({}),
            },
            AgentEvent::Error {
                recoverable: true,
                message: "transient".to_owned(),
            },
        ];
        for event in others {
            assert_eq!(
                turn_input_for_agent_event(&event),
                None,
                "expected {event:?} to carry no turn-end input",
            );
        }
    }

    /// Applying a mapped turn-end event to the FSM produces exactly the same
    /// transition as feeding the equivalent `TurnInput` directly, from every
    /// state. This is the core equivalence: the neutral event path and the
    /// existing input path agree.
    #[test]
    fn mapped_turn_end_events_transition_identically_to_direct_inputs() {
        let states = [
            S::Idle,
            S::AwaitingEcho {
                send_id: 7,
                slash_command: false,
            },
            S::InFlight { send_id: Some(7) },
            S::InFlight { send_id: None },
        ];
        let cases = [
            (St::Completed, TurnInput::Stop),
            (St::Interrupted, TurnInput::Interrupt),
            (St::Failed, TurnInput::Stop),
        ];
        for state in states {
            for (status, expected_input) in cases {
                let mapped = turn_input_for_agent_event(&AgentEvent::TurnCompleted { status })
                    .expect("turn-end event must map to an input");
                assert_eq!(mapped, expected_input, "mapping for {status:?}");
                assert_eq!(
                    transition(state, mapped),
                    transition(state, expected_input),
                    "FSM disagreed for {state:?} via {status:?}",
                );
            }
        }
    }

    /// A representative end-to-end sequence: a turn runs and then completes,
    /// driving the FSM back to `Idle` purely through mapped `AgentEvent`s.
    #[test]
    fn a_completed_turn_sequence_returns_the_fsm_to_idle() {
        // A prompt typed into the pane (consuming no send) starts a turn; the
        // neutral stream reports its completion. Only the turn-end event maps;
        // the start is represented here by the FSM's existing
        // prompt-submission input (the event that would map to it is out of
        // scope for this step).
        let mut state = transition(S::Idle, TurnInput::PromptSubmitted { send_id: None }).next;
        assert_eq!(state, S::InFlight { send_id: None });

        let stop = turn_input_for_agent_event(&AgentEvent::TurnCompleted {
            status: St::Completed,
        })
        .expect("completed turn maps to an input");
        state = transition(state, stop).next;
        assert_eq!(state, S::Idle);
    }
}
