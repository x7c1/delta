import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { ApiClient, ApiError } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import type { PermissionNotice } from '../../store/liveStore';
import { PermissionNoticeCard } from './PermissionNotice';
import { ELIDE_HEAD, ELIDE_LIMIT, ELIDE_TAIL } from './permissionText';
import { REQUEST_SWITCH_GUARD_MS } from './useRequestSwitchGuard';

/** The guidance a provider WITH a terminal gets: the prompt lives there. */
const TERMINAL_GUIDANCE = 'Answer the prompt in the terminal.';

/**
 * The guidance a provider WITHOUT a terminal gets. Asserted through
 * `getByText`, whose default normalizer collapses the JSX source line breaks,
 * so this reads as one line even though the markup wraps.
 */
const UNANSWERABLE_GUIDANCE =
  'This request can no longer be answered — it was already resolved, or the agent connection was lost.';

/** The guidance a closed session gets, whichever provider it runs. */
const SESSION_CLOSED_GUIDANCE =
  'This request can no longer be answered — the session was closed.';

const NOT_PENDING = new ApiError(409, 'not pending', 'permission_not_pending');

/** The 400 a provider without the session-scoped capability answers with. */
const DECISION_UNSUPPORTED = new ApiError(
  400,
  'unsupported decision',
  'permission_decision_unsupported',
);

/** The label of the session-scoped affirmative button. */
const ALLOW_FOR_SESSION = 'Allow for session';

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
  /**
   * Whether the provider accepts a session-scoped allow. Omitted deliberately
   * in the unknown-capability case — the component's default covers that, and
   * it is the opposite of the terminal flag's.
   */
  providerHasAllowForSession?: boolean;
  /** Whether the card's session is closed (omitted = open). */
  sessionClosed?: boolean;
  /** What the decision POST rejects with (no failure = a 204). */
  failWith?: unknown;
  /**
   * Fields to override on the rendered notice — how a case swaps the default
   * `Bash` request for one carrying file-change detail.
   */
  noticeOverrides?: Partial<PermissionNotice>;
}

function renderCard({
  providerHasTerminal,
  providerHasAllowForSession,
  sessionClosed,
  failWith,
  noticeOverrides,
}: RenderOptions = {}) {
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
  const onNotPending = vi.fn();
  const card = (shown: PermissionNotice) => (
    <ApiProvider client={client}>
      <PermissionNoticeCard
        notice={shown}
        providerHasTerminal={providerHasTerminal}
        providerHasAllowForSession={providerHasAllowForSession}
        sessionClosed={sessionClosed}
        onNotPending={onNotPending}
        onOpenTerminal={onOpenTerminal}
        onDismiss={onDismiss}
      />
    </ApiProvider>
  );
  const { rerender } = render(card({ ...notice(), ...noticeOverrides }));
  /** Re-point the mounted card at another request, as a promotion does. */
  const showNext = (next: Partial<PermissionNotice>) =>
    rerender(card({ ...notice(), ...noticeOverrides, ...next }));
  return { decide, onOpenTerminal, onDismiss, onNotPending, showNext };
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

  it.each([
    ['a terminal provider', true],
    ['a terminal-less provider', false],
    ['an unknown capability', undefined],
  ])(
    'tells %s that a closed session can no longer be answered',
    async (_label, providerHasTerminal) => {
      // Closing the session denied the request and took any terminal with it,
      // so the timed-out-hook guidance would point at a pane that is gone.
      const { onDismiss } = renderCard({
        providerHasTerminal,
        sessionClosed: true,
        failWith: NOT_PENDING,
      });

      fireEvent.click(screen.getByRole('button', { name: 'Allow' }));

      expect(
        await screen.findByText(SESSION_CLOSED_GUIDANCE),
      ).toBeInTheDocument();
      expect(screen.queryByText(TERMINAL_GUIDANCE)).not.toBeInTheDocument();
      expect(screen.queryByText(UNANSWERABLE_GUIDANCE)).not.toBeInTheDocument();
      expect(
        screen.queryByRole('button', { name: 'Open terminal' }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole('button', { name: 'Allow' }),
      ).not.toBeInTheDocument();

      fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
      expect(onDismiss).toHaveBeenCalledTimes(1);
    },
  );

  it('keeps the terminal guidance for an open session', async () => {
    // An explicitly open session is the timed-out-hook case the terminal
    // guidance was written for.
    renderCard({
      providerHasTerminal: true,
      sessionClosed: false,
      failWith: NOT_PENDING,
    });

    fireEvent.click(screen.getByRole('button', { name: 'Allow' }));

    expect(await screen.findByText(TERMINAL_GUIDANCE)).toBeInTheDocument();
    expect(
      screen.queryByText(SESSION_CLOSED_GUIDANCE),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Open terminal' }),
    ).toBeInTheDocument();
  });

  it('asks the host to recheck the session on a conflict', async () => {
    // The tab may have missed the session's close (a silently half-open
    // socket), so the 409 asks the host to refetch what `sessionClosed` is
    // derived from.
    const { onNotPending } = renderCard({
      providerHasTerminal: true,
      failWith: NOT_PENDING,
    });

    fireEvent.click(screen.getByRole('button', { name: 'Allow' }));

    await waitFor(() => expect(onNotPending).toHaveBeenCalledTimes(1));
  });

  it('leaves the buttons usable for a retry after a non-conflict failure', async () => {
    // Any other failure (here the 500 a dead agent wire produces) is transient
    // as far as the card knows: no guidance, buttons re-enabled, retry posts
    // again. This holds for a terminal-less provider too.
    const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
    const { decide, onNotPending } = renderCard({
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
    // Nothing suggests the session changed, so no recheck is asked for.
    expect(onNotPending).not.toHaveBeenCalled();
  });
});

describe('PermissionNoticeCard session-scoped allow', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('offers the session-scoped button when the provider declares the capability', async () => {
    // has_allow_for_session: true — the button is offered alongside Allow and
    // Deny, and posts the distinct `allow_for_session` decision. Posting a
    // plain `allow` instead would look identical on screen and quietly lose the
    // whole point: the user would go on being asked.
    const { decide } = renderCard({ providerHasAllowForSession: true });

    fireEvent.click(screen.getByRole('button', { name: ALLOW_FOR_SESSION }));

    await waitFor(() =>
      expect(decide).toHaveBeenCalledWith(7, 'allow_for_session'),
    );
  });

  it('hides the session-scoped button when the provider declares it unsupported', () => {
    // has_allow_for_session: false — the endpoint would answer 400 for this
    // provider, so the control must not be on screen at all.
    renderCard({ providerHasAllowForSession: false });

    expect(
      screen.queryByRole('button', { name: ALLOW_FOR_SESSION }),
    ).not.toBeInTheDocument();
    // The decisions this provider does have are untouched.
    expect(screen.getByRole('button', { name: 'Allow' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Deny' })).toBeInTheDocument();
  });

  it('hides the session-scoped button while the capability is unknown', () => {
    // No `providerHasAllowForSession`: the providers query is unresolved, it
    // failed, or it does not list this session's provider. The default here is
    // the OPPOSITE of the terminal flag's (see
    // HAS_ALLOW_FOR_SESSION_WHEN_UNKNOWN): this one gates a button that acts,
    // and a button that fails when pressed is worse than an absent one.
    renderCard();

    expect(
      screen.queryByRole('button', { name: ALLOW_FOR_SESSION }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Allow' })).toBeInTheDocument();
  });

  it('retires only the session-scoped button when the server refuses the decision', async () => {
    // Reachable on stale capability data (the profile changed under a long-lived
    // tab). The request itself is untouched and still pending, so the card must
    // not fall back to guidance — it drops the one control that cannot work and
    // leaves the answers that can.
    const { decide } = renderCard({
      providerHasAllowForSession: true,
      failWith: DECISION_UNSUPPORTED,
    });

    fireEvent.click(screen.getByRole('button', { name: ALLOW_FOR_SESSION }));

    await waitFor(() =>
      expect(
        screen.queryByRole('button', { name: ALLOW_FOR_SESSION }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.queryByText(TERMINAL_GUIDANCE)).not.toBeInTheDocument();
    expect(screen.queryByText(UNANSWERABLE_GUIDANCE)).not.toBeInTheDocument();

    const allow = screen.getByRole('button', { name: 'Allow' });
    expect(allow).not.toBeDisabled();
    fireEvent.click(allow);
    await waitFor(() => expect(decide).toHaveBeenCalledTimes(2));
    expect(decide).toHaveBeenLastCalledWith(7, 'allow');
  });
});

describe('PermissionNoticeCard file-change detail', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  /** A notice for a request that would edit one file and create another. */
  const fileChangeNotice = (
    reason: string | null = 'write access',
  ): Partial<PermissionNotice> => ({
    toolName: 'file_change',
    toolInput: '{"itemId":"fc_1","turnId":"turn_1"}',
    fileChange: {
      changes: [
        {
          path: 'src/lib.rs',
          kind: 'update',
          diff: '@@ -1 +1 @@\n-old\n+new',
        },
        { path: 'src/added.rs', kind: 'add', diff: '+fresh' },
      ],
      reason,
    },
  });

  it('names the affected files and how each one changes', () => {
    // The failure this replaces: every file-change prompt rendered as the same
    // truncated blob of request params, so the user could not tell one from the
    // next — let alone what it would write.
    renderCard({ noticeOverrides: fileChangeNotice() });

    expect(screen.getByText('src/lib.rs')).toBeInTheDocument();
    expect(screen.getByText('src/added.rs')).toBeInTheDocument();
    expect(screen.getByText('edit')).toBeInTheDocument();
    expect(screen.getByText('new file')).toBeInTheDocument();
    // The request's own params are still listed beside it, as they are.
    expect(
      screen.getByTestId('permission-notice-argument-itemId'),
    ).toHaveTextContent('fc_1');
  });

  it("shows the provider's own reason when it gave one", () => {
    renderCard({ noticeOverrides: fileChangeNotice() });

    expect(screen.getByTestId('permission-notice-reason')).toHaveTextContent(
      'write access',
    );
  });

  it('omits the reason line entirely when the provider gave none', () => {
    renderCard({ noticeOverrides: fileChangeNotice(null) });

    expect(
      screen.queryByTestId('permission-notice-reason'),
    ).not.toBeInTheDocument();
    // The paths are still there: no reason is not no detail.
    expect(screen.getByText('src/lib.rs')).toBeInTheDocument();
  });

  it('keeps the diff behind an expand control', () => {
    // A single approval can carry hundreds of diff lines; inline they would push
    // the Allow/Deny buttons off screen, making the common answer harder to give.
    renderCard({ noticeOverrides: fileChangeNotice() });

    expect(screen.queryByText(/-old/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Diff \(2 files\)/ }));

    expect(screen.getByText(/-old/)).toBeInTheDocument();
    expect(screen.getByText(/\+fresh/)).toBeInTheDocument();
  });

  it('shows only the arguments when there is no detail', () => {
    // A command execution, and a file change whose item the server could not
    // correlate, land here — no empty detail block.
    renderCard();

    expect(screen.getByText('rm -rf scratch')).toBeInTheDocument();
    expect(
      screen.queryByTestId('permission-notice-file-change'),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: /Diff/ }),
    ).not.toBeInTheDocument();
  });

  it('shows only the arguments when the detail names no file', () => {
    // An empty change list says nothing the arguments do not, so it is treated
    // as no detail rather than rendered as an empty block.
    renderCard({
      noticeOverrides: {
        toolName: 'file_change',
        fileChange: { changes: [], reason: null },
      },
    });

    expect(screen.getByText('rm -rf scratch')).toBeInTheDocument();
    expect(
      screen.queryByTestId('permission-notice-file-change'),
    ).not.toBeInTheDocument();
  });

  it('states a requested write root as the broader grant it is', () => {
    // The root is not one of the listed paths: it is a standing permission over
    // a whole tree. Folded into the file list it would read as one more file
    // being edited, which understates what the Allow actually grants.
    renderCard({
      noticeOverrides: { ...fileChangeNotice(), grantRoot: '/repo' },
    });

    expect(screen.getByTestId('permission-notice-grant-root')).toHaveTextContent(
      'Also asks to allow writes anywhere under /repo for the rest of the session.',
    );
    expect(
      screen.getByTestId('permission-notice-file-change'),
    ).not.toHaveTextContent('/repo');
  });

  it('states the write root even with no detail to show it beside', () => {
    // The change set could not be correlated, so the card has only the
    // arguments — and that is exactly when dropping the broadest thing the Allow
    // grants would be worst, since nothing else on the card names a scope.
    renderCard({
      noticeOverrides: {
        toolName: 'file_change',
        toolInput: '{"itemId":"fc_1"}',
        grantRoot: '/repo',
      },
    });

    expect(
      screen.queryByTestId('permission-notice-file-change'),
    ).not.toBeInTheDocument();
    expect(
      screen.getByTestId('permission-notice-grant-root'),
    ).toHaveTextContent('/repo');
  });

  it('shows no write-root line when the provider asked for no root', () => {
    renderCard({ noticeOverrides: fileChangeNotice() });

    expect(
      screen.queryByTestId('permission-notice-grant-root'),
    ).not.toBeInTheDocument();
  });
});

describe('PermissionNoticeCard arguments', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  /** The rendered value of one labelled argument. */
  const argument = (label: string) =>
    screen.getByTestId(`permission-notice-argument-${label}`);

  it('shows every field of the request, each under its name', () => {
    renderCard({
      noticeOverrides: {
        toolName: 'Edit',
        toolInput: JSON.stringify({
          file_path: '/repo/a.rs',
          old_string: 'one',
          new_string: 'two',
          replace_all: true,
        }),
      },
    });

    const list = screen.getByTestId('permission-notice-arguments');
    for (const label of ['file_path', 'old_string', 'new_string', 'replace_all']) {
      expect(list).toHaveTextContent(label);
    }
    expect(argument('file_path')).toHaveTextContent('/repo/a.rs');
    expect(argument('old_string')).toHaveTextContent('one');
    expect(argument('new_string')).toHaveTextContent('two');
    expect(argument('replace_all')).toHaveTextContent('true');
  });

  it("shows a Codex command's working directory", () => {
    renderCard({
      noticeOverrides: {
        toolName: 'ls -la',
        toolInput: JSON.stringify({
          itemId: 'item_1',
          command: 'ls -la',
          cwd: '/repo/sub',
        }),
      },
    });

    expect(argument('cwd')).toHaveTextContent('/repo/sub');
  });

  it('keeps line breaks and indentation as they are', () => {
    const command = 'cat <<EOF\n  indented\nEOF';
    renderCard({ noticeOverrides: { toolInput: JSON.stringify({ command }) } });

    expect(argument('command').textContent).toBe(command);
  });

  it('shows the tail and a padding marker when spaces push the end aside', () => {
    renderCard({
      noticeOverrides: {
        toolInput: JSON.stringify({
          command: `ls${' '.repeat(200)}; rm -rf x`,
        }),
      },
    });

    const value = argument('command');
    expect(value).toHaveTextContent('; rm -rf x');
    expect(value).toHaveTextContent('[200 spaces]');
    expect(value.querySelector('[data-segment="marker"]')).not.toBeNull();
  });

  it('marks long runs of whitespace or blank lines at either end', () => {
    renderCard({
      noticeOverrides: {
        toolInput: JSON.stringify({
          command: `\n\n\n\n\nls${' '.repeat(10)}`,
        }),
      },
    });

    const value = argument('command');
    expect(value).toHaveTextContent('[5 line breaks]');
    expect(value).toHaveTextContent('[10 spaces]');
  });

  it('shows the head, the elided count and the tail of a value over the limit', () => {
    const head = 'H'.repeat(ELIDE_HEAD);
    const middle = 'm'.repeat(ELIDE_LIMIT);
    const tail = 'T'.repeat(ELIDE_TAIL);
    renderCard({
      noticeOverrides: {
        toolInput: JSON.stringify({ command: `${head}${middle}${tail}` }),
      },
    });

    const text = argument('command').textContent ?? '';
    expect(text.startsWith(head)).toBe(true);
    expect(text.endsWith(tail)).toBe(true);
    expect(text).toContain(`[… ${ELIDE_LIMIT} characters elided …]`);
    expect(text).not.toContain('m');

    fireEvent.click(screen.getByRole('button', { name: /Show all/ }));
    expect(argument('command').textContent).toBe(`${head}${middle}${tail}`);
  });

  it('shows a direction-changing character as an escape, with a warning', () => {
    renderCard({
      noticeOverrides: {
        toolInput: JSON.stringify({ command: 'echo safe\u202Etxt.sh' }),
      },
    });

    expect(argument('command')).toHaveTextContent('echo safe\\u{202E}txt.sh');
    expect(argument('command').textContent).not.toContain('\u202E');
    expect(
      screen.getByTestId('permission-notice-invisible-warning'),
    ).toBeInTheDocument();
  });

  it('escapes invisible characters in a heading that names the command', () => {
    // A Codex command execution uses the command itself as the tool name.
    renderCard({
      noticeOverrides: {
        toolName: 'rm\u200B -rf x',
        toolInput: JSON.stringify({ command: 'rm\u200B -rf x' }),
      },
    });

    const heading = screen.getByTestId('permission-notice-tool-name');
    expect(heading).toHaveTextContent('rm\\u{200B} -rf x');
    expect(heading.textContent).not.toContain('\u200B');
  });

  it('escapes control characters but keeps tabs and line breaks', () => {
    renderCard({
      noticeOverrides: {
        toolInput: JSON.stringify({ command: 'a\tb\nc\u0007d' }),
      },
    });

    expect(argument('command').textContent).toBe('a\tb\nc\\u{7}d');
  });

  it('adds no warning to a request without such characters', () => {
    renderCard();

    expect(
      screen.queryByTestId('permission-notice-invisible-warning'),
    ).not.toBeInTheDocument();
  });

  it('shows the whole input unlabelled when it is not an object', () => {
    renderCard({ noticeOverrides: { toolInput: 'not json' } });

    expect(argument('input')).toHaveTextContent('not json');
  });
});

describe('PermissionNoticeCard request switch', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  /** The answer buttons the guard covers. */
  const answerButtons = () => [
    screen.getByRole('button', { name: 'Allow' }),
    screen.getByRole('button', { name: 'Deny' }),
    screen.getByRole('button', { name: 'Dismiss' }),
  ];

  it('leaves the first request it mounts with answerable at once', () => {
    renderCard();

    for (const button of answerButtons()) {
      expect(button).not.toBeDisabled();
    }
    expect(
      screen.queryByTestId('permission-notice-new-request'),
    ).not.toBeInTheDocument();
  });

  it('disables the buttons for a moment after switching to a new request', () => {
    vi.useFakeTimers();
    const { decide, onDismiss, showNext } = renderCard();

    showNext({ requestId: 8, toolInput: '{"command":"cat next.txt"}' });

    expect(
      screen.getByTestId('permission-notice-new-request'),
    ).toHaveTextContent('New request');
    for (const button of answerButtons()) {
      expect(button).toBeDisabled();
    }
    // A click inside the window answers nothing.
    fireEvent.click(screen.getByRole('button', { name: 'Allow' }));
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(decide).not.toHaveBeenCalled();
    expect(onDismiss).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(REQUEST_SWITCH_GUARD_MS - 1);
    });
    expect(screen.getByRole('button', { name: 'Allow' })).toBeDisabled();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    for (const button of answerButtons()) {
      expect(button).not.toBeDisabled();
    }
    expect(
      screen.queryByTestId('permission-notice-new-request'),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Allow' }));
    expect(decide).toHaveBeenCalledWith(8, 'allow');
  });

  it('restarts the window when another request takes its place inside it', () => {
    vi.useFakeTimers();
    const { showNext } = renderCard();

    showNext({ requestId: 8 });
    act(() => {
      vi.advanceTimersByTime(REQUEST_SWITCH_GUARD_MS - 100);
    });
    showNext({ requestId: 9 });
    act(() => {
      vi.advanceTimersByTime(REQUEST_SWITCH_GUARD_MS - 1);
    });
    expect(screen.getByRole('button', { name: 'Allow' })).toBeDisabled();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.getByRole('button', { name: 'Allow' })).not.toBeDisabled();
  });
});
