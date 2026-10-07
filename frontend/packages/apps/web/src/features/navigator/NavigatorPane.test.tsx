import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from 'vitest';
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { setupServer } from 'msw/node';
import {
  createHandlers,
  MOCK_NEWER_RELEASE,
  MOCK_VERSION,
  SESSION_ID,
  SESSION_ID_2,
  SESSION_2_MAIN_THREAD_ID,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type {
  AgentProvider,
  LatestReleaseResponse,
  RateLimitWindow,
  SessionListItem,
  UpdateDownload,
  UpdateInstall,
  UpdateOffer,
} from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useLiveStore } from '../../store/liveStore';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import { NavigatorPane } from './NavigatorPane';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function makeItem(
  id: string,
  mainThreadId: number,
  provider: AgentProvider = 'claude',
): SessionListItem {
  return {
    session: {
      id,
      cwd: `/home/dev/${id}`,
      transcript_path: '',
      title: null,
      status: 'active',
      created_at: '2026-01-01T00:00:00Z',
      branch_at_launch: 'main',
      repo_root: `/home/dev/${id}`,
      repository_display_name: `dev/${id}`,
      provider,
      provider_session_id: null,
      provider_thread_id: null,
      pull_request_number: null,
    },
    open: true,
    main_thread_id: mainThreadId,
    last_activity_at: '2026-01-01T00:00:00Z',
  };
}

const sessions = [
  makeItem(SESSION_ID, 1),
  makeItem(SESSION_ID_2, SESSION_2_MAIN_THREAD_ID),
];

function renderPane(items: SessionListItem[] = sessions) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <NavigatorPane
          sessions={items}
          hasMoreSessions={false}
          isLoadingMoreSessions={false}
          onLoadMoreSessions={() => {}}
        />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('NavigatorPane per-session running indicator', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      unread: {},
    });
    useNavStore.setState({ focusedSessionId: null, activeThreadId: null });
    useComposerStore.setState({ newSessionWorkdir: null });
  });

  it('shows the running indicator only on the session with an in-flight turn', () => {
    // Only the first session has an active turn.
    useLiveStore.setState({ runningThreads: { [SESSION_ID]: { 1: true } } });

    renderPane();

    const rows = screen.getAllByRole('listitem');
    expect(rows).toHaveLength(2);
    // The active session's row shows the indicator; the idle one does not.
    expect(
      within(rows[0]).queryByTestId('session-running'),
    ).toBeInTheDocument();
    expect(
      within(rows[1]).queryByTestId('session-running'),
    ).not.toBeInTheDocument();
    // Exactly one row carries the indicator overall.
    expect(screen.getAllByTestId('session-running')).toHaveLength(1);
  });

  it('shows no running indicator when no session has an in-flight turn', () => {
    renderPane();

    expect(screen.queryByTestId('session-running')).not.toBeInTheDocument();
  });

  it('renders no global footer running indicator', () => {
    // The footer spinner used to appear whenever any turn was in flight; it has
    // been replaced by the per-row indicator above.
    useLiveStore.setState({ runningThreads: { [SESSION_ID]: { 1: true } } });

    renderPane();

    // The only "running" text now lives inside a per-session row (the
    // visually-hidden label), never as a standalone footer spinner.
    const runningRows = screen.getAllByTestId('session-running');
    expect(runningRows).toHaveLength(1);
  });
});

describe('NavigatorPane launched-session placement', () => {
  // More open sessions than the windowed list renders in jsdom's 600px
  // viewport (plus overscan), so a card listed after all of them is unmounted.
  const OPEN_COUNT = 40;
  const openIds = Array.from(
    { length: OPEN_COUNT },
    (_, index) => `open-${String(index).padStart(2, '0')}`,
  );
  const FAILED_ID = 'failed-launch';
  const failedItem: SessionListItem = {
    ...makeItem(FAILED_ID, 999),
    open: false,
  };
  failedItem.session = { ...failedItem.session, status: 'failed' };
  // The server's open-first order: every open session, then the failed one.
  const serverOrder = [
    ...openIds.map((id, index) => makeItem(id, index + 100)),
    failedItem,
  ];

  function failedSpawn(sessionId: string) {
    return {
      sessionId,
      threadId: 999,
      text: 'first message',
      firstSendId: 1,
      workdir: null,
      launchOptionIds: [],
      provider: 'claude' as const,
      worktree: null,
      pullRequestNumber: null,
      status: 'failed' as const,
    };
  }

  /** The session ids of the mounted cards, top to bottom, matched by repo name. */
  function renderedIds(): string[] {
    return screen
      .getAllByRole('listitem')
      .map(
        (row) =>
          [...openIds, FAILED_ID].find((id) =>
            row.textContent?.includes(`dev/${id}`),
          ) ?? '?',
      );
  }

  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      unread: {},
      spawns: [],
    });
    useNavStore.setState({ focusedSessionId: null, activeThreadId: null });
    useComposerStore.setState({ newSessionWorkdir: null });
  });

  it('leaves a failed card it did not launch below the open sessions, out of the window', () => {
    renderPane(serverOrder);

    const ids = renderedIds();
    expect(ids.length).toBeLessThan(serverOrder.length);
    expect(ids).toEqual(openIds.slice(0, ids.length));
    expect(
      screen.queryByRole('status', { name: 'Failed', exact: true }),
    ).not.toBeInTheDocument();
  });

  it('pins the failed card of its own launch to the top, keeping the open-first order of the rest', () => {
    useLiveStore.setState({ spawns: [failedSpawn(FAILED_ID)] });

    renderPane(serverOrder);

    const rows = screen.getAllByRole('listitem');
    expect(
      within(rows[0]).getByRole('status', { name: 'Failed', exact: true }),
    ).toBeInTheDocument();
    const ids = renderedIds();
    expect(ids[0]).toBe(FAILED_ID);
    expect(ids.slice(1)).toEqual(openIds.slice(0, ids.length - 1));
  });

  it('returns the card to the server order once the launch is no longer tracked', () => {
    useLiveStore.setState({ spawns: [failedSpawn(FAILED_ID)] });
    renderPane(serverOrder);
    expect(renderedIds()[0]).toBe(FAILED_ID);

    // Retry or Remove clears the tracked spawn.
    act(() => useLiveStore.getState().clearSpawn(FAILED_ID));

    expect(renderedIds()).not.toContain(FAILED_ID);
    expect(renderedIds()[0]).toBe(openIds[0]);
  });
});

describe('NavigatorPane rate-limit meters', () => {
  // jsdom performs no layout, so `clientWidth` defaults to 0. The rate-limit
  // row now measures its meter track width to translate the budget-line marker
  // by an integer pixel offset (avoiding the sub-pixel shimmer that a
  // percentage-based `right` value causes), and gates rendering the marker on
  // `trackWidth > 0`. Stub `clientWidth` to a non-zero value across this
  // describe block so the marker mounts; restore after each case.
  let originalClientWidth: PropertyDescriptor | undefined;
  beforeEach(() => {
    originalClientWidth = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      'clientWidth',
    );
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
      configurable: true,
      value: 200,
    });
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      rateLimits: {},
      // Live by default: the restored-from-localStorage cases opt in below.
      restoredRateLimitsObservedAt: {},
    });
    // In a thread the footer shows the FOCUSED session's provider's limits, so
    // every case here focuses a session by default — an unfocused navigator
    // speaks for no account and deliberately shows no rows (asserted in its own
    // case below). The new-session cases override this with NEW_SESSION_FOCUS.
    useNavStore.setState({ focusedSessionId: SESSION_ID, activeThreadId: null });
  });
  afterEach(() => {
    if (originalClientWidth) {
      Object.defineProperty(
        HTMLElement.prototype,
        'clientWidth',
        originalClientWidth,
      );
    } else {
      delete (HTMLElement.prototype as unknown as { clientWidth?: number })
        .clientWidth;
    }
  });

  /** A window of `durationSeconds`, as the wire delivers it. */
  function window(
    durationSeconds: number | null,
    usedPercentage: number | null,
    resetsAt: number | null,
  ): RateLimitWindow {
    return {
      duration_seconds: durationSeconds,
      used_percentage: usedPercentage,
      resets_at: resetsAt,
    };
  }

  const FIVE_HOURS = 5 * 60 * 60;
  const SEVEN_DAYS = 7 * 24 * 60 * 60;

  it('renders a row per received window, labeled from its duration', () => {
    // Add a 30s cushion on top of each whole-minute offset so the few ms that
    // elapse between this `Date.now()` and the component's own render-time read
    // cannot cross a minute boundary and flip the displayed countdown.
    const now = Date.now() / 1000;
    useLiveStore.setState({
      rateLimits: {
        claude: [
          window(FIVE_HOURS, 37, now + 2 * 3600 + 13 * 60 + 30),
          window(SEVEN_DAYS, 8, now + 5 * 86400 + 4 * 3600 + 30),
        ],
      },
    });

    renderPane();

    // The `5h` / `7d` labels are DERIVED from the durations above, not
    // hardcoded — which is what lets an unfamiliar window render at all.
    const fiveHour = screen.getByTestId('rate-limit-5h');
    expect(within(fiveHour).getByRole('meter')).toHaveAttribute(
      'aria-valuenow',
      '37',
    );
    expect(screen.getByTestId('rate-limit-5h-pct')).toHaveTextContent('37%');
    expect(screen.getByTestId('rate-limit-5h-reset')).toHaveTextContent(
      '↻ 02h13m',
    );

    const sevenDay = screen.getByTestId('rate-limit-7d');
    expect(within(sevenDay).getByRole('meter')).toHaveAttribute(
      'aria-valuenow',
      '8',
    );
    expect(screen.getByTestId('rate-limit-7d-pct')).toHaveTextContent('8%');
    expect(screen.getByTestId('rate-limit-7d-reset')).toHaveTextContent(
      '↻ 05d04h',
    );
  });

  it('renders a window duration it has never seen before', () => {
    // No provider ships a 24-hour window today; the row must still label and
    // pace itself correctly, because both come from the data.
    useLiveStore.setState({
      rateLimits: { claude: [window(24 * 60 * 60, 50, null)] },
    });

    renderPane();

    expect(screen.getByTestId('rate-limit-1d-pct')).toHaveTextContent('50%');
  });

  it('renders the budget-line marker on each row when resets_at is present', () => {
    // Both fixtures keep the fill strictly inside the current bucket's share
    // so the marker's color assertion is covered by a dedicated test below;
    // here we just care that the marker mounts on each row.
    const now = Date.now() / 1000;
    useLiveStore.setState({
      rateLimits: {
        claude: [
          window(FIVE_HOURS, 40, now + 3 * 60 * 60),
          window(SEVEN_DAYS, 30, now + 5 * 86400),
        ],
      },
    });

    renderPane();

    expect(
      screen.getByTestId('rate-limit-5h-budget-line'),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId('rate-limit-7d-budget-line'),
    ).toBeInTheDocument();
  });

  it('switches the budget-line marker to the panel background color when the fill overtakes it', () => {
    // 5h row: fresh reset (5h remaining) → budget line at 1/5 = 20% from the
    // right; fill at 90% overtakes → marker should carry `bg-surface`.
    // 7d row: 5d remaining → budget line at 3/7 ≈ 42.86% from the right;
    // fill at 5% is well within the bucket → marker keeps the neutral `bg-fg`.
    const now = Date.now() / 1000;
    useLiveStore.setState({
      rateLimits: {
        claude: [
          window(FIVE_HOURS, 90, now + 5 * 60 * 60),
          window(SEVEN_DAYS, 5, now + 5 * 86400),
        ],
      },
    });

    renderPane();

    expect(screen.getByTestId('rate-limit-5h-budget-line')).toHaveClass(
      'bg-surface',
    );
    expect(screen.getByTestId('rate-limit-7d-budget-line')).toHaveClass(
      'bg-fg',
    );
  });

  it('omits the budget-line marker when resets_at is null', () => {
    useLiveStore.setState({
      rateLimits: { claude: [window(FIVE_HOURS, 25, null)] },
    });

    renderPane();

    expect(
      screen.queryByTestId('rate-limit-5h-budget-line'),
    ).not.toBeInTheDocument();
  });

  it('renders a window with no duration unlabeled and unpaced, never guessed', () => {
    // A provider may report a window without saying how long it is. Its
    // percentage is still real, so the row shows — but with no invented label
    // and no budget line drawn against a duration nobody sent.
    const now = Date.now() / 1000;
    useLiveStore.setState({
      rateLimits: { claude: [window(null, 60, now + 3600)] },
    });

    renderPane();

    const row = screen.getByTestId('rate-limit-w1');
    expect(within(row).getByRole('meter')).toHaveAttribute(
      'aria-valuenow',
      '60',
    );
    expect(row).toHaveTextContent('—');
    expect(
      screen.queryByTestId('rate-limit-w1-budget-line'),
    ).not.toBeInTheDocument();
  });

  it('renders no rows (no empty bars) when the account reports no windows', () => {
    useLiveStore.setState({ rateLimits: { claude: [] } });

    renderPane();

    expect(screen.queryByTestId('rate-limits')).not.toBeInTheDocument();
    expect(screen.queryByTestId('rate-limit-5h')).not.toBeInTheDocument();
    expect(screen.queryByTestId('rate-limit-7d')).not.toBeInTheDocument();
  });

  it('renders only the windows the account actually reported', () => {
    const now = Date.now() / 1000;
    useLiveStore.setState({
      rateLimits: { claude: [window(FIVE_HOURS, 50, now + 3600)] },
    });

    renderPane();

    expect(screen.getByTestId('rate-limit-5h')).toBeInTheDocument();
    expect(screen.queryByTestId('rate-limit-7d')).not.toBeInTheDocument();
  });

  it('shows 0% and no reset glyph for a window with null fields', () => {
    // A window can carry null values (no usage reported yet, no reset
    // timestamp): the row renders at 0% and omits the ↻ countdown rather than
    // showing a misleading reset.
    useLiveStore.setState({
      rateLimits: { claude: [window(FIVE_HOURS, null, null)] },
    });

    renderPane();

    const fiveHour = screen.getByTestId('rate-limit-5h');
    expect(within(fiveHour).getByRole('meter')).toHaveAttribute(
      'aria-valuenow',
      '0',
    );
    expect(screen.getByTestId('rate-limit-5h-pct')).toHaveTextContent('0%');
    expect(screen.queryByTestId('rate-limit-5h-reset')).not.toBeInTheDocument();
  });

  it('shows the FOCUSED session provider\'s limits, never another provider\'s', () => {
    // The invariant this keying exists for: with both providers live, the
    // footer must never present Claude's account limits under a focused Codex
    // session. Switching focus swaps the rows; nothing leaks across.
    const items = [
      makeItem(SESSION_ID, 1, 'claude'),
      makeItem(SESSION_ID_2, SESSION_2_MAIN_THREAD_ID, 'codex'),
    ];
    useLiveStore.setState({
      rateLimits: {
        claude: [window(FIVE_HOURS, 37, null)],
        codex: [window(SEVEN_DAYS, 8, null)],
      },
    });

    const { unmount } = renderPane(items);
    expect(screen.getByTestId('rate-limit-5h-pct')).toHaveTextContent('37%');
    expect(screen.queryByTestId('rate-limit-7d')).not.toBeInTheDocument();
    unmount();

    useNavStore.setState({ focusedSessionId: SESSION_ID_2 });
    renderPane(items);
    expect(screen.getByTestId('rate-limit-7d-pct')).toHaveTextContent('8%');
    expect(screen.queryByTestId('rate-limit-5h')).not.toBeInTheDocument();
  });

  it('shows no rows for a provider that has reported nothing', () => {
    // A resumed Codex session before its first account update: the focused
    // provider simply has no entry, and no other provider's rows stand in.
    const items = [makeItem(SESSION_ID, 1, 'codex')];
    useLiveStore.setState({
      rateLimits: { claude: [window(FIVE_HOURS, 37, null)] },
    });

    renderPane(items);

    expect(screen.queryByTestId('rate-limits')).not.toBeInTheDocument();
  });

  it('de-emphasizes restored rows and names when they were observed', () => {
    // The overnight case: the 7d window was last seen last night and has not
    // reset, so the row is shown — de-emphasized, and dated — rather than
    // hidden until the next live snapshot happens to arrive.
    const observedAt = Date.now() - 9 * 60 * 60 * 1000;
    const now = Date.now() / 1000;
    useLiveStore.setState({
      rateLimits: {
        claude: [window(SEVEN_DAYS, 41, now + 5 * 86400 + 4 * 3600 + 30)],
      },
      restoredRateLimitsObservedAt: { claude: observedAt },
    });

    renderPane();

    const row = screen.getByTestId('rate-limit-7d');
    expect(row).toHaveAttribute('data-stale', 'true');
    expect(row.className).toContain('opacity-60');
    // The observed time is reachable from the meter's tooltip.
    const title = within(row).getByRole('meter').getAttribute('title');
    expect(title).toContain('last observed');
    expect(title).toContain(
      new Date(observedAt).toLocaleString(undefined, {
        dateStyle: 'medium',
        timeStyle: 'short',
      }),
    );
    // Only the PERCENTAGE is stale: the countdown and the budget line are
    // derived from `resets_at` and the current clock at render time, so they
    // stay exact on a restored row rather than being frozen or dropped.
    expect(screen.getByTestId('rate-limit-7d-pct')).toHaveTextContent('41%');
    expect(screen.getByTestId('rate-limit-7d-reset')).toHaveTextContent(
      '↻ 05d04h',
    );
    expect(screen.getByTestId('rate-limit-7d-budget-line')).toBeInTheDocument();
  });

  it('returns a row to normal styling when the store clears the stale mark', () => {
    // The one thing the stale/live cases either side of this cannot state:
    // the footer un-dims WITHOUT a reload, the moment the first live snapshot
    // for that provider lands. That only holds if the pane SUBSCRIBES to the
    // provenance map (and keeps it in the memo's deps) rather than reading it
    // once at mount — a wiring slip there leaves both neighbouring cases green.
    const observedAt = Date.now() - 9 * 60 * 60 * 1000;
    const resetsAt = Date.now() / 1000 + 5 * 86400 + 4 * 3600 + 30;
    useLiveStore.setState({
      rateLimits: { claude: [window(SEVEN_DAYS, 41, resetsAt)] },
      restoredRateLimitsObservedAt: { claude: observedAt },
    });

    renderPane();
    expect(screen.getByTestId('rate-limit-7d')).toHaveAttribute(
      'data-stale',
      'true',
    );

    // Clearing the provider's mark is the ONLY thing that changes here — the
    // windows are left exactly as they were, so nothing but the provenance
    // subscription can account for the row un-dimming. (The reducer step that
    // clears the mark is covered end to end in liveStore.test.ts.)
    act(() => {
      useLiveStore.setState({ restoredRateLimitsObservedAt: {} });
    });

    const row = screen.getByTestId('rate-limit-7d');
    expect(row).not.toHaveAttribute('data-stale');
    expect(row.className).not.toContain('opacity-');
    expect(within(row).getByRole('meter').getAttribute('title')).not.toContain(
      'last observed',
    );
    // The numbers are untouched by the styling change.
    expect(screen.getByTestId('rate-limit-7d-pct')).toHaveTextContent('41%');
  });

  it('renders live rows with no de-emphasis and no observed-at note', () => {
    useLiveStore.setState({
      rateLimits: { claude: [window(FIVE_HOURS, 37, null)] },
      restoredRateLimitsObservedAt: {},
    });

    renderPane();

    const row = screen.getByTestId('rate-limit-5h');
    expect(row).not.toHaveAttribute('data-stale');
    expect(row.className).not.toContain('opacity-');
    expect(within(row).getByRole('meter')).toHaveAttribute(
      'title',
      '5h rate limit: 37% used',
    );
  });

  it('de-emphasizes only the provider whose windows are still restored', () => {
    // Claude has been restated live, Codex has not. Focus follows the session,
    // and so does the treatment: nothing leaks across accounts here either.
    const observedAt = Date.now() - 9 * 60 * 60 * 1000;
    const items = [
      makeItem(SESSION_ID, 1, 'claude'),
      makeItem(SESSION_ID_2, SESSION_2_MAIN_THREAD_ID, 'codex'),
    ];
    useLiveStore.setState({
      rateLimits: {
        claude: [window(FIVE_HOURS, 37, null)],
        codex: [window(SEVEN_DAYS, 8, null)],
      },
      restoredRateLimitsObservedAt: { codex: observedAt },
    });

    const { unmount } = renderPane(items);
    expect(screen.getByTestId('rate-limit-5h')).not.toHaveAttribute(
      'data-stale',
    );
    unmount();

    useNavStore.setState({ focusedSessionId: SESSION_ID_2 });
    renderPane(items);
    expect(screen.getByTestId('rate-limit-7d')).toHaveAttribute(
      'data-stale',
      'true',
    );
  });

  it('shows no rows while no session is focused', () => {
    // With nothing focused there is no account the footer could be speaking
    // for, so it stays silent rather than picking a provider arbitrarily.
    useNavStore.setState({ focusedSessionId: null });
    useLiveStore.setState({
      rateLimits: { claude: [window(FIVE_HOURS, 37, null)] },
    });

    renderPane();

    expect(screen.queryByTestId('rate-limits')).not.toBeInTheDocument();
  });

  it('leaves a focused thread\'s single group without a provider heading', () => {
    // The user already knows which provider the session runs on, so the
    // heading would be redundant: the group renders its rows alone, still
    // inside its provider container.
    const items = [makeItem(SESSION_ID, 1, 'claude')];
    useLiveStore.setState({
      rateLimits: {
        claude: [window(FIVE_HOURS, 37, null)],
        codex: [window(SEVEN_DAYS, 8, null)],
      },
    });

    renderPane(items);

    const claude = screen.getByTestId('rate-limits-claude');
    expect(claude).not.toHaveTextContent('Claude Code');
    expect(within(claude).getByTestId('rate-limit-5h-pct')).toHaveTextContent(
      '37%',
    );
    expect(screen.queryByTestId('rate-limits-codex')).not.toBeInTheDocument();
  });

  it('lists every reporting provider on the new-session screen, in provider order', () => {
    // The user is about to choose a provider, so every account's budget is
    // shown. Codex is inserted into the map FIRST: the groups must still read
    // Claude then Codex, never the order the snapshots happened to arrive in.
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    useLiveStore.setState({
      rateLimits: {
        codex: [window(FIVE_HOURS, 61, null)],
        claude: [window(FIVE_HOURS, 37, null), window(SEVEN_DAYS, 8, null)],
      },
    });

    renderPane();

    const block = screen.getByTestId('rate-limits');
    const groups = within(block).getAllByTestId(/^rate-limits-/);
    expect(groups.map((group) => group.dataset.testid)).toEqual([
      'rate-limits-claude',
      'rate-limits-codex',
    ]);
    const [claude, codex] = groups;
    expect(claude).toHaveTextContent('Claude Code');
    expect(codex).toHaveTextContent('Codex');
    // Both accounts report a 5h window; the group container tells them apart.
    expect(within(claude).getByTestId('rate-limit-5h-pct')).toHaveTextContent(
      '37%',
    );
    expect(within(claude).getByTestId('rate-limit-7d-pct')).toHaveTextContent(
      '8%',
    );
    expect(within(codex).getByTestId('rate-limit-5h-pct')).toHaveTextContent(
      '61%',
    );
    expect(within(codex).queryByTestId('rate-limit-7d')).not.toBeInTheDocument();
  });

  it('lists no group on the new-session screen for a provider that has reported nothing', () => {
    // No entry (never used) and an empty list (reports none) both mean no
    // group — the same "no empty bars" rule a single provider's rows follow.
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    useLiveStore.setState({
      rateLimits: { claude: [window(FIVE_HOURS, 37, null)] },
    });

    const { unmount } = renderPane();
    expect(screen.getByTestId('rate-limits-claude')).toBeInTheDocument();
    expect(screen.queryByTestId('rate-limits-codex')).not.toBeInTheDocument();
    unmount();

    useLiveStore.setState({
      rateLimits: { claude: [window(FIVE_HOURS, 37, null)], codex: [] },
    });
    renderPane();
    expect(screen.getByTestId('rate-limits-claude')).toBeInTheDocument();
    expect(screen.queryByTestId('rate-limits-codex')).not.toBeInTheDocument();
  });

  it('renders no footer meters on the new-session screen when no provider has reported', () => {
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    useLiveStore.setState({ rateLimits: { claude: [], codex: [] } });

    renderPane();

    expect(screen.queryByTestId('rate-limits')).not.toBeInTheDocument();
  });

  it('de-emphasizes only the restored group on the new-session screen', () => {
    // "Codex is a restored guess, Claude is live": each group carries its own
    // observation, so the dimming lands on the Codex rows alone.
    const observedAt = Date.now() - 9 * 60 * 60 * 1000;
    useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    useLiveStore.setState({
      rateLimits: {
        claude: [window(FIVE_HOURS, 37, null)],
        codex: [window(FIVE_HOURS, 61, null)],
      },
      restoredRateLimitsObservedAt: { codex: observedAt },
    });

    renderPane();

    const claudeRow = within(
      screen.getByTestId('rate-limits-claude'),
    ).getByTestId('rate-limit-5h');
    const codexRow = within(
      screen.getByTestId('rate-limits-codex'),
    ).getByTestId('rate-limit-5h');
    expect(claudeRow).not.toHaveAttribute('data-stale');
    expect(claudeRow.className).not.toContain('opacity-');
    expect(codexRow).toHaveAttribute('data-stale', 'true');
    expect(codexRow.className).toContain('opacity-60');
  });
});

describe('NavigatorPane settings entry', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      rateLimits: {},
    });
    useNavStore.setState({ settingsOpen: false });
  });

  it('opens the settings overlay when the footer entry is clicked', () => {
    renderPane();

    const entry = screen.getByTestId('settings-entry');
    expect(entry).toHaveAttribute('aria-pressed', 'false');

    fireEvent.click(entry);

    expect(useNavStore.getState().settingsOpen).toBe(true);
  });

  it('marks the entry pressed while the settings overlay is open', () => {
    useNavStore.setState({ settingsOpen: true });

    renderPane();

    expect(screen.getByTestId('settings-entry')).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });
});

describe('NavigatorPane workspace version', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      rateLimits: {},
    });
    useNavStore.setState({
      focusedSessionId: null,
      activeThreadId: null,
      settingsOpen: false,
    });
  });

  it('replaces the connection label with `Delta <version>` once the fetch resolves', async () => {
    // The label element itself always exists (it starts as the previous
    // `Connected` fallback), so `findByTestId` would return immediately
    // without proving the fetch pipeline resolved. Poll the text content
    // with `waitFor` instead — that ties the assertion to the query
    // settling. The `Delta ` prefix is UI copy prepended on the frontend;
    // the backend contract returns the bare version string.
    renderPane();

    await waitFor(() => {
      expect(screen.getByTestId('connection-label')).toHaveTextContent(
        `Delta ${MOCK_VERSION}`,
      );
    });
    // The standalone version row this feature originally shipped with has
    // been folded into the connection label; nothing else should carry the
    // version string.
    expect(screen.queryByTestId('workspace-version')).not.toBeInTheDocument();
  });

  it('keeps the previous `Disconnected` label while the socket is closed, even after the version resolves', async () => {
    // The dot encodes the live connection state; the label mirrors it in
    // non-`open` states so a dropped socket is never silenced by the
    // version swap. The label element itself is always rendered, so a
    // static assertion would pass before the fetch settles too — poll
    // with `waitFor` to give the query time to resolve and re-render, and
    // then confirm the closed state still pins the connection wording.
    useLiveStore.setState({ connection: 'closed' });

    renderPane();

    await waitFor(() => {
      const label = screen.getByTestId('connection-label');
      expect(label).toHaveTextContent('Disconnected');
      expect(label).not.toHaveTextContent(MOCK_VERSION);
    });
  });
});

describe('NavigatorPane newer-release notice', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      rateLimits: {},
    });
    useNavStore.setState({
      focusedSessionId: null,
      activeThreadId: null,
      settingsOpen: false,
    });
  });

  it('links to the release page next to the version when the server reports a newer release', async () => {
    server.use(
      http.get('*/api/latest-release', () =>
        HttpResponse.json({
          newer: MOCK_NEWER_RELEASE,
          offer: 'none',
          download: null,
          installs: false,
          install: null,
        } satisfies LatestReleaseResponse),
      ),
    );

    renderPane();

    const notice = await screen.findByTestId('newer-release');
    expect(notice).toHaveTextContent(`${MOCK_NEWER_RELEASE.version} available`);
    expect(notice).toHaveAttribute('href', MOCK_NEWER_RELEASE.url);
    expect(notice).toHaveAttribute('target', '_blank');
    expect(notice).toHaveAttribute('rel', 'noopener noreferrer');
  });

  it('shows no notice when the server reports no newer release', async () => {
    let asked = false;
    server.use(
      http.get('*/api/latest-release', () => {
        asked = true;
        return HttpResponse.json({
          newer: null,
          offer: 'none',
          download: null,
          installs: false,
          install: null,
        } satisfies LatestReleaseResponse);
      }),
    );

    renderPane();

    // Wait for both footer queries to settle, so the absence is not just the
    // notice's query still pending.
    await waitFor(() => {
      expect(asked).toBe(true);
      expect(screen.getByTestId('connection-label')).toHaveTextContent(
        `Delta ${MOCK_VERSION}`,
      );
    });
    expect(screen.queryByTestId('newer-release')).not.toBeInTheDocument();
  });

  it('asks again shortly after the first answer, so a release the server finds just after startup shows without waiting for the hourly poll', async () => {
    // A page opened together with the server asks before the server's first
    // check has run, so its first answer is "nothing newer".
    let answers = 0;
    server.use(
      http.get('*/api/latest-release', () => {
        answers += 1;
        return HttpResponse.json({
          newer: answers === 1 ? null : MOCK_NEWER_RELEASE,
          offer: 'none',
          download: null,
          installs: false,
          install: null,
        } satisfies LatestReleaseResponse);
      }),
    );
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      renderPane();
      await waitFor(() => expect(answers).toBe(1));
      expect(screen.queryByTestId('newer-release')).not.toBeInTheDocument();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(30_000);
      });

      expect(await screen.findByTestId('newer-release')).toHaveTextContent(
        `${MOCK_NEWER_RELEASE.version} available`,
      );
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('NavigatorPane update control', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      rateLimits: {},
    });
    useNavStore.setState({
      focusedSessionId: null,
      activeThreadId: null,
      settingsOpen: false,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  /**
   * Answer `GET /api/latest-release` with the newer release, `offer`,
   * `download`, and — for an app that installs updates itself (`installs`) —
   * `install`.
   */
  function answerLatest(
    offer: UpdateOffer,
    download: () => UpdateDownload | null = () => null,
    { installs = false, install = () => null }: {
      installs?: boolean;
      install?: () => UpdateInstall | null;
    } = {},
  ) {
    server.use(
      http.get('*/api/latest-release', () =>
        HttpResponse.json({
          newer: MOCK_NEWER_RELEASE,
          offer,
          download: download(),
          installs,
          install: install(),
        } satisfies LatestReleaseResponse),
      ),
    );
  }

  async function noticeShown() {
    await screen.findByTestId('newer-release');
  }

  const CONTROLS = [
    'update-button',
    'update-rebuild-hint',
    'update-downloading',
    'update-ready',
    'update-retry',
    'update-install',
    'update-installing',
    'update-restart',
    'update-restarting',
    'update-install-retry',
    'update-install-unavailable',
    'update-manual',
    'update-download-again',
  ];

  function shownControls() {
    return CONTROLS.filter((id) => screen.queryByTestId(id) !== null);
  }

  it('offers Update only to a desktop release build', async () => {
    answerLatest('update');
    renderPane();
    await noticeShown();
    expect(await screen.findByTestId('update-button')).toHaveTextContent(
      'Update',
    );
    expect(shownControls()).toEqual(['update-button']);
  });

  it('shows the rebuild hint, not Update, to a desktop local build', async () => {
    answerLatest('rebuild');
    renderPane();
    await noticeShown();
    expect(await screen.findByTestId('update-rebuild-hint')).toHaveAttribute(
      'title',
      expect.stringContaining('make desktop'),
    );
    expect(shownControls()).toEqual(['update-rebuild-hint']);
  });

  it('shows neither to the browser version', async () => {
    answerLatest('none');
    renderPane();
    await noticeShown();
    expect(shownControls()).toEqual([]);
  });

  it('shows nothing beside the version when there is no newer release', async () => {
    let asked = false;
    server.use(
      http.get('*/api/latest-release', () => {
        asked = true;
        return HttpResponse.json({
          newer: null,
          offer: 'update',
          download: null,
          installs: false,
          install: null,
        } satisfies LatestReleaseResponse);
      }),
    );
    renderPane();
    await waitFor(() => expect(asked).toBe(true));
    expect(shownControls()).toEqual([]);
  });

  it('starts the download on Update and follows it to ready', async () => {
    let download: UpdateDownload | null = null;
    let posts = 0;
    answerLatest('update', () => download);
    server.use(
      http.post('*/api/latest-release/download', () => {
        posts += 1;
        download = {
          state: 'downloading',
          version: MOCK_NEWER_RELEASE.version,
          received_bytes: 512,
          total_bytes: 2048,
        };
        return HttpResponse.json(download satisfies UpdateDownload, {
          status: 202,
        });
      }),
    );
    renderPane();

    fireEvent.click(await screen.findByTestId('update-button'));

    expect(await screen.findByTestId('update-downloading')).toHaveTextContent(
      'Downloading 25%',
    );
    expect(posts).toBe(1);

    download = { state: 'ready', version: MOCK_NEWER_RELEASE.version };
    expect(
      await screen.findByTestId('update-ready', {}, { timeout: 3000 }),
    ).toHaveTextContent('Update ready');
    expect(shownControls()).toEqual(['update-ready']);
  });

  it('renders a running download with its progress', async () => {
    answerLatest('update', () => ({
      state: 'downloading',
      version: MOCK_NEWER_RELEASE.version,
      received_bytes: 0,
      total_bytes: null,
    }));
    renderPane();
    expect(await screen.findByTestId('update-downloading')).toHaveTextContent(
      'Downloading…',
    );
    expect(shownControls()).toEqual(['update-downloading']);
  });

  it('renders a failed download with its cause and retries it', async () => {
    let download: UpdateDownload | null = {
      state: 'failed',
      version: MOCK_NEWER_RELEASE.version,
      error: 'the download answered with HTTP status 404',
    };
    let posts = 0;
    answerLatest('update', () => download);
    server.use(
      http.post('*/api/latest-release/download', () => {
        posts += 1;
        download = {
          state: 'downloading',
          version: MOCK_NEWER_RELEASE.version,
          received_bytes: 0,
          total_bytes: null,
        };
        return HttpResponse.json(download satisfies UpdateDownload, {
          status: 202,
        });
      }),
    );
    renderPane();

    const retry = await screen.findByTestId('update-retry');
    expect(retry).toHaveTextContent('Update failed · Retry');
    expect(retry).toHaveAttribute(
      'title',
      expect.stringContaining('the download answered with HTTP status 404'),
    );

    fireEvent.click(retry);

    expect(await screen.findByTestId('update-downloading')).toBeInTheDocument();
    expect(posts).toBe(1);
  });

  it('shows a refusal as a failure with the server message', async () => {
    answerLatest('update');
    server.use(
      http.post('*/api/latest-release/download', () =>
        HttpResponse.json(
          {
            error: 'release v0.0.1 has no asset delta-desktop_0.0.1_amd64.deb',
            code: 'update_unsupported',
          },
          { status: 409 },
        ),
      ),
    );
    renderPane();

    fireEvent.click(await screen.findByTestId('update-button'));

    expect(await screen.findByTestId('update-retry')).toHaveAttribute(
      'title',
      expect.stringContaining('has no asset delta-desktop_0.0.1_amd64.deb'),
    );
  });

  const READY: UpdateDownload = {
    state: 'ready',
    version: MOCK_NEWER_RELEASE.version,
  };
  const MANUAL_COMMAND =
    'sudo apt install /home/dev/.local/share/io.github.x7c1.delta/updates/delta-desktop_0.6.0_amd64.deb';

  it('offers no Install where the app does not install updates itself', async () => {
    answerLatest('update', () => READY);
    renderPane();
    expect(await screen.findByTestId('update-ready')).toHaveTextContent(
      'Update ready',
    );
    expect(shownControls()).toEqual(['update-ready']);
  });

  it('offers Install once the download is ready and follows it to Restart', async () => {
    let install: UpdateInstall | null = null;
    let posts = 0;
    answerLatest('update', () => READY, {
      installs: true,
      install: () => install,
    });
    server.use(
      http.post('*/api/latest-release/install', () => {
        posts += 1;
        install = { state: 'installing', version: MOCK_NEWER_RELEASE.version };
        return HttpResponse.json(install satisfies UpdateInstall, {
          status: 202,
        });
      }),
    );
    renderPane();

    expect(await screen.findByTestId('update-install')).toHaveTextContent(
      'Install',
    );
    expect(shownControls()).toEqual(['update-ready', 'update-install']);
    fireEvent.click(screen.getByTestId('update-install'));

    expect(await screen.findByTestId('update-installing')).toHaveTextContent(
      'Installing…',
    );
    expect(shownControls()).toEqual(['update-installing']);
    expect(posts).toBe(1);

    install = { state: 'installed', version: MOCK_NEWER_RELEASE.version };
    expect(
      await screen.findByTestId('update-restart', {}, { timeout: 3000 }),
    ).toHaveTextContent('Restart');
    expect(shownControls()).toEqual(['update-restart']);
  });

  it('restarts the app on Restart', async () => {
    let restarts = 0;
    answerLatest('update', () => READY, {
      installs: true,
      install: () => ({
        state: 'installed',
        version: MOCK_NEWER_RELEASE.version,
      }),
    });
    server.use(
      http.post('*/api/latest-release/restart', () => {
        restarts += 1;
        return new HttpResponse(null, { status: 204 });
      }),
    );
    renderPane();

    fireEvent.click(await screen.findByTestId('update-restart'));

    expect(await screen.findByTestId('update-restarting')).toHaveTextContent(
      'Restarting…',
    );
    expect(restarts).toBe(1);
  });

  it('renders a failed install with its cause, the manual command and a retry', async () => {
    let install: UpdateInstall | null = {
      state: 'failed',
      version: MOCK_NEWER_RELEASE.version,
      error: 'apt-get could not install the update: E: Unmet dependencies',
      manual_command: MANUAL_COMMAND,
    };
    let posts = 0;
    answerLatest('update', () => READY, {
      installs: true,
      install: () => install,
    });
    server.use(
      http.post('*/api/latest-release/install', () => {
        posts += 1;
        install = { state: 'installing', version: MOCK_NEWER_RELEASE.version };
        return HttpResponse.json(install satisfies UpdateInstall, {
          status: 202,
        });
      }),
    );
    renderPane();

    const retry = await screen.findByTestId('update-install-retry');
    expect(retry).toHaveTextContent('Install failed · Retry');
    expect(retry).toHaveAttribute(
      'title',
      expect.stringContaining('E: Unmet dependencies'),
    );
    expect(shownControls()).toEqual(['update-install-retry', 'update-manual']);
    expect(screen.getByTestId('update-manual-command')).toHaveTextContent(
      MANUAL_COMMAND,
    );
    expect(screen.getByTestId('update-release-page')).toHaveAttribute(
      'href',
      MOCK_NEWER_RELEASE.url,
    );

    fireEvent.click(retry);
    expect(await screen.findByTestId('update-installing')).toBeInTheDocument();
    expect(posts).toBe(1);
  });

  it('shows the manual command with a copy button when Delta cannot install', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
    answerLatest('update', () => READY, {
      installs: true,
      install: () => ({
        state: 'unavailable',
        version: MOCK_NEWER_RELEASE.version,
        error:
          'Delta cannot install the update itself: not authorized, or no polkit authentication agent is running',
        manual_command: MANUAL_COMMAND,
      }),
    });
    renderPane();

    expect(
      await screen.findByTestId('update-install-unavailable'),
    ).toHaveAttribute('title', expect.stringContaining('no polkit'));
    expect(shownControls()).toEqual([
      'update-install-retry',
      'update-install-unavailable',
      'update-manual',
    ]);
    expect(screen.getByTestId('update-manual-command')).toHaveTextContent(
      MANUAL_COMMAND,
    );

    const copy = screen.getByRole('button', { name: 'Copy install command' });
    fireEvent.click(copy);
    expect(await within(copy).findByText('Copied')).toBeInTheDocument();
    expect(writeText).toHaveBeenCalledWith(MANUAL_COMMAND);
  });

  it('offers a retry beside the manual command when Delta cannot install', async () => {
    let install: UpdateInstall | null = {
      state: 'unavailable',
      version: MOCK_NEWER_RELEASE.version,
      error:
        'Delta cannot install the update itself: not authorized, or no polkit authentication agent is running',
      manual_command: MANUAL_COMMAND,
    };
    let posts = 0;
    answerLatest('update', () => READY, {
      installs: true,
      install: () => install,
    });
    server.use(
      http.post('*/api/latest-release/install', () => {
        posts += 1;
        install = { state: 'installing', version: MOCK_NEWER_RELEASE.version };
        return HttpResponse.json(install satisfies UpdateInstall, {
          status: 202,
        });
      }),
    );
    renderPane();

    const retry = await screen.findByTestId('update-install-retry');
    expect(retry).toHaveTextContent('Install · Retry');
    expect(retry).toHaveAttribute('title', expect.stringContaining('no polkit'));

    fireEvent.click(retry);
    expect(await screen.findByTestId('update-installing')).toBeInTheDocument();
    expect(posts).toBe(1);
  });

  it('offers downloading again, not a manual command, for a rejected file', async () => {
    const reason = "the update file's sha256 is 00, not release v0.6.0's ff";
    let download: UpdateDownload | null = null;
    let install: UpdateInstall | null = {
      state: 'rejected',
      version: MOCK_NEWER_RELEASE.version,
      error: reason,
    };
    let posts = 0;
    answerLatest('update', () => download, {
      installs: true,
      install: () => install,
    });
    server.use(
      http.post('*/api/latest-release/download', () => {
        posts += 1;
        install = null;
        download = {
          state: 'downloading',
          version: MOCK_NEWER_RELEASE.version,
          received_bytes: 0,
          total_bytes: null,
        };
        return HttpResponse.json(download satisfies UpdateDownload, {
          status: 202,
        });
      }),
    );
    renderPane();

    const again = await screen.findByTestId('update-download-again');
    expect(again).toHaveTextContent('Verification failed · Download again');
    expect(again).toHaveAttribute('title', expect.stringContaining(reason));
    expect(shownControls()).toEqual(['update-download-again']);
    expect(screen.getByTestId('update-release-page')).toHaveAttribute(
      'href',
      MOCK_NEWER_RELEASE.url,
    );
    expect(screen.queryByTestId('update-manual-command')).toBeNull();

    fireEvent.click(again);
    expect(await screen.findByTestId('update-downloading')).toBeInTheDocument();
    expect(posts).toBe(1);
  });

  it('shows an install refusal as a failure with the server message', async () => {
    answerLatest('update', () => READY, { installs: true });
    server.use(
      http.post('*/api/latest-release/install', () =>
        HttpResponse.json(
          {
            error: 'no verified download of the newer release is ready to install',
            code: 'update_not_ready',
          },
          { status: 409 },
        ),
      ),
    );
    renderPane();

    fireEvent.click(await screen.findByTestId('update-install'));

    await waitFor(() =>
      expect(screen.getByTestId('update-install')).toHaveTextContent(
        'Install failed · Retry',
      ),
    );
    expect(screen.getByTestId('update-install')).toHaveAttribute(
      'title',
      expect.stringContaining('no verified download'),
    );
  });
});
