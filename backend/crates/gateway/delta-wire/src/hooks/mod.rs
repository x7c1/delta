//! The Claude Code hook contract (`/hooks/*` control plane).
//!
//! Claude Code fires HTTP hooks at the server during a session; these types
//! are the JSON bodies of those requests (and the one response body Claude
//! Code consumes, [`UserPromptSubmitResponse`]). They are wire types in the
//! same sense as the browser-facing ones: the field names and defaults belong
//! to an external contract — here Claude Code's hook schema — that the domain
//! must not know about. The server's hook handlers convert them into the
//! domain port types (`UserPromptSubmitHook`, `StopHook`, …) at the boundary.
//!
//! Unlike the rest of this crate, none of these types is exported to
//! TypeScript: the hooks cross between the local Claude Code process and the
//! server only, so the browser never sees them and `@delta/wire-gen` has no
//! business carrying them.
//!
//! The payload types derive `Serialize` as well as `Deserialize`: a driver
//! that impersonates Claude Code (such as the `fake-claude` test binary) or a
//! test emitting hook traffic constructs these exact types and serializes
//! them, so both sides of the contract share one definition instead of the
//! emitter hand-rolling JSON that could drift from what the server parses.

mod message_display_payload;
pub use message_display_payload::MessageDisplayPayload;
mod permission_request_payload;
pub use permission_request_payload::PermissionRequestPayload;
mod permission_request_response;
pub use permission_request_response::{
    PermissionDecisionBody, PermissionHookOutput, PermissionRequestResponse,
};
mod post_tool_use_payload;
pub use post_tool_use_payload::PostToolUsePayload;
mod pre_tool_use_payload;
pub use pre_tool_use_payload::PreToolUsePayload;
mod session_end_payload;
pub use session_end_payload::SessionEndPayload;
mod session_start_payload;
pub use session_start_payload::SessionStartPayload;
mod status_line_payload;
pub use status_line_payload::{
    StatusLineContextWindow, StatusLineCost, StatusLineModel, StatusLinePayload,
    StatusLineRateLimitWindow, StatusLineRateLimits, StatusLineWorkspace,
};
mod stop_payload;
pub use stop_payload::StopPayload;
mod user_prompt_submit_payload;
pub use user_prompt_submit_payload::UserPromptSubmitPayload;
mod user_prompt_submit_response;
pub use user_prompt_submit_response::{HookSpecificOutput, UserPromptSubmitResponse};
