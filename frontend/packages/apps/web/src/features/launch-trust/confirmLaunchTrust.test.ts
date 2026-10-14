import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiClient } from '@delta/api-client';
import type { LaunchTrustResponse } from '@delta/wire-gen';
import type { NewSessionLaunch } from '../../store/liveStore';
import { confirmLaunchTrust } from './confirmLaunchTrust';
import { useLaunchTrustStore } from './launchTrustStore';

const WORKTREE_LAUNCH: NewSessionLaunch = {
  workdir: '/home/dev/app',
  launchOptionIds: [],
  provider: 'claude',
  worktree: { start_point: { kind: 'remote_branch', name: 'feature' } },
  pullRequestNumber: null,
};

const ASKS: LaunchTrustResponse = {
  needs_confirmation: true,
  repository_untrusted: true,
  commit: 'abc123',
  agent_config_changes: [],
};

function clientAnswering(answer: () => Promise<LaunchTrustResponse>) {
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  const check = vi.spyOn(client, 'checkLaunchTrust').mockImplementation(answer);
  return { client, check };
}

describe('confirmLaunchTrust', () => {
  beforeEach(() => useLaunchTrustStore.setState({ pending: null }));
  afterEach(() => vi.restoreAllMocks());

  it('asks the server nothing for a launch without a worktree', async () => {
    const { client, check } = clientAnswering(() => Promise.resolve(ASKS));

    await expect(
      confirmLaunchTrust(client, { ...WORKTREE_LAUNCH, worktree: null }),
    ).resolves.toEqual({ proceed: true, confirmedCommit: null });
    expect(check).not.toHaveBeenCalled();
  });

  it('resolves with the shown commit once the user accepts', async () => {
    const { client } = clientAnswering(() => Promise.resolve(ASKS));

    const decision = confirmLaunchTrust(client, WORKTREE_LAUNCH);
    await vi.waitFor(() =>
      expect(useLaunchTrustStore.getState().pending?.check).toEqual(ASKS),
    );
    useLaunchTrustStore.getState().answer(true);

    await expect(decision).resolves.toEqual({
      proceed: true,
      confirmedCommit: 'abc123',
    });
    expect(useLaunchTrustStore.getState().pending).toBeNull();
  });

  it('resolves to send nothing when the user cancels', async () => {
    const { client } = clientAnswering(() => Promise.resolve(ASKS));

    const decision = confirmLaunchTrust(client, WORKTREE_LAUNCH);
    await vi.waitFor(() =>
      expect(useLaunchTrustStore.getState().pending).not.toBeNull(),
    );
    useLaunchTrustStore.getState().answer(false);

    await expect(decision).resolves.toEqual({ proceed: false });
  });

  it('goes ahead unconfirmed when the check itself fails', async () => {
    const { client } = clientAnswering(() =>
      Promise.reject(new Error('network down')),
    );

    await expect(confirmLaunchTrust(client, WORKTREE_LAUNCH)).resolves.toEqual({
      proceed: true,
      confirmedCommit: null,
    });
    expect(useLaunchTrustStore.getState().pending).toBeNull();
  });
});
