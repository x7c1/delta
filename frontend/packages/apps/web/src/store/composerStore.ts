import { create } from 'zustand';
import type { MessageUuid, ThreadId } from '@delta/model';

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

  setDraft: (threadId: ThreadId, text: string) => void;
  clearDraft: (threadId: ThreadId) => void;
  setBranchOrigin: (origin: BranchOrigin | null) => void;
}

export const useComposerStore = create<ComposerState>((set) => ({
  drafts: {},
  branchOrigin: null,

  setDraft: (threadId, text) =>
    set((state) => ({ drafts: { ...state.drafts, [threadId]: text } })),

  clearDraft: (threadId) =>
    set((state) => {
      const next = { ...state.drafts };
      delete next[threadId];
      return { drafts: next };
    }),

  setBranchOrigin: (origin) => set({ branchOrigin: origin }),
}));
