import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { useComposerStore } from './composerStore';

beforeEach(() => {
  useComposerStore.setState({
    drafts: {},
    branchOrigin: null,
    newSessionWorkdir: null,
    workdirDialogOpen: false,
  });
});

afterEach(() => {
  useComposerStore.setState({
    drafts: {},
    branchOrigin: null,
    newSessionWorkdir: null,
    workdirDialogOpen: false,
  });
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
