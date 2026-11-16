/**
 * Ctrl+Tab and Ctrl+Shift+Tab: move to the next or previous session in the
 * navigator's list, in the desktop app.
 *
 * This module holds the pure part of the shortcut — which key presses it
 * answers and which session a press moves to — so it is tested apart from the
 * hook that listens for the key and focuses the session
 * (`features/navigator/useSessionSwitchShortcut.ts`). A further app-wide
 * shortcut follows the same split.
 */

/** Which way a session switch moves through the list. */
export type SessionSwitchDirection = 'next' | 'previous';

/**
 * The direction a key press asks for, or `null` when it is not the session
 * switch: Ctrl+Tab is `next`, Ctrl+Shift+Tab is `previous`, and any press with
 * Alt or Meta held is not ours.
 */
export function sessionSwitchDirection(
  event: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'shiftKey' | 'altKey' | 'metaKey'>,
): SessionSwitchDirection | null {
  if (event.key !== 'Tab' || !event.ctrlKey || event.altKey || event.metaKey) {
    return null;
  }
  return event.shiftKey ? 'previous' : 'next';
}

/**
 * The session a switch moves to, given the focused one and the list's ids in
 * the order it is shown, or `null` when the list is empty.
 *
 * The ends wrap around: past the last id is the first, before the first is the
 * last. With no focused session — `null`, or an id not in the list, such as the
 * new-session screen's marker or a session no longer listed — `next` moves to
 * the first id and `previous` to the last.
 */
export function sessionSwitchTarget<Id extends string>(
  current: string | null,
  orderedIds: readonly Id[],
  direction: SessionSwitchDirection,
): Id | null {
  const count = orderedIds.length;
  if (count === 0) {
    return null;
  }
  const index = current === null ? -1 : orderedIds.indexOf(current as Id);
  if (index === -1) {
    return direction === 'next' ? orderedIds[0] : orderedIds[count - 1];
  }
  const step = direction === 'next' ? 1 : -1;
  return orderedIds[(index + step + count) % count];
}
