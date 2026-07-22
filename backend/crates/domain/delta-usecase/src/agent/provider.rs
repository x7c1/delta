//! The set of AI-agent providers Delta can drive.
//!
//! The provider identity enum lives in `delta-model` because it is a persisted
//! field of the [`delta_model::Session`] entity (like
//! [`delta_model::SessionStatus`]). The provider-neutral agent contract in this
//! crate re-exports it so callers still refer to a single `AgentProvider` type
//! at `delta_usecase::agent::AgentProvider`.
//!
//! The core never branches on this to decide behaviour — it consults
//! [`super::AgentCapabilities`] instead, so a new provider is a new capability
//! profile rather than a new `match` arm scattered across the code.
pub use delta_model::AgentProvider;
