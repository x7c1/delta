use delta_model::Send;

use crate::interactor::testing::*;
use crate::{Interactor, SendTarget};

/// Drive a full send round-trip: queue the send, push its matching user line,
/// run the `UserPromptSubmit` hook so the line is attributed to the send's
/// thread (making it the session's `latest_user_thread`), and complete the
/// turn with a `Stop`. Returns the injected `additionalContext` for that send
/// so a caller can assert on it.
pub(super) async fn round_trip(
    ix: &Interactor<FakeTmux, FakeTranscript, FakeStore, FakeWorkspace>,
    target: SendTarget,
    text: &str,
    quote: Option<&str>,
    uuid: &str,
) -> (Send, Option<String>) {
    let (pending, _) = ix.enqueue_send(target, text, quote).await.unwrap();
    ix.transcript_fake().push(user_line(uuid, text));
    let (_events, additional) = ix.on_user_prompt_submit(submit(text)).await.unwrap();
    // Complete the turn so the next round trip starts idle (under
    // single-outstanding dispatch a send composed mid-turn would be queued).
    ix.on_stop(crate::ports::StopHook {
        session_id: delta_model::SessionId::from("sess-1"),
        stop_reason: None,
    })
    .await
    .unwrap();
    (pending, additional)
}
