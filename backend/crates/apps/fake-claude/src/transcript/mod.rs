//! Writing the JSONL transcript the way Claude Code does.
//!
//! Delta never sees the fake's process state — it reads the transcript file
//! whose path the hook payloads report. So the lines written here mirror the
//! shapes Claude Code writes (and Delta parses): `type: "user"`/`"assistant"`
//! lines with a `uuid`/`parentUuid` chain and a `message.content` that is a
//! bare string or an array of typed blocks, plus the uuid-less
//! `queue-operation` bookkeeping line for a prompt queued mid-turn, the
//! `[Request interrupted by user]` marker, and the move Claude Code makes when
//! the session enters a worktree (see [`TranscriptWriter::relocate`]). Like
//! Claude Code's, every line carries the session's working directory as a
//! top-level `cwd`.
//!
//! This module holds the writer itself — opening, moving, and the append
//! envelope every line shares; `lines` holds the line shapes, and `timestamp`
//! the clock formatting.

mod lines;
#[cfg(test)]
mod test_support;
mod timestamp;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use timestamp::rfc3339_now;

/// Text of the marker line Claude Code writes when the user interrupts the
/// in-flight turn (the plain mid-response variant).
pub const INTERRUPT_MARKER: &str = "[Request interrupted by user]";

/// An append-only JSONL transcript with a consistent `uuid`/`parentUuid` chain.
pub struct TranscriptWriter {
    path: PathBuf,
    session_id: String,
    /// The working directory stamped on every line; changes when the session
    /// relocates.
    cwd: String,
    /// Sequence number of the next line, also seeding its uuid. Starts at the
    /// existing line count so a resume continues the numbering.
    next_seq: usize,
    /// The previous line's uuid, recovered from the file on resume so appended
    /// lines keep chaining.
    last_uuid: Option<String>,
    /// How many physical appends (file writes) this writer has performed.
    /// Test-only seam: it is what lets a test assert that `compact_group`
    /// reaches the file in ONE write. The shared `timestamp` cannot prove
    /// that — [`rfc3339_now`] has second precision, so four separate appends
    /// would stamp the same second on nearly every run.
    #[cfg(test)]
    appends: usize,
}

impl TranscriptWriter {
    /// Open (or create) the transcript at `path`, scanning any existing lines
    /// so appended lines continue the resume's uuid chain and numbering.
    pub fn open(path: &Path, session_id: &str, cwd: &str) -> Result<Self, String> {
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
            cwd: cwd.to_owned(),
            next_seq: lines.len(),
            last_uuid,
            #[cfg(test)]
            appends: 0,
        })
    }

    /// Move the transcript to `new_path` the way Claude Code does when the
    /// session enters a worktree: the file is moved whole (the old path stops
    /// existing), then a uuid-less `relocated` line naming the new working
    /// directory is appended to it. Later lines carry the new `cwd`.
    pub fn relocate(&mut self, new_path: &Path, cwd: &str) -> Result<(), String> {
        if let Some(dir) = new_path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("create transcript dir {}: {e}", dir.display()))?;
        }
        std::fs::rename(&self.path, new_path).map_err(|e| {
            format!(
                "move transcript {} to {}: {e}",
                self.path.display(),
                new_path.display()
            )
        })?;
        self.path = new_path.to_owned();
        self.cwd = cwd.to_owned();
        let line = json!({
            "type": "relocated",
            "sessionId": self.session_id,
            "relocatedCwd": cwd,
        });
        self.write_line(&line)?;
        // Like `queue-operation`, the line occupies a row but joins no chain.
        self.next_seq += 1;
        Ok(())
    }

    /// Append one line of `line_type` with the common envelope (uuid chain,
    /// session id, cwd, timestamp) merged over `extra`'s fields.
    fn append(&mut self, line_type: &str, extra: Value) -> Result<(), String> {
        self.append_atomic_group(vec![(line_type, extra)])
    }

    /// Append a group of lines as ONE write, all sharing a single timestamp.
    ///
    /// Claude Code writes a local-command group (`/compact` and friends —
    /// caveat, bare command name, summary, captured stdout) as one atomic
    /// transcript append whose lines carry the same `timestamp`. Delta's
    /// attribution fold depends on that: it learns a group's `promptId` from
    /// the caveat line and does not seed that knowledge from the store,
    /// because the whole group always arrives in a single tail batch. A
    /// writer that emitted the lines one at a time could be cut by a tail
    /// poll landing mid-group, and the bare `/compact` line would then be
    /// misread as a freshly typed human turn. So the fake writes the group
    /// the same way the real thing does: build every line body first, stamp
    /// them together, and hand the whole blob to [`Self::append_to_file`],
    /// which writes it with one `write_all`.
    ///
    /// Each line advances `next_seq` and `last_uuid`, so the uuid chain and
    /// the resume scan in [`Self::open`] keep working across a group. A lone
    /// line is just the one-element case — that is what [`Self::append`]
    /// passes.
    fn append_atomic_group(&mut self, lines: Vec<(&str, Value)>) -> Result<(), String> {
        let timestamp = rfc3339_now();
        let mut uuids = Vec::with_capacity(lines.len());
        let mut blob = String::new();
        for (line_type, extra) in lines {
            let uuid = format!("{}-u{}", self.session_id, self.next_seq + uuids.len());
            let parent = match uuids.last() {
                Some(previous) => Some(previous),
                None => self.last_uuid.as_ref(),
            };
            let mut line = json!({
                "uuid": &uuid,
                "parentUuid": parent,
                "type": line_type,
                "sessionId": self.session_id,
                "cwd": self.cwd,
                "timestamp": &timestamp,
            });
            if let (Value::Object(target), Value::Object(fields)) = (&mut line, extra) {
                target.extend(fields);
            }
            blob.push_str(&line.to_string());
            blob.push('\n');
            uuids.push(uuid);
        }
        self.append_to_file(&blob)?;

        self.next_seq += uuids.len();
        if let Some(last) = uuids.pop() {
            self.last_uuid = Some(last);
        }
        Ok(())
    }

    /// Append one raw JSONL line.
    fn write_line(&mut self, line: &Value) -> Result<(), String> {
        self.append_to_file(&format!("{line}\n"))
    }

    /// Append `text` to the transcript file in a single write. The one place
    /// bytes reach the file, so it is also where `appends` ticks.
    fn append_to_file(&mut self, text: &str) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("open transcript {}: {e}", self.path.display()))?;
        file.write_all(text.as_bytes())
            .map_err(|e| format!("append transcript {}: {e}", self.path.display()))?;
        #[cfg(test)]
        {
            self.appends += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{read_lines, temp_path};
    use super::*;

    #[test]
    fn chains_uuids_across_lines() {
        let path = temp_path("chain");
        let _ = std::fs::remove_file(&path);
        let mut writer = TranscriptWriter::open(&path, "sess-1", "/work").unwrap();
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
            let mut writer = TranscriptWriter::open(&path, "sess-2", "/work").unwrap();
            writer.user_text("first").unwrap();
        }
        let mut writer = TranscriptWriter::open(&path, "sess-2", "/work").unwrap();
        writer.user_text("second").unwrap();

        let lines = read_lines(&path);
        assert_eq!(lines[1]["uuid"], "sess-2-u1");
        assert_eq!(lines[1]["parentUuid"], "sess-2-u0");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reopening_skips_uuid_less_lines_when_recovering_the_chain() {
        let path = temp_path("reopen-queued");
        let _ = std::fs::remove_file(&path);
        {
            let mut writer = TranscriptWriter::open(&path, "sess-4", "/work").unwrap();
            writer.user_text("first").unwrap();
            writer.queue_operation_enqueue("queued").unwrap();
        }
        let mut writer = TranscriptWriter::open(&path, "sess-4", "/work").unwrap();
        writer.user_text("second").unwrap();

        let lines = read_lines(&path);
        assert_eq!(lines[2]["uuid"], "sess-4-u2");
        assert_eq!(lines[2]["parentUuid"], "sess-4-u0");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn relocate_moves_the_file_whole_and_appends_a_relocated_line() {
        let old = temp_path("relocate-old");
        let new = std::env::temp_dir()
            .join("fake-claude-transcript-tests")
            .join(format!("relocated-{}", std::process::id()))
            .join("sess-7.jsonl");
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::remove_file(&new);
        let mut writer = TranscriptWriter::open(&old, "sess-7", "/work").unwrap();
        writer.user_text("before").unwrap();
        writer.relocate(&new, "/work/wt").unwrap();
        writer
            .assistant_blocks(vec![json!({"type": "text", "text": "after"})])
            .unwrap();

        assert!(!old.exists(), "the transcript is moved, not copied");
        let lines = read_lines(&new);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["message"]["content"], "before");
        assert_eq!(lines[0]["cwd"], "/work");
        assert_eq!(lines[1]["type"], "relocated");
        assert_eq!(lines[1]["relocatedCwd"], "/work/wt");
        assert!(
            lines[1].get("uuid").is_none(),
            "relocated line is uuid-less"
        );
        // The chain continues across the move, and later lines carry the new
        // working directory.
        assert_eq!(lines[2]["uuid"], "sess-7-u2");
        assert_eq!(lines[2]["parentUuid"], "sess-7-u0");
        assert_eq!(lines[2]["cwd"], "/work/wt");
        let _ = std::fs::remove_file(&new);
    }
}
