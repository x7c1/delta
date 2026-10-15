//! When Claude Code rewrites a prompt, the lines that report it at `info` and
//! `warn` say how the texts differ without recording them: a prompt is the
//! user's own words, and the log at its default level is not the place for it.

use delta_model::SessionId;

use crate::interactor::testing::*;

const SENT: &str = "deploy the private-sent-words";
const SUBMITTED: &str = "deploy the private-sent-words, rewritten-private-tail";

#[tokio::test]
async fn a_rewritten_prompt_is_logged_without_its_text() {
    let (logs, _guard) = capture_logs(tracing::Level::INFO);
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();
    ix.enqueue_send(to(main), SENT, None).await.unwrap();

    // Claude Code submits and records something other than the send's text.
    ix.transcript_fake().push(user_line("u-1", SUBMITTED));
    ix.on_user_prompt_submit(submit(SUBMITTED)).await.unwrap();

    let logged = logs.text();
    assert!(
        logged.contains("UserPromptSubmit does not equal the outstanding send's text"),
        "the hook's mismatch is reported at info: {logged}"
    );
    assert!(
        logged.contains("does not spell the send's own text"),
        "the transcript line's mismatch is reported at warn: {logged}"
    );
    assert!(
        logged.contains(&format!("{} chars, sha256 ", SENT.chars().count())),
        "the lines carry each text's length and hash: {logged}"
    );
    assert!(!logged.contains("private-sent-words"), "{logged}");
    assert!(!logged.contains("rewritten-private-tail"), "{logged}");
}
