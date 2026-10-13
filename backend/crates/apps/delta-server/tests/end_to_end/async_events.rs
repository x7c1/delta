//! The async event seam from the interactor to the WebSocket broadcast.

use std::time::Duration;

use crate::support::build_app;

/// The async event seam reaches the broadcast end to end: an event emitted on
/// the interactor's [`emit_async_event`] seam — the asynchronous complement of
/// the synchronous hook/tick return path — is drained by
/// `spawn_async_event_drain` and delivered to a WebSocket subscriber, exactly
/// as a synchronously-broadcast event would be.
///
/// This proves the async-event plumbing: interactor sink → server drain task →
/// broadcast → subscriber. Real producers emit on the seam (the transcript
/// tail's ingest announcements, the Codex pump, deferred launch outcomes), but
/// this drives it directly through the interactor's public emit so a failure
/// here names the plumbing rather than whichever producer was used to reach it.
///
/// [`emit_async_event`]: delta_usecase::Interactor::emit_async_event
#[tokio::test]
async fn async_event_seam_reaches_the_broadcast() {
    let (_app, _tmux, transcript_path, state) = build_app();

    // Subscribe first (a broadcast subscriber only sees events sent after it
    // subscribes), then start the drain that forwards the seam into the
    // broadcast.
    let mut rx = state.subscribe();
    state
        .spawn_async_event_drain()
        .expect("the drain claims the receiver on first call");

    // Emit as an async producer would, after its driving call has returned.
    let event = delta_usecase::SessionEvent::SessionClosed {
        session_id: delta_usecase::SessionId::from("async-seam-session"),
    };
    state.interactor().emit_async_event(event.clone());

    // The subscriber receives the very event that was pushed onto the seam.
    let received = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("the drained event arrives within the timeout")
        .expect("the broadcast channel is open");
    assert_eq!(received, event);

    let _ = std::fs::remove_file(&transcript_path);
}

/// The receiver is single-use: `spawn_async_event_drain` hands it out exactly
/// once, so a second call is a no-op (`None`) rather than spawning a competing
/// drain that would steal events from the first.
#[tokio::test]
async fn async_event_drain_can_only_be_claimed_once() {
    let (_app, _tmux, transcript_path, state) = build_app();
    assert!(
        state.spawn_async_event_drain().is_some(),
        "the first call claims the receiver"
    );
    assert!(
        state.spawn_async_event_drain().is_none(),
        "a second call finds the receiver already taken"
    );
    let _ = std::fs::remove_file(&transcript_path);
}
