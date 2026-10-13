//! Whether an in-flight turn can be interrupted, and by what means.

/// Whether an in-flight turn can be interrupted, and by what means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptCapability {
    /// Interrupt by injecting the interrupt keystroke into the PTY (Claude's
    /// `Escape`).
    PaneKeystroke,
    /// Interrupt via an explicit RPC (Codex's `turn/interrupt`).
    Rpc,
    /// Interrupt is not supported.
    Unsupported,
}
