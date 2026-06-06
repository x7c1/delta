//! [`JsonlTranscript`]: the concrete [`Transcript`] gateway.

use async_trait::async_trait;
use tokio::fs;

use delta_usecase::{Transcript, TranscriptMessage};

use crate::error::Error;
use crate::parse::parse_line;

/// Reads Claude Code JSONL transcripts from the filesystem.
///
/// `read_from` re-reads the file and skips lines already seen by line index.
/// For a local single-session tool the transcript is small, so a full read on
/// each hook is simple and correct; callers that want true streaming can poll
/// this on an interval. Non-blank lines that fail to parse are skipped rather
/// than aborting the whole read, so one malformed line cannot stall ingestion.
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
        from_seq: usize,
    ) -> std::result::Result<Vec<TranscriptMessage>, delta_usecase::Error> {
        let contents = match fs::read_to_string(path).await {
            Ok(c) => c,
            // A not-yet-created transcript is not an error: nothing to read.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::from(e).into()),
        };

        let mut out = Vec::new();
        for line in contents.lines().skip(from_seq) {
            match parse_line(line) {
                Ok(Some(msg)) => out.push(msg),
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(error = %err, "skipping unparsable transcript line");
                }
            }
        }
        Ok(out)
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
        assert_eq!(all.len(), 2);

        let tail = t.read_from(&path, 1).await.unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].flatten_text().as_deref(), Some("b"));
    }

    #[tokio::test]
    async fn missing_file_is_empty_not_error() {
        let t = JsonlTranscript::new();
        let out = t.read_from("/nonexistent/transcript.jsonl", 0).await.unwrap();
        assert!(out.is_empty());
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
        assert_eq!(out.len(), 1);
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
