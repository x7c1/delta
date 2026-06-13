import type { Message } from '@delta/wire-gen';

/**
 * The visible text of a message: its `text`-type content blocks concatenated in
 * order. `thinking`, `tool_use`, and `tool_result` blocks contribute no visible
 * prose and are skipped, mirroring what the live preview accumulates (the
 * `assistant_streaming` deltas carry only the visible text).
 */
function visibleText(message: Message): string {
  return message.content
    .filter((block) => block.type === 'text')
    .map((block) => block.text)
    .join('');
}

/**
 * Whether the thread's persisted messages already contain the assistant message
 * the live preview is streaming — i.e. the in-flight reply has been flushed to
 * the transcript and now renders via the normal pipeline.
 *
 * This makes the live preview's visibility a function of the current persisted
 * state rather than of event timing. The transcript refetch
 * (`transcript_updated` → `useThreadMessagesQuery`) can land BEFORE the
 * turn-end event that clears the preview buffer — and a single turn can persist
 * an earlier assistant message while `turn_completed` only fires at the very
 * end — so during that gap both the live bubble and the persisted message would
 * otherwise show the same text twice. Suppressing the bubble the instant the
 * matching persisted message exists eliminates the duplicate regardless of
 * event/refetch ordering.
 *
 * Matching is precise to avoid hiding a genuinely in-flight reply:
 *
 * - Empty `streamedText` never matches (no preview is shown for it anyway).
 * - Only the LAST assistant-role message is compared. The streaming buffer
 *   always holds the current/latest in-flight message (a new `message_id`
 *   resets it), so its persisted counterpart, once flushed, is the last
 *   assistant message. Comparing only the last one avoids matching an EARLIER
 *   assistant message that merely shares a prefix with the growing stream
 *   (common openers like "Let me…", "Here's…", or a repeated short reply).
 * - The primary rule is trimmed EQUALITY (`persisted === streamed`). A partial
 *   in-flight stream never equals the full persisted text, so it only matches
 *   once the message is actually persisted.
 * - `persisted.startsWith(streamed)` is allowed ONLY when the stream is final
 *   (`streamComplete`). When final, `streamed` is the complete text, so prefix
 *   matching safely covers a persisted copy with trailing whitespace or extra
 *   blocks without matching a mid-stream growing prefix.
 *
 * While the reply is still streaming and not yet persisted, no message matches,
 * so the bubble shows normally; the moment the persisted version lands, the
 * bubble hides — no duplicate and no flash gap. The turn-end clear remains as a
 * backstop for the rare dropped-final-delta case.
 */
export function persistedHasStreamedText(
  messages: Message[],
  streamedText: string,
  streamComplete: boolean,
): boolean {
  const streamed = streamedText.trim();
  if (streamed.length === 0) {
    return false;
  }
  const lastAssistant = lastAssistantMessage(messages);
  if (lastAssistant === null) {
    return false;
  }
  const persisted = visibleText(lastAssistant).trim();
  if (persisted.length === 0) {
    return false;
  }
  if (persisted === streamed) {
    return true;
  }
  return streamComplete && persisted.startsWith(streamed);
}

/** The last assistant-role message in the thread, or `null` if there is none. */
function lastAssistantMessage(messages: Message[]): Message | null {
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === 'assistant') {
      return messages[i];
    }
  }
  return null;
}
