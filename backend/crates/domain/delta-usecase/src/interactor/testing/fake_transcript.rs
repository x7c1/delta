//! In-memory [`Transcript`] fake modelled as a list of file lines per path.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::Result;
use crate::ports::{Transcript, TranscriptMessage, TranscriptRead};

/// An in-memory transcript modelled as a list of file lines, keyed by path so
/// several sessions (each with its own transcript path) can be driven at once.
///
/// Each entry is one transcript line (see [`FakeLine`]): a parsed message, a
/// line that produces no message but still occupies a line and advances the
/// cursor — exactly how the real reader treats Claude Code's
/// `file-history-snapshot` lines — or the `relocated` line Claude Code appends
/// when it moves the transcript.
///
/// The default path matches the single-session [`submit`] helper, so the
/// single-session tests can keep pushing lines without naming a path.
///
/// [`submit`]: super::submit
pub(crate) const DEFAULT_TRANSCRIPT_PATH: &str = "/tmp/t.jsonl";

/// One line of a [`FakeTranscript`] file.
#[derive(Clone)]
enum FakeLine {
    /// A line that parses into a message.
    Message(Box<TranscriptMessage>),
    /// A line that produces no message (blank / no-uuid / unparsable).
    Skipped,
    /// A `relocated` line naming the working directory the transcript was
    /// moved for.
    Relocated(String),
}

#[derive(Default)]
pub(crate) struct FakeTranscript {
    by_path: Mutex<HashMap<String, Vec<FakeLine>>>,
    /// Paths the fake reports as absent from `exists`, modelling a transcript
    /// file that has been removed. By default every path is considered present,
    /// so the resume gate does not perturb the existing open/resume tests; a
    /// test marks a path missing via [`Self::mark_missing`] to exercise the
    /// resume-unavailable path.
    missing: Mutex<Vec<String>>,
    /// Barriers a read of the keyed path must pass before returning, set via
    /// [`Self::gate_reads`]. Lets a concurrency test prove two sessions'
    /// ingests are genuinely in flight at the same time: each read parks on
    /// the shared barrier, so the test only completes if they overlap.
    read_gates: Mutex<HashMap<String, std::sync::Arc<tokio::sync::Barrier>>>,
}

#[async_trait]
impl Transcript for FakeTranscript {
    async fn read_from(&self, path: &str, from_line: usize) -> Result<TranscriptRead> {
        let gate = self.read_gates.lock().unwrap().get(path).cloned();
        if let Some(gate) = gate {
            gate.wait().await;
        }
        let by_path = self.by_path.lock().unwrap();
        let lines = by_path.get(path).cloned().unwrap_or_default();
        let mut messages = Vec::new();
        let mut relocated_cwd = None;
        for (idx, line) in lines.iter().enumerate().skip(from_line) {
            match line {
                FakeLine::Message(msg) => {
                    let mut msg = (**msg).clone();
                    msg.seq = idx as i64;
                    messages.push(msg);
                }
                FakeLine::Skipped => {}
                FakeLine::Relocated(cwd) => relocated_cwd = Some(cwd.clone()),
            }
        }
        Ok(TranscriptRead {
            messages,
            total_lines: lines.len(),
            relocated_cwd,
        })
    }

    async fn exists(&self, path: &str) -> Result<bool> {
        Ok(!self.missing.lock().unwrap().iter().any(|p| p == path))
    }

    /// Any present `<root>/<project dir>/<session id>.jsonl` the fake holds.
    /// Recency is not modelled: of several, the greatest path wins, which
    /// keeps the choice deterministic. (The real adapter's choice between
    /// several is covered by its own tests.)
    async fn find_session_transcript(
        &self,
        root: &str,
        session_id: &str,
    ) -> Result<Option<String>> {
        let file_name = format!("{session_id}.jsonl");
        let missing = self.missing.lock().unwrap().clone();
        let by_path = self.by_path.lock().unwrap();
        Ok(by_path
            .keys()
            .filter(|path| !missing.contains(path))
            .filter(|path| {
                let path = Path::new(path.as_str());
                path.file_name() == Some(std::ffi::OsStr::new(&file_name))
                    && path.parent().and_then(Path::parent) == Some(Path::new(root))
            })
            .max()
            .cloned())
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
            .push(FakeLine::Message(Box::new(line)));
    }

    /// Move the transcript at `from` to `to` whole, then append a `relocated`
    /// line naming `cwd` — what Claude Code does when the session enters a
    /// worktree. `from` reports absent afterwards, like the moved-away file.
    pub(crate) fn relocate(&self, from: &str, to: &str, cwd: &str) {
        let mut by_path = self.by_path.lock().unwrap();
        let mut lines = by_path.remove(from).unwrap_or_default();
        lines.push(FakeLine::Relocated(cwd.to_owned()));
        by_path.insert(to.to_owned(), lines);
        drop(by_path);
        self.mark_missing(from);
    }

    /// Mark a transcript path as absent, so [`Transcript::exists`] reports
    /// `false` for it — modelling a removed transcript that makes
    /// `claude --resume` impossible.
    pub(crate) fn mark_missing(&self, path: &str) {
        self.missing.lock().unwrap().push(path.to_owned());
    }

    /// Park every future read of `path` on `gate` until enough participants
    /// arrive (see [`tokio::sync::Barrier`]). Sharing one barrier across two
    /// paths is how a test asserts those two reads overlap in time.
    pub(crate) fn gate_reads(&self, path: &str, gate: std::sync::Arc<tokio::sync::Barrier>) {
        self.read_gates
            .lock()
            .unwrap()
            .insert(path.to_owned(), gate);
    }

    /// Append a line that produces no message but still occupies a line and
    /// advances the cursor (e.g. Claude Code's `file-history-snapshot`).
    pub(crate) fn push_skipped_line(&self) {
        self.by_path
            .lock()
            .unwrap()
            .entry(DEFAULT_TRANSCRIPT_PATH.to_owned())
            .or_default()
            .push(FakeLine::Skipped);
    }
}
