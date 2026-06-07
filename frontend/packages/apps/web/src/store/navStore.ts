import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import type { SessionId, ThreadId } from '@delta/model';

/** Default terminal pane width in pixels (matches the former Tailwind w-96). */
export const DEFAULT_TERMINAL_WIDTH = 384;

const MIN_TERMINAL_WIDTH = 280;
const MAX_TERMINAL_WIDTH = 720;

/**
 * Clamp a requested terminal width to a sensible range: never narrower than
 * {@link MIN_TERMINAL_WIDTH}, and never wider than {@link MAX_TERMINAL_WIDTH}
 * or 60% of the viewport (whichever is smaller) so the pane cannot crowd out
 * the transcript.
 */
export function clampTerminalWidth(
  width: number,
  viewportWidth = typeof window === 'undefined' ? Infinity : window.innerWidth,
): number {
  const upper = Math.min(MAX_TERMINAL_WIDTH, viewportWidth * 0.6);
  // Guard against tiny viewports where 60% drops below the minimum.
  const max = Math.max(MIN_TERMINAL_WIDTH, upper);
  return Math.min(Math.max(width, MIN_TERMINAL_WIDTH), max);
}

/**
 * The id of the focused session, or a sentinel for the cold-start / "New"
 * composer state that has no session id yet (a fresh spawn has no id until its
 * first hook binds it — see the multi-session design notes in the workspace).
 */
export const NEW_SESSION_FOCUS = '__new__';
export type FocusedSession = SessionId | typeof NEW_SESSION_FOCUS | null;

/**
 * Navigation/layout state: the focused session, the active thread within it, and
 * terminal pane visibility. Focus is purely client-side — the server emits no
 * focus event.
 */
export interface NavState {
  /** Focused session id, the new-session sentinel, or null before load. */
  focusedSessionId: FocusedSession;
  /** Active thread within the focused session (scoped to it). */
  activeThreadId: ThreadId | null;
  /** Whether the terminal pane is shown (persistent pane on large screens, or
   *  the slide-in overlay on small screens). */
  terminalOpen: boolean;
  /** Width of the persistent terminal pane in pixels (large screens only). */
  terminalWidth: number;

  /**
   * Focus a session. Switching to a different session clears the active thread
   * (the workspace reconciles it to that session's main); re-focusing the same
   * session leaves the active thread untouched.
   */
  setFocusedSession: (sessionId: FocusedSession) => void;
  setActiveThread: (threadId: ThreadId) => void;
  setTerminalOpen: (open: boolean) => void;
  toggleTerminal: () => void;
  /** Set the terminal pane width, clamped to the allowed range. */
  setTerminalWidth: (width: number) => void;
}

/** localStorage key for the persisted layout state. */
export const NAV_STORAGE_KEY = 'delta-nav';

/**
 * Navigation/layout store. The focused session, active thread, terminal
 * visibility, and terminal width are **persisted to localStorage** so a browser
 * reload restores the same layout instead of snapping back to a closed terminal
 * on `main`. A restored focused session that no longer exists, or an active
 * thread outside the focused session, is reconciled by the workspace (see
 * `WorkspaceScreen`).
 */
export const useNavStore = create<NavState>()(
  persist(
    (set) => ({
      focusedSessionId: null,
      activeThreadId: null,
      terminalOpen: false,
      terminalWidth: DEFAULT_TERMINAL_WIDTH,

      setFocusedSession: (sessionId) =>
        set((state) =>
          state.focusedSessionId === sessionId
            ? state
            : { focusedSessionId: sessionId, activeThreadId: null },
        ),
      setActiveThread: (threadId) => set({ activeThreadId: threadId }),
      setTerminalOpen: (open) => set({ terminalOpen: open }),
      toggleTerminal: () =>
        set((state) => ({ terminalOpen: !state.terminalOpen })),
      setTerminalWidth: (width) => set({ terminalWidth: clampTerminalWidth(width) }),
    }),
    {
      name: NAV_STORAGE_KEY,
      storage: createJSONStorage(() => localStorage),
      // Persist only the layout values, never the action functions.
      partialize: (state) => ({
        focusedSessionId: state.focusedSessionId,
        activeThreadId: state.activeThreadId,
        terminalOpen: state.terminalOpen,
        terminalWidth: state.terminalWidth,
      }),
      // Re-clamp the restored width in case the viewport shrank since last time.
      onRehydrateStorage: () => (state) => {
        if (state) {
          state.terminalWidth = clampTerminalWidth(state.terminalWidth);
        }
      },
    },
  ),
);
