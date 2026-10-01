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
 * terminal pane visibility (per session). Focus is purely client-side — the
 * server emits no focus event.
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
  /**
   * Whether the terminal pane is shown (persistent pane on large screens, or
   * the slide-in overlay on small screens), per session: `true` / `false` is a
   * choice the user made for that session, and a session with no entry falls
   * back to the layout's default (see {@link isTerminalOpen}).
   *
   * Per session rather than one flag so that closing one session's terminal
   * does not close it for a new session, which should open with its pane in
   * view: a fresh launch can stop on an interactive prompt (Claude Code's
   * workspace-trust dialog) that only a human at the pane can answer. Entries
   * for sessions no longer listed are dropped by
   * {@link NavState.pruneTerminalOpen}.
   */
  terminalOpenBySession: Record<SessionId, boolean>;
  /**
   * Whether the terminal pane is shown while no session is focused (the
   * new-session screen, or before the list loads): there is no session to key
   * a choice on, so this screen keeps a single flag, closed by default.
   */
  terminalOpenWithoutSession: boolean;
  /**
   * Whether the comms-log pane is shown — the right-pane window a session whose
   * provider has no terminal gets instead (see `ProviderCapabilities`).
   *
   * Deliberately its OWN flag rather than a shared "right pane open" bit: both
   * are persisted, and with a single bit a `true` left behind by a Claude session
   * would open the comms pane the moment a Codex session took focus (and vice
   * versa) — a pane the user never asked for. With two flags each pane only ever
   * reopens for the kind of session it belongs to.
   */
  commsOpen: boolean;
  /**
   * Width of the persistent right pane in pixels (large screens only), shared by
   * the terminal and comms-log panes: at most one is ever shown, and a user who
   * has sized the right column means that size for the column, not for one
   * provider's version of it.
   */
  terminalWidth: number;

  /**
   * Focus a session from a USER navigation (a navigator row or sub-thread
   * click). Switching to a different session clears the active thread (the
   * workspace reconciles it to that session's main) and dismisses the settings
   * overlay — picking a conversation closes the modal layered over it.
   * Re-focusing the same session is a true no-op.
   *
   * Programmatic focus resolution must use {@link reconcileFocusedSession}
   * instead.
   */
  setFocusedSession: (sessionId: FocusedSession) => void;
  /**
   * Focus a session because the WORKSPACE resolved what focus should be — a
   * tracked spawn registering, or a persisted/absent focus being reconciled
   * against the loaded session list. Identical to
   * {@link setFocusedSession} except that it never touches the settings
   * overlay: these calls land asynchronously (whenever the session list
   * refetch resolves), so letting them dismiss the overlay would tear down a
   * dialog the user opened moments earlier, at a moment nothing about the
   * user's own actions predicts.
   */
  reconcileFocusedSession: (sessionId: FocusedSession) => void;
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
  /** Open or close the terminal for the focused session (see
   *  {@link terminalOpenBySession}). */
  setTerminalOpen: (open: boolean) => void;
  /**
   * Flip the focused session's terminal. Takes the layout because a session
   * with no saved choice starts from the layout's default (see
   * {@link isTerminalOpen}), and the flip is from what the user is looking at.
   */
  toggleTerminal: (isLargeScreen: boolean) => void;
  /**
   * Drop the saved terminal choice of every session not in `sessionIds`, so
   * the record does not grow forever. Call it only with the complete list —
   * a partial one would forget the choices of sessions on unloaded pages.
   */
  pruneTerminalOpen: (sessionIds: Iterable<SessionId>) => void;
  /**
   * Carry the new-session screen's terminal choice over to the session a send
   * from that screen just spawned: when {@link terminalOpenWithoutSession} is
   * open, the spawned session starts with its terminal open too.
   *
   * Only an OPEN flag is carried. A session with no entry already reads as
   * closed on the small layout, and on the large one closed would override the
   * open default a fresh launch needs (its pane may be waiting on a prompt only
   * a human can answer). Without this, on the small layout the overlay the
   * user opened before sending would vanish the moment focus moves to the new
   * session, which has no entry and so falls back to closed. A choice already
   * recorded for the session is left alone.
   */
  carryNewSessionTerminalOver: (sessionId: SessionId) => void;
  setCommsOpen: (open: boolean) => void;
  toggleComms: () => void;
  /** Set the terminal pane width, clamped to the allowed range. */
  setTerminalWidth: (width: number) => void;
}

/** localStorage key for the persisted layout state. */
export const NAV_STORAGE_KEY = 'delta-nav';

/**
 * Whether the terminal pane is shown for the focused session.
 *
 * A session the user has not opened or closed the terminal for yet is **open**
 * on the large-screen layout, where the terminal is a persistent column beside
 * the conversation, so a new session comes up with its pane in view — the one
 * way to see that the launch is waiting on a prompt only a human can answer.
 * On the small-screen layout the terminal is a slide-in overlay that covers the
 * conversation, so there the same session starts **closed**.
 */
export function isTerminalOpen(
  state: Pick<
    NavState,
    'focusedSessionId' | 'terminalOpenBySession' | 'terminalOpenWithoutSession'
  >,
  isLargeScreen: boolean,
): boolean {
  const focused = state.focusedSessionId;
  if (focused === null || focused === NEW_SESSION_FOCUS) {
    return state.terminalOpenWithoutSession;
  }
  return state.terminalOpenBySession[focused] ?? isLargeScreen;
}

/** The terminal-open write for the focused session: its record entry, or the
 *  no-session flag when nothing is focused. */
function terminalOpenChange(
  state: NavState,
  open: boolean,
): Partial<NavState> {
  const focused = state.focusedSessionId;
  if (focused === null || focused === NEW_SESSION_FOCUS) {
    return { terminalOpenWithoutSession: open };
  }
  return {
    terminalOpenBySession: { ...state.terminalOpenBySession, [focused]: open },
  };
}

/** The layout fields written to {@link NAV_STORAGE_KEY}. */
type PersistedNav = Pick<
  NavState,
  | 'focusedSessionId'
  | 'activeThreadId'
  | 'settingsOpen'
  | 'terminalOpenBySession'
  | 'terminalOpenWithoutSession'
  | 'commsOpen'
  | 'terminalWidth'
>;

/** The persisted fields restored exactly as stored. */
const RESTORED_AS_IS = [
  'focusedSessionId',
  'activeThreadId',
  'settingsOpen',
  'terminalOpenWithoutSession',
  'commsOpen',
  'terminalWidth',
] as const satisfies readonly (keyof PersistedNav)[];

/** Keep only the well-formed entries of a restored per-session record. */
function restoreTerminalOpenBySession(raw: unknown): Record<SessionId, boolean> {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) {
    return {};
  }
  const restored: Record<SessionId, boolean> = {};
  for (const [id, open] of Object.entries(raw)) {
    if (typeof open === 'boolean') {
      restored[id] = open;
    }
  }
  return restored;
}

/**
 * The session-scoped state a real focus change drops: the active thread (the
 * workspace reconciles it to the newly focused session's main) and any pending
 * cross-lane jump intent, which only ever describes the session being left.
 * Shared by the user-driven and workspace-driven focus setters, which differ
 * only in whether they also dismiss the settings overlay.
 */
function focusChange(sessionId: FocusedSession) {
  return {
    focusedSessionId: sessionId,
    activeThreadId: null,
    activeThreadJumpTarget: null,
  };
}

/**
 * Navigation/layout store. The focused session, active thread, right-pane
 * visibility (the terminal per session, the comms log as one flag), and
 * right-pane width are **persisted to localStorage** so a browser reload
 * restores the same layout instead of snapping back to a closed terminal on
 * `main`. A restored focused session that no longer exists, or an active
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
      terminalOpenBySession: {},
      terminalOpenWithoutSession: false,
      commsOpen: false,
      terminalWidth: DEFAULT_TERMINAL_WIDTH,

      setFocusedSession: (sessionId) =>
        set((state) =>
          state.focusedSessionId === sessionId
            ? state
            : // A user-driven focus change dismisses the settings overlay —
              // picking a conversation closes the modal layered over the
              // workspace.
              { ...focusChange(sessionId), settingsOpen: false },
        ),
      reconcileFocusedSession: (sessionId) =>
        set((state) =>
          state.focusedSessionId === sessionId ? state : focusChange(sessionId),
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
      setTerminalOpen: (open) =>
        set((state) => terminalOpenChange(state, open)),
      toggleTerminal: (isLargeScreen) =>
        set((state) =>
          terminalOpenChange(state, !isTerminalOpen(state, isLargeScreen)),
        ),
      pruneTerminalOpen: (sessionIds) =>
        set((state) => {
          const keep = new Set(sessionIds);
          const entries = Object.entries(state.terminalOpenBySession);
          const kept = entries.filter(([id]) => keep.has(id));
          // Leave the state untouched when nothing is dropped, so a refetch
          // that lists the same sessions does not re-render every subscriber.
          if (kept.length === entries.length) {
            return state;
          }
          return { terminalOpenBySession: Object.fromEntries(kept) };
        }),
      carryNewSessionTerminalOver: (sessionId) =>
        set((state) =>
          state.terminalOpenWithoutSession &&
          state.terminalOpenBySession[sessionId] === undefined
            ? {
                terminalOpenBySession: {
                  ...state.terminalOpenBySession,
                  [sessionId]: true,
                },
              }
            : state,
        ),
      setCommsOpen: (open) => set({ commsOpen: open }),
      toggleComms: () => set((state) => ({ commsOpen: !state.commsOpen })),
      setTerminalWidth: (width) => set({ terminalWidth: clampTerminalWidth(width) }),
    }),
    {
      name: NAV_STORAGE_KEY,
      storage: createJSONStorage(() => localStorage),
      // Persist only the layout values, never the action functions.
      partialize: (state): PersistedNav => ({
        focusedSessionId: state.focusedSessionId,
        activeThreadId: state.activeThreadId,
        settingsOpen: state.settingsOpen,
        terminalOpenBySession: state.terminalOpenBySession,
        terminalOpenWithoutSession: state.terminalOpenWithoutSession,
        commsOpen: state.commsOpen,
        terminalWidth: state.terminalWidth,
      }),
      // Restore only the fields this version persists. In particular the
      // single `terminalOpen` flag an earlier version stored is dropped rather
      // than migrated: it described no session in particular, and carrying it
      // over would close the pane of every session nobody has chosen for yet.
      merge: (persisted, current) => {
        const saved = (persisted ?? {}) as Partial<PersistedNav>;
        const restored: Partial<PersistedNav> = {};
        for (const key of RESTORED_AS_IS) {
          if (key in saved) {
            Object.assign(restored, { [key]: saved[key] });
          }
        }
        return {
          ...current,
          ...restored,
          terminalOpenBySession: restoreTerminalOpenBySession(
            saved.terminalOpenBySession,
          ),
        };
      },
      // Re-clamp the restored width in case the viewport shrank since last time.
      onRehydrateStorage: () => (state) => {
        if (state) {
          state.terminalWidth = clampTerminalWidth(state.terminalWidth);
        }
      },
    },
  ),
);
