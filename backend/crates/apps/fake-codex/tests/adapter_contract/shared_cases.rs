//! The shared mechanical cases every adapter must satisfy, run unchanged
//! against the Codex adapter.

use crate::support::default_adapter;

#[tokio::test]
async fn launch_returns_provider_session_id() {
    agent_contract::case_launch_returns_provider_session_id(&default_adapter().await).await;
}

#[tokio::test]
async fn send_emits_user_prompt_accepted() {
    agent_contract::case_send_emits_user_prompt_accepted(&default_adapter().await).await;
}

#[tokio::test]
async fn context_injection_does_not_pollute_visible_prompt() {
    agent_contract::case_context_injection_does_not_pollute_visible_prompt(
        &default_adapter().await,
    )
    .await;
}

#[tokio::test]
async fn interrupt_is_accepted_when_supported() {
    agent_contract::case_interrupt_is_accepted_when_supported(&default_adapter().await).await;
}

#[tokio::test]
async fn close_ends_the_session() {
    agent_contract::case_close_ends_the_session(&default_adapter().await).await;
}
