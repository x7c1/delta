import { create } from 'zustand';
import type { ThreadId } from '@delta/model';

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

/** Navigation/layout state: the active thread and terminal pane visibility. */
export interface NavState {
  activeThreadId: ThreadId | null;
  /** Whether the terminal pane is shown (persistent pane on large screens, or
   *  the slide-in overlay on small screens). */
  terminalOpen: boolean;
  /** Width of the persistent terminal pane in pixels (large screens only).
   *  Session-only; not persisted. */
  terminalWidth: number;

  setActiveThread: (threadId: ThreadId) => void;
  setTerminalOpen: (open: boolean) => void;
  toggleTerminal: () => void;
  /** Set the terminal pane width, clamped to the allowed range. */
  setTerminalWidth: (width: number) => void;
}

export const useNavStore = create<NavState>((set) => ({
  activeThreadId: null,
  terminalOpen: false,
  terminalWidth: DEFAULT_TERMINAL_WIDTH,

  setActiveThread: (threadId) => set({ activeThreadId: threadId }),
  setTerminalOpen: (open) => set({ terminalOpen: open }),
  toggleTerminal: () => set((state) => ({ terminalOpen: !state.terminalOpen })),
  setTerminalWidth: (width) => set({ terminalWidth: clampTerminalWidth(width) }),
}));
