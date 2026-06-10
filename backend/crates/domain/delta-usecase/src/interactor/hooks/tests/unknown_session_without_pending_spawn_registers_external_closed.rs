use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// A `UserPromptSubmit` for an unknown id with NO matching pending spawn is an
/// external claude: it registers a closed data session (no open pane) and emits
/// external input, without panicking.
#[tokio::test]
async fn unknown_session_without_pending_spawn_registers_external_closed() {
    let ix = interactor();
    ix.transcript_fake()
        .push_to("/outside/t.jsonl", user_line("u-x", "typed outside"));

    let (events, _) = ix
        .on_user_prompt_submit(submit_in(
            "sess-X",
            "/outside/t.jsonl",
            "/outside",
            "typed outside",
        ))
        .await
        .unwrap();

    // Registered, but closed (no live pane), and reported as external input.
    assert!(events.contains(&SessionEvent::SessionRegistered {
        session_id: SessionId::from("sess-X"),
    }));
    assert!(events.iter().any(|e| matches!(
        e,
        SessionEvent::ExternalInput { prompt, .. } if prompt == "typed outside"
    )));
    assert!(
        ix.pane_for_session(&SessionId::from("sess-X"))
            .await
            .is_none(),
        "an external session has no open pane"
    );
}
