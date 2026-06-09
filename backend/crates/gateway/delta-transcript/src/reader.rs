//! [`JsonlTranscript`]: the concrete [`Transcript`] gateway.

use async_trait::async_trait;
use tokio::fs;

use delta_usecase::{Transcript, TranscriptRead};

use crate::error::Error;
use crate::parse::parse_line;

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

        let mut messages = Vec::new();
        let mut total_lines = 0;
        for (idx, line) in contents.lines().enumerate() {
            total_lines = idx + 1;
            if idx < from_line {
                continue;
            }
            match parse_line(line) {
                Ok(Some(mut msg)) => {
                    // The message's absolute line index is its persisted `seq`.
                    msg.seq = idx as i64;
                    messages.push(msg);
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(error = %err, "skipping unparsable transcript line");
                }
            }
        }
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

    /// A minimal temp-file helper avoiding an extra dependency.
    fn tempfile_jsonl() -> TempJsonl {
        let mut p = std::env::temp_dir();
        let unique = format!(
            "delta-transcript-test-{}-{:?}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
