//! `ExternalInput` follows *consumption*, not text.
//!
//! "Unmatched" used to mean "the prompt text does not equal the outstanding
//! send", which fired this event for Delta's own rewritten messages. It now
//! means what it says: the prompt consumed no send at all — nothing was
//! outstanding, so it really was typed straight into the pane (the
//! resume-window variant is pinned in the enqueue tests). A prompt that *does*
//! consume an outstanding send is Delta's own however Claude Code rewrote it,
//! and is never announced here.

use delta_model::SessionId;

use crate::interactor::testing::*;
use crate::ports::SessionEvent;
use crate::turn::TurnState;

#[tokio::test]
async fn unmatched_prompt_is_external_input() {
    let ix = interactor();
    let session = SessionId::from("sess-1");
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    ix.transcript_fake()
        .push(user_line("u-ext", "typed directly"));

    // Nothing of Delta's is outstanding for this session.
    assert!(ix
        .store()
        .head_dispatched_send(&session)
        .await
        .unwrap()
        .is_none());

    let (events, additional) = ix
        .on_user_prompt_submit(submit("typed directly"))
        .await
        .unwrap();
    assert!(additional.is_none());
    assert!(events.iter().any(|e| matches!(
        e,
        SessionEvent::ExternalInput { prompt, .. } if prompt == "typed directly"
    )));
    // The turn it started belongs to no send.
    assert_eq!(
        ix.live_state_for(&session).await.turn,
        TurnState::InFlight { send_id: None },
    );
}
