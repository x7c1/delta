//! [`JsonlTranscript`]: the concrete [`Transcript`] gateway.

use async_trait::async_trait;
use tokio::fs;

use delta_usecase::{Transcript, TranscriptRead};

use crate::error::Error;
use crate::parse::{correlate_turn_durations, parse_line_outcome, ParsedLine};

/// Reads Claude Code JSONL transcripts from the filesystem.
///
/// `read_from` re-reads the file and skips lines already seen by their 0-based
/// line index. For a local single-session tool the transcript is small, so a
/// full read on each hook is simple and correct; callers that want true
/// streaming can poll this on an interval. Lines that produce no message —
/// blank, no-uuid (e.g. `file-history-snapshot`), or unparsable — still advance
/// the line index, so the reported `total_lines` and each message's `seq` track
/// the file's true position and the caller's line cursor reads each line exactly
/// once. Unparsable non-blank lines are skipped (with a warning) rather than
/// aborting the whole read, so one malformed line cannot stall ingestion.
///
/// Only newline-terminated lines are consumed. The transcript is appended
/// incrementally with unbuffered `writeln!`, which emits the JSON body and its
/// terminating `\n` as separate write syscalls, so a read that races a
/// mid-append line can observe a partial final line (truncated JSON, or
/// complete JSON without its `\n` yet). Counting that partial line into
/// `total_lines` would advance the caller's persistent cursor past it; once the
/// line is completed it would sit before the cursor and never be re-read,
/// permanently dropping it. So a trailing remainder with no `\n` is treated as
/// a line still being written and is deliberately deferred to the next read:
/// iteration and `total_lines` cover only the prefix up to and including the
/// last `\n`, which contains only fully-written lines.
#[derive(Debug, Default, Clone)]
pub struct JsonlTranscript;

impl JsonlTranscript {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Transcript for JsonlTranscript {
    async fn read_from(
        &self,
        path: &str,
        from_line: usize,
    ) -> std::result::Result<TranscriptRead, delta_usecase::Error> {
        let contents = match fs::read_to_string(path).await {
            Ok(c) => c,
            // A not-yet-created transcript is not an error: nothing to read.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(TranscriptRead {
                    messages: Vec::new(),
                    total_lines: 0,
                })
            }
            Err(e) => return Err(Error::from(e).into()),
        };

        // Consume only newline-terminated lines: a trailing remainder without a
        // `\n` is a line still being appended, so defer it to the next read
        // instead of counting it (see the type doc for why). Slicing up to and
        // including the last `\n` yields the terminated prefix (an empty slice
        // when no newline has been written yet), whose `lines()` never yields a
        // partial line.
        let terminated = match contents.rfind('\n') {
            Some(last) => &contents[..=last],
            None => "",
        };

        // Parse every line in range into an outcome first, keeping `turn_duration`
        // outcomes interleaved in file order, so a turn's latency can be
        // correlated onto the turn's assistant message (it is written right after
        // it). Message outcomes already carry their absolute line index as `seq`.
        let mut outcomes = Vec::new();
        let mut total_lines = 0;
        for (idx, line) in terminated.lines().enumerate() {
            total_lines = idx + 1;
            if idx < from_line {
                continue;
            }
            match parse_line_outcome(line) {
                Ok(ParsedLine::Message(mut msg)) => {
                    // The message's absolute line index is its persisted `seq`.
                    msg.seq = idx as i64;
                    outcomes.push(ParsedLine::Message(msg));
                }
                Ok(other @ ParsedLine::TurnDuration { .. }) => outcomes.push(other),
                Ok(ParsedLine::Skip) => {}
                Err(err) => {
                    tracing::warn!(error = %err, "skipping unparsable transcript line");
                }
            }
        }

        // Stamp each turn's response time onto its assistant message, then drop
        // the now-consumed `turn_duration` outcomes — they are not messages.
        correlate_turn_durations(&mut outcomes);
        let messages = outcomes
            .into_iter()
            .filter_map(|outcome| match outcome {
                ParsedLine::Message(msg) => Some(*msg),
                ParsedLine::TurnDuration { .. } | ParsedLine::Skip => None,
            })
            .collect();

        Ok(TranscriptRead {
            messages,
            total_lines,
        })
    }

    async fn exists(&self, path: &str) -> std::result::Result<bool, delta_usecase::Error> {
        fs::try_exists(path)
            .await
            .map_err(|e| Error::from(e).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn reads_from_offset_and_skips_seen_lines() {
        let mut file = tempfile_jsonl();
        writeln!(
            file,
            r#"{{"uuid":"u1","type":"user","message":{{"content":"a","role":"user"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"uuid":"u2","type":"user","message":{{"content":"b","role":"user"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        let path = file.path().to_str().unwrap().to_owned();

        let t = JsonlTranscript::new();
        let all = t.read_from(&path, 0).await.unwrap();
        assert_eq!(all.messages.len(), 2);
        assert_eq!(all.total_lines, 2);
        // Each message carries its absolute 0-based line index as its seq.
        assert_eq!(all.messages[0].seq, 0);
        assert_eq!(all.messages[1].seq, 1);

        let tail = t.read_from(&path, 1).await.unwrap();
        assert_eq!(tail.messages.len(), 1);
        assert_eq!(tail.total_lines, 2);
        assert_eq!(tail.messages[0].flatten_text().as_deref(), Some("b"));
        assert_eq!(tail.messages[0].seq, 1);
    }

    #[tokio::test]
    async fn missing_file_is_empty_not_error() {
        let t = JsonlTranscript::new();
        let out = t.read_from("/nonexistent/transcript.jsonl", 0).await.unwrap();
        assert!(out.messages.is_empty());
        assert_eq!(out.total_lines, 0);
    }

    #[tokio::test]
    async fn exists_reports_presence() {
        let file = tempfile_jsonl();
        let path = file.path().to_str().unwrap().to_owned();

        let t = JsonlTranscript::new();
        assert!(t.exists(&path).await.unwrap(), "an existing file is present");
        assert!(
            !t.exists("/nonexistent/transcript.jsonl").await.unwrap(),
            "a missing file is absent, not an error"
        );
    }

    #[tokio::test]
    async fn malformed_line_is_skipped() {
        let mut file = tempfile_jsonl();
        writeln!(file, "not json at all").unwrap();
        writeln!(
            file,
            r#"{{"uuid":"u1","type":"user","message":{{"content":"ok","role":"user"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        let path = file.path().to_str().unwrap().to_owned();

        let out = JsonlTranscript::new().read_from(&path, 0).await.unwrap();
        assert_eq!(out.messages.len(), 1);
        // The skipped malformed line still counts toward total_lines and shifts
        // the parsed message's seq to its true index.
        assert_eq!(out.total_lines, 2);
        assert_eq!(out.messages[0].seq, 1);
    }

    /// A no-uuid line (e.g. Claude Code's `file-history-snapshot`) interleaved
    /// between two real turns must not consume a message's line index: the
    /// surrounding messages keep their true file positions as `seq`, and
    /// `total_lines` counts every line. This is the file-position invariant the
    /// line-based cursor relies on to read each line exactly once.
    #[tokio::test]
    async fn no_uuid_line_advances_index_without_a_message() {
        let mut file = tempfile_jsonl();
        writeln!(
            file,
            r#"{{"uuid":"u1","type":"user","message":{{"content":"q","role":"user"}}}}"#
        )
        .unwrap();
        writeln!(file, r#"{{"type":"file-history-snapshot","messageId":"x"}}"#).unwrap();
        writeln!(
            file,
            r#"{{"uuid":"a1","type":"assistant","message":{{"content":"r","role":"assistant"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        let path = file.path().to_str().unwrap().to_owned();

        let out = JsonlTranscript::new().read_from(&path, 0).await.unwrap();
        assert_eq!(out.messages.len(), 2, "only the two uuid-bearing lines parse");
        assert_eq!(out.total_lines, 3, "the no-uuid line still counts");
        assert_eq!(out.messages[0].seq, 0);
        assert_eq!(out.messages[1].seq, 2, "assistant sits after the skipped line");
    }

    /// A line still mid-append (no trailing `\n` yet, possibly truncated JSON)
    /// must not be consumed: it is neither returned as a message nor counted
    /// toward `total_lines`, so the caller's persistent line cursor never
    /// advances past content that is not all there yet.
    #[tokio::test]
    async fn unterminated_final_line_is_deferred() {
        let mut file = tempfile_jsonl();
        writeln!(
            file,
            r#"{{"uuid":"u1","type":"user","message":{{"content":"a","role":"user"}}}}"#
        )
        .unwrap();
        // A second line written WITHOUT a terminating newline — exactly what a
        // reader racing the appender can observe mid-write.
        write!(
            file,
            r#"{{"uuid":"u2","type":"user","message":{{"content":"b","role":"user"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        let path = file.path().to_str().unwrap().to_owned();

        let t = JsonlTranscript::new();
        let out = t.read_from(&path, 0).await.unwrap();
        assert_eq!(out.messages.len(), 1, "only the terminated line is consumed");
        assert_eq!(
            out.total_lines, 1,
            "the unterminated final line does not count"
        );
        assert_eq!(out.messages[0].seq, 0);
        assert_eq!(out.messages[0].flatten_text().as_deref(), Some("a"));
    }

    /// Once the deferred line gains its trailing newline, reading again from the
    /// previous `total_lines` returns it exactly once with its true `seq` — no
    /// permanent loss and no double-read.
    #[tokio::test]
    async fn deferred_line_is_read_once_after_completion() {
        let mut file = tempfile_jsonl();
        writeln!(
            file,
            r#"{{"uuid":"u1","type":"user","message":{{"content":"a","role":"user"}}}}"#
        )
        .unwrap();
        write!(
            file,
            r#"{{"uuid":"u2","type":"user","message":{{"content":"b","role":"user"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        let path = file.path().to_str().unwrap().to_owned();

        let t = JsonlTranscript::new();
        let first = t.read_from(&path, 0).await.unwrap();
        assert_eq!(first.messages.len(), 1);
        assert_eq!(first.total_lines, 1);

        // The appender completes the line with its terminating newline.
        writeln!(file).unwrap();
        file.flush().unwrap();

        // Resume from the cursor the first read advanced to.
        let next = t.read_from(&path, first.total_lines).await.unwrap();
        assert_eq!(
            next.messages.len(),
            1,
            "the completed line is read exactly once"
        );
        assert_eq!(next.total_lines, 2);
        assert_eq!(next.messages[0].seq, 1, "at its true file position");
        assert_eq!(next.messages[0].flatten_text().as_deref(), Some("b"));
    }

    /// A turn's `turn_duration` system line back-fills `response_time_ms` onto
    /// the turn's assistant message through the full reader path, while leaving
    /// the system line itself out of the produced messages and keeping `seq`
    /// aligned to true file position.
    #[tokio::test]
    async fn turn_duration_back_fills_the_assistant_response_time() {
        let mut file = tempfile_jsonl();
        writeln!(
            file,
            r#"{{"uuid":"u1","type":"user","message":{{"content":"q","role":"user"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"uuid":"a1","type":"assistant","message":{{"role":"assistant","model":"claude-opus-4-8","content":[{{"type":"text","text":"r"}}]}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"uuid":"d1","type":"system","subtype":"turn_duration","durationMs":4221}}"#
        )
        .unwrap();
        file.flush().unwrap();
        let path = file.path().to_str().unwrap().to_owned();

        let out = JsonlTranscript::new().read_from(&path, 0).await.unwrap();
        // The user and assistant lines parse; the turn_duration line yields no
        // message but still counts toward total_lines.
        assert_eq!(out.messages.len(), 2);
        assert_eq!(out.total_lines, 3);
        let assistant = &out.messages[1];
        assert_eq!(assistant.seq, 1);
        assert_eq!(assistant.model.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(assistant.response_time_ms, Some(4221.0));
        assert_eq!(out.messages[0].response_time_ms, None);
    }

    /// A minimal temp-file helper avoiding an extra dependency.
    ///
    /// The suffix combines the pid with a process-global atomic counter rather
    /// than a timestamp: tests run on parallel threads in one process, and two
    /// that minted the same nanosecond stamp would collide on one path — then
    /// one test's `Drop` deletes the file the other is still reading, surfacing
    /// as a spurious "0 messages" failure. A monotonic counter is collision-free.
    fn tempfile_jsonl() -> TempJsonl {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut p = std::env::temp_dir();
        let unique = format!(
            "delta-transcript-test-{}-{}.jsonl",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        );
        p.push(unique);
        let file = std::fs::File::create(&p).unwrap();
        TempJsonl { path: p, file }
    }

    struct TempJsonl {
        path: std::path::PathBuf,
        file: std::fs::File,
    }

    impl TempJsonl {
        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Write for TempJsonl {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.file.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.file.flush()
        }
    }

    impl Drop for TempJsonl {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
