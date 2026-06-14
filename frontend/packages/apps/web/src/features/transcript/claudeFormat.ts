import type { Message } from '@delta/wire-gen';

/**
 * Claude Code text-format detection for the frontend, in one place.
 *
 * Mirrors the backend conventions (see the `delta-attribution` crate's
 * `claude_format` module). The backend owns role/attribution; the frontend
 * only needs these prefixes for presentational decisions, so the detection is
 * duplicated here verbatim rather than shipped over the wire.
 */

/**
 * Prompt prefix Claude Code uses when it injects a background-task completion
 * notification. Such a submission is a harness injection the agent responds to,
 * not human-authored prose, so the conversation pane folds it by default.
 */
const TASK_NOTIFICATION_PREFIX = '<task-notification>';

/**
 * Whether a (trimmed) prompt string is a harness-injected task notification.
 * Matches the backend `is_task_notification`: trim leading whitespace, then
 * check the prefix.
 */
export function isTaskNotificationText(text: string): boolean {
  return text.trimStart().startsWith(TASK_NOTIFICATION_PREFIX);
}

/**
 * Whether a message is a task-notification user turn. The harness submits the
 * notification as a normal `role: "user"` line (not a meta row), so the
 * meta-folding does not apply; the conversation pane detects it here and folds
 * the block presentationally. Backend role/attribution is unchanged.
 *
 * The notification text arrives as the first `text` block of the message, so
 * the check mirrors the backend prompt-prefix detection on that block.
 */
export function isTaskNotificationMessage(message: Message): boolean {
  if (message.role !== 'user') {
    return false;
  }
  const firstText = message.content.find((block) => block.type === 'text');
  return firstText !== undefined && isTaskNotificationText(firstText.text);
}
