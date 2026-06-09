//! Reading and parsing the Claude Code JSONL transcript.

use async_trait::async_trait;

use crate::error::Result;
use crate::ports::transcript_read::TranscriptRead;

/// Reads and parses the Claude Code JSONL transcript.
#[async_trait]
pub trait Transcript: Send + Sync {
    /// Read the transcript at `path`, skipping lines whose 0-based index is
    /// `< from_line`, and report both the parsed messages and the file's total
    /// line count.
    ///
    /// Each returned message carries its absolute line index as its `seq`.
    /// Skipped lines (blank, no-uuid, unparsable) still advance the index, so
    /// `seq` and `total_lines` reflect the file's true position and a
    /// line-based cursor reads each line exactly once. A missing file yields no
    /// messages and `total_lines = 0`.
    async fn read_from(&self, path: &str, from_line: usize) -> Result<TranscriptRead>;

    /// Report whether a transcript file exists at `path`.
    ///
    /// Unlike [`Self::read_from`], which deliberately treats a missing file as
    /// empty, this distinguishes "present" from "absent" so callers can gate on
    /// it. `claude --resume <id>` cannot replay a conversation whose transcript
    /// has been removed, so the resume path uses this to refuse upfront rather
    /// than spawning a doomed session.
    async fn exists(&self, path: &str) -> Result<bool>;
}

#[async_trait]
impl Transcript for Box<dyn Transcript> {
    async fn read_from(&self, path: &str, from_line: usize) -> Result<TranscriptRead> {
        (**self).read_from(path, from_line).await
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        (**self).exists(path).await
    }
}
