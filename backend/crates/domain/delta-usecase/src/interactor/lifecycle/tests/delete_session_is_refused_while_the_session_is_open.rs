use delta_model::SessionId;

use crate::error::Error;
use crate::interactor::testing::*;

/// An open session is not the user's to remove — idle or mid-turn — and the
/// refusal leaves every row where it was.
///
/// Open-ness is process-runtime state, not a column, so this is read off the
/// session's own runtime (the authority the session list annotates rows with)
/// rather than inferred from `SessionStatus`, which says `active` for a session
/// that has since been closed. The browser only offers `Remove` on a closed
/// card, so this answers a stale one (another tab reopened the session).
#[tokio::test]
async fn delete_session_is_refused_while_the_session_is_open() {
    let ix = interactor();
    // `seed_session` leaves `sess-1` open, ready and *idle*: its registration
    // turn is completed by the `Stop` it fires.
    ix.seed_session().await;
    let id = SessionId::from("sess-1");
    assert!(ix.is_session_open(&id).await, "open and idle");

    let err = ix
        .delete_session(&id)
        .await
        .expect_err("an open session must not be removed");
    assert!(
        matches!(&err, Error::SessionOpen(open) if open == "sess-1"),
        "the open session is surfaced as SessionOpen (409 `session_open`), got {err:?}"
    );
    assert!(
        ix.store().session(&id).await.unwrap().is_some(),
        "a refused removal deletes nothing"
    );

    // Mid-turn is the same check, and the turn is not interrupted by the
    // refusal: a bare `UserPromptSubmit` marks the turn in flight.
    ix.on_user_prompt_submit(submit("a question"))
        .await
        .unwrap();
    let err = ix
        .delete_session(&id)
        .await
        .expect_err("a mid-turn session must not be removed either");
    assert!(
        matches!(&err, Error::SessionOpen(open) if open == "sess-1"),
        "mid-turn is refused the same way as idle, got {err:?}"
    );
    assert!(
        ix.store().session(&id).await.unwrap().is_some(),
        "the row survives the mid-turn refusal too"
    );
    assert!(
        ix.is_session_open(&id).await,
        "the refusal leaves the session open; it does not close it as a side effect"
    );
}
