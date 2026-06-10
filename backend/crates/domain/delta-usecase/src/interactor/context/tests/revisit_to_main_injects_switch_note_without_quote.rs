use delta_model::{MessageUuid, SessionId};

use crate::interactor::context::frame_thread_switch_context;
use crate::interactor::testing::*;

use super::support::round_trip;

#[tokio::test]
async fn revisit_to_main_injects_switch_note_without_quote() {
    let ix = interactor();
    ix.on_user_prompt_submit(submit("seed")).await.unwrap();
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Enter a branch first, so the latest user thread is the child.
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

    // Return to main (no quote): a switch from the child back to the trunk.
    // `main` has no root passage, so the note names it without citing a quote.
    let (_, additional) = round_trip(&ix, to(main), "back to main", None, "u-main").await;
    let expected = frame_thread_switch_context(child, main, None);
    assert_eq!(additional, Some(expected));
    let note = additional.unwrap();
    assert!(note.contains("the main thread"));
    assert!(!note.contains('"'), "no quote is cited for main");
    assert!(note.contains("not replying to the message immediately above"));
}
