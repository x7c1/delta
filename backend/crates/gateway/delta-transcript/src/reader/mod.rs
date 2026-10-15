//! [`JsonlTranscript`]: the concrete [`Transcript`] gateway.

mod find_session_transcript;

use std::io::Read;
use std::path::Path;

use async_trait::async_trait;
use tokio::fs;

use delta_usecase::{Transcript, TranscriptRead};

use crate::error::{Error, Result};
use crate::parse::{correlate_turn_durations, parse_line_outcome, ParsedLine};

/// The largest transcript [`JsonlTranscript`] reads, in bytes.
///
/// Every read takes the whole file (see the type doc), so without a bound one
/// huge file would make every hook — and every background tail tick — cost time
/// and memory in proportion to it. Real long sessions stay in the tens of
/// megabytes; 256 MiB is comfortably above them while keeping one read's
/// buffer bounded. A larger transcript is refused with [`Error::TooLarge`].
pub const MAX_TRANSCRIPT_BYTES: u64 = 256 * 1024 * 1024;

/// Reads Claude Code JSONL transcripts from the filesystem.
///
/// `read_from` re-reads the file and skips lines already seen by their 0-based
/// line index. For a local single-session tool the transcript is small, so a
/// full read on each hook is simple and correct; callers that want true
/// streaming can poll this on an interval. Lines that produce no message —
/// blank, no-uuid (e.g. `file-history-snapshot`), not UTF-8, or unparsable —
/// still advance the line index, so the reported `total_lines` and each
/// message's `seq` track the file's true position and the caller's line cursor
/// reads each line exactly once. Unparsable non-blank lines are skipped (with a
/// warning), and lines that are not UTF-8 are skipped (at `debug`, with their
/// line number), rather than aborting the whole read, so one malformed line
/// cannot stall ingestion.
///
/// The read is bounded: only a regular file is read — a FIFO, a device or a
/// directory is refused with [`Error::NotRegularFile`], and the file is opened
/// non-blocking so that check cannot itself block — and a file over
/// [`MAX_TRANSCRIPT_BYTES`] is refused with [`Error::TooLarge`].
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
        // The whole file is read on every call, and the caller skips the lines
        // before its cursor. Reading only the bytes past the last offset would
        // make a read cost what was appended rather than the file's size, but it
        // changes what the reader is handed (a byte offset, not a line index), so
        // it is left for a change to the `Transcript` contract; until then the
        // size cap bounds the cost.
        let owned_path = path.to_owned();
        let contents = tokio::task::spawn_blocking(move || read_transcript(Path::new(&owned_path)))
            .await
            .map_err(|source| Error::ReadAborted {
                path: path.into(),
                source,
            })??;
        // A not-yet-created transcript is not an error: nothing to read.
        let Some(contents) = contents else {
            return Ok(TranscriptRead {
                messages: Vec::new(),
                total_lines: 0,
                relocated_cwd: None,
            });
        };

        // Consume only newline-terminated lines: a trailing remainder without a
        // `\n` is a line still being appended, so defer it to the next read
        // instead of counting it (see the type doc for why). Splitting the
        // prefix that stops just before the last `\n` yields exactly the
        // complete lines (one empty line for a file that is a lone `\n`); with
        // no `\n` written yet there are none.
        let lines = contents
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|last| contents[..last].split(|&b| b == b'\n'))
            .into_iter()
            .flatten();

        // Parse every line in range into an outcome first, keeping `turn_duration`
        // outcomes interleaved in file order, so a turn's latency can be
        // correlated onto the turn's assistant message (it is written right after
        // it). Message outcomes already carry their absolute line index as `seq`.
        let mut outcomes = Vec::new();
        let mut total_lines = 0;
        let mut relocated_cwd = None;
        for (idx, line) in lines.enumerate() {
            total_lines = idx + 1;
            if idx < from_line {
                continue;
            }
            // Decoded line by line, so one line that is not UTF-8 costs only
            // itself. A `\r` before the `\n` is dropped, as `str::lines` does.
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let line = match std::str::from_utf8(line) {
                Ok(line) => line,
                Err(err) => {
                    tracing::debug!(
                        path,
                        line = idx + 1,
                        error = %err,
                        "skipping a transcript line that is not UTF-8"
                    );
                    continue;
                }
            };
            match parse_line_outcome(line) {
                Ok(ParsedLine::Message(mut msg)) => {
                    // The message's absolute line index is its persisted `seq`.
                    msg.seq = idx as i64;
                    outcomes.push(ParsedLine::Message(msg));
                }
                Ok(other @ ParsedLine::TurnDuration { .. }) => outcomes.push(other),
                // A moved transcript can carry several `relocated` lines (one
                // per move); the last one names where the session is now.
                Ok(ParsedLine::Relocated { cwd }) => relocated_cwd = Some(cwd),
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
                ParsedLine::TurnDuration { .. }
                | ParsedLine::Relocated { .. }
                | ParsedLine::Skip => None,
            })
            .collect();

        Ok(TranscriptRead {
            messages,
            total_lines,
            relocated_cwd,
        })
    }

    async fn exists(&self, path: &str) -> std::result::Result<bool, delta_usecase::Error> {
        fs::try_exists(path)
            .await
            .map_err(|e| Error::io(path, e).into())
    }

    async fn find_session_transcript(
        &self,
        root: &str,
        session_id: &str,
    ) -> std::result::Result<Option<String>, delta_usecase::Error> {
        find_session_transcript::find_session_transcript(root, session_id)
            .await
            .map_err(Into::into)
    }
}

/// The bytes of the transcript at `path`, or `None` when there is no file
/// there yet.
///
/// Opened non-blocking, so a FIFO at the path does not park this thread
/// waiting for a writer; the open file's own metadata then decides, so what is
/// checked is what is read. Anything but a regular file is refused, as is a
/// file over [`MAX_TRANSCRIPT_BYTES`] — including one that grows past it while
/// it is read, which the bounded read below catches.
fn read_transcript(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match open_nonblocking(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::io(path, err)),
    };
    let metadata = file.metadata().map_err(|err| Error::io(path, err))?;
    if !metadata.file_type().is_file() {
        return Err(Error::NotRegularFile {
            path: path.to_path_buf(),
        });
    }
    let too_large = |size| Error::TooLarge {
        path: path.to_path_buf(),
        size,
        cap: MAX_TRANSCRIPT_BYTES,
    };
    if metadata.len() > MAX_TRANSCRIPT_BYTES {
        return Err(too_large(metadata.len()));
    }
    // The file is regular, so the non-blocking flag has no effect on reading it.
    let mut contents = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_TRANSCRIPT_BYTES + 1)
        .read_to_end(&mut contents)
        .map_err(|err| Error::io(path, err))?;
    if contents.len() as u64 > MAX_TRANSCRIPT_BYTES {
        return Err(too_large(contents.len() as u64));
    }
    Ok(Some(contents))
}

#[cfg(unix)]
fn open_nonblocking(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_nonblocking(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
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

    /// A transcript Claude Code moved keeps every earlier line and gains a
    /// `relocated` line (plus other uuid-less bookkeeping): those occupy lines
    /// but produce no message, and the read reports the last relocated cwd.
    #[tokio::test]
    async fn relocated_lines_advance_the_index_and_report_the_last_cwd() {
        let mut file = tempfile_jsonl();
        writeln!(
            file,
            r#"{{"uuid":"u1","type":"user","message":{{"content":"a","role":"user"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"relocated","sessionId":"s1","relocatedCwd":"/work/wt-a"}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"worktree-state","worktreeSession":{{"worktreePath":"/work/wt-a"}},"sessionId":"s1"}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"relocated","relocatedCwd":"/work/wt-b","sessionId":"s1"}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"uuid":"a1","type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"b"}}]}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        let path = file.path().to_str().unwrap().to_owned();

        let t = JsonlTranscript::new();
        let all = t.read_from(&path, 0).await.unwrap();
        assert_eq!(all.total_lines, 5);
        assert_eq!(all.messages.len(), 2);
        assert_eq!(all.messages[1].seq, 4);
        assert_eq!(all.relocated_cwd.as_deref(), Some("/work/wt-b"));

        // A read starting past the relocated lines reports none.
        let tail = t.read_from(&path, 4).await.unwrap();
        assert_eq!(tail.relocated_cwd, None);
    }

    #[tokio::test]
    async fn missing_file_is_empty_not_error() {
        let t = JsonlTranscript::new();
        let out = t
            .read_from("/nonexistent/transcript.jsonl", 0)
            .await
            .unwrap();
        assert!(out.messages.is_empty());
        assert_eq!(out.total_lines, 0);
    }

    /// A read that fails for a reason other than a missing file surfaces an
    /// error naming the transcript it was reading.
    #[tokio::test]
    async fn a_failed_read_names_the_transcript_path() {
        // Reading a directory as a file fails with an IO error that is not
        // `NotFound`, so it is reported instead of read as empty.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap().to_owned();

        let t = JsonlTranscript::new();
        let err = t.read_from(&path, 0).await.unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(&path),
            "the error must name {path}, got {message:?}"
        );
    }

    /// A FIFO at the transcript path is refused as not a regular file, without
    /// blocking on the writer that never comes.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_fifo_is_refused_without_blocking() {
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("transcript.jsonl");
        let c_path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `c_path` is a valid NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let path = fifo.to_str().unwrap().to_owned();

        let read = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            JsonlTranscript::new().read_from(&path, 0),
        )
        .await
        .expect("reading a FIFO must not block");
        let message = read.unwrap_err().to_string();
        assert!(
            message.contains("not a regular file") && message.contains(&path),
            "the error names the FIFO as not a regular file, got {message:?}"
        );
        // The port renders the error to a message; the reader's own result
        // carries the variant. Safe to call directly: the read above proved the
        // open does not block.
        let err = read_transcript(&fifo).unwrap_err();
        assert!(
            matches!(&err, Error::NotRegularFile { path } if *path == fifo),
            "expected the not-a-regular-file error, got {err:?}"
        );
    }

    /// A transcript over the size cap is refused, not read (and not read as
    /// empty).
    #[test]
    fn a_file_over_the_cap_is_refused() {
        let file = tempfile_jsonl();
        // Sparse, so the test writes no real data.
        file.file.set_len(MAX_TRANSCRIPT_BYTES + 1).unwrap();

        let err = read_transcript(file.path()).unwrap_err();
        assert!(
            matches!(
                err,
                Error::TooLarge { size, cap, .. }
                    if size == MAX_TRANSCRIPT_BYTES + 1 && cap == MAX_TRANSCRIPT_BYTES
            ),
            "expected the too-large error, got {err:?}"
        );
    }

    /// The same refusal reaches the caller through the port, naming the file.
    #[tokio::test]
    async fn a_file_over_the_cap_is_an_error_for_the_caller() {
        let file = tempfile_jsonl();
        file.file.set_len(MAX_TRANSCRIPT_BYTES + 1).unwrap();
        let path = file.path().to_str().unwrap().to_owned();

        let message = JsonlTranscript::new()
            .read_from(&path, 0)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            message.contains(&path) && message.contains("limit"),
            "the error names the oversized transcript, got {message:?}"
        );
    }

    /// One line that is not UTF-8 is skipped on its own: every other line is
    /// still read, at its true position.
    #[tokio::test]
    async fn a_line_that_is_not_utf8_is_skipped_alone() {
        let mut file = tempfile_jsonl();
        writeln!(
            file,
            r#"{{"uuid":"u1","type":"user","message":{{"content":"a","role":"user"}}}}"#
        )
        .unwrap();
        file.write_all(b"{\"uuid\":\"bad\",\"note\":\"\xff\xfe\"}\n")
            .unwrap();
        writeln!(
            file,
            r#"{{"uuid":"u3","type":"user","message":{{"content":"c","role":"user"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        let path = file.path().to_str().unwrap().to_owned();

        let out = JsonlTranscript::new().read_from(&path, 0).await.unwrap();
        assert_eq!(out.total_lines, 3, "the skipped line still counts");
        let texts: Vec<_> = out
            .messages
            .iter()
            .map(|m| (m.seq, m.flatten_text()))
            .collect();
        assert_eq!(
            texts,
            vec![(0, Some("a".to_owned())), (2, Some("c".to_owned()))]
        );
    }

    /// Lines ending in `\r\n` read as they did through `str::lines`.
    #[tokio::test]
    async fn a_crlf_line_ending_is_accepted() {
        let mut file = tempfile_jsonl();
        file.write_all(
            b"{\"uuid\":\"u1\",\"type\":\"user\",\"message\":{\"content\":\"a\",\"role\":\"user\"}}\r\n",
        )
        .unwrap();
        file.flush().unwrap();
        let path = file.path().to_str().unwrap().to_owned();

        let out = JsonlTranscript::new().read_from(&path, 0).await.unwrap();
        assert_eq!(out.total_lines, 1);
        assert_eq!(out.messages[0].flatten_text().as_deref(), Some("a"));
    }

    #[tokio::test]
    async fn exists_reports_presence() {
        let file = tempfile_jsonl();
        let path = file.path().to_str().unwrap().to_owned();

        let t = JsonlTranscript::new();
        assert!(
            t.exists(&path).await.unwrap(),
            "an existing file is present"
        );
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
        writeln!(
            file,
            r#"{{"type":"file-history-snapshot","messageId":"x"}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"uuid":"a1","type":"assistant","message":{{"content":"r","role":"assistant"}}}}"#
        )
        .unwrap();
        file.flush().unwrap();
        let path = file.path().to_str().unwrap().to_owned();

        let out = JsonlTranscript::new().read_from(&path, 0).await.unwrap();
        assert_eq!(
            out.messages.len(),
            2,
            "only the two uuid-bearing lines parse"
        );
        assert_eq!(out.total_lines, 3, "the no-uuid line still counts");
        assert_eq!(out.messages[0].seq, 0);
        assert_eq!(
            out.messages[1].seq, 2,
            "assistant sits after the skipped line"
        );
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
        assert_eq!(
            out.messages.len(),
            1,
            "only the terminated line is consumed"
        );
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
