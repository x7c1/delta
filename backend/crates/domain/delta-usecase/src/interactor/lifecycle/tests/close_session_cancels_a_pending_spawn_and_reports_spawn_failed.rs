use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::SendTarget;

/// Closing a session whose pane is up but still unbound cancels the launch.
///
/// A spawn that has not bound holds no conversation — its row was written
/// eagerly when the send was accepted and nothing has been ingested against it
/// — so the close is not the tear-down-but-keep a bound session gets: it is the
/// same outcome the watchdog produces for a spawn that never binds. The pane is
/// reclaimed and the row is marked `failed`.
///
/// This ending is the user's own doing, so it must keep reading as a close and
/// not as a breakage: the event sets `cancelled`, and the reason persisted on
/// the row names the close rather than something that broke.
#[tokio::test]
async fn close_session_cancels_a_pending_spawn_and_reports_spawn_failed() {
    let ix = interactor();

    let (send, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: crate::AgentProvider::Claude,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "start something",
            None,
        )
        .await
        .expect("the new-session send is accepted");
    let session_id = send.session_id.clone();

    // The launch is done and its pane is up, but no hook has bound it: this is
    // the state a wedged launch sits in, and the state the user closes from.
    assert_eq!(
        ix.pending_session_ids().await,
        vec![session_id.clone()],
        "the spawn is pending its first hook"
    );

    let events = ix.close_session(&session_id).await.unwrap();

    // Reported as a cancelled launch, naming the close.
    assert_eq!(
        events,
        vec![SessionEvent::SpawnFailed {
            session_id: session_id.clone(),
            pane_token: Some("delta-1".to_owned()),
            reason: Some("closed while starting".to_owned()),
            // The user asked for this, so the browser words it as a cancel
            // rather than a failure.
            cancelled: true,
        }],
    );
    // The pane the launch stood up is gone…
    assert_eq!(
        ix.tmux_fake().killed.lock().unwrap().clone(),
        vec!["delta-1".to_owned()],
    );
    // …the spawn can no longer be bound by a hook arriving late…
    assert!(
        ix.pending_session_ids().await.is_empty(),
        "the pending spawn is taken, not left for a late hook to bind"
    );
    // …and the row stays, marked `failed`, with the sentence that names the
    // close rather than a breakage.
    let session = ix
        .store()
        .session(&session_id)
        .await
        .unwrap()
        .expect("the row of a cancelled launch is kept");
    assert_eq!(session.status, delta_model::SessionStatus::Failed);
    assert_eq!(
        session.failure_reason.as_deref(),
        Some("closed while starting"),
        "the persisted reason names the close, so the screen that reads it \
         words the ending as a cancel"
    );
    // The first prompt is still on the row — nothing had to ride out on the
    // event to survive.
    assert_eq!(
        ix.store()
            .open_sends(&session_id)
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.text)
            .collect::<Vec<_>>(),
        vec!["start something".to_owned()],
    );
}
