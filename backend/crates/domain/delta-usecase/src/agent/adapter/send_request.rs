//! Inputs for sending a user prompt into an open session.

/// Inputs for sending a user prompt into an open session.
#[derive(Debug, Clone)]
pub struct SendRequest {
    /// The visible prompt text, exactly as it should appear to the user. Any
    /// hidden per-turn context is injected out of band by the adapter
    /// (capability [`crate::agent::ContextInjectionCapability::HiddenPerTurn`]) and is
    /// deliberately NOT part of this text.
    pub text: String,
}
