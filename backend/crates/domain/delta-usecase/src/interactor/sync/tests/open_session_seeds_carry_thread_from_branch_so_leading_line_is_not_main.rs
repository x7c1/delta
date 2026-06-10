use crate::interactor::testing::*;

use super::support::{closed_session_with_pending_branch, ingested_thread};

/// The root fix closes the window above. `open_session` catches the DB up to
/// the prior branch turn before returning, so by the time the post-resume tail
/// batch is synced `latest_user_thread` is the branch. A non-user line leading
/// that batch then follows the branch carry thread, not `main`.
#[tokio::test]
async fn open_session_seeds_carry_thread_from_branch_so_leading_line_is_not_main() {
    let (ix, id, main, child) = closed_session_with_pending_branch().await;

    // The prior branch user line sits in the transcript, unsynced (DB behind).
    ix.transcript_fake().push_to(
        "/elsewhere/t.jsonl",
        user_line("u-branch", "prior branch prompt"),
    );
    assert!(
        ix.store().latest_user_thread(&id).await.unwrap().is_none(),
        "precondition: DB behind transcript"
    );

    // Resume: the catch-up sync ingests the prior branch turn, so the branch
    // becomes the known latest user thread.
    ix.open_session(&id).await.unwrap();
    assert_eq!(
        ix.store().latest_user_thread(&id).await.unwrap(),
        Some(child),
        "open caught the DB up to the prior branch user line"
    );

    // The post-resume tail now arrives as its own batch, leading with a
    // non-user line. It follows the branch carry thread, not main.
    ix.transcript_fake().push_to(
        "/elsewhere/t.jsonl",
        assistant_line("a-lead", "post-resume reply"),
    );
    ix.poll_transcript().await.unwrap();
    assert_eq!(
        ingested_thread(&ix, "a-lead"),
        Some(child),
        "the leading non-user line follows the branch carry thread, not main"
    );

    // Nothing leaked onto main.
    let main_view = ix.thread_view(main).await.unwrap();
    assert!(
        main_view.is_empty(),
        "no line was mis-attributed to main, got: {:?}",
        main_view
            .iter()
            .map(|m| m.uuid.as_str())
            .collect::<Vec<_>>()
    );
}
