//! Claude Code's static provider profile: what the composition root needs to
//! know about Claude without a live session.
//!
//! Delta drives Claude through the Interactor's own tmux PTY, hook and
//! transcript paths in `delta-usecase`; nothing here launches or talks to a
//! `claude` process. What this crate holds is the provider-specific vocabulary
//! those paths and the composition root read:
//!
//! - [`CLAUDE_CAPABILITIES`], Claude's capability profile, read by the
//!   composition root's per-provider capability accessor;
//! - [`CLAUDE_LAUNCH_OPTION_CATALOG`], the launch options Delta ships for
//!   Claude;
//! - [`launch_option_cardinality`] and [`is_dangerous_launch_option`], which
//!   classify a Claude launch option's flag as repeatable or single-valued and
//!   as one that switches Claude Code's permission system off.
//!
//! [`launch_option_cardinality`]: fn@launch_option_cardinality
//! [`is_dangerous_launch_option`]: fn@is_dangerous_launch_option

use delta_usecase::{
    AgentCapabilities, ContextInjectionCapability, EventCapability, InterruptCapability,
    LaunchCapability, PermissionCapability, ResumeCapability, SessionIdentityCapability,
    SessionScopedAllowCapability, TerminalCapability, TranscriptCapability,
};

/// Claude's static capability profile, read by the composition root's
/// per-provider capability accessor.
///
/// Today's Claude reality: a tmux-hosted PTY, a Delta-pinned session id, resume
/// via `--resume`, events reconstructed from hooks + the JSONL transcript,
/// hook-carried permission decisions that answer one request each, hidden
/// per-turn context via the `UserPromptSubmit` hook, interrupt by injecting the
/// `Escape` keystroke, and an attachable pane.
pub const CLAUDE_CAPABILITIES: AgentCapabilities = AgentCapabilities {
    launch: LaunchCapability::PtyCommand,
    session_identity: SessionIdentityCapability::DeltaCanSetId,
    resume: ResumeCapability::Supported,
    events: EventCapability::HookAndTranscript,
    transcript: TranscriptCapability::JsonlFile,
    permission: PermissionCapability::HookDecision,
    // The permission hook's response carries a per-request `behavior` and has no
    // session-scoped form, so Delta cannot express one here at all — declared
    // rather than left implicit, so the browser never offers a button whose
    // press this provider would have to refuse.
    session_scoped_allow: SessionScopedAllowCapability::Unsupported,
    context_injection: ContextInjectionCapability::HiddenPerTurn,
    interrupt: InterruptCapability::PaneKeystroke,
    terminal: TerminalCapability::AttachablePty,
};

mod launch_option_catalog;
pub use launch_option_catalog::CLAUDE_LAUNCH_OPTION_CATALOG;

mod launch_option_cardinality;
pub use launch_option_cardinality::launch_option_cardinality;

mod launch_option_danger;
pub use launch_option_danger::is_dangerous_launch_option;
