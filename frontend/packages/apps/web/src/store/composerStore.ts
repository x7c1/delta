import { create } from 'zustand';
import type { MessageUuid, ThreadId } from '@delta/model';
import type { WorktreeStartPoint } from '@delta/wire-gen';

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
   * Whether the new session should start in a fresh git worktree of the
   * selected `newSessionWorkdir`. Only meaningful when that directory is a git
   * repository (the picker hides the toggle otherwise). Default OFF, in which
   * case the send omits `worktree` (today's behavior). Like
   * `newSessionWorkdir`, it is session-only: reset when the selected directory
   * changes, on a successful new-session send, and whenever the new-session
   * state is left.
   */
  newSessionWorktreeEnabled: boolean;
  /**
   * Where the worktree's branch should be cut from when
   * `newSessionWorktreeEnabled` is on. Defaults to the repo's current `HEAD`
   * (the safe, no-fetch choice), or a named remote branch the user picks. Only
   * read when the toggle is on.
   */
  newSessionWorktreeStartPoint: WorktreeStartPoint;
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
  setNewSessionWorktreeEnabled: (enabled: boolean) => void;
  setNewSessionWorktreeStartPoint: (startPoint: WorktreeStartPoint) => void;
  setNewSessionLaunchOptionIds: (ids: number[]) => void;
  openWorkdirDialog: () => void;
  closeWorkdirDialog: () => void;
}

/**
 * The default worktree start-point: the repository's current `HEAD`. The safe
 * choice — it needs no `git fetch` — so it is both the initial value and what
 * the toggle resets to whenever it is switched off or the directory changes.
 */
export const DEFAULT_WORKTREE_START_POINT: WorktreeStartPoint = { kind: 'head' };

export const useComposerStore = create<ComposerState>((set) => ({
  drafts: {},
  branchOrigin: null,
  newSessionWorkdir: null,
  newSessionWorktreeEnabled: false,
  newSessionWorktreeStartPoint: DEFAULT_WORKTREE_START_POINT,
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

  setNewSessionWorkdir: (workdir) =>
    // A new directory selection invalidates any previous git/worktree choice
    // (the new directory may not be a git repo, and its branches differ), so
    // reset the worktree state back to its defaults alongside the workdir.
    set({
      newSessionWorkdir: workdir,
      newSessionWorktreeEnabled: false,
      newSessionWorktreeStartPoint: DEFAULT_WORKTREE_START_POINT,
    }),

  setNewSessionWorktreeEnabled: (enabled) =>
    // Switching the toggle off returns the start-point to the safe default so a
    // later re-enable does not silently reuse a stale remote-branch pick.
    set(
      enabled
        ? { newSessionWorktreeEnabled: true }
        : {
            newSessionWorktreeEnabled: false,
            newSessionWorktreeStartPoint: DEFAULT_WORKTREE_START_POINT,
          },
    ),

  setNewSessionWorktreeStartPoint: (startPoint) =>
    set({ newSessionWorktreeStartPoint: startPoint }),

  setNewSessionLaunchOptionIds: (ids) =>
    set({ newSessionLaunchOptionIds: ids }),

  openWorkdirDialog: () => set({ workdirDialogOpen: true }),

  closeWorkdirDialog: () => set({ workdirDialogOpen: false }),
}));
