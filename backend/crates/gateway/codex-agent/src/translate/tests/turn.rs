//! `turn/started` and `turn/completed`.

use super::*;

#[test]
fn turn_started_carries_the_turn_id_from_the_nested_turn() {
    let events = translate_notification(&notification(
        "turn/started",
        json!({ "threadId": "thr_1", "turn": { "id": "turn_1", "status": "inProgress", "items": [] } }),
    ));
    assert_eq!(
        events,
        vec![AgentEvent::TurnStarted {
            provider_turn_id: Some("turn_1".to_owned())
        }]
    );
}

#[test]
fn turn_started_without_a_turn_id_is_still_a_turn() {
    let events = translate_notification(&notification(
        "turn/started",
        json!({ "threadId": "thr_1" }),
    ));
    assert_eq!(
        events,
        vec![AgentEvent::TurnStarted {
            provider_turn_id: None
        }]
    );
}

#[test]
fn turn_completed_maps_each_status_from_the_nested_turn() {
    for (wire, expected) in [
        ("completed", TurnStatus::Completed),
        ("interrupted", TurnStatus::Interrupted),
        ("failed", TurnStatus::Failed),
    ] {
        let events = translate_notification(&notification(
            "turn/completed",
            json!({ "threadId": "thr_1", "turn": { "id": "turn_1", "status": wire, "items": [] } }),
        ));
        assert_eq!(events, vec![AgentEvent::TurnCompleted { status: expected }]);
    }
}

#[test]
fn an_unknown_or_absent_status_is_failed() {
    assert_eq!(
        translate_notification(&notification(
            "turn/completed",
            json!({ "turn": { "id": "turn_1", "status": "weird", "items": [] } })
        )),
        vec![AgentEvent::TurnCompleted {
            status: TurnStatus::Failed
        }]
    );
    assert_eq!(
        translate_notification(&notification("turn/completed", json!({}))),
        vec![AgentEvent::TurnCompleted {
            status: TurnStatus::Failed
        }]
    );
}

#[test]
fn started_turn_id_reads_the_nested_turn_id_only_for_turn_started() {
    assert_eq!(
        started_turn_id(&notification(
            "turn/started",
            json!({ "threadId": "t", "turn": { "id": "turn_7", "status": "inProgress", "items": [] } })
        )),
        Some("turn_7".to_owned())
    );
    // A `turn/completed` is not where the adapter learns the active turn id.
    assert_eq!(
        started_turn_id(&notification(
            "turn/completed",
            json!({ "turn": { "id": "turn_7", "status": "completed", "items": [] } })
        )),
        None
    );
    assert!(is_turn_completed(&notification(
        "turn/completed",
        json!({})
    )));
    assert!(!is_turn_completed(&notification("turn/started", json!({}))));
}
