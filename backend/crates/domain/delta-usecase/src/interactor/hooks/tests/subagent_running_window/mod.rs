//! The subagent running-indicator window, now driven by the parent's
//! transcript ingest.
//!
//! Lighting the indicator is no longer a side effect of `PreToolUse(Agent)`:
//! the parent's JSONL contains the assistant message whose `tool_use(Agent)`
//! block is the authoritative signal, and `sync_transcript` emits
//! `Effect::SubagentIndicatorStarted` for it (see
//! [`delta_attribution::attribute_lines`]). `PreToolUse(Agent)` then force-syncs
//! the parent transcript so the indicator lights with the same low latency as
//! before. The mechanism naturally excludes nested subagents: a nested
//! `tool_use(Agent)` lands in the SUBAGENT's JSONL, never the parent's, so the
//! parent's ingest cannot accidentally produce a stuck indicator (the
//! depth>=2 regression PR #190's `transcript_path` filter could not catch on
//! Claude Code 2.1.193).
//!
//! Foreground vs background and the turn-end sweep are unchanged.

mod background;
mod foreground;
mod foreground_run_of_a_background_launch;
mod task_id_fallback;

/// A modern `Agent` tool_use input shape: no `run_in_background` key. Modern
/// Claude Code dropped the parameter from the schema and made `Agent`/`Task`
/// calls async by default, so this input is treated as background — the same
/// path [`BACKGROUND_AGENT_INPUT`] takes via the explicit flag. Use
/// [`FOREGROUND_AGENT_INPUT`] for the older explicit-foreground shape that
/// stays foreground.
const AGENT_INPUT: &str =
    r#"{"subagent_type":"general-purpose","description":"Run ls and count entries","prompt":"…"}"#;

/// An `Agent` launched with `run_in_background: true`: its `PostToolUse` fires
/// at launch (the call returned, not the subagent), and its completion arrives
/// later as a `<task-notification>`.
const BACKGROUND_AGENT_INPUT: &str = r#"{"subagent_type":"general-purpose","description":"Long crawl","prompt":"…","run_in_background":true}"#;

/// An `Agent` launched with an explicit `run_in_background: false`. Modern
/// Claude Code makes `Agent`/`Task` calls async by default, so the explicit
/// flag is what keeps the call foreground: its `PostToolUse(Agent)` closes the
/// indicator window when the subagent itself finishes, and a still-running
/// foreground subagent is swept at turn end.
const FOREGROUND_AGENT_INPUT: &str = r#"{"subagent_type":"general-purpose","description":"Run ls and count entries","prompt":"…","run_in_background":false}"#;

fn running_tool_use_ids(state: &crate::SessionLiveState) -> Vec<String> {
    state
        .running_subagents
        .iter()
        .map(|s| s.tool_use_id.clone())
        .collect()
}

fn is_background(state: &crate::SessionLiveState, tool_use_id: &str) -> bool {
    state
        .running_subagents
        .iter()
        .find(|s| s.tool_use_id == tool_use_id)
        .map(|s| s.background)
        .unwrap_or_else(|| panic!("no running subagent {tool_use_id}"))
}
