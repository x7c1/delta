//! Subagent-launch round trips.

use delta_usecase::RunningSubagentLaunch;

use super::super::SqliteStore;
use super::new_session;

#[tokio::test]
async fn subagent_launches_round_trip_and_clear() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    let child = store
        .create_thread(&session.id, "side topic", Some(main))
        .await
        .unwrap()
        .id;

    // No launches recorded yet.
    assert!(store
        .outstanding_subagent_launches(&session.id)
        .await
        .unwrap()
        .is_empty());

    // Record two launches against different threads.
    store
        .record_subagent_launch(&session.id, "toolu_a", child)
        .await
        .unwrap();
    store
        .record_subagent_launch(&session.id, "toolu_b", main)
        .await
        .unwrap();
    let launches = store
        .outstanding_subagent_launches(&session.id)
        .await
        .unwrap();
    assert_eq!(
        launches.get("toolu_a").map(|launch| launch.thread_id),
        Some(child)
    );
    assert_eq!(
        launches.get("toolu_b").map(|launch| launch.thread_id),
        Some(main)
    );
    assert!(
        launches.values().all(|launch| launch.task_id.is_none()),
        "a fresh launch carries no task_id until upgrade_subagent_task_id runs"
    );

    // Upgrading an entry sets its task_id; re-record keeps that upgrade.
    store
        .upgrade_subagent_task_id(&session.id, "toolu_a", "a31425032172620ed")
        .await
        .unwrap();
    assert_eq!(
        store
            .outstanding_subagent_launches(&session.id)
            .await
            .unwrap()
            .get("toolu_a")
            .and_then(|launch| launch.task_id.clone()),
        Some("a31425032172620ed".to_owned())
    );

    // Re-recording the same id refreshes the thread rather than erroring, and
    // must NOT wipe the previously-upgraded task_id.
    store
        .record_subagent_launch(&session.id, "toolu_a", main)
        .await
        .unwrap();
    let after = store
        .outstanding_subagent_launches(&session.id)
        .await
        .unwrap();
    assert_eq!(
        after.get("toolu_a").map(|launch| launch.thread_id),
        Some(main)
    );
    assert_eq!(
        after
            .get("toolu_a")
            .and_then(|launch| launch.task_id.clone()),
        Some("a31425032172620ed".to_owned()),
        "the previously-upgraded task_id survives a re-record"
    );

    // Upgrading an unknown id is a silent no-op (the launch may have already
    // been folded by its completion notification).
    store
        .upgrade_subagent_task_id(&session.id, "toolu_unknown", "anything")
        .await
        .unwrap();

    // Clearing one leaves the other; clearing an unknown id is a no-op.
    store
        .clear_subagent_launch(&session.id, "toolu_a")
        .await
        .unwrap();
    store
        .clear_subagent_launch(&session.id, "nonexistent")
        .await
        .unwrap();
    let remaining = store
        .outstanding_subagent_launches(&session.id)
        .await
        .unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining.get("toolu_b").map(|launch| launch.thread_id),
        Some(main)
    );
}

/// A launch marked as lighting the running indicator reads back with what it
/// was marked with, in launch order; an unmarked launch (a background `Bash`)
/// and a marked launch that completed do not read back at all.
#[tokio::test]
async fn running_subagent_launches_round_trip_the_indicator_fields() {
    let store = SqliteStore::open_in_memory().unwrap();
    let (session, main) = store.register_session(new_session()).await.unwrap();
    let child = store
        .create_thread(&session.id, "side topic", Some(main))
        .await
        .unwrap()
        .id;

    for (tool_use_id, thread) in [
        ("toolu_first", child),
        ("toolu_bash", main),
        ("toolu_second", main),
        ("toolu_done", main),
    ] {
        store
            .record_subagent_launch(&session.id, tool_use_id, thread)
            .await
            .unwrap();
    }
    store
        .record_subagent_indicator(
            &session.id,
            "toolu_first",
            Some("general-purpose"),
            Some("Long crawl"),
        )
        .await
        .unwrap();
    store
        .record_subagent_indicator(&session.id, "toolu_second", None, None)
        .await
        .unwrap();
    store
        .record_subagent_indicator(&session.id, "toolu_done", Some("explore"), None)
        .await
        .unwrap();
    store
        .upgrade_subagent_task_id(&session.id, "toolu_first", "agent_abc")
        .await
        .unwrap();
    // A re-record (a re-ingested launch line) keeps the marking.
    store
        .record_subagent_launch(&session.id, "toolu_first", child)
        .await
        .unwrap();
    store
        .clear_subagent_launch(&session.id, "toolu_done")
        .await
        .unwrap();
    // Marking an unknown launch (a foreground call leaves none) is a no-op.
    store
        .record_subagent_indicator(&session.id, "toolu_foreground", Some("x"), None)
        .await
        .unwrap();

    assert_eq!(
        store.running_subagent_launches(&session.id).await.unwrap(),
        vec![
            RunningSubagentLaunch {
                tool_use_id: "toolu_first".into(),
                thread_id: child,
                task_id: Some("agent_abc".into()),
                subagent_type: Some("general-purpose".into()),
                description: Some("Long crawl".into()),
            },
            RunningSubagentLaunch {
                tool_use_id: "toolu_second".into(),
                thread_id: main,
                task_id: None,
                subagent_type: None,
                description: None,
            },
        ]
    );
    assert_eq!(
        store
            .outstanding_subagent_launches(&session.id)
            .await
            .unwrap()
            .len(),
        3,
        "the unmarked Bash launch is still outstanding for attribution"
    );
}
