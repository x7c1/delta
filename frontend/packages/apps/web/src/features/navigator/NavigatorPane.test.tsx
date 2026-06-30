import {
  afterAll,
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import {
  createHandlers,
  SESSION_ID,
  SESSION_ID_2,
  SESSION_2_MAIN_THREAD_ID,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { SessionListItem } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useLiveStore } from '../../store/liveStore';
import { useNavStore } from '../../store/navStore';
import { useComposerStore } from '../../store/composerStore';
import { NavigatorPane } from './NavigatorPane';

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function makeItem(id: string, mainThreadId: number): SessionListItem {
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

function renderPane() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <NavigatorPane
          sessions={sessions}
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

describe('NavigatorPane rate-limit meters', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      rateLimits: null,
    });
    useNavStore.setState({ focusedSessionId: null, activeThreadId: null });
  });

  it('renders both meter rows with percentages and reset labels', () => {
    // Add a 30s cushion on top of each whole-minute offset so the few ms that
    // elapse between this `Date.now()` and the component's own render-time read
    // cannot cross a minute boundary and flip the displayed countdown.
    const now = Date.now() / 1000;
    useLiveStore.setState({
      rateLimits: {
        fiveHour: {
          used_percentage: 37,
          resets_at: now + 2 * 3600 + 13 * 60 + 30,
        },
        sevenDay: {
          used_percentage: 8,
          resets_at: now + 5 * 86400 + 4 * 3600 + 30,
        },
      },
    });

    renderPane();

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

  it('renders the elapsed-time marker on each row when resets_at is present', () => {
    // Pick resets that leave a clean fraction of the window remaining so the
    // expected marker position is easy to verify: 5h window with 1h left = 80%
    // elapsed; 7d window with 1d left = 6/7 ≈ 85.71…% elapsed.
    const FIVE_HOURS = 5 * 60 * 60;
    const SEVEN_DAYS = 7 * 24 * 60 * 60;
    const now = Date.now() / 1000;
    useLiveStore.setState({
      rateLimits: {
        fiveHour: {
          used_percentage: 40,
          resets_at: now + 1 * 60 * 60,
        },
        sevenDay: {
          used_percentage: 50,
          resets_at: now + 1 * 86400,
        },
      },
    });

    renderPane();

    const fiveHourMarker = screen.getByTestId('rate-limit-5h-elapsed-marker');
    expect(fiveHourMarker).toBeInTheDocument();
    const fiveHourRight = parseFloat(
      (fiveHourMarker as HTMLElement).style.right,
    );
    // 4h elapsed out of 5h = 80%. Tolerance covers the few ms between the
    // test's Date.now() snapshot and the component's own read.
    const fiveHourExpected = ((FIVE_HOURS - 1 * 60 * 60) / FIVE_HOURS) * 100;
    expect(fiveHourRight).toBeGreaterThan(fiveHourExpected - 0.5);
    expect(fiveHourRight).toBeLessThan(fiveHourExpected + 0.5);

    const sevenDayMarker = screen.getByTestId('rate-limit-7d-elapsed-marker');
    expect(sevenDayMarker).toBeInTheDocument();
    const sevenDayRight = parseFloat(
      (sevenDayMarker as HTMLElement).style.right,
    );
    const sevenDayExpected = ((SEVEN_DAYS - 1 * 86400) / SEVEN_DAYS) * 100;
    expect(sevenDayRight).toBeGreaterThan(sevenDayExpected - 0.5);
    expect(sevenDayRight).toBeLessThan(sevenDayExpected + 0.5);
  });

  it('omits the elapsed-time marker when resets_at is null', () => {
    useLiveStore.setState({
      rateLimits: {
        fiveHour: { used_percentage: 25, resets_at: null },
        sevenDay: null,
      },
    });

    renderPane();

    expect(
      screen.queryByTestId('rate-limit-5h-elapsed-marker'),
    ).not.toBeInTheDocument();
  });

  it('renders neither row (no empty bars) when the snapshot has no rate limits', () => {
    useLiveStore.setState({
      rateLimits: { fiveHour: null, sevenDay: null },
    });

    renderPane();

    expect(screen.queryByTestId('rate-limits')).not.toBeInTheDocument();
    expect(screen.queryByTestId('rate-limit-5h')).not.toBeInTheDocument();
    expect(screen.queryByTestId('rate-limit-7d')).not.toBeInTheDocument();
  });

  it('renders only the 5h row when the 7d window is absent', () => {
    const now = Date.now() / 1000;
    useLiveStore.setState({
      rateLimits: {
        fiveHour: { used_percentage: 50, resets_at: now + 3600 },
        sevenDay: null,
      },
    });

    renderPane();

    expect(screen.getByTestId('rate-limit-5h')).toBeInTheDocument();
    expect(screen.queryByTestId('rate-limit-7d')).not.toBeInTheDocument();
  });

  it('renders only the 7d row when the 5h window is absent', () => {
    const now = Date.now() / 1000;
    useLiveStore.setState({
      rateLimits: {
        fiveHour: null,
        sevenDay: { used_percentage: 12, resets_at: now + 86400 },
      },
    });

    renderPane();

    expect(screen.queryByTestId('rate-limit-5h')).not.toBeInTheDocument();
    expect(screen.getByTestId('rate-limit-7d')).toBeInTheDocument();
  });

  it('shows 0% and no reset glyph for a present window with null fields', () => {
    // A present window can still carry null values (no usage reported yet, no
    // reset timestamp): the row renders at 0% and omits the ↻ countdown rather
    // than showing a misleading reset.
    useLiveStore.setState({
      rateLimits: {
        fiveHour: { used_percentage: null, resets_at: null },
        sevenDay: null,
      },
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
});

describe('NavigatorPane settings entry', () => {
  beforeEach(() => {
    useLiveStore.setState({
      connection: 'open',
      notices: {},
      runningThreads: {},
      rateLimits: null,
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
