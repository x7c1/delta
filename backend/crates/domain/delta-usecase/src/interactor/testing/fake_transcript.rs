//! In-memory [`Transcript`] fake modelled as a list of file lines per path.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::Result;
use crate::ports::{Transcript, TranscriptMessage, TranscriptRead};

/// An in-memory transcript modelled as a list of file lines, keyed by path so
/// several sessions (each with its own transcript path) can be driven at once.
///
/// Each entry is one transcript line: `Some(msg)` is a parsed message,
/// `None` is a line that produces no message (blank / no-uuid / unparsable)
/// but still occupies a line and advances the cursor — exactly how the real
/// reader treats Claude Code's `file-history-snapshot` lines.
///
/// The default path matches the single-session [`submit`] helper, so the
/// single-session tests can keep pushing lines without naming a path.
///
/// [`submit`]: super::submit
pub(crate) const DEFAULT_TRANSCRIPT_PATH: &str = "/tmp/t.jsonl";

#[derive(Default)]
pub(crate) struct FakeTranscript {
    by_path: Mutex<HashMap<String, Vec<Option<TranscriptMessage>>>>,
    /// Paths the fake reports as absent from `exists`, modelling a transcript
    /// file that has been removed. By default every path is considered present,
    /// so the resume gate does not perturb the existing open/resume tests; a
    /// test marks a path missing via [`Self::mark_missing`] to exercise the
    /// resume-unavailable path.
    missing: Mutex<Vec<String>>,
}

#[async_trait]
impl Transcript for FakeTranscript {
    async fn read_from(&self, path: &str, from_line: usize) -> Result<TranscriptRead> {
        let by_path = self.by_path.lock().unwrap();
        let lines = by_path.get(path).cloned().unwrap_or_default();
        let messages = lines
            .iter()
            .enumerate()
            .skip(from_line)
            .filter_map(|(idx, line)| {
                line.clone().map(|mut msg| {
                    msg.seq = idx as i64;
                    msg
                })
            })
            .collect();
        Ok(TranscriptRead {
            messages,
            total_lines: lines.len(),
        })
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        Ok(!self.missing.lock().unwrap().iter().any(|p| p == path))
    }
}

impl FakeTranscript {
    /// Append a parsed message as the next line of the default transcript.
    pub(crate) fn push(&self, line: TranscriptMessage) {
        self.push_to(DEFAULT_TRANSCRIPT_PATH, line);
    }

    /// Append a parsed message as the next line of a specific transcript path.
    pub(crate) fn push_to(&self, path: &str, line: TranscriptMessage) {
        self.by_path
            .lock()
            .unwrap()
            .entry(path.to_owned())
            .or_default()
            .push(Some(line));
    }

    /// Mark a transcript path as absent, so [`Transcript::exists`] reports
    /// `false` for it — modelling a removed transcript that makes
    /// `claude --resume` impossible.
    pub(crate) fn mark_missing(&self, path: &str) {
        self.missing.lock().unwrap().push(path.to_owned());
    }

    /// Append a line that produces no message but still occupies a line and
    /// advances the cursor (e.g. Claude Code's `file-history-snapshot`).
    pub(crate) fn push_skipped_line(&self) {
        self.by_path
            .lock()
            .unwrap()
            .entry(DEFAULT_TRANSCRIPT_PATH.to_owned())
            .or_default()
            .push(None);
    }
}
