import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import type { MessageUuid, ThreadId } from '@delta/model';
import type {
  AgentProvider,
  PullRequest,
  WorktreeStartPoint,
} from '@delta/wire-gen';
import { DEFAULT_PROVIDER } from '../providers';

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
   * How the current {@link ComposerState.newSessionWorkdir} was picked. Written
   * atomically with the workdir itself, so the two can never disagree.
   *
   * The provenance is what makes the worktree UI honest: a `pr` pick has
   * already decided the branch (the PR's head ref), so the worktree controls
   * render as a locked summary instead of a selector the user could move off
   * the PR. A `directory` pick keeps the full selector. The provenance also
   * drives the PR tab's "you picked this" row highlight, so at most one row
   * reads as the active pick across the three tabs — a directory pick resets
   * the provenance and the highlight goes with it.
   */
  newSessionWorkdirSource: NewSessionWorkdirSource;
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
   * field is needed.
   *
   * Carries the wire {@link WorktreeStartPoint} union plus an extra
   * `pending_remote_branch` sentinel that records "the user toggled worktree
   * on (or picked the Other-remote-branch radio) but has not chosen a concrete
   * branch yet". Dogfooding showed the typical case is to start from a
   * specific remote branch, so the toggle's default lands on the picker mode
   * rather than silently committing to `HEAD`. The sentinel never reaches the
   * wire — the composer omits `worktree` while it is present and the Send
   * button stays disabled, so the backend (which rejects worktree requests
   * without a concrete branch) never sees it. Only read when the toggle is on.
   */
  newSessionWorktreeStartPoint: WorktreeStartPointSelection;
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
  /**
   * Which AI-agent provider the next new session launches on — the top-level
   * axis of the new-session form, since it changes the backend binary and gates
   * capability-dependent controls. Session-only, like the other `newSession*`
   * ephemerals: reset to {@link DEFAULT_NEW_SESSION_PROVIDER} whenever the
   * new-session compose state is left. Attached to the send as `provider`; the
   * composer omits it for the Claude default so a Claude send stays byte-for-
   * byte identical to today's.
   */
  newSessionProvider: AgentProvider;
  /**
   * Whether `newSessionProvider` has been seeded from the persisted
   * default-provider setting (or explicitly set by the user) yet for the
   * current new-session compose state.
   *
   * The provider selector seeds the initial value from the Settings
   * `defaultProvider` exactly once when a fresh new-session compose is entered.
   * This flag distinguishes "not seeded yet" (seed from the default) from "user
   * has since picked a provider" (leave their choice alone) — mirroring the
   * launch-options seed guard so a later re-seed (e.g. the default changing
   * mid-compose) never clobbers an explicit per-session choice. Reset to
   * `false` together with `newSessionProvider` whenever the new-session compose
   * state is (re)entered or cleared, so the next fresh compose reseeds.
   */
  newSessionProviderSeeded: boolean;

  setDraft: (threadId: ThreadId, text: string) => void;
  clearDraft: (threadId: ThreadId) => void;
  setBranchOrigin: (origin: BranchOrigin | null) => void;
  /**
   * Commit a directory pick (Repository / Directory tab, or a `null` reset when
   * the new-session state is left). Stamps `directory` provenance, so a prior
   * PR pick — its provenance, and with it the PR row highlight and the locked
   * worktree summary — never survives into a directory-picked session.
   */
  setNewSessionWorkdir: (workdir: string | null) => void;
  /**
   * Commit a PR pick from the PR tab: the PR's local clone as the workdir, `pr`
   * provenance, and the worktree forced on at the PR's head branch. One action
   * so the whole pick lands in a single store update — writing the workdir
   * first would momentarily reset the worktree state it then has to set again.
   */
  setNewSessionWorkdirFromPr: (workdir: string, pr: PullRequest) => void;
  setNewSessionWorktreeEnabled: (enabled: boolean) => void;
  setNewSessionWorktreeStartPoint: (startPoint: WorktreeStartPointSelection) => void;
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
  /**
   * Set the selected provider from a user interaction in the selector. Marks
   * the selection as seeded, so a later seed will not overwrite an explicit
   * per-session choice with the persisted default.
   */
  setNewSessionProvider: (provider: AgentProvider) => void;
  /**
   * Seed the initial provider from the persisted default-provider setting.
   * A no-op once the provider has already been seeded (or the user has picked
   * one), so it only ever supplies the initial value. Marks the selection
   * seeded.
   */
  seedNewSessionProvider: (provider: AgentProvider) => void;
  /**
   * Reset the provider to {@link DEFAULT_NEW_SESSION_PROVIDER} and clear the
   * seeded flag, so the next new-session compose reseeds from the persisted
   * default. Used wherever the new-session compose state is left.
   */
  resetNewSessionProvider: () => void;
}

/**
 * A PR-picked workdir: the identity of the pull request the session is for.
 * Only the fields the UI reads are carried, named after the wire
 * {@link PullRequest} so they cannot drift from it — `head_ref` is the branch
 * the worktree is locked to, `number`/`repo_owner`/`repo_name` label the lock,
 * and `url` identifies the picked row for its highlight.
 */
export type NewSessionPrWorkdirSource = { kind: 'pr' } & Pick<
  PullRequest,
  'url' | 'number' | 'repo_owner' | 'repo_name' | 'head_ref'
>;

/**
 * How the new session's working directory was picked — see
 * {@link ComposerState.newSessionWorkdirSource}. `directory` covers every
 * plain path pick (a Repository-tab clone, a Directory-tab row, a browse
 * result) as well as the "nothing picked yet" state; only the PR tab produces
 * `pr`.
 */
export type NewSessionWorkdirSource =
  | { kind: 'directory' }
  | NewSessionPrWorkdirSource;

/**
 * The provenance of a directory pick, and of the "no directory picked yet"
 * state a fresh compose starts in. Shared by the store's initial state and by
 * `setNewSessionWorkdir`, so the reset path and the fresh state cannot drift.
 */
export const DEFAULT_NEW_SESSION_WORKDIR_SOURCE: NewSessionWorkdirSource = {
  kind: 'directory',
};

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
 * The provider a fresh new-session compose starts on before the persisted
 * default-provider setting seeds it, and the value it resets to when the
 * new-session state is left. Aliases the app-wide {@link DEFAULT_PROVIDER}
 * so the pre-seed placeholder can never drift from the fresh-install default
 * the selector seeds it with.
 */
export const DEFAULT_NEW_SESSION_PROVIDER: AgentProvider = DEFAULT_PROVIDER;

/**
 * Selection state for the worktree start-point: the wire union plus a
 * `pending_remote_branch` sentinel for the "Other remote branch picker is
 * open but no branch has been chosen yet" UI state. The sentinel never
 * reaches the wire (see {@link ComposerState.newSessionWorktreeStartPoint}).
 */
export type WorktreeStartPointSelection =
  | WorktreeStartPoint
  | { kind: 'pending_remote_branch' };

/**
 * The default worktree start-point when the toggle is first flipped on:
 * "Other remote branch" with no branch picked yet. Dogfooding showed the
 * typical case is to start from a specific remote branch, so the picker
 * opens directly in branch-list mode rather than silently committing to
 * `HEAD`. Send stays disabled until a concrete branch is chosen.
 *
 * Used by the store on directory change / toggle off too, so a stale branch
 * pick can never bleed back into the next worktree session.
 */
export const DEFAULT_WORKTREE_START_POINT: WorktreeStartPointSelection = {
  kind: 'pending_remote_branch',
};

/** localStorage key for the persisted composer state slice. */
export const COMPOSER_STORAGE_KEY = 'delta-composer';

export const useComposerStore = create<ComposerState>()(
  persist(
    (set) => ({
      drafts: {},
      branchOrigin: null,
      newSessionWorkdir: null,
      newSessionWorkdirSource: DEFAULT_NEW_SESSION_WORKDIR_SOURCE,
      newSessionWorktreeEnabled: false,
      newSessionWorktreeStartPoint: DEFAULT_WORKTREE_START_POINT,
      newSessionLaunchOptionIds: [],
      newSessionLaunchOptionsSeeded: false,
      workdirDialogOpen: false,
      newSessionTab: DEFAULT_NEW_SESSION_TAB,
      newSessionProvider: DEFAULT_NEW_SESSION_PROVIDER,
      newSessionProviderSeeded: false,

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
        // The provenance resets with it: this path is only ever a directory
        // pick (or a clear), so a stale `pr` provenance must not outlive it.
        set({
          newSessionWorkdir: workdir,
          newSessionWorkdirSource: DEFAULT_NEW_SESSION_WORKDIR_SOURCE,
          newSessionWorktreeEnabled: false,
          newSessionWorktreeStartPoint: DEFAULT_WORKTREE_START_POINT,
        }),

      setNewSessionWorkdirFromPr: (workdir, pr) =>
        // The PR's head ref is by definition a non-default branch: cut the
        // worktree to check that branch out itself (the `use_remote_branch`
        // mode) so resuming a PR's work simply attaches to its branch.
        set({
          newSessionWorkdir: workdir,
          newSessionWorkdirSource: {
            kind: 'pr',
            url: pr.url,
            number: pr.number,
            repo_owner: pr.repo_owner,
            repo_name: pr.repo_name,
            head_ref: pr.head_ref,
          },
          newSessionWorktreeEnabled: true,
          newSessionWorktreeStartPoint: {
            kind: 'use_remote_branch',
            name: pr.head_ref,
          },
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

      setNewSessionProvider: (provider) =>
        set({ newSessionProvider: provider, newSessionProviderSeeded: true }),

      seedNewSessionProvider: (provider) =>
        set((state) =>
          state.newSessionProviderSeeded
            ? state
            : { newSessionProvider: provider, newSessionProviderSeeded: true },
        ),

      resetNewSessionProvider: () =>
        set({
          newSessionProvider: DEFAULT_NEW_SESSION_PROVIDER,
          newSessionProviderSeeded: false,
        }),
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
