/**
 * The footer's workspace version and the newer-release notice.
 *
 * Shared fixtures live in NavigatorPane.testkit.tsx.
 */
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
import { act, screen, waitFor } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { MOCK_NEWER_RELEASE, MOCK_VERSION } from '@delta/api-mocks';
import type { LatestReleaseResponse } from '@delta/wire-gen';
import { useLiveStore } from '../../store/liveStore';
import { useNavStore } from '../../store/navStore';
import { renderPane, server } from './NavigatorPane.testkit';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

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
