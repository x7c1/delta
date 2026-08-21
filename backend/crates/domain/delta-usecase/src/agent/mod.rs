//! The provider-neutral agent contract.
//!
//! This module holds the frozen contract that lets Delta drive more than one AI
//! agent behind one abstraction:
//!
//! - [`AgentProvider`] — which backend a session runs on (recorded/surfaced,
//!   never branched on for behaviour).
//! - [`AgentCapabilities`] — what a provider can and cannot do; the core keys
//!   UI and behaviour off this, not off the provider.
//! - [`AgentEvent`] — the single neutral fact stream the core reasons over.
//! - [`AgentAdapter`] — the trait every provider is driven through.
//!
//! Only neutral types live here. Provider-specific wire schema (JSON-RPC
//! shapes, hook payloads, transcript formats) belongs to the gateway adapters
//! that implement [`AgentAdapter`].

mod adapter;
mod capabilities;
mod content_source;
mod event;
mod factory;
mod provider;

pub use adapter::{
    AgentAdapter, AgentEventStream, AgentSessionHandle, ContentSourceRequest, LaunchOptionSpec,
    LaunchRequest, PtyHandle, ResumeRequest, SendReceipt, SendRequest,
};
pub use capabilities::{
    AgentCapabilities, ContextInjectionCapability, EventCapability, ForkCapability,
    InterruptCapability, LaunchCapability, PermissionCapability, ResumeCapability,
    SessionIdentityCapability, SessionScopedAllowCapability, SteerCapability, TerminalCapability,
    TranscriptCapability,
};
pub use content_source::{AgentContentSource, NullContentSource};
pub use event::{
    AgentEvent, AgentFileChange, AgentFileChangeDetail, AgentFileChangeKind,
    AgentPermissionRequest, AgentTokenUsage, SessionEndReason, TurnStatus,
};
pub use factory::AgentAdapterFactory;
pub use provider::AgentProvider;
