import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { ApiClient, ApiError } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import type { PermissionNotice } from '../../store/liveStore';
import { PermissionNoticeCard } from './PermissionNotice';

/** The guidance a provider WITH a terminal gets: the prompt lives there. */
const TERMINAL_GUIDANCE = 'Answer the prompt in the terminal.';

/**
 * The guidance a provider WITHOUT a terminal gets. Asserted through
 * `getByText`, whose default normalizer collapses the JSX source line breaks,
 * so this reads as one line even though the markup wraps.
 */
const UNANSWERABLE_GUIDANCE =
  'This request can no longer be answered — it was already resolved, or the agent connection was lost.';

const NOT_PENDING = new ApiError(409, 'not pending', 'permission_not_pending');

function notice(): PermissionNotice {
  return {
    kind: 'permission',
    requestId: 7,
    toolName: 'Bash',
    toolInput: '{"command":"rm -rf scratch"}',
    dismissed: false,
    queued: [],
    pendingCount: 1,
  };
}

interface RenderOptions {
  /**
   * The provider capability under test. Omitted deliberately in the
   * unknown-capability case — that is what the component's default covers.
   */
  providerHasTerminal?: boolean;
  /** What the decision POST rejects with (no failure = a 204). */
  failWith?: unknown;
}

function renderCard({ providerHasTerminal, failWith }: RenderOptions = {}) {
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  const decide = vi
    .spyOn(client, 'decidePermission')
    .mockImplementation(() =>
      failWith === undefined
        ? Promise.resolve()
        : Promise.reject(failWith as Error),
    );
  const onOpenTerminal = vi.fn();
  const onDismiss = vi.fn();
  render(
    <ApiProvider client={client}>
      <PermissionNoticeCard
        notice={notice()}
        providerHasTerminal={providerHasTerminal}
        onOpenTerminal={onOpenTerminal}
        onDismiss={onDismiss}
      />
    </ApiProvider>,
  );
  return { decide, onOpenTerminal, onDismiss };
}

describe('PermissionNoticeCard conflict fallback', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('points a terminal provider at the terminal on a conflict', async () => {
    // has_terminal: true — the 409 means the hook's browser-decision wait timed
    // out and the interactive prompt owns the question, so the terminal is
    // exactly where the user must answer.
    const { onOpenTerminal } = renderCard({
      providerHasTerminal: true,
      failWith: NOT_PENDING,
    });

    fireEvent.click(screen.getByRole('button', { name: 'Allow' }));

    expect(await screen.findByText(TERMINAL_GUIDANCE)).toBeInTheDocument();
    expect(screen.queryByText(UNANSWERABLE_GUIDANCE)).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Allow' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Deny' })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Open terminal' }));
    expect(onOpenTerminal).toHaveBeenCalledTimes(1);
  });

  it('tells a terminal-less provider the request can no longer be answered', async () => {
    // has_terminal: false — a headless provider has no prompt anywhere to
    // answer, so the terminal guidance would send the user nowhere and the
    // "Open terminal" button would open a pane the session does not have.
    const { onDismiss } = renderCard({
      providerHasTerminal: false,
      failWith: NOT_PENDING,
    });

    fireEvent.click(screen.getByRole('button', { name: 'Allow' }));

    expect(await screen.findByText(UNANSWERABLE_GUIDANCE)).toBeInTheDocument();
    expect(screen.queryByText(TERMINAL_GUIDANCE)).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Open terminal' }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Allow' })).not.toBeInTheDocument();

    // Dismiss is the only affordance left, and it still clears the card.
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('defaults an unknown capability to the terminal guidance', async () => {
    // No `providerHasTerminal`: the providers query is unresolved or does not
    // list this session's provider. The default is the terminal guidance (see
    // HAS_TERMINAL_WHEN_UNKNOWN) — the routine 409 belongs to a terminal
    // provider, and wrongly telling that user the prompt is unanswerable would
    // leave a live prompt hanging.
    renderCard({ failWith: NOT_PENDING });

    fireEvent.click(screen.getByRole('button', { name: 'Deny' }));

    expect(await screen.findByText(TERMINAL_GUIDANCE)).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Open terminal' }),
    ).toBeInTheDocument();
  });

  it('leaves the buttons usable for a retry after a non-conflict failure', async () => {
    // Any other failure (here the 500 a dead agent wire produces) is transient
    // as far as the card knows: no guidance, buttons re-enabled, retry posts
    // again. This holds for a terminal-less provider too.
    const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
    const { decide } = renderCard({
      providerHasTerminal: false,
      failWith: new ApiError(500, 'broken pipe'),
    });

    fireEvent.click(screen.getByRole('button', { name: 'Allow' }));
    await waitFor(() => expect(logged).toHaveBeenCalled());

    expect(screen.queryByText(TERMINAL_GUIDANCE)).not.toBeInTheDocument();
    expect(screen.queryByText(UNANSWERABLE_GUIDANCE)).not.toBeInTheDocument();
    const allow = screen.getByRole('button', { name: 'Allow' });
    expect(allow).not.toBeDisabled();

    fireEvent.click(allow);
    await waitFor(() => expect(decide).toHaveBeenCalledTimes(2));
  });
});
