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
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { setupServer } from 'msw/node';
import {
  MAIN_THREAD_ID,
  createHandlers,
  mockThreads,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import { useComposerStore } from '../../store/composerStore';
import type { MessageItemProps } from './MessageItem';
import { resetTimelineExpandedForTests } from './useTimelineExpanded';
import { TranscriptPane } from './TranscriptPane';

// Capture every onSelectQuote reference TranscriptPane hands the message rows.
// A stub MessageItem replaces the real component so we can assert the parent's
// callback identity directly, without the heavy Markdown / Collapsible subtree
// re-rendering on every probe. The stub renders a placeholder so the parent's
// keyed list still mounts in order.
const capturedOnSelectQuote: Array<MessageItemProps['onSelectQuote']> = [];
vi.mock('./MessageItem', () => ({
  MessageItem: (props: MessageItemProps) => {
    capturedOnSelectQuote.push(props.onSelectQuote);
    return (
      <div
        data-testid="message-item-stub"
        data-message-uuid={props.message.uuid}
      />
    );
  },
}));

const server = setupServer(...createHandlers());

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

function renderPane(threads = mockThreads, activeThreadId = MAIN_THREAD_ID) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  const active = threads.find((t) => t.id === activeThreadId)!;
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <TranscriptPane
          threads={threads}
          activeThread={active}
          readOnly={false}
        />
      </ApiProvider>
    </QueryClientProvider>,
  );
}

describe('TranscriptPane onSelectQuote stability (v24)', () => {
  beforeEach(() => {
    capturedOnSelectQuote.length = 0;
    useNavStore.setState({
      activeThreadId: MAIN_THREAD_ID,
      focusedSessionId: NEW_SESSION_FOCUS,
      preNewSessionFocus: null,
    });
    useLiveStore.setState({
      sending: [],
      localSends: {},
      spawns: [],
      notices: {},
      streamingMessages: {},
      runningSubagents: {},
    });
    useComposerStore.setState({
      drafts: {},
      branchOrigin: null,
      newSessionWorkdir: null,
      workdirDialogOpen: false,
    });
    // Reset the timeline-expanded cache so this test starts from the
    // default collapsed state regardless of which case ran before. The
    // localStorage clear avoids any stale per-session key bleeding in.
    window.localStorage.clear();
    resetTimelineExpandedForTests();
  });

  it('keeps the onSelectQuote reference stable across a branch-chip hover', async () => {
    // Branch-chip hover sets the local `hoveredBranchTitle` state, which is the
    // canonical "unrelated state churns, parent re-renders" probe. With v24's
    // useCallback, the captured onSelectQuote identity must be `===` after the
    // hover; without it, the inline arrow handed a fresh closure to every row,
    // defeating MessageItem's React.memo bail-out.
    renderPane();

    // Wait until the transcript has mounted with at least one stub message item.
    await waitFor(() => {
      expect(
        screen.getAllByTestId('message-item-stub').length,
      ).toBeGreaterThan(0);
    });

    // The most recent captured handler for the first message row. Hover-driven
    // re-renders push new entries to `capturedOnSelectQuote`; we compare the
    // first row's callback identity across the boundary.
    const beforeRef = capturedOnSelectQuote[0];
    expect(beforeRef).toBeTypeOf('function');

    // Drive an unrelated state update on TranscriptPane: hover over the branch
    // chip the mock dataset already sprouts in the main thread. This sets
    // `hoveredBranchTitle` and re-renders the pane without touching the active
    // thread, the messages, or the pairing.
    const chip = await screen.findByRole('button', { name: /^Enter / });
    fireEvent.mouseEnter(chip);

    // After the hover-triggered re-render lands, the parent should have handed
    // the SAME function instance back to the stubbed MessageItem for the first
    // row. Identity (`===`) is the contract memo's shallow compare relies on.
    await waitFor(() => {
      expect(capturedOnSelectQuote.length).toBeGreaterThan(
        // At least one fresh capture after the initial mount batch.
        1,
      );
    });
    const afterRef =
      capturedOnSelectQuote[capturedOnSelectQuote.length - 1];
    expect(afterRef).toBe(beforeRef);
  });
});
