use delta_model::{MessageUuid, SessionId};

use crate::interactor::context::frame_thread_switch_context;
use crate::interactor::testing::*;

use super::support::round_trip;

#[tokio::test]
async fn revisit_to_branch_injects_switch_note_with_root_quote() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // First entry into a branch off some message: this creates the child thread
    // (titled after the locator quote) and makes it the latest user thread.
    let parent = MessageUuid::from("uuid-parent");
    let (branch_send, _) = round_trip(
        &ix,
        branch_off(main, &parent),
        "into branch",
        Some("[root quote]"),
        "u-branch",
    )
    .await;
    let child = branch_send.thread_id;
    assert_ne!(child, main);

    // Move back to main (no quote): the latest user thread becomes main again.
    round_trip(&ix, to(main), "back on main", None, "u-main").await;

    // Now re-visit the child thread (no quote): a thread switch from main to the
    // child, so the note re-cites the child's root quote and re-focuses the
    // model onto that earlier thread.
    let (_, additional) = round_trip(&ix, to(child), "more on branch", None, "u-revisit").await;
    let expected = frame_thread_switch_context(main, child, Some("[root quote]"));
    assert_eq!(additional, Some(expected));
    let note = additional.unwrap();
    assert!(note.contains(&format!("thread:{}", child.value())));
    assert!(note.contains("[root quote]"));
    assert!(note.contains("not replying to the message immediately above"));
}
