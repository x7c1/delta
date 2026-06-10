use delta_model::SessionId;

use crate::interactor::testing::*;

use super::support::round_trip;

#[tokio::test]
async fn unknown_previous_thread_injects_nothing() {
    // Regression: on the first prompt after a session resume (and on the very
    // first turn), no user line is persisted yet, so `latest_user_thread`
    // reports `None` at the moment `thread_switch_context` runs. That is an
    // UNKNOWN previous thread, not a switch — the user may simply be continuing.
    // Asserting a switch there ("The user has switched to thread:N") is false
    // and misleads the model, so nothing must be injected.
    let ix = interactor();
    // Register the session (creates its `main` thread) without persisting any
    // user line: `submit` carries no matching transcript line, so it syncs
    // nothing. This mirrors the resume boundary where no user line is visible
    // to `latest_user_thread` yet.
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // No prior persisted user line exists, so `latest_user_thread` is `None`.
    assert!(
        ix.store()
            .latest_user_thread(&session)
            .await
            .unwrap()
            .is_none(),
        "precondition: previous thread is unknown"
    );

    // A plain send to main with no locator quote: the previous thread is
    // unknown, so this is not a switch and no re-focus note is injected.
    let (_, additional) = round_trip(&ix, to(main), "first prompt", None, "u-1").await;
    assert!(
        additional.is_none(),
        "unknown previous thread must not inject a switch note, got: {additional:?}"
    );
}
