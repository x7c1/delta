import { create } from 'zustand';
import type { MessageUuid, ThreadId } from '@delta/model';

/**
 * Stable draft / pending-queue key for the new-session composer state, which has
 * no real thread id yet (a fresh spawn has no thread until its first hook
 * binds). Negative so it never collides with a server-issued thread id.
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

  setDraft: (threadId: ThreadId, text: string) => void;
  clearDraft: (threadId: ThreadId) => void;
  setBranchOrigin: (origin: BranchOrigin | null) => void;
  setNewSessionWorkdir: (workdir: string | null) => void;
}

export const useComposerStore = create<ComposerState>((set) => ({
  drafts: {},
  branchOrigin: null,
  newSessionWorkdir: null,

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
}));
