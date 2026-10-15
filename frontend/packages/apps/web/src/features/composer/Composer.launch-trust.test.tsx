/**
 * Sending a new worktree session through the launch-trust check: the dialog
 * that asks before Claude Code's trust is pre-accepted, and what each answer
 * sends.
 *
 * Shared fixtures live in Composer.testkit.tsx.
 */
import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { ApiClient } from '@delta/api-client';
import type {
  LaunchTrustRequest,
  LaunchTrustResponse,
  SendRequest,
} from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useComposerStore } from '../../store/composerStore';
import { useLiveStore } from '../../store/liveStore';
import { LaunchTrustDialog } from '../launch-trust/LaunchTrustDialog';
import { useLaunchTrustStore } from '../launch-trust/launchTrustStore';
import { Composer } from './Composer';
import { resetComposerState, server } from './Composer.testkit';

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

const SETTINGS_DIFF = [
  'diff --git a/.claude/settings.json b/.claude/settings.json',
  '@@ -1,3 +1,3 @@',
  '-  "hooks": {}',
  '+  "hooks": { "Stop": [] }',
].join('\n');

const NEEDS_CONFIRMATION: LaunchTrustResponse = {
  needs_confirmation: true,
  repository_untrusted: false,
  commit: 'c0ffee1234',
  agent_config_changes: [
    {
      path: '.claude/settings.json',
      diff: SETTINGS_DIFF,
      diff_truncated: false,
    },
    { path: '.mcp.json', diff: '@@\n+{}', diff_truncated: false },
  ],
};

/**
 * Answer the launch-trust check with `check`, capture its request and the
 * body of every `POST /api/sends`, and render the new-session composer on a
 * PR branch's worktree, with the app's one launch-trust dialog beside it.
 */
function renderWorktreeComposer(check: LaunchTrustResponse) {
  const checks: LaunchTrustRequest[] = [];
  const sends: SendRequest[] = [];
  server.use(
    http.post('*/api/launch-trust', async ({ request }) => {
      checks.push((await request.json()) as LaunchTrustRequest);
      return HttpResponse.json(check);
    }),
    http.post('*/api/sends', async ({ request }) => {
      sends.push((await request.json()) as SendRequest);
      return HttpResponse.json(
        {
          send: {
            id: 7,
            session_id: 'sess-new',
            thread_id: 70,
            semantic_parent_uuid: null,
            text: 'review this',
            locator_quote: null,
            status: 'dispatched',
            matched_uuid: null,
            created_at: '2026-01-01T00:00:00Z',
          },
        },
        { status: 201 },
      );
    }),
  );
  useComposerStore.setState({
    newSessionWorkdir: '/home/dev/projects/delta',
    newSessionWorktreeEnabled: true,
    newSessionWorktreeStartPoint: { kind: 'use_remote_branch', name: 'pr-42' },
  });
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <ApiProvider client={new ApiClient({ baseUrl: 'http://localhost' })}>
        <Composer mode={{ kind: 'new-session' }} />
        <LaunchTrustDialog />
      </ApiProvider>
    </QueryClientProvider>,
  );
  fireEvent.change(screen.getByRole('textbox'), {
    target: { value: 'review this' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Send' }));
  return { checks, sends };
}

describe('Composer launch-trust confirmation', () => {
  beforeEach(() => {
    resetComposerState();
    useLaunchTrustStore.setState({ pending: null });
  });

  it('lists the changed paths and shows each diff', async () => {
    const { checks, sends } = renderWorktreeComposer(NEEDS_CONFIRMATION);

    const dialog = await screen.findByTestId('launch-trust-dialog');
    expect(checks).toEqual([
      {
        workdir: '/home/dev/projects/delta',
        start_point: { kind: 'use_remote_branch', name: 'pr-42' },
        provider: 'claude',
      },
    ]);
    expect(
      screen.getAllByTestId('launch-trust-path').map((el) => el.textContent),
    ).toEqual(['.claude/settings.json', '.mcp.json']);
    const diffs = screen.getAllByTestId('launch-trust-diff');
    expect(diffs[0].textContent).toBe(SETTINGS_DIFF);
    expect(dialog.textContent).toContain('changes agent configuration');
    // Nothing goes out while the question is open.
    expect(sends).toHaveLength(0);
    expect(screen.getByRole('button', { name: 'Send' })).toBeDisabled();
  });

  it('says so when the repository is not trusted yet', async () => {
    renderWorktreeComposer({
      ...NEEDS_CONFIRMATION,
      repository_untrusted: true,
      agent_config_changes: [],
    });

    const dialog = await screen.findByTestId('launch-trust-dialog');
    expect(dialog.textContent).toContain('has not been trusted');
    expect(screen.queryByTestId('launch-trust-path')).toBeNull();
  });

  it('launches with the confirmed commit on accept', async () => {
    const { sends } = renderWorktreeComposer(NEEDS_CONFIRMATION);

    fireEvent.click(await screen.findByTestId('launch-trust-accept'));

    await waitFor(() => expect(sends).toHaveLength(1));
    expect(sends[0]).toMatchObject({
      new_session: true,
      text: 'review this',
      worktree: {
        start_point: { kind: 'use_remote_branch', name: 'pr-42' },
        confirmed_commit: 'c0ffee1234',
      },
    });
    expect(screen.queryByTestId('launch-trust-dialog')).toBeNull();
    // The spawn a failed launch would retry keeps no confirmation, so a
    // retry asks again.
    await waitFor(() => expect(useLiveStore.getState().spawns).toHaveLength(1));
    expect(useLiveStore.getState().spawns[0].worktree).toEqual({
      start_point: { kind: 'use_remote_branch', name: 'pr-42' },
    });
  });

  it('launches nothing on cancel and keeps the draft', async () => {
    const { sends } = renderWorktreeComposer(NEEDS_CONFIRMATION);

    fireEvent.click(await screen.findByTestId('launch-trust-cancel'));

    await waitFor(() =>
      expect(screen.queryByTestId('launch-trust-dialog')).toBeNull(),
    );
    expect(sends).toHaveLength(0);
    expect(useLiveStore.getState().spawns).toHaveLength(0);
    expect(screen.getByRole('textbox')).toHaveValue('review this');
    expect(screen.getByRole('button', { name: 'Send' })).toBeEnabled();
  });

  it('launches directly when no confirmation is needed', async () => {
    const { checks, sends } = renderWorktreeComposer({
      needs_confirmation: false,
      repository_untrusted: false,
      commit: 'c0ffee1234',
      agent_config_changes: [],
    });

    await waitFor(() => expect(sends).toHaveLength(1));
    expect(checks).toHaveLength(1);
    expect(screen.queryByTestId('launch-trust-dialog')).toBeNull();
    expect(sends[0]).toMatchObject({
      worktree: { start_point: { kind: 'use_remote_branch', name: 'pr-42' } },
    });
    expect(
      (sends[0] as { worktree?: { confirmed_commit?: string } }).worktree
        ?.confirmed_commit,
    ).toBeUndefined();
  });
});
