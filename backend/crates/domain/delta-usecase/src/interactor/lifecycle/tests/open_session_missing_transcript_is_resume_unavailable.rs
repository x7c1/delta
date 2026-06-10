use delta_model::SessionId;

use crate::interactor::testing::*;

/// Opening a known-but-closed session whose transcript file is gone is rejected
/// with `ResumeUnavailable` (which the API layer maps to 409): `claude --resume`
/// would have nothing to replay, so the gate refuses before minting a token,
/// writing settings, or spawning. No pane is created and the session stays
/// closed.
#[tokio::test]
async fn open_session_missing_transcript_is_resume_unavailable() {
    use crate::error::Error;

    let ix = interactor();
    // Register a known-but-closed session, then model its transcript as removed.
    ix.on_user_prompt_submit(submit_in(
        "sess-R",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-R");
    ix.transcript_fake().mark_missing("/elsewhere/t.jsonl");
    assert!(ix.pane_for_session(&id).await.is_none(), "starts closed");

    let err = ix
        .open_session(&id)
        .await
        .expect_err("a missing transcript makes resume impossible");
    assert!(
        matches!(err, Error::ResumeUnavailable(ref s) if s == "sess-R"),
        "the session id is surfaced as ResumeUnavailable, got: {err:?}"
    );

    // Nothing was spawned and no settings were written: the gate runs before any
    // of that, and the session remains closed.
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "a resume-unavailable open must not spawn a pane"
    );
    assert!(
        ix.workspace_fake().written.lock().unwrap().is_empty(),
        "a resume-unavailable open must not write session settings"
    );
    assert!(
        ix.pane_for_session(&id).await.is_none(),
        "the session stays closed"
    );
}
