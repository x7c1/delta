//! The result of reading a transcript: the parsed messages, the file's true
//! line count, and the lines that could not be parsed.

use delta_attribution::TranscriptMessage;

use crate::ports::UnparsableTranscriptLine;

/// The outcome of a single transcript read.
///
/// `total_lines` is the file's full line count (including blank, no-uuid, and
/// unparsable lines that produced no message), so callers can advance a
/// line-based cursor past everything consumed — not just the messages that
/// parsed. This keeps the cursor in step with the file even when lines are
/// skipped, so each line is read exactly once and `seq` values stay gap-free
/// relative to the file.
///
/// An unparsable line does not fail the read: it is skipped like any other
/// line that produces no message, and handed back in
/// [`Self::unparsable_lines`] for the caller to report. The reader does not log
/// it itself, because only the caller knows when its cursor has moved past the
/// line; reporting it then is what makes each line reported once rather than
/// on every re-read of the same range.
#[derive(Debug)]
pub struct TranscriptRead {
    /// The messages parsed from lines at or after the requested start index,
    /// each carrying its absolute 0-based line index as its `seq`.
    pub messages: Vec<TranscriptMessage>,
    /// The total number of lines in the file (the next read's start index).
    pub total_lines: usize,
    /// The working directory named by the last `relocated` line in the read
    /// range, if any. Claude Code appends such a line when it moves the
    /// transcript to the project directory of a new working directory (on
    /// entering a worktree); it is never a message, so it surfaces here instead.
    pub relocated_cwd: Option<String>,
    /// One [`UnparsableTranscriptLine`] per line at or after the requested start
    /// index that could not be parsed, in file order.
    pub unparsable_lines: Vec<UnparsableTranscriptLine>,
}
