//! A branch send whose transcript line does not read like the send text is
//! still attributed to the branch.
//!
//! Seen on a real session: a send to a branch thread reached Claude Code with
//! extra characters in it (typed into the pane in the gap between Delta's paste
//! and its Enter). It was delivered exactly once — positional consumption did
//! its job — but attribution was still decided by text back then, so the user
//! line and the whole reply landed on `main`: from the branch's pane the turn
//! had simply vanished. Attribution is positional now too, so the rewritten
//! line opens the child thread, the reply follows it, and the send row claims
//! that line's uuid.

use delta_model::{MessageUuid, SendStatus, SessionId};

use crate::interactor::testing::*;

#[tokio::test]
async fn rewritten_branch_echo_still_attributes_to_the_child() {
    let ix = interactor();
    ix.seed_session().await;
    let session = SessionId::from("sess-1");
    let main = ix.store().main_thread_id(&session).await.unwrap();

    // Branch off an existing message and queue the first branch send.
    let parent = MessageUuid::from("uuid-parent");
    let (pending, _) = ix
        .enqueue_send(branch_off(main, &parent), "branch text", None)
        .await
        .unwrap();
    let child = pending.thread_id;
    assert_ne!(child, main);

    // What Claude Code recorded differs from what Delta sent.
    ix.transcript_fake()
        .push(user_line("u-b", "branch text with extra characters"));
    ix.on_user_prompt_submit(submit("branch text with extra characters"))
        .await
        .unwrap();

    // The reply is ingested at Stop and must carry forward to the child.
    ix.transcript_fake()
        .push(assistant_line("a-b", "branch reply"));
    ix.on_stop(crate::ports::StopHook {
        session_id: session.clone(),
        stop_reason: None,
    })
    .await
    .unwrap();

    // Both lines land on the child, so the branch pane shows the turn.
    let child_view = ix.thread_view(child).await.unwrap();
    let child_uuids: Vec<&str> = child_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(
        child_uuids.contains(&"u-b"),
        "the rewritten user line lands on the child; got {child_uuids:?}"
    );
    assert!(
        child_uuids.contains(&"a-b"),
        "the reply follows it onto the child; got {child_uuids:?}"
    );
    let user_msg = child_view
        .iter()
        .find(|m| m.uuid.as_str() == "u-b")
        .unwrap();
    assert_eq!(
        user_msg.semantic_parent_uuid,
        Some(parent),
        "the branch parent comes from the send, not from the text"
    );

    // And neither leaked onto main — the symptom the incident showed.
    let main_view = ix.thread_view(main).await.unwrap();
    let main_uuids: Vec<&str> = main_view.iter().map(|m| m.uuid.as_str()).collect();
    assert!(!main_uuids.contains(&"u-b"), "got {main_uuids:?}");
    assert!(!main_uuids.contains(&"a-b"), "got {main_uuids:?}");

    // The send row claims that line: `matched`, bound to its uuid — not
    // settled uuid-less at turn end.
    let settled = ix.store().send(pending.id).await.unwrap().unwrap();
    assert_eq!(settled.status, SendStatus::Matched);
    assert_eq!(
        settled.matched_uuid.as_ref().map(|u| u.as_str()),
        Some("u-b"),
        "the consumed send is bound to the line it was consumed by"
    );
}
