//! How a scripted turn's `turn/*` and `item/*` frames translate into the
//! neutral events: the prompt, the turn's start and completion, tool items, and
//! the interrupt.

use agent_contract::launch_request;
use delta_usecase::{AgentAdapter, AgentEvent, SendRequest, TurnStatus};

use crate::support::{adapter_with, collect_until, is_turn_completed, turn_scenario};

/// `send_emits_user_prompt_and_turn_started`: a send emits `UserPromptAccepted`
/// and the turn's `turn/started` notification projects `TurnStarted`.
#[tokio::test]
async fn send_emits_user_prompt_and_turn_started() {
    let (adapter, _guard) = adapter_with(&turn_scenario("")).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "hello codex".to_owned(),
            },
        )
        .await
        .expect("send");
    let events = collect_until(&mut stream, is_turn_completed).await;
    assert!(
        events.iter().any(
            |e| matches!(e, AgentEvent::UserPromptAccepted { text, .. } if text == "hello codex")
        ),
        "expected UserPromptAccepted, got {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::TurnStarted { .. })),
        "expected TurnStarted, got {events:?}"
    );
}

/// `turn_completion_is_emitted_once`: exactly one `TurnCompleted(Completed)` per
/// turn, and the completed assistant message replays as `AssistantMessage`.
#[tokio::test]
async fn turn_completion_is_emitted_once_and_carries_the_assistant_message() {
    let (adapter, _guard) = adapter_with(&turn_scenario("")).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "go".to_owned(),
            },
        )
        .await
        .expect("send");
    let events = collect_until(&mut stream, is_turn_completed).await;
    let completions = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                AgentEvent::TurnCompleted {
                    status: TurnStatus::Completed
                }
            )
        })
        .count();
    assert_eq!(
        completions, 1,
        "exactly one TurnCompleted per turn: {events:?}"
    );
    assert!(
        events.iter().any(
            |e| matches!(e, AgentEvent::AssistantMessage { text, .. } if text == "scripted reply")
        ),
        "expected the assistant message, got {events:?}"
    );
}

/// Tool items translate to `ToolStarted` / `ToolCompleted`.
#[tokio::test]
async fn tool_items_translate_to_tool_events() {
    let scenario = r#"{
        "thread_id": "thr_tool",
        "turn": {
            "turn_id": "turn_tool",
            "emit": [
                { "type": "turn_started" },
                { "type": "item_started", "item": { "id": "t1", "type": "commandExecution", "command": "ls", "cwd": "/tmp", "status": "inProgress", "commandActions": [] } },
                { "type": "item_completed", "item": { "id": "t1", "type": "commandExecution", "command": "ls", "cwd": "/tmp", "status": "completed", "commandActions": [], "aggregatedOutput": "a\nb", "exitCode": 0 } },
                { "type": "turn_completed", "status": "completed" }
            ]
        }
    }"#;
    let (adapter, _guard) = adapter_with(scenario).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "run it".to_owned(),
            },
        )
        .await
        .expect("send");
    let events = collect_until(&mut stream, is_turn_completed).await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::ToolStarted { name, .. } if name == "command_execution"
        )),
        "expected ToolStarted, got {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::ToolCompleted { .. })),
        "expected ToolCompleted, got {events:?}"
    );
}

/// `interrupt_ends_turn`: interrupting drives `turn/interrupt`, whose
/// `turn/completed{interrupted}` projects `TurnCompleted(Interrupted)`.
#[tokio::test]
async fn interrupt_ends_turn() {
    // A turn that emits nothing on its own, so the only completion is the one
    // the interrupt produces.
    let scenario = r#"{
        "thread_id": "thr_interrupt",
        "turn": { "turn_id": "turn_interrupt", "emit": [ { "type": "turn_started" } ] }
    }"#;
    let (adapter, _guard) = adapter_with(scenario).await;
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    adapter
        .send(
            &handle,
            SendRequest {
                text: "long task".to_owned(),
            },
        )
        .await
        .expect("send");
    adapter.interrupt(&handle).await.expect("interrupt");
    let events = collect_until(&mut stream, is_turn_completed).await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::TurnCompleted {
                status: TurnStatus::Interrupted
            }
        )),
        "expected TurnCompleted(Interrupted), got {events:?}"
    );
}
