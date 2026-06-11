use crate::interactor::testing::*;

use super::support::{closed_session_with_pending_branch, ingested_thread};

/// `carry_thread` regression — the PRE-FIX behaviour this fix removes.
///
/// `sync_transcript` seeds `carry_thread` from
/// `latest_user_thread().unwrap_or(main)`. When the DB is behind the transcript
/// at the resume boundary, `latest_user_thread` is `None`, so the seed defaults
/// to `main`. A non-user line that leads the synced batch — before any user
/// line in it re-corrects `carry_thread` — is then mis-attributed to `main`,
/// even though it is the tail of the user's prior (branch) turn.
///
/// This drives that batch directly (no `open_session` catch-up) to pin the
/// mechanism the fix targets.
#[tokio::test]
async fn db_behind_mis_seeds_carry_thread_to_main_for_a_leading_non_user_line() {
    let (ix, id, main, _child) = closed_session_with_pending_branch().await;
    // Open the session so the tail (scoped to open sessions) ingests the batch
    // below. The pending branch was written closed only to avoid resuming during
    // setup; this test drives `poll_transcript` directly to exercise the
    // `carry_thread` seeding, which is independent of open/closed.
    ix.bind_open_session("delta-R", &id).await;

    // The DB is behind: no user row yet, so the latest user thread is unknown.
    assert!(
        ix.store().latest_user_thread(&id).await.unwrap().is_none(),
        "precondition: DB behind transcript"
    );

    // A batch whose head is a non-user line (the tail of the prior branch turn),
    // with no user line in it to re-correct the carry thread.
    ix.transcript_fake().push_to(
        "/elsewhere/t.jsonl",
        assistant_line("a-lead", "leading reply"),
    );
    ix.poll_transcript().await.unwrap();

    assert_eq!(
        ingested_thread(&ix, "a-lead"),
        Some(main),
        "with the DB behind, the leading non-user line is mis-attributed to main"
    );
}
