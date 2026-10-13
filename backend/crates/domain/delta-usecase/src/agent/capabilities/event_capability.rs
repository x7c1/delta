//! How the provider delivers its event stream.

/// How the provider delivers its event stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCapability {
    /// Structured, id-tagged turn/item events pushed over the wire (Codex).
    StructuredTurnEvents,
    /// Reconstructed from HTTP hooks plus a JSONL transcript tail (Claude).
    HookAndTranscript,
}
