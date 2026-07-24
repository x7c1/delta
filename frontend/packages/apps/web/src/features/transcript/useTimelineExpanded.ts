import { useCallback, useEffect, useState } from 'react';
import type { SessionId } from '@delta/model';
import {
  readSessionScoped,
  writeSessionScoped,
} from '../../store/sessionScopedStorage';

/**
 * Sub-key under {@link sessionScopedKey} for the timeline footer's
 * expanded/collapsed state. The preference is **per session** — sessions
 * with many subthreads usually want the timeline expanded while
 * single-thread sessions prefer it collapsed, and a global toggle made the
 * UI flip on every session switch. The full localStorage key for a given
 * session is `delta.session.<sessionId>.thread-timeline-overlay.expanded`.
 *
 * The previous device-global key (`delta.thread-timeline-overlay.expanded`)
 * is intentionally NOT migrated — see delta's `docs/guides/compatibility.md`
 * for the 0.x no-compat policy, and the related plan note: a global default
 * is no longer meaningful once the preference is session-scoped.
 */
export const TIMELINE_EXPANDED_SUBKEY = 'thread-timeline-overlay.expanded';

/**
 * Read the persisted expanded preference for a session; defaults to collapsed
 * when no preference has been saved yet or the storage layer is unavailable
 * (SSR / privacy-mode browsers). The boolean is encoded as the strings
 * `'true'` / `'false'` (consistent with the previous device-global key) so a
 * DevTools peek still reads naturally.
 */
function readPersistedExpanded(sessionId: SessionId): boolean {
  return (
    readSessionScoped<boolean>(
      sessionId,
      TIMELINE_EXPANDED_SUBKEY,
      (raw) => raw === 'true',
    ) ?? false
  );
}

/**
 * Persist the expanded preference for a session. Failures are swallowed by
 * the helper so a quota error or a disabled-storage browser never crashes
 * the footer — the UI keeps working in-memory for the page session.
 */
function writePersistedExpanded(sessionId: SessionId, expanded: boolean): void {
  writeSessionScoped<boolean>(
    sessionId,
    TIMELINE_EXPANDED_SUBKEY,
    expanded,
    (value) => (value ? 'true' : 'false'),
  );
}

/**
 * Module-scoped store for the timeline expanded preference, keyed by session
 * id. Multiple components read the same flag for a given session (the
 * timeline itself, and the transcript pane that switches its top-row layout
 * so the Terminal button moves below when the timeline is expanded). A click
 * on the toggle must update every subscriber for THAT session on the same
 * tick — per-component `useState` would only sync via the `storage` event,
 * which does not fire on same-document writes. A small pub-sub keeps every
 * subscriber in lockstep without pulling a full store in for one boolean
 * per session.
 *
 * The cache is per-session because two open transcripts of different
 * sessions (a `react-query` cache warmup, a debug overlay, etc.) must NOT
 * share a value: that was the device-global behaviour the migration to
 * per-session keys is meant to remove.
 */
interface TimelineExpandedEntry {
  value: boolean | null;
  listeners: Set<(value: boolean) => void>;
}

const timelineExpandedCache = new Map<SessionId, TimelineExpandedEntry>();

function getEntry(sessionId: SessionId): TimelineExpandedEntry {
  let entry = timelineExpandedCache.get(sessionId);
  if (entry === undefined) {
    entry = { value: null, listeners: new Set() };
    timelineExpandedCache.set(sessionId, entry);
  }
  return entry;
}

function getTimelineExpanded(sessionId: SessionId): boolean {
  const entry = getEntry(sessionId);
  if (entry.value === null) {
    entry.value = readPersistedExpanded(sessionId);
  }
  return entry.value;
}

function setTimelineExpanded(sessionId: SessionId, next: boolean): void {
  const entry = getEntry(sessionId);
  entry.value = next;
  writePersistedExpanded(sessionId, next);
  for (const listener of entry.listeners) {
    listener(next);
  }
}

/**
 * Drop the in-memory cache so a test that clears `localStorage` between cases
 * starts from a fresh read rather than the previous case's last write. With
 * no argument, clears every session's cached value (the common test reset);
 * with a specific id, clears just that one. Production code does not need
 * this — the cache lives for the page session.
 */
export function resetTimelineExpandedForTests(sessionId?: SessionId): void {
  if (sessionId === undefined) {
    timelineExpandedCache.clear();
    return;
  }
  const entry = timelineExpandedCache.get(sessionId);
  if (entry !== undefined) {
    entry.value = null;
  }
}

/**
 * Expanded/collapsed state for the timeline footer, persisted to localStorage
 * per session so the preference is remembered independently for each session
 * — large multi-thread sessions can stay expanded while short single-thread
 * ones stay collapsed, instead of one device-wide toggle.
 *
 * The argument is the focused session id, or `null` while the session list
 * is still loading / no session is focused. `null` falls back to in-memory
 * only (no read, no write): the UI degrades to collapsed for that frame
 * without leaking a literal `null` into the storage key. Once a real id
 * arrives the hook re-subscribes and reads the per-session value.
 *
 * Initial state is collapsed when no preference has been saved for that
 * session. All consumers of the same session share one value (see the
 * module-scoped store above), so toggling in one place updates the others
 * on the same tick. Exported so tests and the transcript pane (which
 * switches its top-row layout on the same flag) can read and drive the
 * toggle.
 */
export function useTimelineExpanded(
  sessionId: SessionId | null,
): [boolean, () => void] {
  const [expanded, setExpanded] = useState<boolean>(() =>
    sessionId === null ? false : getTimelineExpanded(sessionId),
  );
  useEffect(() => {
    if (sessionId === null) {
      // No session to bind to: drop back to the collapsed default until a
      // real id arrives. The previous subscription (if any) was already
      // torn down by the previous effect's cleanup.
      setExpanded(false);
      return;
    }
    const entry = getEntry(sessionId);
    const listener = (value: boolean) => setExpanded(value);
    entry.listeners.add(listener);
    // Sync to the current value in case it changed between render and
    // subscribe (e.g. another consumer toggled it in the same render pass,
    // or the session id just switched and the previous value was stale).
    setExpanded(getTimelineExpanded(sessionId));
    return () => {
      entry.listeners.delete(listener);
    };
  }, [sessionId]);
  const toggle = useCallback(() => {
    if (sessionId === null) {
      // The toggle button is hidden by the caller when there is no session
      // (the timeline does not render), so this branch is defensive — but
      // a stray click during a session switch must not write a `null` id.
      return;
    }
    setTimelineExpanded(sessionId, !getTimelineExpanded(sessionId));
  }, [sessionId]);
  return [expanded, toggle];
}
