//! Resuming a Codex session across a server restart, at the actor level.
//!
//! The multi-turn fix ([`codex_second_send_dispatches_over_the_adapter`]) covers
//! the *same-process* case: `open_agent()` is still `Some`, so a subsequent send
//! dispatches over the live binding. This covers the case that binding is **gone**
//! — the process restarted, so the runtime starts from `SessionRuntime::default()`
//! and `open_agent()` is `None`, while the persisted session row + provider ids
//! survive. A send that arrives now must reconnect the adapter via `thread/resume`
//! (reattaching to the SAME provider thread) and dispatch, instead of falling into
//! Claude's `ensure_open()` → `open_session()` (`claude --resume`) path, which a
//! terminal-less session cannot take (`ResumeUnavailable`).
//!
//! [`codex_second_send_dispatches_over_the_adapter`]:
//! super::codex_second_send_dispatches_over_the_adapter

use std::time::Duration;

use delta_model::{AgentProvider, SessionId};

use crate::agent::{AgentEvent, TurnStatus};
use crate::error::Error;
use crate::interactor::testing::*;
use crate::SendTarget;

/// Poll `f` until it returns `Some`, or panic after a short deadline. The event
/// pump runs on a background task, so persisted content lands asynchronously;
/// this yields to it between checks.
async fn wait_for<T, F, Fut>(what: &str, mut f: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = f().await {
            return value;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timed out waiting for {what}");
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// A closed Codex session (its in-process binding lost across a restart, its
/// persisted row + provider ids intact) resumes over the adapter on the next
/// send: `thread/resume` reattaches to the same provider thread, the content
/// source is re-seeded at the persisted message count, the turn dispatches and
/// completes, and the conversation **continues** — existing messages preserved,
/// new ones appended with the next sequence numbers (no renumber, no duplicate).
#[tokio::test]
async fn codex_closed_session_send_resumes_across_restart() {
    let factory = FakeAgentFactory::new("thr_resume", Some("turn_x"));
    let events = factory.event_sender();
    let ix = interactor_with_codex_factory(factory.clone());

    // Turn 1: open the Codex session with a first prompt.
    let (first, _) = ix
        .enqueue_send(
            SendTarget::NewSession {
                pull_request_number: None,
                provider: AgentProvider::Codex,
                workdir: None,
                launch_option_ids: Vec::new(),
                worktree: None,
            },
            "first message",
            None,
        )
        .await
        .unwrap();
    ix.await_launch().await;
    let session_id = first.session_id.clone();
    let main_thread = ix.store().main_thread_id(&session_id).await.unwrap();

    // Drive turn 1's content through the pump so it persists: a user prompt, an
    // assistant reply, then the turn completes. These land as messages seq 0 and
    // 1 (a fresh spawn seeds the content source at 0).
    events
        .send(AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "first message".to_owned(),
            at_ms: None,
        })
        .expect("the pump's stream is live");
    events
        .send(AgentEvent::AssistantMessage {
            provider_item_id: "a1".to_owned(),
            text: "reply one".to_owned(),
            at_ms: None,
        })
        .expect("the pump's stream is live");
    events
        .send(AgentEvent::TurnCompleted {
            status: TurnStatus::Completed,
        })
        .expect("the pump's stream is live");

    wait_for("turn 1's two messages to persist", || {
        let ix = &ix;
        let sid = session_id.clone();
        async move { (ix.store().message_count(&sid).await.unwrap() == 2).then_some(()) }
    })
    .await;

    // Simulate the restart: drop the in-process binding (adapter + content source
    // + pump), leaving only the persisted row + provider ids — exactly the state
    // a fresh process boots into.
    ix.with_runtime(&session_id, |state| {
        let _ = state.remove_open_agent();
    })
    .await;
    let bound = ix
        .with_runtime(&session_id, |state| state.open_agent().is_some())
        .await;
    assert!(
        !bound,
        "the restart left the session closed (no bound agent)"
    );

    // Turn 2: a plain send into the same thread. Before the fix this fell to the
    // Claude resume path and failed; here it must reconnect via `thread/resume`
    // and dispatch over the freshly-bound adapter.
    let (second, events_out) = ix
        .enqueue_send(
            SendTarget::Thread {
                thread_id: main_thread,
                branch_from: None,
            },
            "second message",
            None,
        )
        .await
        .expect("the closed Codex session resumes and dispatches, not a Claude resume");
    assert_eq!(second.session_id, session_id, "same session");
    assert_eq!(second.thread_id, main_thread, "written against its thread");
    assert!(
        events_out.is_empty(),
        "a Codex dispatch returns no synchronous session events"
    );

    // The reconnect reattached to the SAME provider thread via `resume` (not a
    // fresh `launch`), and the content source was re-seeded at the persisted
    // count (2) — not 0, which would renumber history. The fresh spawn's seed (0)
    // and the resume's seed (2) are both recorded, in order.
    {
        let log = factory.log();
        let log = log.lock().unwrap();
        let resumed: Vec<&str> = log
            .resumes
            .iter()
            .map(|req| req.provider_session_id.as_str())
            .collect();
        assert_eq!(
            resumed,
            vec!["thr_resume"],
            "resume reattached to the persisted provider thread id"
        );
        let seeds: Vec<i64> = log.content_requests.iter().map(|r| r.seed_seq).collect();
        assert_eq!(
            seeds,
            vec![0, 2],
            "the content source seeds at 0 on spawn and at the persisted count on resume"
        );
        assert_eq!(
            log.sends,
            vec!["first message".to_owned(), "second message".to_owned()],
            "both turns dispatched over the adapter"
        );
    }
    let rebound = ix
        .with_runtime(&session_id, |state| state.open_agent().is_some())
        .await;
    assert!(rebound, "the resume rebound the open agent");

    // Drive turn 2's content through the reconnected pump: it must append after
    // the preserved history (seq 2 and 3), not overwrite or renumber it.
    let events2 = factory.event_sender();
    events2
        .send(AgentEvent::UserPromptAccepted {
            provider_message_id: None,
            text: "second message".to_owned(),
            at_ms: None,
        })
        .expect("the reconnected pump's stream is live");
    events2
        .send(AgentEvent::AssistantMessage {
            provider_item_id: "a2".to_owned(),
            text: "reply two".to_owned(),
            at_ms: None,
        })
        .expect("the reconnected pump's stream is live");
    events2
        .send(AgentEvent::TurnCompleted {
            status: TurnStatus::Completed,
        })
        .expect("the reconnected pump's stream is live");

    wait_for("turn 2's two messages to append", || {
        let ix = &ix;
        let sid = session_id.clone();
        async move { (ix.store().message_count(&sid).await.unwrap() == 4).then_some(()) }
    })
    .await;

    // The persisted conversation continued: four messages, seqs 0..=3 with no gap
    // or duplicate, turn 1's two messages preserved and turn 2's two appended.
    let messages = ix.store().thread_messages(main_thread).await.unwrap();
    let seqs: Vec<i64> = messages.iter().map(|m| m.seq).collect();
    assert_eq!(
        seqs,
        vec![0, 1, 2, 3],
        "history continued with contiguous sequence numbers (no renumber/duplicate)"
    );
    let texts: Vec<&str> = messages
        .iter()
        .map(|m| m.content_text.as_deref().unwrap_or(""))
        .collect();
    assert_eq!(
        texts,
        vec!["first message", "reply one", "second message", "reply two"],
        "the resumed turn's messages append after the preserved originals"
    );
}

/// A closed **Claude** session's send is untouched by the Codex resume branch:
/// even with a Codex adapter factory wired into the interactor, a Claude session
/// takes the pane/`claude --resume` path — proven here by a missing transcript
/// still surfacing `ResumeUnavailable`, with the Codex adapter never contacted.
#[tokio::test]
async fn closed_claude_session_send_still_takes_the_claude_path() {
    let factory = FakeAgentFactory::new("thr_unused", Some("turn_unused"));
    let ix = interactor_with_codex_factory(factory.clone());

    // Register a Claude session (default provider), then make its transcript
    // unresumable — the classic closed-Claude case.
    ix.on_user_prompt_submit(submit_in(
        "sess-claude",
        "/elsewhere/t.jsonl",
        "/elsewhere",
        "seed",
    ))
    .await
    .unwrap();
    let id = SessionId::from("sess-claude");
    ix.transcript_fake().mark_missing("/elsewhere/t.jsonl");
    let main = ix.store().main_thread_id(&id).await.unwrap();

    let err = ix
        .enqueue_send(
            SendTarget::Thread {
                thread_id: main,
                branch_from: None,
            },
            "after restart",
            None,
        )
        .await
        .expect_err("a Claude session with a missing transcript is resume-unavailable");
    assert!(
        matches!(err, Error::ResumeUnavailable(ref s) if s == "sess-claude"),
        "the Claude resume path ran (ResumeUnavailable), got: {err:?}"
    );

    // The Codex adapter was never contacted for a Claude session.
    let log = factory.log();
    let log = log.lock().unwrap();
    assert!(
        log.resumes.is_empty() && log.launches.is_empty() && log.sends.is_empty(),
        "the Codex adapter was never touched on the Claude path"
    );
}
