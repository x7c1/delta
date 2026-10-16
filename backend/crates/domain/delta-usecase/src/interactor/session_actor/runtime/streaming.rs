//! [`StreamingCursor`]: where the in-flight turn's un-indexed streaming
//! fragments (Codex's `AssistantDelta`) are in their message, so each
//! broadcast fragment gets the next chunk index.

use super::SessionRuntime;

/// The position of the in-flight turn's current streaming message: its id and
/// how many fragments have been broadcast for it.
///
/// The server forwards each streaming fragment to the browser as an
/// `AssistantStreaming` event and keeps no copy of the text; the browser
/// accumulates the preview by chunk index. Claude's `MessageDisplay` hook
/// numbers its own chunks, so it needs no state here. Codex deltas arrive
/// un-indexed, so this cursor assigns them consecutive indexes per message id.
/// It is cleared when the turn ends (see [`SessionRuntime::apply_turn`]), so
/// a new turn's first fragment starts at index 0.
#[derive(Debug)]
pub(super) struct StreamingCursor {
    /// The message id of the fragments being counted (Codex's provider item
    /// id). A fragment whose id differs starts a fresh count.
    message_id: String,
    /// How many fragments have been broadcast for `message_id`, which is the
    /// index the next one receives.
    emitted: u32,
}

impl SessionRuntime {
    /// Assign the chunk index for the next un-indexed streaming fragment of
    /// `message_id` and advance the cursor past it.
    pub fn next_streaming_index(&mut self, message_id: &str) -> u32 {
        match self.streaming_cursor.as_mut() {
            Some(cursor) if cursor.message_id == message_id => {
                let index = cursor.emitted;
                cursor.emitted += 1;
                index
            }
            _ => {
                self.streaming_cursor = Some(StreamingCursor {
                    message_id: message_id.to_owned(),
                    emitted: 1,
                });
                0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::{TurnInput, TurnState};

    #[test]
    fn fragments_of_one_message_get_consecutive_indexes() {
        let mut state = SessionRuntime::default();
        let indexes: Vec<u32> = (0..3)
            .map(|_| state.next_streaming_index("item-1"))
            .collect();
        assert_eq!(indexes, vec![0, 1, 2]);
    }

    #[test]
    fn a_new_message_id_starts_at_zero() {
        let mut state = SessionRuntime::default();
        state.next_streaming_index("item-1");
        state.next_streaming_index("item-1");
        assert_eq!(state.next_streaming_index("item-2"), 0);
        assert_eq!(state.next_streaming_index("item-2"), 1);
    }

    #[test]
    fn a_turn_end_resets_the_index_for_the_same_message_id() {
        let mut state = SessionRuntime::default();
        state.apply_turn(TurnInput::PromptSubmitted { send_id: None });
        state.next_streaming_index("item-1");
        state.next_streaming_index("item-1");
        let result = state.apply_turn(TurnInput::Interrupt);
        assert_eq!(result.next, TurnState::Idle);
        assert_eq!(state.next_streaming_index("item-1"), 0);
    }
}
