//! The result of reading a transcript: the parsed messages plus the file's
//! true line count.

use crate::ports::transcript_message::TranscriptMessage;

/// The outcome of a single transcript read.
///
/// `total_lines` is the file's full line count (including blank, no-uuid, and
/// unparsable lines that produced no message), so callers can advance a
/// line-based cursor past everything consumed — not just the messages that
/// parsed. This keeps the cursor in step with the file even when lines are
/// skipped, so each line is read exactly once and `seq` values stay gap-free
/// relative to the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptRead {
    /// The messages parsed from lines at or after the requested start index,
    /// each carrying its absolute 0-based line index as its `seq`.
    pub messages: Vec<TranscriptMessage>,
    /// The total number of lines in the file (the next read's start index).
    pub total_lines: usize,
}
