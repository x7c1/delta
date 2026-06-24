import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  DEFAULT_NEW_SESSION_TAB,
  DEFAULT_WORKTREE_START_POINT,
  useComposerStore,
} from './composerStore';

const RESET_STATE = {
  drafts: {},
  branchOrigin: null,
  newSessionWorkdir: null,
  newSessionWorktreeEnabled: false,
  newSessionWorktreeStartPoint: DEFAULT_WORKTREE_START_POINT,
  workdirDialogOpen: false,
  newSessionTab: DEFAULT_NEW_SESSION_TAB,
} as const;

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
  it('defaults the toggle off with the HEAD start-point', () => {
    const state = useComposerStore.getState();
    expect(state.newSessionWorktreeEnabled).toBe(false);
    expect(state.newSessionWorktreeStartPoint).toEqual({ kind: 'head' });
  });

  it('switching the toggle off resets the start-point to HEAD', () => {
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
    expect(state.newSessionWorktreeStartPoint).toEqual({ kind: 'head' });
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
    expect(state.newSessionWorktreeStartPoint).toEqual({ kind: 'head' });
  });

  it('clearing the directory (leaving new-session / on send) resets worktree state', () => {
    const store = useComposerStore.getState();
    store.setNewSessionWorkdir('/home/dev/repo');
    store.setNewSessionWorktreeEnabled(true);

    useComposerStore.getState().setNewSessionWorkdir(null);
    const state = useComposerStore.getState();
    expect(state.newSessionWorkdir).toBeNull();
    expect(state.newSessionWorktreeEnabled).toBe(false);
    expect(state.newSessionWorktreeStartPoint).toEqual({ kind: 'head' });
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
