import type { StateCreator } from 'zustand';
import type { SessionId, ThreadId } from '@delta/model';
import type { EventReducer } from './eventReducer';

/** The streaming-preview state alone, the only field this module touches. */
type StreamingState = Pick<StreamingSlice, 'streamingMessages'>;

/**
 * The provisional live preview of an in-flight turn's assistant message,
 * accumulated from the `assistant_streaming` events the `MessageDisplay` hook
 * produces. Shown as a live bubble at the conversation tail while the turn
 * generates — including an assistant's pre-tool preamble, visible before the
 * user answers a blocking tool prompt.
 *
 * It is NOT a REST resource: the deltas carry no transcript id, so this cannot
 * be id-joined to the eventually-persisted message. It is reconciled per turn —
 * cleared on `turn_completed` / `turn_interrupted` / `session_closed` (and on a
 * reconnect, see {@link TurnLifecycleSlice.resetTurnEphemera}), after which the
 * persisted assistant message renders via the normal transcript pipeline.
 */
export interface StreamingMessage {
  /** The hook's display-message id (not a transcript id). */
  messageId: string;
  /** The in-flight turn's thread, so the bubble only shows on its own thread. */
  threadId: ThreadId;
  /** The accumulated visible text so far (chunks joined in index order). */
  text: string;
  /** True once the final delta has arrived. */
  done: boolean;
  /**
   * The chunks received so far, keyed by `index`. Kept so out-of-order or
   * duplicate deliveries reconcile deterministically — {@link text} is always
   * recomputed by joining these in ascending index order.
   */
  chunks: Record<number, string>;
}

export interface StreamingSlice {
  /**
   * The provisional live preview of each session's in-flight assistant message,
   * keyed by session id. Appended by `assistant_streaming` and cleared on turn
   * end / close / reconnect (see {@link StreamingMessage}). At most one per
   * session — `claude` streams one message at a time.
   */
  streamingMessages: Record<SessionId, StreamingMessage>;
}

/**
 * Drop the streaming preview of one session, returning the changed slice (empty
 * object when none existed). Used when the turn ends — the persisted assistant
 * message then renders via the normal pipeline — and on a reconnect.
 */
export function dropStreamingForSession(
  state: StreamingState,
  sessionId: SessionId,
): Partial<StreamingState> {
  if (!state.streamingMessages[sessionId]) {
    return {};
  }
  const streamingMessages = { ...state.streamingMessages };
  delete streamingMessages[sessionId];
  return { streamingMessages };
}

export const createStreamingSlice: StateCreator<
  StreamingSlice,
  [],
  [],
  StreamingSlice
> = () => ({
  streamingMessages: {},
});

// A chunk of the in-flight turn's assistant message arrived. Append
// it to the session's live preview (a new message_id, or the first
// chunk after a turn end cleared the buffer, starts fresh). Chunks
// are kept by index and the text recomputed by joining them in
// ascending order, so out-of-order or duplicate deliveries reconcile
// deterministically. Cleared per turn by the turn-end events.
export const reduceAssistantStreaming: EventReducer<
  StreamingState,
  'assistant_streaming'
> = (state, event) => {
  const prev = state.streamingMessages[event.session_id];
  const chunks =
    prev && prev.messageId === event.message_id
      ? { ...prev.chunks, [event.index]: event.delta }
      : { [event.index]: event.delta };
  const text = Object.keys(chunks)
    .map(Number)
    .sort((a, b) => a - b)
    .map((index) => chunks[index])
    .join('');
  return {
    streamingMessages: {
      ...state.streamingMessages,
      [event.session_id]: {
        messageId: event.message_id,
        threadId: event.thread_id,
        text,
        done: event.final,
        chunks,
      },
    },
  };
};
