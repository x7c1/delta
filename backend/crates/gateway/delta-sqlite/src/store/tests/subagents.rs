//! Subagent-launch round trips.

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
