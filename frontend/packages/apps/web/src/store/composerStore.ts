import { create } from 'zustand';
import type { MessageUuid, ThreadId } from '@delta/model';

/**
 * Stable DRAFT key for the new-session composer state, which has no real
 * thread id yet. Negative so it never collides with a server-issued thread id.
 * Used only for the pre-submit draft text (the UI compose state) — pending
 * sends never key on it: they carry the real ids the server returns.
 */
export const NEW_SESSION_DRAFT_KEY = -1 as ThreadId;

/**
 * Composer state: per-thread draft text (kept while switching threads) and the
 * pending branch origin selected via "branch from here". Session-only.
 */

export interface BranchOrigin {
  /** The thread the selected message belongs to (the branch parent thread). */
  parentThreadId: ThreadId;
  /** The selected message = the semantic parent of the new branch. */
  semanticParentUuid: MessageUuid;
  /** The selected text range within that message = the locator quote. */
  locatorQuote: string;
}

export interface ComposerState {
  drafts: Record<ThreadId, string>;
  branchOrigin: BranchOrigin | null;
  /**
   * The working directory chosen in the new-session picker, or `null` for the
   * default (the send then omits `workdir`, preserving today's behavior).
   * Like `drafts`, it is session-only state; it is cleared on a successful
   * new-session send and whenever the new-session state is left.
   */
  newSessionWorkdir: string | null;
  /**
   * The ids of the registered launch options selected for the next new
   * session, in selection order. Empty means "apply no extra launch flags"
   * (the send then omits `launch_option_ids`, preserving today's behavior).
   * Like `newSessionWorkdir`, it is session-only: cleared on a successful
   * new-session send and whenever the new-session state is left.
   */
  newSessionLaunchOptionIds: number[];
  /**
   * Whether the new-session working-directory picker modal is open. Lifted from
   * local component state into the store so the "New" button can (re)open it
   * even when the app is already in the new-session state (no focus transition
   * to drive a component-local auto-open effect). Session-only; not persisted.
   */
  workdirDialogOpen: boolean;

  setDraft: (threadId: ThreadId, text: string) => void;
  clearDraft: (threadId: ThreadId) => void;
  setBranchOrigin: (origin: BranchOrigin | null) => void;
  setNewSessionWorkdir: (workdir: string | null) => void;
  setNewSessionLaunchOptionIds: (ids: number[]) => void;
  openWorkdirDialog: () => void;
  closeWorkdirDialog: () => void;
}

export const useComposerStore = create<ComposerState>((set) => ({
  drafts: {},
  branchOrigin: null,
  newSessionWorkdir: null,
  newSessionLaunchOptionIds: [],
  workdirDialogOpen: false,

  setDraft: (threadId, text) =>
    set((state) => ({ drafts: { ...state.drafts, [threadId]: text } })),

  clearDraft: (threadId) =>
    set((state) => {
      const next = { ...state.drafts };
      delete next[threadId];
      return { drafts: next };
    }),

  setBranchOrigin: (origin) => set({ branchOrigin: origin }),

  setNewSessionWorkdir: (workdir) => set({ newSessionWorkdir: workdir }),

  setNewSessionLaunchOptionIds: (ids) =>
    set({ newSessionLaunchOptionIds: ids }),

  openWorkdirDialog: () => set({ workdirDialogOpen: true }),

  closeWorkdirDialog: () => set({ workdirDialogOpen: false }),
}));
