import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
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
   * Where the worktree starts from when `newSessionWorktreeEnabled` is on, and
   * for a branch start-point whether the worktree cuts a fresh branch
   * (`remote_branch`) or works on the branch itself (`use_remote_branch`). The
   * use-vs-new mode is encoded directly in the value's `kind`, so no separate
   * field is needed. Defaults to the repo's current `HEAD` (the safe, no-fetch,
   * always-new-branch choice). Only read when the toggle is on.
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
   * Whether `newSessionLaunchOptionIds` has been seeded from the registry's
   * `default_enabled` options yet for the current new-session compose state.
   *
   * The picker seeds the initial selection from the options marked
   * `default_enabled` exactly once, the first time the registry loads. This flag
   * distinguishes "not seeded yet" (seed from defaults) from "user has since
   * unchecked everything" (an empty `newSessionLaunchOptionIds` that must be
   * left alone) — both look like an empty id array otherwise. It is reset to
   * `false` together with `newSessionLaunchOptionIds` whenever the new-session
   * compose state is (re)entered or cleared, so the next fresh compose reseeds.
   */
  newSessionLaunchOptionsSeeded: boolean;
  /**
   * Whether the new-session working-directory picker modal is open. Lifted from
   * local component state into the store so the "New" button can (re)open it
   * even when the app is already in the new-session state (no focus transition
   * to drive a component-local auto-open effect). Session-only; not persisted.
   */
  workdirDialogOpen: boolean;
  /**
   * Which tab the new-session screen shows: PR / Repository / Directory.
   * Persisted to localStorage so a reload restores the user's last choice;
   * defaults to `'repository'` on first run because the dogfooding insight
   * behind the redesign is that most sessions start from a known repo.
   * Restored on rehydration; an unknown value falls back to the default.
   */
  newSessionTab: NewSessionTab;

  setDraft: (threadId: ThreadId, text: string) => void;
  clearDraft: (threadId: ThreadId) => void;
  setBranchOrigin: (origin: BranchOrigin | null) => void;
  setNewSessionWorkdir: (workdir: string | null) => void;
  setNewSessionWorktreeEnabled: (enabled: boolean) => void;
  setNewSessionWorktreeStartPoint: (startPoint: WorktreeStartPoint) => void;
  /**
   * Set the selected launch-option ids from a user interaction in the picker.
   * Marks the selection as seeded, so the picker will not later overwrite an
   * explicit choice (including unchecking everything) with the defaults.
   */
  setNewSessionLaunchOptionIds: (ids: number[]) => void;
  /**
   * Seed the initial selection from the registry's `default_enabled` options.
   * A no-op once the selection has already been seeded (or the user has touched
   * it), so it only ever supplies the initial value. Marks the selection seeded.
   */
  seedNewSessionLaunchOptionIds: (ids: number[]) => void;
  /**
   * Clear the launch-option selection and the seeded flag, so the next
   * new-session compose reseeds from the defaults. Used wherever the
   * new-session compose state is (re)entered or left.
   */
  resetNewSessionLaunchOptions: () => void;
  openWorkdirDialog: () => void;
  closeWorkdirDialog: () => void;
  setNewSessionTab: (tab: NewSessionTab) => void;
}

/** The new-session screen's three tabs. */
export type NewSessionTab = 'pr' | 'repository' | 'directory';

/**
 * The valid `newSessionTab` values, used by the persistence hydration step
 * to fall back to the default when a foreign value lands in localStorage
 * (a different build, a typo, an experiment that left a trail behind). The
 * `as const` tuple narrows to the literal union {@link NewSessionTab}.
 */
const NEW_SESSION_TABS: readonly NewSessionTab[] = ['pr', 'repository', 'directory'];

/**
 * The initial new-session tab on a fresh install: Repository. Dogfooding
 * showed most session starts are "go back to the repo I was working on",
 * so the picker leads with that lens.
 */
export const DEFAULT_NEW_SESSION_TAB: NewSessionTab = 'repository';

/**
 * The default worktree start-point: the repository's current `HEAD`. The safe
 * choice — it needs no `git fetch` — so it is both the initial value and what
 * the toggle resets to whenever it is switched off or the directory changes.
 */
export const DEFAULT_WORKTREE_START_POINT: WorktreeStartPoint = { kind: 'head' };

/** localStorage key for the persisted composer state slice. */
export const COMPOSER_STORAGE_KEY = 'delta-composer';

export const useComposerStore = create<ComposerState>()(
  persist(
    (set) => ({
      drafts: {},
      branchOrigin: null,
      newSessionWorkdir: null,
      newSessionWorktreeEnabled: false,
      newSessionWorktreeStartPoint: DEFAULT_WORKTREE_START_POINT,
      newSessionLaunchOptionIds: [],
      newSessionLaunchOptionsSeeded: false,
      workdirDialogOpen: false,
      newSessionTab: DEFAULT_NEW_SESSION_TAB,

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
        // Switching the toggle off returns the start-point to the safe default so
        // a later re-enable does not silently reuse a stale remote-branch pick.
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
        set({ newSessionLaunchOptionIds: ids, newSessionLaunchOptionsSeeded: true }),

      seedNewSessionLaunchOptionIds: (ids) =>
        set((state) =>
          state.newSessionLaunchOptionsSeeded
            ? state
            : { newSessionLaunchOptionIds: ids, newSessionLaunchOptionsSeeded: true },
        ),

      resetNewSessionLaunchOptions: () =>
        set({ newSessionLaunchOptionIds: [], newSessionLaunchOptionsSeeded: false }),

      openWorkdirDialog: () => set({ workdirDialogOpen: true }),

      closeWorkdirDialog: () => set({ workdirDialogOpen: false }),

      setNewSessionTab: (tab) => set({ newSessionTab: tab }),
    }),
    {
      name: COMPOSER_STORAGE_KEY,
      storage: createJSONStorage(() => localStorage),
      // Only the last-used tab is persisted: drafts, branch origin, picker
      // selections, and the dialog visibility are session-only state that a
      // reload deliberately starts fresh from.
      partialize: (state) => ({ newSessionTab: state.newSessionTab }),
      onRehydrateStorage: () => (state) => {
        if (state && !NEW_SESSION_TABS.includes(state.newSessionTab)) {
          state.newSessionTab = DEFAULT_NEW_SESSION_TAB;
        }
      },
    },
  ),
);
