import type { StateCreator } from 'zustand';
import type { ThreadId } from '@delta/model';

export interface UnreadSlice {
  /**
   * Unread counts keyed by thread id; cleared when a thread becomes active.
   *
   * The single source of truth for unread, at thread granularity. Bumped on a
   * `turn_completed` for a thread the user is not currently viewing (a
   * background session, or a non-active thread of the focused session) and on
   * external input to the focused thread. The navigator's collapsed session row
   * OR-aggregates over the session's threads (any unread → dot), and the
   * expanded thread tree shows each thread's own count — so there is no separate
   * session-level unread map to drift from this one. In-memory only (resets on
   * reload): persistence across reload would need backend support and is out of
   * scope.
   */
  unread: Record<ThreadId, number>;

  bumpUnread: (threadId: ThreadId) => void;
  clearUnread: (threadId: ThreadId) => void;
}

export const createUnreadSlice: StateCreator<UnreadSlice, [], [], UnreadSlice> = (
  set,
) => ({
  unread: {},

  bumpUnread: (threadId) =>
    set((state) => ({
      unread: { ...state.unread, [threadId]: (state.unread[threadId] ?? 0) + 1 },
    })),

  clearUnread: (threadId) =>
    set((state) => {
      if (!state.unread[threadId]) {
        return state;
      }
      const next = { ...state.unread };
      delete next[threadId];
      return { unread: next };
    }),
});
