//! Writing the JSONL transcript the way Claude Code does.
//!
//! Delta never sees the fake's process state — it reads the transcript file
//! whose path the hook payloads report. So the lines written here mirror the
//! shapes Claude Code writes (and Delta parses): `type: "user"`/`"assistant"`
//! lines with a `uuid`/`parentUuid` chain and a `message.content` that is a
//! bare string or an array of typed blocks, plus the uuid-less
//! `queue-operation` bookkeeping line for a prompt queued mid-turn and the
//! `[Request interrupted by user]` marker.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// Text of the marker line Claude Code writes when the user interrupts the
/// in-flight turn (the plain mid-response variant).
pub const INTERRUPT_MARKER: &str = "[Request interrupted by user]";

/// An append-only JSONL transcript with a consistent `uuid`/`parentUuid` chain.
pub struct TranscriptWriter {
    path: PathBuf,
    session_id: String,
    /// Sequence number of the next line, also seeding its uuid. Starts at the
    /// existing line count so a resume continues the numbering.
    next_seq: usize,
    /// The previous line's uuid, recovered from the file on resume so appended
    /// lines keep chaining.
    last_uuid: Option<String>,
}

impl TranscriptWriter {
    /// Open (or create) the transcript at `path`, scanning any existing lines
    /// so appended lines continue the resume's uuid chain and numbering.
    pub fn open(path: &Path, session_id: &str) -> Result<Self, String> {
        let existing = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(err) => return Err(format!("read transcript {}: {err}", path.display())),
        };
        let lines: Vec<&str> = existing.lines().filter(|l| !l.trim().is_empty()).collect();
        // The chain parent is the last line that HAS a uuid: bookkeeping lines
        // (`queue-operation`) are uuid-less and do not participate in the chain.
        let last_uuid = lines
            .iter()
            .rev()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find_map(|value| value["uuid"].as_str().map(|s| s.to_owned()));
        Ok(Self {
            path: path.to_owned(),
            session_id: session_id.to_owned(),
            next_seq: lines.len(),
            last_uuid,
        })
    }

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
    pub fn compact_group(&mut self) -> Result<(), String> {
        let prompt_id = format!("prompt_compact_{}", self.next_seq);
        // Caveat (isMeta=true).
        let caveat_msg = json!({
            "role": "user",
            "content": "<local-command-caveat>Caveat: The messages below were \
                        generated by the user while running local commands. \
                        DO NOT respond to these messages...</local-command-caveat>",
        });
        self.append(
            "user",
            json!({
                "message": caveat_msg,
                "isMeta": true,
                "promptId": &prompt_id,
            }),
        )?;
        // Bare command-name.
        let name_msg = json!({ "role": "user", "content": "/compact" });
        self.append(
            "user",
            json!({ "message": name_msg, "promptId": &prompt_id }),
        )?;
        // Summary (isCompactSummary=true).
        let summary_msg = json!({
            "role": "user",
            "content": "<summary of the previous conversation>",
        });
        self.append(
            "user",
            json!({
                "message": summary_msg,
                "isCompactSummary": true,
                "promptId": &prompt_id,
            }),
        )?;
        // Captured stdout — folded to Meta by the gateway parser via its
        // content marker (no flag needed).
        let stdout_msg = json!({
            "role": "user",
            "content": "<local-command-stdout>Compacted.</local-command-stdout>",
        });
        self.append(
            "user",
            json!({ "message": stdout_msg, "promptId": &prompt_id }),
        )
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

    /// Append one line of `line_type` with the common envelope (uuid chain,
    /// session id, timestamp) merged over `extra`'s fields.
    fn append(&mut self, line_type: &str, extra: Value) -> Result<(), String> {
        let uuid = format!("{}-u{}", self.session_id, self.next_seq);
        let mut line = json!({
            "uuid": uuid,
            "parentUuid": self.last_uuid,
            "type": line_type,
            "sessionId": self.session_id,
            "timestamp": rfc3339_now(),
        });
        if let (Value::Object(target), Value::Object(fields)) = (&mut line, extra) {
            target.extend(fields);
        }
        self.write_line(&line)?;

        self.last_uuid = Some(uuid);
        self.next_seq += 1;
        Ok(())
    }

    /// Append one raw JSONL line.
    fn write_line(&self, line: &Value) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("open transcript {}: {e}", self.path.display()))?;
        writeln!(file, "{line}").map_err(|e| format!("append transcript line: {e}"))
    }
}

/// The current wall-clock time as an RFC 3339 UTC timestamp (second
/// precision), without pulling in a date-time dependency for one format.
fn rfc3339_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rfc3339_from_unix(secs as i64)
}

/// Format unix seconds as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Uses the standard civil-from-days algorithm (Howard Hinnant's
/// `civil_from_days`) for the date part.
fn rfc3339_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };

    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("fake-claude-transcript-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{name}-{}.jsonl", std::process::id()))
    }

    fn read_lines(path: &Path) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn chains_uuids_across_lines() {
        let path = temp_path("chain");
        let _ = std::fs::remove_file(&path);
        let mut writer = TranscriptWriter::open(&path, "sess-1").unwrap();
        writer.user_text("hello").unwrap();
        writer
            .assistant_blocks(vec![json!({"type": "text", "text": "hi"})])
            .unwrap();

        let lines = read_lines(&path);
        assert_eq!(lines[0]["uuid"], "sess-1-u0");
        assert_eq!(lines[0]["parentUuid"], Value::Null);
        assert_eq!(lines[0]["type"], "user");
        assert_eq!(lines[0]["message"]["content"], "hello");
        assert_eq!(lines[1]["uuid"], "sess-1-u1");
        assert_eq!(lines[1]["parentUuid"], "sess-1-u0");
        assert_eq!(lines[1]["message"]["content"][0]["text"], "hi");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reopening_continues_the_chain() {
        let path = temp_path("reopen");
        let _ = std::fs::remove_file(&path);
        {
            let mut writer = TranscriptWriter::open(&path, "sess-2").unwrap();
            writer.user_text("first").unwrap();
        }
        let mut writer = TranscriptWriter::open(&path, "sess-2").unwrap();
        writer.user_text("second").unwrap();

        let lines = read_lines(&path);
        assert_eq!(lines[1]["uuid"], "sess-2-u1");
        assert_eq!(lines[1]["parentUuid"], "sess-2-u0");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn queue_operation_enqueue_line_is_uuid_less_and_off_the_chain() {
        let path = temp_path("queued");
        let _ = std::fs::remove_file(&path);
        let mut writer = TranscriptWriter::open(&path, "sess-3").unwrap();
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
    fn reopening_skips_uuid_less_lines_when_recovering_the_chain() {
        let path = temp_path("reopen-queued");
        let _ = std::fs::remove_file(&path);
        {
            let mut writer = TranscriptWriter::open(&path, "sess-4").unwrap();
            writer.user_text("first").unwrap();
            writer.queue_operation_enqueue("queued").unwrap();
        }
        let mut writer = TranscriptWriter::open(&path, "sess-4").unwrap();
        writer.user_text("second").unwrap();

        let lines = read_lines(&path);
        assert_eq!(lines[2]["uuid"], "sess-4-u2");
        assert_eq!(lines[2]["parentUuid"], "sess-4-u0");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn formats_known_unix_seconds() {
        assert_eq!(rfc3339_from_unix(0), "1970-01-01T00:00:00Z");
        // 2026-01-01T00:00:00Z
        assert_eq!(rfc3339_from_unix(1_767_225_600), "2026-01-01T00:00:00Z");
    }
}
