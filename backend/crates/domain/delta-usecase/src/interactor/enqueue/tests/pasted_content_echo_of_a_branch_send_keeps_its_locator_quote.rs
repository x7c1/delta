//! A branch send whose echo comes back inside Claude Code's pasted-content
//! wrapper is still recognized as the send's own text.
//!
//! Delta types every send as a bracketed paste, and recent Claude Code builds
//! wrap a long enough paste as
//! `<pasted_content id="XXXX">` … `</pasted_content id="XXXX">` in both the
//! `UserPromptSubmit` prompt and the transcript line. Before the echo matching
//! learned about the wrapper, every such branch send lost its locator-quote
//! frame: the model never learned which passage the user had quoted.

use delta_model::{MessageUuid, SendStatus, SessionId};

use crate::interactor::context::{frame_branch_entry_context, frame_locator_context};
use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// The composed branch message, 20+ characters so Claude Code wraps it.
const SEND_TEXT: &str = "実装に着手してほしいのですが、その前に認証をこちらで済ませておかないといけない、という理解であっていますか";

/// What Claude Code submits and records for it (observed verbatim).
const WRAPPED_ECHO: &str = "\n\n<pasted_content id=\"d626\">\n実装に着手してほしいのですが、その前に認証をこちらで済ませておかないといけない、という理解であっていますか\n</pasted_content id=\"d626\">\n";

const QUOTE: &str = "the quoted passage";

#[tokio::test]
async fn pasted_content_echo_of_a_branch_send_keeps_its_locator_quote() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let parent = MessageUuid::from("uuid-parent");
    let (send, _) = ix
        .enqueue_send(branch_off(main, &parent), SEND_TEXT, Some(QUOTE))
        .await
        .unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);

    ix.transcript_fake().push(user_line("uuid-1", WRAPPED_ECHO));

    let (events, additional) = ix
        .on_user_prompt_submit(submit(WRAPPED_ECHO))
        .await
        .unwrap();

    // The quote frame is injected exactly as it is for an unwrapped echo.
    let expected =
        frame_branch_entry_context(&frame_locator_context(QUOTE).unwrap(), send.thread_id);
    assert_eq!(additional, Some(expected));

    // The turn is announced on the branch, naming the line the echo produced —
    // which is stored unwrapped, so pairing it with the wrapped prompt proves
    // both sides are read through the same wrapper rule.
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::TurnStarted { send_id, thread_id, matched_uuid, .. }
                if *send_id == send.id
                    && *thread_id == send.thread_id
                    && *matched_uuid == MessageUuid::from("uuid-1")
        )),
        "the wrapped echo announces the send's turn; got {events:?}"
    );

    let matched = ix.store().send(send.id).await.unwrap().expect("send row");
    assert_eq!(matched.status, SendStatus::Matched);
}
