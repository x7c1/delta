//! The line shapes the fake writes: prompts, assistant blocks, tool results,
//! harness-injected notifications, the `/compact` group, and the uuid-less
//! `queue-operation` bookkeeping line. Each one goes through the writer's
//! common envelope (`TranscriptWriter::append` in the parent module).

use serde_json::{json, Value};

use super::timestamp::rfc3339_now;
use super::{TranscriptWriter, INTERRUPT_MARKER};

impl TranscriptWriter {
    /// Append a `type: "user"` line carrying a bare-string prompt.
    pub fn user_text(&mut self, text: &str) -> Result<(), String> {
        let message = json!({ "role": "user", "content": text });
        self.append("user", json!({ "message": message }))
    }

    /// Append the `type: "user"` line a dequeued prompt is replayed as: a
    /// plain user line (claude stamps it `promptSource: "queued"`), exactly
    /// like a TUI-typed prompt apart from that provenance field.
    pub fn dequeued_user_text(&mut self, text: &str) -> Result<(), String> {
        let message = json!({ "role": "user", "content": text });
        self.append(
            "user",
            json!({ "message": message, "promptSource": "queued" }),
        )
    }

    /// Append the interrupt marker (a `role: user` line belonging to the
    /// aborted turn, not a new human turn).
    pub fn interrupt_marker(&mut self) -> Result<(), String> {
        self.user_text(INTERRUPT_MARKER)
    }

    /// Append a `type: "assistant"` line with typed content blocks.
    pub fn assistant_blocks(&mut self, blocks: Vec<Value>) -> Result<(), String> {
        let message = json!({ "role": "assistant", "content": blocks });
        self.append("assistant", json!({ "message": message }))
    }

    /// Append the `tool_result` carrier: a `role: user` line with no
    /// author-written text, belonging to the in-flight turn.
    pub fn tool_result(&mut self, tool_use_id: &str, is_error: bool) -> Result<(), String> {
        let message = json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": if is_error { "User rejected tool use" } else { "done" },
                "is_error": is_error,
            }],
        });
        self.append("user", json!({ "message": message }))
    }

    /// Append the harness-injected `<task-notification>` line claude writes when
    /// a background tool call (`run_in_background: true`) completes: a plain
    /// `role: user` line whose correlation elements identify the launching
    /// tool call. It belongs to the in-flight turn (a programmatic
    /// continuation, not a new human turn), exactly like a `tool_result`.
    ///
    /// `task_id` is the background-task identifier (the launch's `agentId`)
    /// and is always written into `<task-id>`. `tool_use_id` is the launching
    /// tool's id and is written into `<tool-use-id>` unless `omit_tool_use_id`
    /// is set — recent Claude Code versions drop that element while keeping
    /// `<task-id>`, so omitting it here lets a scenario reproduce that exact
    /// shape.
    pub fn task_notification(
        &mut self,
        tool_use_id: &str,
        task_id: &str,
        omit_tool_use_id: bool,
    ) -> Result<(), String> {
        let body = if omit_tool_use_id {
            format!(
                "<task-notification>\n\
                 <task-id>{task_id}</task-id>\n\
                 <status>completed</status>\n\
                 </task-notification>"
            )
        } else {
            format!(
                "<task-notification>\n\
                 <task-id>{task_id}</task-id>\n\
                 <tool-use-id>{tool_use_id}</tool-use-id>\n\
                 <status>completed</status>\n\
                 </task-notification>"
            )
        };
        self.user_text(&body)
    }

    /// Append the `tool_result` carrier of a `TaskOutput` retrieval: the
    /// report Claude Code writes when the parent reads a background task's
    /// result itself. `<retrieval_status>` says whether the retrieval worked;
    /// `<task_id>` names the task read (note the UNDERSCORE — the
    /// harness-injected `<task-notification>` spells the same id `<task-id>`)
    /// and `<status>` its state (`completed`/`failed`/`killed` when finished,
    /// `running` for a non-blocking poll of one still working).
    ///
    /// No `<task-notification>` follows a retrieval, so this line is the only
    /// evidence the retrieved task is over.
    ///
    /// The bytes mirror a real retrieval report: the block's `content` is a
    /// PLAIN STRING (not the array of text blocks a model-authored result
    /// uses), the elements are separated by blank lines, and `<task_type>`
    /// sits between the id and the status — so the e2e exercises the same
    /// shape the server parses in production.
    pub fn task_output_result(
        &mut self,
        tool_use_id: &str,
        task_id: &str,
        status: &str,
    ) -> Result<(), String> {
        let body = format!(
            "<retrieval_status>success</retrieval_status>\n\n\
             <task_id>{task_id}</task_id>\n\n\
             <task_type>local_agent</task_type>\n\n\
             <status>{status}</status>\n\n\
             <output>\nfake-claude background agent output\n</output>"
        );
        let message = json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": body,
                "is_error": false,
            }],
        });
        self.append("user", json!({ "message": message }))
    }

    /// Write the four-line group Claude Code produces when `/compact` runs
    /// (auto- or manually-triggered): a leading `<local-command-caveat>` user
    /// line flagged `isMeta`, the bare command-name line (`/compact`), the
    /// summary line flagged `isCompactSummary` carrying the previous-
    /// conversation summary, and the captured `<local-command-stdout>`. All
    /// four share a single `promptId` — the attribution layer recognizes the
    /// group by it. The summary line is the trigger for
    /// `Effect::AutoCompactFinished` on the Delta side.
    ///
    /// The four lines land in ONE append with ONE timestamp (see
    /// [`Self::append_atomic_group`]); they must never be written one at a
    /// time, or a tail poll can cut the group in half.
    pub fn compact_group(&mut self) -> Result<(), String> {
        let prompt_id = format!("prompt_compact_{}", self.next_seq);
        self.append_atomic_group(vec![
            // Caveat (isMeta=true).
            (
                "user",
                json!({
                    "message": {
                        "role": "user",
                        "content": "<local-command-caveat>Caveat: The messages below \
                                    were generated by the user while running local \
                                    commands. DO NOT respond to these \
                                    messages...</local-command-caveat>",
                    },
                    "isMeta": true,
                    "promptId": &prompt_id,
                }),
            ),
            // Bare command-name.
            (
                "user",
                json!({
                    "message": { "role": "user", "content": "/compact" },
                    "promptId": &prompt_id,
                }),
            ),
            // Summary (isCompactSummary=true).
            (
                "user",
                json!({
                    "message": {
                        "role": "user",
                        "content": "<summary of the previous conversation>",
                    },
                    "isCompactSummary": true,
                    "promptId": &prompt_id,
                }),
            ),
            // Captured stdout — folded to Meta by the gateway parser via its
            // content marker (no flag needed).
            (
                "user",
                json!({
                    "message": {
                        "role": "user",
                        "content": "<local-command-stdout>Compacted.</local-command-stdout>",
                    },
                    "promptId": &prompt_id,
                }),
            ),
        ])
    }

    /// Append the bookkeeping line claude writes when a prompt is submitted
    /// while a turn is in flight: a **uuid-less** `queue-operation` enqueue
    /// record carrying the queued text. It does not join the uuid chain — the
    /// prompt's real message is the plain user line written at dequeue.
    pub fn queue_operation_enqueue(&mut self, content: &str) -> Result<(), String> {
        let line = json!({
            "type": "queue-operation",
            "operation": "enqueue",
            "content": content,
            "sessionId": self.session_id,
            "timestamp": rfc3339_now(),
        });
        self.write_line(&line)?;
        // The line still occupies a transcript row; keep the uuid seed
        // tracking the file position like every other append.
        self.next_seq += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{read_lines, temp_path};
    use super::*;

    #[test]
    fn queue_operation_enqueue_line_is_uuid_less_and_off_the_chain() {
        let path = temp_path("queued");
        let _ = std::fs::remove_file(&path);
        let mut writer = TranscriptWriter::open(&path, "sess-3", "/work").unwrap();
        writer.user_text("first").unwrap();
        writer.queue_operation_enqueue("later please").unwrap();
        writer.dequeued_user_text("later please").unwrap();

        let lines = read_lines(&path);
        assert_eq!(lines[1]["type"], "queue-operation");
        assert_eq!(lines[1]["operation"], "enqueue");
        assert_eq!(lines[1]["content"], "later please");
        assert!(lines[1].get("uuid").is_none(), "enqueue line is uuid-less");
        // The dequeued replay is a plain user line chaining past the
        // bookkeeping line, stamped with its provenance.
        assert_eq!(lines[2]["type"], "user");
        assert_eq!(lines[2]["message"]["content"], "later please");
        assert_eq!(lines[2]["promptSource"], "queued");
        assert_eq!(lines[2]["parentUuid"], "sess-3-u0");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compact_group_lands_as_four_lines_sharing_one_prompt_id_and_timestamp() {
        let path = temp_path("compact-group");
        let _ = std::fs::remove_file(&path);
        let mut writer = TranscriptWriter::open(&path, "sess-5", "/work").unwrap();
        writer.user_text("before").unwrap();
        let appends_before = writer.appends;
        writer.compact_group().unwrap();

        // The whole group reaches the file in ONE write — the property the
        // server's attribution fold depends on (it never seeds a group's
        // `promptId` from the store, so a tail poll cutting the group in half
        // would misread the bare `/compact` line as a human turn). The shared
        // `timestamp` asserted below cannot stand in for this check: at the
        // writer's second precision, four separate appends would carry the
        // same stamp on nearly every run.
        assert_eq!(
            writer.appends - appends_before,
            1,
            "the four lines must land as a single append"
        );

        let lines = read_lines(&path);
        assert_eq!(lines.len(), 5, "one prior line plus the four-line group");
        let group = &lines[1..];

        // One promptId and one timestamp across the whole group, as a real
        // single-append group carries.
        let prompt_id = group[0]["promptId"].as_str().unwrap();
        let timestamp = group[0]["timestamp"].as_str().unwrap();
        for line in group {
            assert_eq!(line["type"], "user");
            assert_eq!(line["promptId"], prompt_id);
            assert_eq!(line["timestamp"], timestamp);
        }

        // Consecutive uuids chained through parentUuid, continuing from the
        // line written before the group.
        assert_eq!(group[0]["uuid"], "sess-5-u1");
        assert_eq!(group[0]["parentUuid"], "sess-5-u0");
        assert_eq!(group[1]["uuid"], "sess-5-u2");
        assert_eq!(group[1]["parentUuid"], "sess-5-u1");
        assert_eq!(group[2]["uuid"], "sess-5-u3");
        assert_eq!(group[2]["parentUuid"], "sess-5-u2");
        assert_eq!(group[3]["uuid"], "sess-5-u4");
        assert_eq!(group[3]["parentUuid"], "sess-5-u3");

        // The flags that identify the group's members.
        assert_eq!(group[0]["isMeta"], true);
        assert_eq!(group[1]["message"]["content"], "/compact");
        assert_eq!(group[2]["isCompactSummary"], true);
        assert!(group[1].get("isMeta").is_none());
        assert!(group[3].get("isCompactSummary").is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn appends_after_a_compact_group_continue_the_chain() {
        let path = temp_path("compact-group-after");
        let _ = std::fs::remove_file(&path);
        {
            let mut writer = TranscriptWriter::open(&path, "sess-6", "/work").unwrap();
            writer.compact_group().unwrap();
            writer.user_text("right after").unwrap();
        }
        // Reopening resumes numbering past the group, too.
        let mut writer = TranscriptWriter::open(&path, "sess-6", "/work").unwrap();
        writer.user_text("after reopen").unwrap();

        let lines = read_lines(&path);
        assert_eq!(lines.len(), 6);
        // The group opened the transcript, so its head line chains from
        // nothing (the writer had no previous uuid to parent it to).
        assert_eq!(lines[0]["uuid"], "sess-6-u0");
        assert_eq!(lines[0]["parentUuid"], Value::Null);
        assert_eq!(lines[3]["uuid"], "sess-6-u3");
        assert_eq!(lines[4]["uuid"], "sess-6-u4");
        assert_eq!(lines[4]["parentUuid"], "sess-6-u3");
        assert_eq!(lines[4]["message"]["content"], "right after");
        assert_eq!(lines[5]["uuid"], "sess-6-u5");
        assert_eq!(lines[5]["parentUuid"], "sess-6-u4");
        let _ = std::fs::remove_file(&path);
    }
}
