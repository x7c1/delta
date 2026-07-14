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
 * The id of the focused session, or the new-session marker: the cold-start /
 * "New" composer state, where the user is composing a first message and no
 * session exists yet.
 *
 * This is purely a UI screen mode — the navigation analogue of "no session is
 * focused; show the new-session screen". No DATA may key on it: once the first
 * Send is accepted, every send/spawn record uses the REAL session id the
 * server returned, and the workspace focuses that id directly when the spawn
 * registers.
 */
export const NEW_SESSION_FOCUS = '__new__';
export type FocusedSession = SessionId | typeof NEW_SESSION_FOCUS | null;

/**
 * The navigation intent recorded by a timeline-initiated cross-lane jump: the
 * exact message the playhead landed on (`targetUuid`) and the lane it lives in
 * (`threadId`). Written atomically with the active-thread switch (see
 * {@link NavState.setActiveThreadWithJumpTarget}) so TranscriptPane's
 * thread-change layout effect can read it synchronously in the same commit and
 * decline to jump-to-tail — the transcript pane must land on the jump target,
 * not the newly focused lane's tail.
 *
 * Purely ephemeral: never persisted, and cleared the moment any plain
 * (non-jump) navigation happens (navigator/chip/breadcrumb) or once the jump's
 * scroll has settled.
 */
export interface ThreadJumpTarget {
  threadId: ThreadId;
  targetUuid: string;
}

/**
 * Navigation/layout state: the focused session, the active thread within it, and
 * terminal pane visibility. Focus is purely client-side — the server emits no
 * focus event.
 */
export interface NavState {
  /** Focused session id, the new-session sentinel, or null before load. */
  focusedSessionId: FocusedSession;
  /**
   * The focus the user was on when they entered the new-session state, so it can
   * be restored if they dismiss the new-session intent (e.g. cancel the
   * working-directory picker). Never persisted — a reload must never restore a
   * stale intent.
   */
  preNewSessionFocus: FocusedSession;
  /** Active thread within the focused session (scoped to it). */
  activeThreadId: ThreadId | null;
  /**
   * The pending cross-lane jump intent (see {@link ThreadJumpTarget}), or
   * `null` when the current active-thread change was not a timeline jump or the
   * jump has already settled. Never persisted.
   */
  activeThreadJumpTarget: ThreadJumpTarget | null;
  /**
   * Whether the settings overlay is shown. When set, the settings dialog is
   * layered on top of the workspace (the center conversation pane stays in
   * place beneath it) and the navigator highlights its settings entry. Opened
   * from the navigator's lower-left settings entry and closed via the dialog's
   * Close button, Esc, or a backdrop click.
   */
  settingsOpen: boolean;
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
  /**
   * Enter the new-session state, stashing the current focus so it can be
   * restored on cancel. Only stashes when not already in new-session, so
   * repeated entries don't overwrite the real previous focus.
   */
  startNewSession: () => void;
  /**
   * Dismiss the new-session intent, returning to the previously-focused session
   * if there is a real one to return to; otherwise a no-op (the empty initial
   * screen keeps new-session as the mandatory default).
   */
  cancelNewSession: () => void;
  /**
   * Switch the active thread from a plain (non-timeline) navigation source —
   * navigator selection, branch chip, breadcrumb. Clears any pending jump
   * intent so the transcript pane keeps its usual stick-to-bottom jump +
   * armed-stick behavior for these sources.
   */
  setActiveThread: (threadId: ThreadId) => void;
  /**
   * Switch the active thread as part of a timeline-initiated cross-lane jump,
   * recording the jump target atomically with the switch. TranscriptPane's
   * thread-change layout effect reads {@link activeThreadJumpTarget}
   * synchronously in the resulting commit and lands on the target instead of
   * the lane's tail.
   */
  setActiveThreadWithJumpTarget: (
    threadId: ThreadId,
    targetUuid: string,
  ) => void;
  /**
   * Clear the pending jump intent once the jump's scroll has settled. When
   * `expectedTargetUuid` is given, only clears if the current intent still
   * points at that uuid, so a later jump's intent is never clobbered by an
   * earlier jump's settle callback.
   */
  clearActiveThreadJumpTarget: (expectedTargetUuid?: string) => void;
  /** Open the settings overlay. */
  openSettings: () => void;
  /** Close the settings overlay, returning to the workspace beneath it. */
  closeSettings: () => void;
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
      preNewSessionFocus: null,
      activeThreadId: null,
      activeThreadJumpTarget: null,
      settingsOpen: false,
      terminalOpen: false,
      terminalWidth: DEFAULT_TERMINAL_WIDTH,

      setFocusedSession: (sessionId) =>
        set((state) =>
          state.focusedSessionId === sessionId && !state.settingsOpen
            ? state
            : // Focusing a session always closes the settings overlay — picking
              // a conversation dismisses the modal layered over the workspace.
              {
                focusedSessionId: sessionId,
                activeThreadId: null,
                activeThreadJumpTarget: null,
                settingsOpen: false,
              },
        ),
      startNewSession: () =>
        set((state) => ({
          preNewSessionFocus:
            state.focusedSessionId === NEW_SESSION_FOCUS
              ? state.preNewSessionFocus
              : state.focusedSessionId,
          focusedSessionId: NEW_SESSION_FOCUS,
          activeThreadId: null,
          activeThreadJumpTarget: null,
          // Starting a new session closes the settings overlay.
          settingsOpen: false,
        })),
      cancelNewSession: () =>
        set((state) => {
          const prev = state.preNewSessionFocus;
          // Only a real session id is a valid place to return to. null / the
          // new-session sentinel mean there is nowhere to go (e.g. the empty
          // initial screen), so stay in new-session.
          if (prev === null || prev === NEW_SESSION_FOCUS) {
            return state;
          }
          return {
            focusedSessionId: prev,
            activeThreadId: null,
            activeThreadJumpTarget: null,
            preNewSessionFocus: null,
          };
        }),
      setActiveThread: (threadId) =>
        set({ activeThreadId: threadId, activeThreadJumpTarget: null }),
      setActiveThreadWithJumpTarget: (threadId, targetUuid) =>
        set({
          activeThreadId: threadId,
          activeThreadJumpTarget: { threadId, targetUuid },
        }),
      clearActiveThreadJumpTarget: (expectedTargetUuid) =>
        set((state) => {
          if (state.activeThreadJumpTarget === null) {
            return state;
          }
          if (
            expectedTargetUuid !== undefined &&
            state.activeThreadJumpTarget.targetUuid !== expectedTargetUuid
          ) {
            // A newer jump has already replaced the intent — leave it alone.
            return state;
          }
          return { activeThreadJumpTarget: null };
        }),
      openSettings: () => set({ settingsOpen: true }),
      closeSettings: () => set({ settingsOpen: false }),
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
        settingsOpen: state.settingsOpen,
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
