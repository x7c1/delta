//! At boot, a session whose remembered tmux session is still running becomes
//! open on that very pane: nothing is launched, and what the agent wrote while
//! Delta was down is ingested from the stored cursor on.

use crate::interactor::testing::*;

use super::readoption_support::{left_behind, pane_for, survives, SURVIVING_TOKEN};

#[tokio::test]
async fn boot_readopts_a_surviving_pane_without_launching_and_catches_up_its_transcript() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", SURVIVING_TOKEN).await;
    survives(&ix, SURVIVING_TOKEN);
    // The agent kept working while Delta was down.
    ix.transcript_fake().push_to(
        "/work/sess-A.jsonl",
        user_line("written-while-down", "hello"),
    );

    let summary = ix.readopt_surviving_sessions(false).await.unwrap();

    assert_eq!(summary.adopted, 1);
    assert_eq!(summary.hooks_unreachable, 0);
    assert_eq!(
        ix.bound_pane(&id).await,
        Some(pane_for(SURVIVING_TOKEN)),
        "open on the pane that survived"
    );
    assert!(ix.is_session_open(&id).await);
    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no launch command ran: neither a fresh spawn nor `claude --resume`"
    );
    assert!(ix.tmux_fake().killed.lock().unwrap().is_empty());

    // Caught up from the stored cursor: the line written while Delta was down
    // is a message now, and the one already ingested was not read again.
    let main = ix.store().main_thread_id(&id).await.unwrap();
    let uuids: Vec<String> = ix
        .store()
        .thread_messages(main)
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.uuid.as_str().to_owned())
        .collect();
    assert_eq!(uuids, vec!["written-while-down".to_owned()]);
    assert_eq!(
        ix.store().transcript_lines_read(&id).await.unwrap(),
        2,
        "the cursor moved past the caught-up line"
    );

    // A send goes straight into the adopted pane.
    ix.enqueue_send(to(main), "next", None).await.unwrap();
    let sent = ix.tmux_fake().sent.lock().unwrap().clone();
    assert_eq!(sent, vec![(pane_for(SURVIVING_TOKEN), "next".to_owned())]);
    assert!(ix.tmux_fake().created.lock().unwrap().is_empty());
}
