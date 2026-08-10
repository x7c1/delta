//! A send carrying an image-attachment path correlates with the echo Claude
//! Code actually submits.
//!
//! Delta types `<body>` plus the attachment's path; Claude Code's composer
//! swallows the path, reads the file, and submits `[Image #N]<body>` instead.
//! Exact text equality can therefore never hold, and before the echo matching
//! learned about the rewrite this send was treated as unechoed on every
//! attempt — requeued, re-typed on the next idle, and answered again, forever.

use delta_model::{SendStatus, SessionId};

use crate::interactor::testing::*;
use crate::ports::SessionEvent;

/// The composed text: body first, attachment path on its own line with its
/// spaces shell-escaped, exactly as the incident's send row carried it.
const SEND_TEXT: &str = "can you read this picture\n\
                         /home/dev/pictures/Screenshot\\ 2026-08-09\\ at\\ 11.51.52.png";

/// What Claude Code submits instead: the placeholder hoisted to the front, the
/// path gone, and the newline that separated them gone with it.
const ECHOED_PROMPT: &str = "[Image #2]can you read this picture";

#[tokio::test]
async fn image_attachment_send_matches_its_rewritten_echo() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    let (send, _) = ix.enqueue_send(to(main), SEND_TEXT, None).await.unwrap();
    assert_eq!(send.status, SendStatus::Dispatched);

    // The transcript records the rewritten form too (the placeholder is in the
    // line's text block; the image block itself carries no text).
    ix.transcript_fake()
        .push(user_line("uuid-1", ECHOED_PROMPT));

    let (events, _) = ix
        .on_user_prompt_submit(submit(ECHOED_PROMPT))
        .await
        .unwrap();

    // The turn belongs to the send's thread, not to "someone typed in the
    // pane".
    assert!(
        events.iter().any(|e| matches!(
            e,
            SessionEvent::TurnStarted { send_id, thread_id, .. }
                if *send_id == send.id && *thread_id == main
        )),
        "the attachment send's turn is attributed to it; got {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, SessionEvent::ExternalInput { .. })),
        "the echo is not external input"
    );

    // And the send is consumed: matched, not outstanding, so nothing re-types
    // it on the next idle.
    let matched = ix.store().send(send.id).await.unwrap().expect("send row");
    assert_eq!(matched.status, SendStatus::Matched);
    assert!(ix
        .store()
        .head_dispatched_send(&session)
        .await
        .unwrap()
        .is_none());
}
