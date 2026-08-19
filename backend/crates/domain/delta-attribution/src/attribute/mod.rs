//! The attribution fold: parsed transcript lines in, attributed messages and
//! effects out.
//!
//! Split into one file per concern: the public data types
//! ([`OutstandingSend`], [`SubagentLaunch`], [`AttributionState`], [`Effect`],
//! [`Attributed`]), the [`attribute_lines`] orchestration loop, and the two
//! large per-line phases it delegates to — `content_blocks` (permission /
//! background-launch / indicator handling for a line's content blocks) and
//! `thread_resolution` (the thread the line is attributed to, and the send /
//! subagent effects that follow) — plus `forked_skill`, the harness-launched
//! background agent a slash command's skill runs as, which writes no
//! `tool_use` block for `content_blocks` to see.

mod attribute_lines;
mod attributed;
mod content_blocks;
mod effect;
mod forked_skill;
mod outstanding_send;
mod state;
mod subagent_launch;
mod thread_resolution;

pub use attribute_lines::attribute_lines;
pub use attributed::Attributed;
pub use effect::Effect;
pub use outstanding_send::OutstandingSend;
pub use state::AttributionState;
pub use subagent_launch::SubagentLaunch;
