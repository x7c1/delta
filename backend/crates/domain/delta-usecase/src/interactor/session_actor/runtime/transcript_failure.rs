//! The background tail's record of the transcript failure it already
//! reported, so a failure that repeats on every tick is logged once.

use super::SessionRuntime;

impl SessionRuntime {
    /// Record that the background tail failed to read this session's
    /// transcript with `failure` — the failure's message rendered with its
    /// whole cause chain ([`crate::with_causes()`]) — returning `true` when
    /// that is news worth reporting: the first failure, or one that differs
    /// from the last reported.
    ///
    /// **Two failures are the same when their rendered messages are equal.**
    /// The message names the kind (refused, I/O, parse, aborted), the path,
    /// and every cause below it, so the same refusal with the same size or the
    /// same `permission denied` on the same file repeats silently; another
    /// kind, another file, another OS error, or another size is a different
    /// failure and reported.
    pub(in crate::interactor) fn note_transcript_failure(&mut self, failure: String) -> bool {
        if self.reported_transcript_failure.as_ref() == Some(&failure) {
            return false;
        }
        self.reported_transcript_failure = Some(failure);
        true
    }

    /// Forget the reported failure now that the transcript was read, handing
    /// it back when there was one so the caller can log the recovery. A later
    /// failure is then news again, even if it is the same as before.
    pub(in crate::interactor) fn clear_transcript_failure(&mut self) -> Option<String> {
        self.reported_transcript_failure.take()
    }
}
