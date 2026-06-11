use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// `SessionStart(startup)` then `UserPromptSubmit` (and the reverse order) both
/// end with exactly one bound session and the deferred first prompt written
/// once. The bind step is idempotent: whichever signal arrives first binds, the
/// other is a no-op for binding while still doing its own work.
#[tokio::test]
async fn session_start_then_user_prompt_bind_once() {
    // Order A: SessionStart(startup) first, then the matching UserPromptSubmit.
    {
        let ix = interactor();
        // A composer-initiated New carries a deferred first prompt.
        ix.enqueue_send(
            crate::SendTarget::NewSession { workdir: None },
            "first prompt",
            None,
        )
        .await
        .unwrap();
        let session_id = ix.pending_session_ids().await.remove(0);

        // SessionStart binds + registers + writes the deferred first prompt.
        let events = ix
            .on_session_start(session_start(session_id.as_str(), "startup"))
            .await
            .unwrap();
        assert_eq!(
            registered_count(&events),
            1,
            "SessionStart registers exactly once"
        );

        // The matching UserPromptSubmit must not re-register or re-bind.
        let (events2, _) = ix
            .on_user_prompt_submit(submit_in(
                session_id.as_str(),
                "/tmp/t.jsonl",
                "/work",
                "first prompt",
            ))
            .await
            .unwrap();
        assert_eq!(
            registered_count(&events2),
            0,
            "the already-bound session does not re-register on UserPromptSubmit"
        );

        // Exactly one session, one bound pane, one deferred-first pending row.
        assert_eq!(ix.store().list_sessions().await.unwrap().len(), 1);
        assert!(ix.pane_for_session(&session_id).await.is_some());
        let head = ix
            .store()
            .head_pending_send(&session_id)
            .await
            .unwrap()
            .expect("the deferred first prompt was written once");
        assert_eq!(head.text, "first prompt");
    }

    // Order B: UserPromptSubmit first, then SessionStart(startup).
    {
        let ix = interactor();
        ix.enqueue_send(
            crate::SendTarget::NewSession { workdir: None },
            "first prompt",
            None,
        )
        .await
        .unwrap();
        let session_id = ix.pending_session_ids().await.remove(0);

        let (events, _) = ix
            .on_user_prompt_submit(submit_in(
                session_id.as_str(),
                "/tmp/t.jsonl",
                "/work",
                "first prompt",
            ))
            .await
            .unwrap();
        assert_eq!(registered_count(&events), 1);

        // SessionStart now finds the spawn already bound: no second registration.
        let events2 = ix
            .on_session_start(session_start(session_id.as_str(), "startup"))
            .await
            .unwrap();
        assert_eq!(
            registered_count(&events2),
            0,
            "SessionStart is a no-op once UserPromptSubmit already bound"
        );

        assert_eq!(ix.store().list_sessions().await.unwrap().len(), 1);
        assert!(ix.pane_for_session(&session_id).await.is_some());
    }
}

fn registered_count(events: &[SessionEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, SessionEvent::SessionRegistered { .. }))
        .count()
}
