import type { StateCreator } from 'zustand';
import type { ThreadId } from '@delta/model';

export interface ThreadActivitySlice {
  /**
   * The newest activity seen on each thread since this page loaded, keyed by
   * thread id, as an ISO-8601 UTC timestamp.
   *
   * The navigator's "most recently active sub-thread" mark is seeded from the
   * threads query (`Thread.last_activity_at`); this map is what moves it while
   * the session is open, without waiting for a refetch. It is written from the
   * live events that name a thread and mean "a message landed here", and read
   * in preference to the query value when a thread has an entry.
   *
   * Unlike {@link UnreadSlice.unread}, the thread being viewed is NOT skipped:
   * unread answers "did something happen while you were away", which the active
   * thread is by definition exempt from, while this answers "where did the last
   * message land" — and the active thread is a perfectly ordinary answer.
   *
   * Values only ever move forward: live events for one thread can interleave
   * with each other, and a mark that jumped backwards would point at an older
   * thread. In-memory only (resets on reload), at which point the query values
   * take over again.
   */
  threadActivity: Record<ThreadId, string>;

  /**
   * Record activity on a thread, keeping the later of the stored and the given
   * timestamp. `at` defaults to the client clock, which is what the live events
   * carrying no timestamp of their own have to use; every value in this map
   * comes from that same clock, so they stay comparable with each other.
   */
  noteThreadActivity: (threadId: ThreadId, at?: string) => void;
}

export const createThreadActivitySlice: StateCreator<
  ThreadActivitySlice,
  [],
  [],
  ThreadActivitySlice
> = (set) => ({
  threadActivity: {},

  noteThreadActivity: (threadId, at) =>
    set((state) => {
      const next = at ?? new Date().toISOString();
      const current = state.threadActivity[threadId];
      if (current !== undefined && current >= next) {
        return state;
      }
      return {
        threadActivity: { ...state.threadActivity, [threadId]: next },
      };
    }),
});
