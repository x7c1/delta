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
 * Whether the thread's persisted messages already contain an assistant message
 * whose visible text matches the live-streamed text — i.e. the in-flight reply
 * has been flushed to the transcript and now renders via the normal pipeline.
 *
 * This makes the live preview's visibility a function of the current persisted
 * state rather than of event timing. The transcript refetch
 * (`transcript_updated` → `useThreadMessagesQuery`) can land BEFORE the
 * turn-end event that clears the preview buffer — and a single turn can persist
 * an earlier assistant message while `turn_completed` only fires at the very
 * end — so during that gap both the live bubble and the persisted message would
 * otherwise show the same text twice. Suppressing the bubble the instant a
 * matching persisted message exists eliminates the duplicate regardless of
 * event/refetch ordering.
 *
 * Matching is conservative to avoid hiding a genuinely in-flight reply:
 *
 * - Only assistant-role messages are considered.
 * - Empty `streamedText` never matches (no preview is shown for it anyway).
 * - A persisted message matches when its trimmed visible text EQUALS the
 *   trimmed streamed text (the accumulated deltas equal the persisted text
 *   block), or, for robustness against a late final delta, when the persisted
 *   text `startsWith` the streamed text (the persisted copy is the complete,
 *   authoritative version).
 *
 * While the reply is still streaming and not yet persisted, no message matches,
 * so the bubble shows normally; the moment the persisted version lands, the
 * bubble hides — no duplicate and no flash gap.
 */
export function persistedHasStreamedText(
  messages: Message[],
  streamedText: string,
): boolean {
  const streamed = streamedText.trim();
  if (streamed.length === 0) {
    return false;
  }
  return messages.some((message) => {
    if (message.role !== 'assistant') {
      return false;
    }
    const persisted = visibleText(message).trim();
    if (persisted.length === 0) {
      return false;
    }
    return persisted === streamed || persisted.startsWith(streamed);
  });
}
