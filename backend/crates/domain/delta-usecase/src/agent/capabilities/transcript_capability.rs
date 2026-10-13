//! Whether, and how, a durable transcript is available.

/// Whether, and how, a durable transcript is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptCapability {
    /// A JSONL transcript file the adapter tails (Claude).
    JsonlFile,
    /// The transcript is only what the pushed event stream carries.
    EventStreamOnly,
    /// No transcript is available.
    None,
}
