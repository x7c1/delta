//! [`StreamingMessage`]: the live, provisional preview of the in-flight
//! turn's assistant message, accumulated from streaming chunks.

use delta_model::ThreadId;

use super::SessionRuntime;

/// The live, provisional preview of the in-flight turn's assistant message,
/// accumulated from the `MessageDisplay` hook's chunks.
///
/// Claude Code streams the visible assistant text in chunks (one display
/// segment each) before the transcript JSONL is flushed. Delta buffers them
/// here so the browser can show the reply forming at the conversation tail —
/// including an assistant's pre-tool preamble, which appears before a blocking
/// tool prompt blocks. It is never persisted: the chunks share one `message_id`
/// that does not match any transcript id, so this is reconciled per turn — it
/// is cleared when the turn ends (see [`SessionRuntime::apply_turn`]) and the
/// persisted message, ingested by the normal transcript sync, renders instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingMessage {
    /// The hook's display-message id (not a transcript id). A chunk whose id
    /// differs from the current buffer's starts a fresh message.
    pub message_id: String,
    /// The in-flight turn's thread, so the browser only shows the preview on
    /// the thread it belongs to.
    pub thread_id: ThreadId,
    /// The chunks received so far, paired with their `index`. Kept sparse and
    /// joined in index order on read, so out-of-order delivery is tolerated.
    pub chunks: Vec<(u32, String)>,
    /// Whether the final chunk has arrived.
    pub final_: bool,
}

impl StreamingMessage {
    /// The accumulated text so far, chunks joined in `index` order.
    ///
    /// The server broadcasts deltas incrementally (the client accumulates), so
    /// the joined text is only read back by tests asserting accumulation.
    #[cfg(test)]
    pub fn text(&self) -> String {
        let mut ordered: Vec<&(u32, String)> = self.chunks.iter().collect();
        ordered.sort_by_key(|(index, _)| *index);
        ordered
            .into_iter()
            .map(|(_, chunk)| chunk.as_str())
            .collect()
    }
}

impl SessionRuntime {
    /// Accumulate one `MessageDisplay` chunk into the live preview, returning
    /// the buffer's running text so the caller can broadcast the increment.
    ///
    /// A chunk whose `message_id` differs from the current buffer's starts a
    /// fresh preview (a new message began), as does the first chunk after a
    /// turn end cleared the buffer. Chunks are stored sparsely by `index` and
    /// joined in order on read, so out-of-order delivery is tolerated; a
    /// repeated `index` overwrites (the latest delivery wins).
    pub fn accumulate_streaming(
        &mut self,
        message_id: &str,
        thread_id: ThreadId,
        index: u32,
        final_: bool,
        delta: String,
    ) {
        let buffer = match self.streaming_message.as_mut() {
            Some(existing) if existing.message_id == message_id => existing,
            _ => {
                self.streaming_message = Some(StreamingMessage {
                    message_id: message_id.to_owned(),
                    thread_id,
                    chunks: Vec::new(),
                    final_: false,
                });
                self.streaming_message
                    .as_mut()
                    .expect("just inserted the streaming buffer")
            }
        };
        buffer.thread_id = thread_id;
        if let Some(slot) = buffer.chunks.iter_mut().find(|(i, _)| *i == index) {
            slot.1 = delta;
        } else {
            buffer.chunks.push((index, delta));
        }
        buffer.final_ = buffer.final_ || final_;
    }

    /// Accumulate one streaming fragment whose transport carries no explicit
    /// chunk index (Codex's `AssistantDelta`), auto-assigning the next index and
    /// returning it so the caller can broadcast the increment.
    ///
    /// Where Claude's `MessageDisplay` hook numbers its chunks, Codex deltas
    /// arrive un-indexed; the next index is the count of fragments already held
    /// for this `message_id` (0 when a different message id starts a fresh
    /// preview), so repeated deltas for one item append in arrival order rather
    /// than overwriting. Delegates to [`Self::accumulate_streaming`] for the
    /// actual buffering, so the per-turn reconciliation (cleared at turn end) is
    /// identical to the hook path.
    pub fn accumulate_streaming_delta(
        &mut self,
        message_id: &str,
        thread_id: ThreadId,
        final_: bool,
        delta: String,
    ) -> u32 {
        let index = match &self.streaming_message {
            Some(existing) if existing.message_id == message_id => existing.chunks.len() as u32,
            _ => 0,
        };
        self.accumulate_streaming(message_id, thread_id, index, final_, delta);
        index
    }

    /// The current live preview, if a message is streaming.
    ///
    /// The preview is broadcast as it accumulates rather than read back in
    /// production, so this accessor exists for the streaming tests' assertions.
    #[cfg(test)]
    pub fn streaming_message(&self) -> Option<&StreamingMessage> {
        self.streaming_message.as_ref()
    }
}
