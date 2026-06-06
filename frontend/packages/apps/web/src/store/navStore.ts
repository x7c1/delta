import { create } from 'zustand';
import type { ThreadId } from '@delta/model';

/** Navigation/layout state: the active thread and terminal pane visibility. */
export interface NavState {
  activeThreadId: ThreadId | null;
  /** Whether the terminal pane is shown (persistent pane on large screens, or
   *  the slide-in overlay on small screens). */
  terminalOpen: boolean;

  setActiveThread: (threadId: ThreadId) => void;
  setTerminalOpen: (open: boolean) => void;
  toggleTerminal: () => void;
}

export const useNavStore = create<NavState>((set) => ({
  activeThreadId: null,
  terminalOpen: false,

  setActiveThread: (threadId) => set({ activeThreadId: threadId }),
  setTerminalOpen: (open) => set({ terminalOpen: open }),
  toggleTerminal: () => set((state) => ({ terminalOpen: !state.terminalOpen })),
}));
