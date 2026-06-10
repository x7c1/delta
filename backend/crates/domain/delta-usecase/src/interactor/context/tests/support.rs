use delta_model::PendingSend;

use crate::interactor::testing::*;
use crate::{Interactor, SendTarget};

/// Drive a full send round-trip: queue the send, push its matching user line,
/// and run the `UserPromptSubmit` hook so the line is attributed to the send's
/// thread (making it the session's `latest_user_thread`). Returns the injected
/// `additionalContext` for that send so a caller can assert on it.
pub(super) async fn round_trip(
    ix: &Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace>,
    target: SendTarget,
    text: &str,
    quote: Option<&str>,
    uuid: &str,
) -> (PendingSend, Option<String>) {
    let pending = ix.enqueue_send(target, text, quote).await.unwrap();
    ix.transcript_fake().push(user_line(uuid, text));
    let (_events, additional) = ix.on_user_prompt_submit(submit(text)).await.unwrap();
    (pending, additional)
}
