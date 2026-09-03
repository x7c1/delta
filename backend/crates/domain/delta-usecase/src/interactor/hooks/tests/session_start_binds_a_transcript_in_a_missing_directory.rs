use delta_model::SessionStatus;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// The shape a launch in a brand-new working directory produces: Claude Code
/// creates the per-project transcript directory lazily, on its first write —
/// which comes *after* `SessionStart` — so the hook names a `.jsonl` inside a
/// directory that does not exist yet. Confinement must accept it and the spawn
/// must bind on this first call; requiring the parent to exist wedged every
/// first launch in a fresh worktree.
#[tokio::test]
async fn session_start_binds_a_transcript_in_a_missing_directory() {
    let root = tempfile::tempdir().unwrap();
    let ix = interactor_with_transcript_root(root.path().to_str().unwrap());
    ix.new_session().await.unwrap();
    let session_id = ix.pending_session_ids().await.remove(0);

    // `<root>/projects/<cwd-slug>/<id>.jsonl` with neither directory created.
    let transcript = root
        .path()
        .join("projects/-home-user-code-fresh-worktree")
        .join(format!("{}.jsonl", session_id.as_str()));
    assert!(!transcript.parent().unwrap().exists());
    let transcript = transcript.to_str().unwrap();

    let events = ix
        .on_session_start(session_start_at(session_id.as_str(), "startup", transcript))
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
        .expect("the session row was activated");
    assert_eq!(row.status, SessionStatus::Active);
    assert_eq!(row.transcript_path.as_deref(), Some(transcript));
}
