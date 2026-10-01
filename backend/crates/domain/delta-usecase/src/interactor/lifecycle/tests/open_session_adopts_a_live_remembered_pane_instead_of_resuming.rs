//! The never-double-launch backstop: a send to a closed session whose
//! remembered tmux session is still running adopts that pane instead of
//! launching `claude --resume` next to it — whatever boot did or did not do.

use crate::interactor::testing::*;

use super::readoption_support::{left_behind, pane_for, survives, SURVIVING_TOKEN};

#[tokio::test]
async fn open_session_adopts_a_live_remembered_pane_instead_of_resuming() {
    let ix = interactor();
    // Boot never re-adopted this session (it was not run here), so the
    // session reads as closed while its agent is still up.
    let id = left_behind(&ix, "sess-A", SURVIVING_TOKEN).await;
    survives(&ix, SURVIVING_TOKEN);
    assert!(!ix.is_session_open(&id).await);

    let main = ix.store().main_thread_id(&id).await.unwrap();
    ix.enqueue_send(to(main), "hello again", None)
        .await
        .unwrap();

    assert!(
        ix.tmux_fake().created.lock().unwrap().is_empty(),
        "no `claude --resume` was launched beside the running agent"
    );
    assert_eq!(ix.bound_pane(&id).await, Some(pane_for(SURVIVING_TOKEN)));
    assert_eq!(
        ix.tmux_fake().sent.lock().unwrap().clone(),
        vec![(pane_for(SURVIVING_TOKEN), "hello again".to_owned())],
        "the send was typed into the surviving pane straight away"
    );
}

/// A remembered pane that is gone does not block the resume: its record is
/// cleared and the session resumes as it always has.
#[tokio::test]
async fn open_session_resumes_when_the_remembered_pane_is_gone() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", SURVIVING_TOKEN).await;

    ix.open_session(&id).await.unwrap();

    let created = ix.tmux_fake().created.lock().unwrap().clone();
    assert_eq!(created.len(), 1);
    assert!(created[0].command.iter().any(|a| a == "--resume"));
    assert_eq!(
        ix.store()
            .remembered_pane(&id)
            .await
            .unwrap()
            .map(|p| p.tmux_session),
        Some(created[0].name.clone()),
        "the record now names the resumed pane"
    );
}

/// A probe that cannot run is not "gone": resuming past it could start a
/// second agent beside a running one, so the open is refused and nothing is
/// launched.
#[tokio::test]
async fn open_session_refuses_to_resume_when_the_remembered_pane_cannot_be_probed() {
    let ix = interactor();
    let id = left_behind(&ix, "sess-A", SURVIVING_TOKEN).await;
    ix.tmux_fake().fail_probes();

    assert!(matches!(
        ix.open_session(&id).await,
        Err(crate::error::Error::Tmux(_))
    ));
    assert!(ix.tmux_fake().created.lock().unwrap().is_empty());
    assert!(ix.store().remembered_pane(&id).await.unwrap().is_some());
}

/// A stale record whose tmux session name was minted again for another
/// session is not adopted: Delta reuses `delta-<n>` names once tmux no longer
/// knows them, so a live `delta-1` may be someone else's pane. Binding that
/// other session takes the name away from the stale row, and the send resumes
/// it in a pane of its own.
#[tokio::test]
async fn open_session_does_not_adopt_a_name_minted_again_for_another_session() {
    let ix = interactor();
    // The record outlived its pane (its clear failed, or tmux could not be
    // asked at boot), so `delta-1` is free again in this process.
    let stale = left_behind(&ix, "sess-stale", "delta-1").await;

    ix.new_session().await.unwrap();
    let other = ix.pending_session_ids().await.remove(0);
    ix.on_user_prompt_submit(submit_in(
        other.as_str(),
        "/work/delta-1/t.jsonl",
        "/work/delta-1",
        "hi",
    ))
    .await
    .unwrap();
    assert_eq!(ix.bound_pane(&other).await, Some(pane_for("delta-1")));
    assert_eq!(
        ix.store().remembered_pane(&stale).await.unwrap(),
        None,
        "binding the reused name cleared the stale row's record"
    );

    ix.open_session(&stale).await.unwrap();

    let stale_pane = ix.bound_pane(&stale).await;
    assert_ne!(
        stale_pane,
        Some(pane_for("delta-1")),
        "the stale session was not bound to the other session's pane"
    );
    let created = ix.tmux_fake().created.lock().unwrap().clone();
    let resumed = created.last().expect("the stale session was resumed");
    assert!(resumed.command.iter().any(|a| a == "--resume"));
    assert_eq!(stale_pane, Some(pane_for(&resumed.name)));
}
