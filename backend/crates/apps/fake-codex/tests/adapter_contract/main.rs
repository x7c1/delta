//! The provider-neutral adapter contract suite, run against
//! [`CodexAppServerAdapter`](codex_agent::CodexAppServerAdapter) driven by the
//! real `fake-codex` app-server binary.
//!
//! Two layers run here:
//!
//! 1. The **shared** cases from [`agent_contract`] — the mechanical operations
//!    every adapter must satisfy — run against the Codex adapter unchanged,
//!    proving the neutral contract is provider-independent.
//! 2. **Codex-specific** cases drive the scripted app-server through a full turn:
//!    the structured `turn/*` / `item/*` notifications translate into
//!    `TurnStarted` / `AssistantMessage` / `ToolStarted` / `ToolCompleted` /
//!    `TurnCompleted`, the real `item/commandExecution/requestApproval` and
//!    `item/fileChange/requestApproval` server requests become a
//!    `PermissionRequested` the adapter answers (allow → `accept`, deny →
//!    `decline`), `turn/interrupt` ends the turn, and — the invariant that
//!    matters most for an app-server with no interactive fallback — a server
//!    request Delta does not model (including `item/permissions/requestApproval`,
//!    whose response is a permission profile rather than a decision) surfaces as
//!    `UnsupportedInteraction` without the turn hanging.
//!
//! Correctness here is "against the fake": the wire shapes are the inferred
//! contract shared by `codex-agent`'s `wire`/`translate` modules and these
//! scenarios. Real-`codex` verification is a later phase.
//!
//! The suite is split by behaviour: the shared cases, the session lifecycle, the
//! turn translation, permissions, the unsupported server requests, usage, and the
//! file-change approval detail. `support` holds what more than one of them needs.

mod support;

mod file_change_detail;
mod permissions;
mod session_lifecycle;
mod shared_cases;
mod turn_translation;
mod unsupported_requests;
mod usage;
