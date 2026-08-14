import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { PullRequest } from '@delta/wire-gen';
import {
  DEFAULT_NEW_SESSION_PROVIDER,
  DEFAULT_NEW_SESSION_TAB,
  DEFAULT_NEW_SESSION_WORKDIR_SOURCE,
  DEFAULT_WORKTREE_START_POINT,
  useComposerStore,
} from './composerStore';

const RESET_STATE = {
  drafts: {},
  branchOrigin: null,
  newSessionWorkdir: null,
  newSessionWorkdirSource: DEFAULT_NEW_SESSION_WORKDIR_SOURCE,
  newSessionWorktreeEnabled: false,
  newSessionWorktreeStartPoint: DEFAULT_WORKTREE_START_POINT,
  workdirDialogOpen: false,
  newSessionTab: DEFAULT_NEW_SESSION_TAB,
  newSessionProvider: DEFAULT_NEW_SESSION_PROVIDER,
  newSessionProviderSeeded: false,
} as const;

/** The PR the provenance tests pick, shaped like the wire payload. */
const PR: PullRequest = {
  number: 174,
  title: 'feat: add Repository tab to the new-session screen',
  repo_owner: 'x7c1',
  repo_name: 'delta',
  head_ref: 'feat/repo-tab',
  head_repo_owner: 'x7c1',
  head_repo_name: 'delta',
  draft: false,
  url: 'https://github.com/x7c1/delta/pull/174',
  updated_at: '2026-06-24T00:00:00Z',
  author_login: 'x7c1',
  has_local_clone: true,
};

beforeEach(() => {
  useComposerStore.setState(RESET_STATE);
});

afterEach(() => {
  useComposerStore.setState(RESET_STATE);
});

describe('composerStore workdir dialog', () => {
  it('defaults to closed', () => {
    expect(useComposerStore.getState().workdirDialogOpen).toBe(false);
  });

  it('openWorkdirDialog / closeWorkdirDialog toggle the open state', () => {
    useComposerStore.getState().openWorkdirDialog();
    expect(useComposerStore.getState().workdirDialogOpen).toBe(true);

    useComposerStore.getState().closeWorkdirDialog();
    expect(useComposerStore.getState().workdirDialogOpen).toBe(false);
  });
});

describe('composerStore worktree selection', () => {
  it('defaults the toggle off with the pending-branch start-point', () => {
    // Dogfooding default: the toggle is off, and once enabled the picker
    // lands in "Other remote branch" mode (the `pending_remote_branch`
    // sentinel) so the user picks a specific remote branch.
    const state = useComposerStore.getState();
    expect(state.newSessionWorktreeEnabled).toBe(false);
    expect(state.newSessionWorktreeStartPoint).toEqual({
      kind: 'pending_remote_branch',
    });
  });

  it('switching the toggle off resets the start-point to the picker default', () => {
    const store = useComposerStore.getState();
    store.setNewSessionWorktreeEnabled(true);
    store.setNewSessionWorktreeStartPoint({
      kind: 'remote_branch',
      name: 'develop',
    });
    expect(useComposerStore.getState().newSessionWorktreeStartPoint).toEqual({
      kind: 'remote_branch',
      name: 'develop',
    });

    useComposerStore.getState().setNewSessionWorktreeEnabled(false);
    const state = useComposerStore.getState();
    expect(state.newSessionWorktreeEnabled).toBe(false);
    expect(state.newSessionWorktreeStartPoint).toEqual({
      kind: 'pending_remote_branch',
    });
  });

  it('toggling the worktree off then on returns to the pending-branch default', () => {
    // Regression: a stale branch pick from a previous worktree session must
    // not bleed back when the toggle is re-enabled.
    const store = useComposerStore.getState();
    store.setNewSessionWorktreeEnabled(true);
    store.setNewSessionWorktreeStartPoint({
      kind: 'use_remote_branch',
      name: 'feature/x',
    });
    store.setNewSessionWorktreeEnabled(false);
    store.setNewSessionWorktreeEnabled(true);

    const state = useComposerStore.getState();
    expect(state.newSessionWorktreeEnabled).toBe(true);
    expect(state.newSessionWorktreeStartPoint).toEqual({
      kind: 'pending_remote_branch',
    });
  });

  it('changing the selected directory resets the worktree state', () => {
    const store = useComposerStore.getState();
    store.setNewSessionWorkdir('/home/dev/repo');
    store.setNewSessionWorktreeEnabled(true);
    store.setNewSessionWorktreeStartPoint({
      kind: 'remote_branch',
      name: 'feature/x',
    });

    // A new directory selection invalidates the previous git/branch choice.
    useComposerStore.getState().setNewSessionWorkdir('/home/dev/other');
    const state = useComposerStore.getState();
    expect(state.newSessionWorkdir).toBe('/home/dev/other');
    expect(state.newSessionWorktreeEnabled).toBe(false);
    expect(state.newSessionWorktreeStartPoint).toEqual({
      kind: 'pending_remote_branch',
    });
  });

  it('clearing the directory (leaving new-session / on send) resets worktree state', () => {
    const store = useComposerStore.getState();
    store.setNewSessionWorkdir('/home/dev/repo');
    store.setNewSessionWorktreeEnabled(true);

    useComposerStore.getState().setNewSessionWorkdir(null);
    const state = useComposerStore.getState();
    expect(state.newSessionWorkdir).toBeNull();
    expect(state.newSessionWorktreeEnabled).toBe(false);
    expect(state.newSessionWorktreeStartPoint).toEqual({
      kind: 'pending_remote_branch',
    });
  });
});


describe('composerStore newSessionTab', () => {
  it('defaults to repository on a fresh state', () => {
    expect(useComposerStore.getState().newSessionTab).toBe(DEFAULT_NEW_SESSION_TAB);
    expect(DEFAULT_NEW_SESSION_TAB).toBe('repository');
  });

  it('setNewSessionTab updates the active tab', () => {
    useComposerStore.getState().setNewSessionTab('directory');
    expect(useComposerStore.getState().newSessionTab).toBe('directory');

    useComposerStore.getState().setNewSessionTab('pr');
    expect(useComposerStore.getState().newSessionTab).toBe('pr');
  });
});

describe('composerStore newSessionProvider seed guard', () => {
  it('defaults to the Claude constant, not yet seeded', () => {
    const state = useComposerStore.getState();
    expect(state.newSessionProvider).toBe(DEFAULT_NEW_SESSION_PROVIDER);
    expect(DEFAULT_NEW_SESSION_PROVIDER).toBe('claude');
    expect(state.newSessionProviderSeeded).toBe(false);
  });

  it('seedNewSessionProvider supplies the initial value and marks it seeded', () => {
    useComposerStore.getState().seedNewSessionProvider('codex');
    expect(useComposerStore.getState().newSessionProvider).toBe('codex');
    expect(useComposerStore.getState().newSessionProviderSeeded).toBe(true);
  });

  it('seedNewSessionProvider is a no-op once the provider is seeded', () => {
    // An explicit pick seeds the selection...
    useComposerStore.getState().setNewSessionProvider('claude');
    expect(useComposerStore.getState().newSessionProviderSeeded).toBe(true);
    // ...so a later seed (e.g. the default changing) must not overwrite it.
    useComposerStore.getState().seedNewSessionProvider('codex');
    expect(useComposerStore.getState().newSessionProvider).toBe('claude');
  });

  it('setNewSessionProvider marks the selection seeded', () => {
    useComposerStore.getState().setNewSessionProvider('codex');
    expect(useComposerStore.getState().newSessionProvider).toBe('codex');
    expect(useComposerStore.getState().newSessionProviderSeeded).toBe(true);
  });

  it('resetNewSessionProvider clears the provider and the seeded flag', () => {
    useComposerStore.getState().setNewSessionProvider('codex');
    useComposerStore.getState().resetNewSessionProvider();
    const state = useComposerStore.getState();
    expect(state.newSessionProvider).toBe(DEFAULT_NEW_SESSION_PROVIDER);
    expect(state.newSessionProviderSeeded).toBe(false);
  });
});

describe('composerStore newSessionWorkdirSource', () => {
  it('starts as a directory pick on a fresh state', () => {
    expect(useComposerStore.getState().newSessionWorkdirSource).toEqual({
      kind: 'directory',
    });
  });

  it('a PR pick commits the clone, the provenance, and the head-branch worktree at once', () => {
    useComposerStore
      .getState()
      .setNewSessionWorkdirFromPr('/home/dev/projects/delta', PR);

    const state = useComposerStore.getState();
    expect(state.newSessionWorkdir).toBe('/home/dev/projects/delta');
    expect(state.newSessionWorkdirSource).toEqual({
      kind: 'pr',
      url: 'https://github.com/x7c1/delta/pull/174',
      number: 174,
      repo_owner: 'x7c1',
      repo_name: 'delta',
      head_ref: 'feat/repo-tab',
    });
    expect(state.newSessionWorktreeEnabled).toBe(true);
    expect(state.newSessionWorktreeStartPoint).toEqual({
      kind: 'use_remote_branch',
      name: 'feat/repo-tab',
    });
  });

  it('a later directory pick drops the PR provenance and its worktree', () => {
    // The mutual-exclusion rule: the provenance is what the PR row highlight
    // and the locked worktree summary read, so moving on to a directory must
    // take both with it.
    const store = useComposerStore.getState();
    store.setNewSessionWorkdirFromPr('/home/dev/projects/delta', PR);
    useComposerStore.getState().setNewSessionWorkdir('/home/dev/projects/other');

    const state = useComposerStore.getState();
    expect(state.newSessionWorkdirSource).toEqual({ kind: 'directory' });
    expect(state.newSessionWorktreeEnabled).toBe(false);
    expect(state.newSessionWorktreeStartPoint).toEqual(
      DEFAULT_WORKTREE_START_POINT,
    );
  });

  it('clearing the directory (leaving new-session) drops the PR provenance', () => {
    const store = useComposerStore.getState();
    store.setNewSessionWorkdirFromPr('/home/dev/projects/delta', PR);
    useComposerStore.getState().setNewSessionWorkdir(null);

    const state = useComposerStore.getState();
    expect(state.newSessionWorkdir).toBeNull();
    expect(state.newSessionWorkdirSource).toEqual({ kind: 'directory' });
  });
});
