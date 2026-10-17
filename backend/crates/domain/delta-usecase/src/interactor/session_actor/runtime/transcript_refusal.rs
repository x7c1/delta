//! The background tail's record of the transcript refusal it already
//! reported, so a refusal that repeats on every tick is logged once.

use crate::ports::TranscriptRefusal;

use super::SessionRuntime;

impl SessionRuntime {
    /// Record that the background tail found this session's transcript
    /// refused for `reason`, returning `true` when that is news worth
    /// reporting: the first refusal, or one that differs from the last
    /// reported (another kind, or another size).
    pub(in crate::interactor) fn note_transcript_refusal(
        &mut self,
        reason: TranscriptRefusal,
    ) -> bool {
        if self.reported_transcript_refusal == Some(reason) {
            return false;
        }
        self.reported_transcript_refusal = Some(reason);
        true
    }

    /// Forget the reported refusal now that the transcript was read, handing
    /// it back when there was one so the caller can log the recovery. A later
    /// refusal is then news again.
    pub(in crate::interactor) fn clear_transcript_refusal(&mut self) -> Option<TranscriptRefusal> {
        self.reported_transcript_refusal.take()
    }
}
