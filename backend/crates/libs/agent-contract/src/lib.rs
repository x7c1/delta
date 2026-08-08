//! The provider-neutral adapter contract suite, shared across every
//! [`AgentAdapter`] implementation.
//!
//! Each case is written generically over `&impl AgentAdapter` and reads the
//! adapter's declared [`AgentCapabilities`] to decide what to assert, so the
//! *same* case body runs against the Claude adapter and the Codex adapter (and
//! any future provider) unchanged. A concrete adapter's test module supplies the
//! fixture and calls the case; this crate owns the assertions.
//!
//! ## What lives here vs. in an adapter's own tests
//!
//! Only the cases drivable purely through the [`AgentAdapter`] trait live here —
//! launch/send/interrupt/close and the events those operations emit directly.
//! Cases that need a provider-specific *stimulus* to reach the interesting state
//! (Claude's hook/transcript ingestion seam; Codex's scripted app-server
//! notifications and its `resolve_permission` reply path) are driven from that
//! adapter's own test module, because the way the stimulus is delivered is
//! itself provider-specific. Those adapter-side cases still assert the same
//! neutral [`AgentEvent`]s this crate's cases do.

use delta_usecase::{
    AgentAdapter, AgentEvent, AgentEventStream, ContextInjectionCapability, InterruptCapability,
    LaunchRequest, SendRequest, SessionEndReason,
};

/// A launch request usable by any adapter. Providers that let Delta pin the
/// session id ([`delta_usecase::SessionIdentityCapability::DeltaCanSetId`]) adopt
/// this `session_id`; providers that mint their own ignore it.
pub fn launch_request() -> LaunchRequest {
    LaunchRequest {
        session_id: "01920000-0000-7000-8000-000000000001".to_owned(),
        workdir: "/tmp/workdir".to_owned(),
        launch_options: Vec::new(),
        first_prompt: None,
    }
}

/// Drain every event currently buffered on `stream`, returning once the stream
/// is closed. The caller must ensure the adapter (and thus the sender) is gone,
/// or this blocks waiting for more.
pub async fn drain(stream: &mut AgentEventStream) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    while let Some(event) = stream.recv().await {
        events.push(event);
    }
    events
}

/// `launch_returns_provider_session_id`: launching yields a handle carrying a
/// provider session id, and the stream opens with a matching `SessionStarted`.
pub async fn case_launch_returns_provider_session_id<A: AgentAdapter>(adapter: &A) {
    let handle = adapter.launch(launch_request()).await.expect("launch");
    assert!(
        !handle.provider_session_id.is_empty(),
        "launch must return a provider session id"
    );
    let mut stream = adapter.events(&handle);
    match stream.recv().await {
        Some(AgentEvent::SessionStarted {
            provider_session_id,
        }) => assert_eq!(provider_session_id, handle.provider_session_id),
        other => panic!("expected SessionStarted, got {other:?}"),
    }
}

/// `send_emits_user_prompt_accepted`: a send surfaces the prompt as an accepted
/// user prompt on the event stream. (The `TurnStarted` half of the plan's
/// `send_emits_user_prompt_and_turn_started` is asserted per-adapter, where the
/// turn-start stimulus is provider-specific.)
pub async fn case_send_emits_user_prompt_accepted<A: AgentAdapter>(adapter: &A) {
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    assert!(matches!(
        stream.recv().await,
        Some(AgentEvent::SessionStarted { .. })
    ));
    adapter
        .send(
            &handle,
            SendRequest {
                text: "hello agent".to_owned(),
            },
        )
        .await
        .expect("send");
    match stream.recv().await {
        Some(AgentEvent::UserPromptAccepted { text, .. }) => assert_eq!(text, "hello agent"),
        other => panic!("expected UserPromptAccepted, got {other:?}"),
    }
}

/// `context_injection_does_not_pollute_visible_prompt` (asserted only for
/// `HiddenPerTurn` providers): the visible prompt the adapter reports is exactly
/// what was sent — hidden per-turn context is injected out of band, never folded
/// into the visible text.
pub async fn case_context_injection_does_not_pollute_visible_prompt<A: AgentAdapter>(adapter: &A) {
    if adapter.capabilities().context_injection != ContextInjectionCapability::HiddenPerTurn {
        return;
    }
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    assert!(matches!(
        stream.recv().await,
        Some(AgentEvent::SessionStarted { .. })
    ));
    let visible = "just the user's words";
    adapter
        .send(
            &handle,
            SendRequest {
                text: visible.to_owned(),
            },
        )
        .await
        .expect("send");
    match stream.recv().await {
        Some(AgentEvent::UserPromptAccepted { text, .. }) => assert_eq!(
            text, visible,
            "the visible prompt must not carry injected context"
        ),
        other => panic!("expected UserPromptAccepted, got {other:?}"),
    }
}

/// `interrupt_is_accepted_when_supported` (skipped for
/// `InterruptCapability::Unsupported`): interrupting an open session succeeds.
/// The turn-ending event it produces is asserted per-adapter.
pub async fn case_interrupt_is_accepted_when_supported<A: AgentAdapter>(adapter: &A) {
    if adapter.capabilities().interrupt == InterruptCapability::Unsupported {
        return;
    }
    let handle = adapter.launch(launch_request()).await.expect("launch");
    adapter.interrupt(&handle).await.expect("interrupt");
}

/// `close_ends_the_session`: closing an open session succeeds and emits
/// `SessionEnded`.
pub async fn case_close_ends_the_session<A: AgentAdapter>(adapter: &A) {
    let handle = adapter.launch(launch_request()).await.expect("launch");
    let mut stream = adapter.events(&handle);
    assert!(matches!(
        stream.recv().await,
        Some(AgentEvent::SessionStarted { .. })
    ));
    adapter.close(&handle).await.expect("close");
    match stream.recv().await {
        Some(AgentEvent::SessionEnded { reason }) => {
            assert_eq!(reason, SessionEndReason::Closed)
        }
        other => panic!("expected SessionEnded, got {other:?}"),
    }
}
