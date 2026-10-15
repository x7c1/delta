import { useEffect, useState } from 'react';

/**
 * How long the card's buttons stay disabled after it switches to a different
 * request. Long enough to absorb a second click (or a double click) aimed at
 * the request just answered, short enough not to slow a deliberate answer.
 */
export const REQUEST_SWITCH_GUARD_MS = 700;

/** What {@link useRequestSwitchGuard} reports for the current render. */
export interface RequestSwitchGuard {
  /** Whether the card switched requests within the guard window. */
  guarded: boolean;
  /**
   * Whether this render is the first one for a new request id. The card uses
   * it to reset the per-request state in the same render, so not even one
   * frame shows the previous request's state on the new one.
   */
  switched: boolean;
}

/**
 * Guard a card's answer buttons when the request it shows is replaced.
 *
 * Answering one request can promote the next into the same place on screen,
 * so a quick second click — or the second half of a double click — would land
 * on a request the user has not read. For {@link REQUEST_SWITCH_GUARD_MS}
 * after the switch the buttons stay disabled and the card marks the change.
 *
 * The switch is detected while rendering (not in an effect), so the very
 * first render showing the new request is already guarded; an effect only
 * schedules the end of the window. The request a card first mounts with is
 * not guarded: nothing was in its place to click.
 */
export function useRequestSwitchGuard(requestId: number): RequestSwitchGuard {
  const [shownId, setShownId] = useState(requestId);
  const [guardedId, setGuardedId] = useState<number | null>(null);
  const switched = shownId !== requestId;
  if (switched) {
    setShownId(requestId);
    setGuardedId(requestId);
  }

  useEffect(() => {
    if (guardedId === null) {
      return;
    }
    const timer = setTimeout(() => setGuardedId(null), REQUEST_SWITCH_GUARD_MS);
    return () => clearTimeout(timer);
  }, [guardedId]);

  return { guarded: switched || guardedId === requestId, switched };
}
