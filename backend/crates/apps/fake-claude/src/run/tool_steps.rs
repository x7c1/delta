//! The tool-call steps: a `tool_use` and everything that refers back to it —
//! its `PostToolUse`, permission prompt, `tool_result`, and the background
//! task's `<task-notification>` or `TaskOutput` retrieval.

use delta_wire::hooks::{PermissionRequestPayload, PostToolUsePayload, PreToolUsePayload};
use serde_json::{json, Value};

use crate::scenario::Step;

use super::engine::Engine;

/// The tool Claude Code calls to RETRIEVE a background task's result. Mirrors
/// `delta_attribution::claude_format::TASK_OUTPUT_TOOL_NAME`; the fake-claude
/// crate deliberately does not depend on the domain crate (it speaks only the
/// wire contract), so the name is restated here.
const TASK_OUTPUT_TOOL_NAME: &str = "TaskOutput";

/// Whether a `tool_use` step should be treated as a background launch — i.e.
/// the launch returns immediately and a `<task-notification>` later reports
/// completion, so an `agentId` is minted alongside the tool_use id.
///
/// Mirrors `delta_attribution::claude_format::launches_in_background`: modern
/// Claude Code dropped the `run_in_background` parameter from the
/// `Agent`/`Task` schema and made those calls async by default, so the absence
/// of the key means background for those tools. An explicit value is still
/// honoured for any tool (so a Bash invocation still needs the opt-in to mint
/// an agent id). The fake-claude crate deliberately does not depend on the
/// domain crate (it speaks only the wire contract), so the predicate is
/// duplicated here; the unit test in `claude_format` is the authoritative
/// definition.
fn launches_in_background(tool_name: &str, tool_input: &Value) -> bool {
    let explicit = tool_input.get("run_in_background").and_then(Value::as_bool);
    match (tool_name, explicit) {
        (_, Some(b)) => b,
        ("Agent" | "Task", None) => true,
        _ => false,
    }
}

/// The most recent `tool_use`, kept so `permission_request` and `tool_result`
/// steps can refer to it without restating the call.
pub(super) struct ToolUse {
    id: String,
    name: String,
    input: Value,
    /// The background-task identifier the launching tool's `tool_result`
    /// reports for an `Agent` launched with `run_in_background: true`. Minted
    /// at `tool_use` time so the following `post_tool_use` step can include it
    /// in `tool_response.agentId`, and a later `task_notification` step can
    /// emit it in the `<task-id>` element. A `TaskOutput` retrieval carries
    /// over the id of the task it read, so a following retrieval step can name
    /// the same task; `None` for every other call.
    task_id: Option<String>,
}

impl Engine {
    /// `tool_use`: write the call and fire `PreToolUse`, remembering it for the
    /// steps that refer back to it.
    pub(super) fn tool_use(&mut self, name: &str, input: &Value) -> Result<(), String> {
        let id = format!("toolu_fake_{:04}", self.tool_use_seq);
        // Background `Agent`/`Task` launches mint a background-task
        // identifier alongside the tool_use id; the real `claude`
        // reports it as `agentId` in the launching tool's tool_result
        // (PostToolUse) and again in the eventual `<task-notification>`
        // body. Mint one here for the same kinds so the same id ties
        // both observations together.
        let task_id = if launches_in_background(name, input) {
            Some(format!("agent_fake_{:04}", self.tool_use_seq))
        } else {
            None
        };
        self.tool_use_seq += 1;
        self.transcript.assistant_blocks(vec![json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input,
        })])?;
        self.fire(
            "PreToolUse",
            &self.endpoints.pre_tool_use,
            &PreToolUsePayload {
                session_id: self.session_id.clone(),
                tool_name: name.to_owned(),
                tool_input: input.clone(),
                tool_use_id: id.clone(),
                transcript_path: self.transcript_path.clone(),
            },
        );
        self.last_tool_use = Some(ToolUse {
            id,
            name: name.to_owned(),
            input: input.clone(),
            task_id,
        });
        Ok(())
    }

    /// `post_tool_use`: fire `PostToolUse` for the most recent tool call.
    pub(super) fn post_tool_use(&mut self) -> Result<(), String> {
        // Signal that the most recent tool call completed, mirroring the
        // real `claude` PostToolUse hook. Used to close a subagent's
        // running window without writing a `tool_result`.
        //
        // For a background `Agent` launch the real hook's
        // `tool_response` carries the subagent's `agentId` — Delta
        // records it on the launch row as a fallback correlation key
        // for the eventual `<task-notification>`. Surface the same
        // shape here so the server's PostToolUse handler hits the
        // background-upgrade branch.
        let tool_use = self
            .last_tool_use
            .as_ref()
            .ok_or("post_tool_use step without a preceding tool_use")?;
        let tool_response = tool_use
            .task_id
            .as_deref()
            .map(|task_id| json!({ "agentId": task_id }))
            .unwrap_or(Value::Null);
        self.fire(
            "PostToolUse",
            &self.endpoints.post_tool_use,
            &PostToolUsePayload {
                session_id: self.session_id.clone(),
                tool_name: tool_use.name.clone(),
                tool_use_id: tool_use.id.clone(),
                tool_response,
                transcript_path: self.transcript_path.clone(),
            },
        );
        Ok(())
    }

    /// `permission_request`: fire `PermissionRequest` and run the branch the
    /// server's decision selects.
    pub(super) fn permission_request(
        &mut self,
        on_allow: &[Step],
        on_deny: &[Step],
    ) -> Result<(), String> {
        let tool_use = self
            .last_tool_use
            .as_ref()
            .ok_or("permission_request step without a preceding tool_use")?;
        let payload = PermissionRequestPayload {
            session_id: self.session_id.clone(),
            tool_name: tool_use.name.clone(),
            tool_input: tool_use.input.clone(),
            transcript_path: self.transcript_path.clone(),
        };
        // `fire` reads the whole response before returning, so this
        // BLOCKS until the server's permission hook responds — exactly
        // like the real `claude` awaiting its permission hook. The
        // server holds the response until a browser decision or its
        // decision deadline (env-shrunk under e2e, well inside the
        // socket read timeout in hooks.rs); the body then either
        // carries `hookSpecificOutput.decision.behavior` or is the
        // empty passthrough.
        let body = self.fire(
            "PermissionRequest",
            &self.endpoints.permission_request,
            &payload,
        );
        let behavior = body
            .as_deref()
            .and_then(|b| serde_json::from_str::<Value>(b).ok())
            .and_then(|v| {
                v.pointer("/hookSpecificOutput/decision/behavior")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        let branch = match behavior.as_deref() {
            Some("allow") => on_allow,
            Some("deny") => on_deny,
            Some(other) => return Err(format!("unknown permission decision behavior: {other}")),
            // Empty passthrough: no decision was made in the browser,
            // so the dialog stays with the TUI — the scenario's
            // following steps script that path.
            None => return Ok(()),
        };
        for step in branch {
            self.execute(step)?;
        }
        Ok(())
    }

    /// `tool_result`: write the most recent tool call's result.
    pub(super) fn tool_result(&mut self, is_error: bool) -> Result<(), String> {
        let id = self
            .last_tool_use
            .as_ref()
            .map(|t| t.id.clone())
            .ok_or("tool_result step without a preceding tool_use")?;
        self.transcript.tool_result(&id, is_error)
    }

    /// `task_notification`: write the background task's completion line.
    pub(super) fn task_notification(&mut self, drop_tool_use_id: bool) -> Result<(), String> {
        // The harness-injected completion line for a background tool
        // call: a `<task-notification>` user line correlating back to
        // the launching tool call. The server folds it and finishes
        // the background subagent's running window. With
        // `drop_tool_use_id: true` the body omits the `<tool-use-id>`
        // element — the recent Claude Code shape that motivated the
        // task-id fallback correlation — so only `<task-id>` is
        // available to match against.
        let tool_use = self
            .last_tool_use
            .as_ref()
            .ok_or("task_notification step without a preceding tool_use")?;
        let task_id = tool_use.task_id.as_deref().ok_or(
            "task_notification step requires the preceding tool_use to be a background \
             Agent (run_in_background: true) so a task_id was minted",
        )?;
        self.transcript
            .task_notification(&tool_use.id, task_id, drop_tool_use_id)
    }

    /// `task_output`: retrieve the background task's result with a
    /// `TaskOutput` call.
    pub(super) fn task_output(&mut self, status: &str) -> Result<(), String> {
        // The parent reading a background task's result ITSELF: a
        // `TaskOutput` call naming the task minted at launch, blocking
        // until it finishes. Claude Code injects NO
        // `<task-notification>` for a task consumed this way, so the
        // retrieval's own `tool_result` — written below — is the only
        // signal the server can clear the running indicator from.
        let task_id = self
            .last_tool_use
            .as_ref()
            .ok_or("task_output step without a preceding tool_use")?
            .task_id
            .clone()
            .ok_or(
                "task_output step requires the preceding tool_use to be a background \
                 Agent (async by default, or run_in_background: true) so a task_id \
                 was minted",
            )?;
        let id = format!("toolu_fake_{:04}", self.tool_use_seq);
        self.tool_use_seq += 1;
        let input = json!({ "task_id": task_id, "block": true });
        self.transcript.assistant_blocks(vec![json!({
            "type": "tool_use",
            "id": id,
            "name": TASK_OUTPUT_TOOL_NAME,
            "input": input,
        })])?;
        self.fire(
            "PreToolUse",
            &self.endpoints.pre_tool_use,
            &PreToolUsePayload {
                session_id: self.session_id.clone(),
                tool_name: TASK_OUTPUT_TOOL_NAME.to_owned(),
                tool_input: input.clone(),
                tool_use_id: id.clone(),
                transcript_path: self.transcript_path.clone(),
            },
        );
        self.fire(
            "PostToolUse",
            &self.endpoints.post_tool_use,
            &PostToolUsePayload {
                session_id: self.session_id.clone(),
                tool_name: TASK_OUTPUT_TOOL_NAME.to_owned(),
                tool_use_id: id.clone(),
                // A retrieval is not a launch, so its response carries
                // no `agentId`.
                tool_response: Value::Null,
                transcript_path: self.transcript_path.clone(),
            },
        );
        self.transcript.task_output_result(&id, &task_id, status)?;
        // The retrieval is now the most recent tool call, exactly as
        // the real transcript records it. It keeps the task it read,
        // so a FOLLOWING `task_output` step retrieves the same task
        // again — the poll-then-finished sequence `status: "running"`
        // exists for, which would otherwise fail for want of a
        // preceding launch.
        self.last_tool_use = Some(ToolUse {
            id,
            name: TASK_OUTPUT_TOOL_NAME.to_owned(),
            input,
            task_id: Some(task_id),
        });
        Ok(())
    }
}
