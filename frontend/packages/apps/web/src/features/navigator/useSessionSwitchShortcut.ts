import { useEffect, useRef } from 'react';
import type { SessionId } from '@delta/model';
import { useNavStore } from '../../store/navStore';
import { isLinuxShell, isMacosShell } from '../../shell';
import {
  sessionSwitchDirection,
  sessionSwitchTarget,
} from '../../shortcuts/sessionSwitch';

/**
 * Bind Ctrl+Tab / Ctrl+Shift+Tab to focusing the next / previous session of
 * the navigator, inside the desktop shell only.
 *
 * Browsers keep Ctrl+Tab for switching their own tabs and never hand it to the
 * page, so outside the shell nothing is bound and a Ctrl+Tab that does reach
 * the page is left alone.
 *
 * The listener is on `window` in the capture phase, so it sees the key before
 * the terminal, the composer or any other input does, and stops it there: the
 * key moves between sessions wherever the focus is. Each keydown, a repeat
 * included, moves one row; at either end of the list it does nothing.
 *
 * @param orderedIds The loaded sessions' ids in the order the list shows them.
 *   Only these count: the shortcut never fetches another page.
 * @param scrollToIndex Bring the row at an index of `orderedIds` into view.
 */
export function useSessionSwitchShortcut(
  orderedIds: readonly SessionId[],
  scrollToIndex: (index: number) => void,
): void {
  // Read through refs so the listener is bound once, not on every re-render
  // of the pane (which happens on every scroll frame).
  const latest = useRef({ orderedIds, scrollToIndex });
  useEffect(() => {
    latest.current = { orderedIds, scrollToIndex };
  });

  useEffect(() => {
    if (!isMacosShell() && !isLinuxShell()) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      const direction = sessionSwitchDirection(event);
      if (direction === null) {
        return;
      }
      // Consumed even when there is nowhere to move, so the key never falls
      // through to the terminal or an input.
      event.preventDefault();
      event.stopPropagation();
      const { orderedIds: ids, scrollToIndex: scroll } = latest.current;
      const nav = useNavStore.getState();
      const target = sessionSwitchTarget(
        nav.focusedSessionId,
        ids,
        direction,
      );
      // Nothing to do in an empty list or at the end the key points past.
      if (target === null || target === nav.focusedSessionId) {
        return;
      }
      // The same focus change a click on the card makes.
      nav.setFocusedSession(target);
      scroll(ids.indexOf(target));
    };
    window.addEventListener('keydown', onKeyDown, { capture: true });
    return () =>
      window.removeEventListener('keydown', onKeyDown, { capture: true });
  }, []);
}
