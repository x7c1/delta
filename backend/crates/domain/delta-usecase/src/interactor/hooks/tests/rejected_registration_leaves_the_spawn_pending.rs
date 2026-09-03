use delta_model::SessionStatus;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// A registering hook whose transcript path is refused must not consume the
/// pending spawn. Binding is the point of no return — every later hook finds
/// nothing pending and falls back to the stored row — so a spawn bound without
/// a registered row would be wedged forever: a live pane whose row stays
/// `spawning` with no transcript path, hence nothing for the conversation
/// source to tail and no retry. So the fallible registration runs first, and a
/// rejection leaves the spawn pending for the next hook to retry.
#[tokio::test]
async fn rejected_registration_leaves_the_spawn_pending() {
    let root = tempfile::tempdir().unwrap();
    let ix = interactor_with_transcript_root(root.path().to_str().unwrap());
    // Cold-start spawn (no first prompt), so `SessionStart(startup)` is the
    // first hook to reach it.
    ix.new_session().await.unwrap();
    let session_id = ix.pending_session_ids().await.remove(0);

    // A transcript outside the confinement root: registration refuses it.
    let err = ix
        .on_session_start(session_start_at(
            session_id.as_str(),
            "startup",
            "/etc/secret.jsonl",
        ))
        .await
        .expect_err("an out-of-root transcript path is refused");
    assert!(
        matches!(err, crate::Error::InvalidTranscriptPath(_)),
        "expected the confinement error, got {err:?}"
    );

    // Nothing was bound, and the spawn is still pending — so the launch is
    // still visible to the stale-pending sweep, and to the next hook.
    assert_eq!(
        ix.pending_session_ids().await,
        vec![session_id.clone()],
        "a refused registration must not consume the pending spawn"
    );
    assert!(ix.pane_for_session(&session_id).await.is_none());
    let row = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the eagerly-created row is still there");
    assert_eq!(row.status, SessionStatus::Spawning);
    assert_eq!(row.transcript_path, None, "the refused path was not stored");

    // The next hook for the same id, now naming a valid path, binds and
    // registers it normally.
    let valid = root
        .path()
        .join("never-created-dir")
        .join(format!("{}.jsonl", session_id.as_str()));
    let valid = valid.to_str().unwrap();
    let events = ix
        .on_session_start(session_start_at(session_id.as_str(), "startup", valid))
        .await
        .unwrap();

    assert!(events.contains(&SessionEvent::SessionRegistered {
        session_id: session_id.clone(),
    }));
    assert_eq!(
        ix.pane_for_session(&session_id).await,
        Some("delta-1:0.0".to_owned())
    );
    assert!(ix.pending_session_ids().await.is_empty());
    let row = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the retried hook activated the row");
    assert_eq!(row.status, SessionStatus::Active);
    assert_eq!(row.transcript_path.as_deref(), Some(valid));
}
