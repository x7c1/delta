//! Reading and parsing the Claude Code JSONL transcript.

use async_trait::async_trait;

use crate::error::Result;
use crate::ports::transcript_message::TranscriptMessage;

/// Reads and parses the Claude Code JSONL transcript.
#[async_trait]
pub trait Transcript: Send + Sync {
    /// Read all currently available lines from the transcript at `path`,
    /// skipping the first `from_seq` lines already seen.
    async fn read_from(&self, path: &str, from_seq: usize) -> Result<Vec<TranscriptMessage>>;
}

#[async_trait]
impl Transcript for Box<dyn Transcript> {
    async fn read_from(&self, path: &str, from_seq: usize) -> Result<Vec<TranscriptMessage>> {
        (**self).read_from(path, from_seq).await
    }
}
