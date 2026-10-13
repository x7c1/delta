/**
 * The top region: the floating breadcrumb and timeline cards, the pinned
 * expanded container, and the top reserve measured from them.
 *
 * Shared fixtures live in TranscriptPane.testkit.tsx.
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
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  BRANCH_THREAD_ID,
  MAIN_THREAD_ID,
  SESSION_ID,
  mockThreads,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { useNavStore } from '../../store/navStore';
import { resetTimelineExpandedForTests } from './useTimelineExpanded';
import { TranscriptPane } from './TranscriptPane';
import {
  bodyEl,
  installControllableResizeObserver,
  lookupObserver,
  renderPane,
  resetTranscriptPaneState,
  restoreResizeObserver,
  server,
  timelineExpandedKey,
} from './TranscriptPane.testkit';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('TranscriptPane', () => {
  beforeEach(resetTranscriptPaneState);

  describe('dynamic bottom reserve (composer auto-grow follow)', () => {
    beforeEach(installControllableResizeObserver);
    afterEach(restoreResizeObserver);

    // v19 top-row layout. The collapsed state places the breadcrumb and
    // the {Thread + Terminal} cluster as INDIVIDUAL absolute floating
    // cards over the body (preserved from v18). The expanded state
    // wraps the entire top region in a SINGLE absolute container
    // pinned to the top of the Panel body region — it does NOT scroll
    // with the conversation, so scrubbing the timeline (which scrolls
    // the conversation) no longer drags the timeline off-screen the
    // way v18's in-flow expanded layout did. Inside the container the
    // children use normal flow: the timeline card on top, the
    // breadcrumb + Terminal row underneath. The body reserves
    // `padding-top` in BOTH states (via `--delta-top-region-reserve`)
    // so the first message clears the pinned region.
    describe('top region overlay (v19 pinned expanded container)', () => {
      it('renders the breadcrumb and the right-side cluster as individual absolute cards in the collapsed state — no shared white bar wrapper', async () => {
        renderPane(mockThreads, BRANCH_THREAD_ID);
        const topRegion = await screen.findByTestId('transcript-top-region');
        // The collapsed wrapper itself is layout-less (`display: contents`)
        // — there is no full-width white bar with a background, padding,
        // and stretched left/right insets the v17 layout used. A
        // regression that re-introduced that bar would put `bg-surface`
        // back on the wrapper or some ancestor.
        expect(topRegion.getAttribute('data-expanded')).toBe('false');
        // Negative lookahead excludes `bg-surface-elevated` (a different
        // semantic token used elsewhere) from the substring match.
        expect(topRegion.className).not.toMatch(/\bbg-surface(?!-)/);
        expect(topRegion.className).not.toContain('left-0');
        expect(topRegion.className).not.toContain('right-0');

        // Each floating card is itself an absolute element, pinned with
        // the shared `overlay-inset` token so they read as one row.
        const breadcrumb = await screen.findByTestId(
          'transcript-breadcrumb-overlay',
        );
        expect(breadcrumb.className).toContain('absolute');
        expect(breadcrumb.className).toContain('top-overlay-inset');
        expect(breadcrumb.className).toContain('left-overlay-inset');
        expect(breadcrumb.className).toContain('z-20');

        const rightCluster = await screen.findByTestId('transcript-top-row');
        expect(rightCluster.getAttribute('data-expanded')).toBe('false');
        expect(rightCluster.className).toContain('absolute');
        expect(rightCluster.className).toContain('top-overlay-inset');
        expect(rightCluster.className).toContain('right-overlay-inset');
        expect(rightCluster.className).toContain('z-20');
        // The right cluster carries NO shared surface-bar background or
        // border on purpose: each pill inside already has its own card
        // chrome (TIMELINE_TOGGLE_BUTTON_CLASS / PANE_TOGGLE_BUTTON_CLASS).
        // A regression that re-introduced the v17 single-bar look would
        // put `bg-surface` back on this cluster wrapper.
        // Negative lookahead excludes `bg-surface-elevated` (a different
        // semantic token used elsewhere) from the substring match.
        expect(rightCluster.className).not.toMatch(/\bbg-surface(?!-)/);

        // The breadcrumb and the right cluster are siblings under the
        // top-region wrapper, NOT nested inside one shared white-bar
        // container.
        expect(breadcrumb.parentElement).toBe(rightCluster.parentElement);

        // The breadcrumb and timeline toggle render inside their
        // respective floating cards (so both still live under the
        // `transcript-top-region` umbrella).
        expect(
          within(breadcrumb).getByRole('navigation', { name: 'Breadcrumb' }),
        ).toBeInTheDocument();
        expect(
          within(rightCluster).getByTestId('thread-timeline-toggle'),
        ).toBeInTheDocument();
      });

      it("reserves the body's padding-top from --delta-top-region-reserve so the first message clears the floating cards", async () => {
        renderPane(mockThreads, BRANCH_THREAD_ID);
        // The body reads the variable through `padding-top` (set inline so
        // jsdom can read it back as a literal CSS value). Without this
        // reserve the first message would render under the absolute
        // floating cards on initial paint.
        expect(bodyEl().style.paddingTop).toBe(
          'var(--delta-top-region-reserve, 0)',
        );
      });

      it('places the Terminal button inside the right-side cluster alongside the timeline toggle when collapsed', async () => {
        // The Terminal button is forwarded into TranscriptPane as the
        // `paneToggleButton` slot (see WorkspaceScreen). It must render
        // INSIDE the floating right-side cluster so it stays on screen
        // alongside the timeline toggle; if it were a sibling outside
        // the cluster, it would scroll away with the conversation.
        const queryClient = new QueryClient({
          defaultOptions: { queries: { retry: false } },
        });
        const client = new ApiClient({ baseUrl: 'http://localhost' });
        render(
          <QueryClientProvider client={queryClient}>
            <ApiProvider client={client}>
              <TranscriptPane
                threads={mockThreads}
                activeThread={mockThreads.find((t) => t.id === MAIN_THREAD_ID)!}
                readOnly={false}
                paneToggleButton={
                  <button data-testid="terminal-toggle">Terminal</button>
                }
              />
            </ApiProvider>
          </QueryClientProvider>,
        );

        const row = await screen.findByTestId('transcript-top-row');
        expect(row.getAttribute('data-expanded')).toBe('false');
        const toggle = within(row).getByTestId('thread-timeline-toggle');
        const terminal = within(row).getByTestId('terminal-toggle');
        expect(toggle).toBeInTheDocument();
        expect(terminal).toBeInTheDocument();
        // DOM order: timeline toggle first, then the Terminal button —
        // so the two pills read left-to-right inside the right cluster.
        expect(
          toggle.compareDocumentPosition(terminal) &
            Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
      });

      it('places the breadcrumb and Terminal as a single normal-flow row under the expanded timeline card', async () => {
        // Seed the persisted preference to expanded so the timeline
        // mounts open. The expanded layout drops the absolute overlay
        // entirely: the expanded card grows full-width on top in normal
        // flow, and a single row carrying the breadcrumb (left) and the
        // Terminal button (right) sits directly underneath it. The
        // Thread icon is absent in this row — the expanded card itself
        // replaces it.
        // Point the focus at a real session id so the timeline's
        // per-session expand hook can persist against the matching
        // localStorage key — the beforeEach pins focus to
        // `NEW_SESSION_FOCUS`, which collapses to a null id inside the hook.
        useNavStore.setState({ focusedSessionId: SESSION_ID });
        window.localStorage.setItem(timelineExpandedKey(), 'true');
        resetTimelineExpandedForTests();

        const queryClient = new QueryClient({
          defaultOptions: { queries: { retry: false } },
        });
        const client = new ApiClient({ baseUrl: 'http://localhost' });
        render(
          <QueryClientProvider client={queryClient}>
            <ApiProvider client={client}>
              <TranscriptPane
                threads={mockThreads}
                activeThread={mockThreads.find((t) => t.id === BRANCH_THREAD_ID)!}
                readOnly={false}
                paneToggleButton={
                  <button data-testid="terminal-toggle">Terminal</button>
                }
              />
            </ApiProvider>
          </QueryClientProvider>,
        );

        const region = await screen.findByTestId('transcript-top-region');
        expect(region.getAttribute('data-expanded')).toBe('true');

        const row = await screen.findByTestId('transcript-top-row');
        expect(row.getAttribute('data-expanded')).toBe('true');
        // Single in-flow row, laid out horizontally with the breadcrumb
        // on the left and the Terminal on the right.
        expect(row.className).toContain('flex');
        expect(row.className).not.toContain('flex-col');
        // Both pieces live inside this same row — no separate breadcrumb
        // row of its own.
        const breadcrumbNav = within(row).getByRole('navigation', {
          name: 'Breadcrumb',
        });
        const terminal = within(row).getByTestId('terminal-toggle');
        expect(breadcrumbNav).toBeInTheDocument();
        expect(terminal).toBeInTheDocument();
        // DOM order: breadcrumb first, then Terminal.
        expect(
          breadcrumbNav.compareDocumentPosition(terminal) &
            Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
        // The expanded timeline card lives in the region above this row,
        // not inside it — the row is the one underneath.
        expect(
          within(row).queryByTestId('thread-timeline-overlay'),
        ).not.toBeInTheDocument();
        // There is no Thread toggle in the under-row either: the
        // expanded card replaces it in this state.
        expect(
          within(row).queryByTestId('thread-timeline-toggle'),
        ).not.toBeInTheDocument();
        // The expanded timeline card sits ABOVE the under-row inside
        // the top region.
        const expandedCard = within(region).getByTestId(
          'thread-timeline-overlay',
        );
        expect(
          expandedCard.compareDocumentPosition(row) &
            Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
      });

      it('pins the expanded top region as a SINGLE absolute container so it does not scroll with the conversation (v19 regression fix)', async () => {
        // The v18 expanded layout placed the timeline card + the
        // breadcrumb/Terminal under-row directly into the scrolling
        // body's normal flow. Scrubbing the timeline jumps the
        // conversation; with the timeline in flow, the conversation
        // scroll dragged the timeline itself off-screen and the user
        // could not scrub again. v19 wraps the entire top region in a
        // single absolute container that pins to the top of the Panel
        // body region — the container does not participate in the
        // body's scroll. This test pins that pinning contract.
        // Point the focus at a real session id so the timeline's
        // per-session expand hook can persist against the matching
        // localStorage key — the beforeEach pins focus to
        // `NEW_SESSION_FOCUS`, which collapses to a null id inside the hook.
        useNavStore.setState({ focusedSessionId: SESSION_ID });
        window.localStorage.setItem(timelineExpandedKey(), 'true');
        resetTimelineExpandedForTests();

        renderPane(mockThreads, BRANCH_THREAD_ID);

        const region = await screen.findByTestId('transcript-top-region');
        expect(region.getAttribute('data-expanded')).toBe('true');
        // The container is `absolute top-0 left-0 right-0 z-20` so it
        // anchors to the closest positioned ancestor (the Panel body
        // wrapper, which is `position: relative`) — outside the
        // scrolling body — and stays glued to the top edge across
        // conversation scroll. Without these classes the v18 bug
        // returns: the container would flow with the conversation.
        expect(region.className).toContain('absolute');
        expect(region.className).toContain('top-0');
        expect(region.className).toContain('left-0');
        expect(region.className).toContain('right-0');
        expect(region.className).toContain('z-20');

        // Proxy for "container stays inside the viewport after a body
        // scroll": the container's closest positioned ancestor — the
        // element absolute escapes to — is OUTSIDE the scrolling body
        // (the Panel body div with `overflow-y-auto`). Walking the
        // ancestry from the container, we should reach the relative
        // positioning context BEFORE crossing the scroll body, so the
        // container is anchored to the viewport-pinning region and
        // never to the scroll content. A regression that put the
        // container inside the scrolling body's positioning context
        // (e.g. by making the body itself `position: relative`, or by
        // dropping `absolute`) would fail this check.
        const body = bodyEl();
        let cursor: HTMLElement | null = region.parentElement;
        let positionedAncestor: HTMLElement | null = null;
        while (cursor !== null) {
          // jsdom does not compute layout, but className inspection is
          // enough: the Panel wrapper carries the `relative` Tailwind
          // class, the scroll body carries `overflow-y-auto`.
          if (cursor.className.includes('relative')) {
            positionedAncestor = cursor;
            break;
          }
          cursor = cursor.parentElement;
        }
        expect(positionedAncestor).not.toBeNull();
        // The relative ancestor must be the Panel body region's
        // wrapper, which is the PARENT of the scrolling body — NOT
        // the scrolling body itself or any descendant of it.
        expect(positionedAncestor!.contains(body)).toBe(true);
        expect(body.contains(positionedAncestor!)).toBe(false);
      });

      it('renders the expanded under-row as a normal-flow child INSIDE the pinned container — no individual absolute positioning', async () => {
        // Core v19 invariant: the container is pinned, the children
        // are in normal flow. A regression that bolted `absolute` back
        // onto the under-row (the v17 mistake, re-applied to the new
        // container shape) would either escape the under-row from the
        // container or stack it on top of the timeline card.
        // Point the focus at a real session id so the timeline's
        // per-session expand hook can persist against the matching
        // localStorage key — the beforeEach pins focus to
        // `NEW_SESSION_FOCUS`, which collapses to a null id inside the hook.
        useNavStore.setState({ focusedSessionId: SESSION_ID });
        window.localStorage.setItem(timelineExpandedKey(), 'true');
        resetTimelineExpandedForTests();

        const queryClient = new QueryClient({
          defaultOptions: { queries: { retry: false } },
        });
        const client = new ApiClient({ baseUrl: 'http://localhost' });
        render(
          <QueryClientProvider client={queryClient}>
            <ApiProvider client={client}>
              <TranscriptPane
                threads={mockThreads}
                activeThread={mockThreads.find((t) => t.id === BRANCH_THREAD_ID)!}
                readOnly={false}
                paneToggleButton={
                  <button data-testid="terminal-toggle">Terminal</button>
                }
              />
            </ApiProvider>
          </QueryClientProvider>,
        );

        const region = await screen.findByTestId('transcript-top-region');
        const timelineCard = within(region).getByTestId(
          'thread-timeline-overlay',
        );
        const underRow = within(region).getByTestId('transcript-top-row');

        // Container uses a flex column so its two children stack
        // top-to-bottom in normal flow.
        expect(region.className).toContain('flex');
        expect(region.className).toContain('flex-col');

        // Neither child carries its own `absolute` positioning — they
        // are normal-flow children of the pinned container.
        expect(timelineCard.className).not.toContain('absolute');
        expect(underRow.className).not.toContain('absolute');

        // DOM order: timeline card first, then the under-row underneath.
        expect(
          timelineCard.compareDocumentPosition(underRow) &
            Node.DOCUMENT_POSITION_FOLLOWING,
        ).toBeTruthy();
      });

      it("reserves the body's padding-top from --delta-top-region-reserve in the expanded state too (so the first message clears the pinned container)", async () => {
        // v19: the expanded container is absolute (same as the
        // collapsed floating cards), so it takes NO layout space
        // inside the body — the body must reserve a matching
        // `padding-top` from the container's measured height, or the
        // first message renders under the container on initial paint.
        // The mechanism mirrors collapsed: same CSS variable, same
        // inline style on the body. A regression that dropped the
        // expanded reserve (the v18 assumption that "expanded is in
        // flow, no reserve needed") would let the conversation render
        // under the pinned container.
        // Point the focus at a real session id so the timeline's
        // per-session expand hook can persist against the matching
        // localStorage key — the beforeEach pins focus to
        // `NEW_SESSION_FOCUS`, which collapses to a null id inside the hook.
        useNavStore.setState({ focusedSessionId: SESSION_ID });
        window.localStorage.setItem(timelineExpandedKey(), 'true');
        resetTimelineExpandedForTests();

        renderPane(mockThreads, BRANCH_THREAD_ID);

        const region = await screen.findByTestId('transcript-top-region');
        await waitFor(() => expect(lookupObserver(region)).not.toBeNull());

        region.getBoundingClientRect = () => ({ height: 220 }) as DOMRect;
        act(() => {
          const fire = lookupObserver(region)!;
          fire([], fire as unknown as ResizeObserver);
        });

        await waitFor(() =>
          expect(
            bodyEl().style.getPropertyValue('--delta-top-region-reserve'),
          ).toBe('220px'),
        );
        // The body's padding-top references the variable in BOTH
        // states — same mechanism, only the variable's value differs.
        expect(bodyEl().style.paddingTop).toBe(
          'var(--delta-top-region-reserve, 0)',
        );
      });

      it('switches the ResizeObserver target when timelineExpanded flips (collapsed cards → single expanded container)', async () => {
        // Critical v19 mechanism: the ResizeObserver disconnects and
        // re-binds when `timelineExpanded` changes, so the live
        // observation target always matches the rendered state. Going
        // collapsed → expanded must drop the breadcrumb/cluster
        // observation and pick up the container; going back must do
        // the reverse. A leak (observing nodes that no longer exist)
        // or a stale binding (observing the wrong state's nodes)
        // breaks the reserve when the user toggles.
        //
        // Pin the focused session to a real id so the timeline's per-session
        // expand toggle actually persists — under the new-session sentinel
        // the toggle is a no-op (the hook has no id to bind to).
        useNavStore.setState({ focusedSessionId: SESSION_ID });
        renderPane(mockThreads, BRANCH_THREAD_ID);

        // Collapsed initially: the breadcrumb and right cluster are
        // both observed.
        const collapsedBreadcrumb = await screen.findByTestId(
          'transcript-breadcrumb-overlay',
        );
        const collapsedCluster = await screen.findByTestId('transcript-top-row');
        expect(collapsedCluster.getAttribute('data-expanded')).toBe('false');
        await waitFor(() =>
          expect(lookupObserver(collapsedBreadcrumb)).not.toBeNull(),
        );
        await waitFor(() =>
          expect(lookupObserver(collapsedCluster)).not.toBeNull(),
        );

        // Toggle to expanded — the collapsed cards unmount, the
        // expanded container mounts. The observer must disconnect
        // from the old nodes and observe the new container.
        const toggle = within(collapsedCluster).getByTestId(
          'thread-timeline-toggle',
        );
        act(() => {
          fireEvent.click(toggle);
        });

        const region = await screen.findByTestId('transcript-top-region');
        expect(region.getAttribute('data-expanded')).toBe('true');

        // Old nodes are gone from observations (the ControllableRO's
        // `disconnect` clears them).
        expect(lookupObserver(collapsedBreadcrumb)).toBeNull();
        expect(lookupObserver(collapsedCluster)).toBeNull();
        // The expanded container is now observed.
        await waitFor(() => expect(lookupObserver(region)).not.toBeNull());

        // Toggle back — re-binds on the collapsed cards.
        const expandedToggle = within(region).getByTestId(
          'thread-timeline-toggle',
        );
        act(() => {
          fireEvent.click(expandedToggle);
        });

        const collapsedClusterAgain = await screen.findByTestId(
          'transcript-top-row',
        );
        await waitFor(() =>
          expect(collapsedClusterAgain.getAttribute('data-expanded')).toBe(
            'false',
          ),
        );
        await waitFor(() =>
          expect(lookupObserver(collapsedClusterAgain)).not.toBeNull(),
        );
        const collapsedBreadcrumbAgain = await screen.findByTestId(
          'transcript-breadcrumb-overlay',
        );
        await waitFor(() =>
          expect(lookupObserver(collapsedBreadcrumbAgain)).not.toBeNull(),
        );
      });

      it('drives --delta-top-region-reserve from the max of the breadcrumb and the right cluster heights (collapsed only)', async () => {
        renderPane(mockThreads, BRANCH_THREAD_ID);
        const breadcrumb = await screen.findByTestId(
          'transcript-breadcrumb-overlay',
        );
        const rightCluster = await screen.findByTestId('transcript-top-row');
        await waitFor(() =>
          expect(lookupObserver(breadcrumb)).not.toBeNull(),
        );
        await waitFor(() =>
          expect(lookupObserver(rightCluster)).not.toBeNull(),
        );

        // Both cards report their own measured height; the body's
        // reserve is the taller of the two (the visual row height).
        breadcrumb.getBoundingClientRect = () => ({ height: 40 }) as DOMRect;
        rightCluster.getBoundingClientRect = () => ({ height: 72 }) as DOMRect;
        act(() => {
          const fire = lookupObserver(rightCluster)!;
          fire([], fire as unknown as ResizeObserver);
        });

        await waitFor(() =>
          expect(
            bodyEl().style.getPropertyValue('--delta-top-region-reserve'),
          ).toBe('72px'),
        );

        // The right cluster shrinks below the breadcrumb: the reserve
        // tracks the new tallest side.
        rightCluster.getBoundingClientRect = () => ({ height: 28 }) as DOMRect;
        act(() => {
          const fire = lookupObserver(breadcrumb)!;
          fire([], fire as unknown as ResizeObserver);
        });
        await waitFor(() =>
          expect(
            bodyEl().style.getPropertyValue('--delta-top-region-reserve'),
          ).toBe('40px'),
        );
      });

      it('preserves the body\'s scrollTop when the floating cards are measured (no scroll yank)', async () => {
        // Measuring the floating cards must not move the scroll position
        // the user is reading. The observer effect only writes the
        // `--delta-top-region-reserve` CSS variable on the body and
        // never touches `scrollTop`.
        renderPane(mockThreads, BRANCH_THREAD_ID);
        const rightCluster = await screen.findByTestId('transcript-top-row');
        await waitFor(() =>
          expect(lookupObserver(rightCluster)).not.toBeNull(),
        );

        const body = bodyEl();
        Object.defineProperty(body, 'scrollHeight', {
          configurable: true,
          get: () => 4000,
        });
        Object.defineProperty(body, 'clientHeight', {
          configurable: true,
          get: () => 400,
        });
        // Park the user well above the bottom (so stick-to-bottom is OFF):
        // any unintended jump would land at scrollHeight (4000) instead.
        body.scrollTop = 1200;
        fireEvent.scroll(body);

        rightCluster.getBoundingClientRect = () => ({ height: 96 }) as DOMRect;
        act(() => {
          const fire = lookupObserver(rightCluster)!;
          fire([], fire as unknown as ResizeObserver);
        });
        await waitFor(() =>
          expect(
            bodyEl().style.getPropertyValue('--delta-top-region-reserve'),
          ).toBe('96px'),
        );

        // The scroll position the user was reading at stays put.
        expect(body.scrollTop).toBe(1200);
      });
    });
  });
});
