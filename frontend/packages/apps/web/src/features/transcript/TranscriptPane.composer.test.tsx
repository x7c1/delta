/**
 * The composer stack: the rail above the composer card and the context-usage
 * bar along its top edge.
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
import { http, HttpResponse } from 'msw';
import {
  BRANCH_THREAD_ID,
  MAIN_THREAD_ID,
  SESSION_ID,
  mockThreads,
} from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { NEW_SESSION_FOCUS, useNavStore } from '../../store/navStore';
import { useLiveStore } from '../../store/liveStore';
import { useComposerStore } from '../../store/composerStore';
import { TranscriptPane } from './TranscriptPane';
import { renderPane, server } from './TranscriptPane.testkit';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('TranscriptPane composer rail', () => {
  beforeEach(() => {
    useNavStore.setState({
      activeThreadId: MAIN_THREAD_ID,
      activeThreadJumpTarget: null,
      focusedSessionId: SESSION_ID,
      preNewSessionFocus: null,
    });
    useLiveStore.setState({
      sending: [],
      localSends: {},
      spawns: [],
      notices: {},
      streamingMessages: {},
      runningSubagents: {},
      contextUsage: {},
      rateLimits: {},
    });
    useComposerStore.setState({
      drafts: {},
      branchOrigin: null,
      newSessionWorkdir: null,
      workdirDialogOpen: false,
    });
  });

  /**
   * Render the pane with the composer, either in a thread or in the new-session
   * state. `readOnly` shows the closed-session notice, which is the cheapest way
   * to put the notices card above the composer.
   */
  function renderRailPane({ newSession = false, readOnly = false } = {}) {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const client = new ApiClient({ baseUrl: 'http://localhost' });
    const active = mockThreads.find((t) => t.id === MAIN_THREAD_ID)!;
    if (newSession) {
      useNavStore.setState({ focusedSessionId: NEW_SESSION_FOCUS });
    }
    return render(
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={client}>
          <TranscriptPane
            threads={mockThreads}
            activeThread={newSession ? null : active}
            readOnly={readOnly}
            newSession={newSession}
          />
        </ApiProvider>
      </QueryClientProvider>,
    );
  }

  it('carries the provider tabs on the rail, not in the card stack', async () => {
    renderRailPane({ newSession: true });

    const rail = await screen.findByTestId('composer-rail');
    expect(within(rail).getByTestId('provider-selector')).toBeInTheDocument();
    // The card's flow stack no longer opens with the selector — the provider
    // axis moved out of the card entirely.
    const card = screen.getByTestId('composer-card');
    expect(within(card).queryByTestId('provider-selector')).toBeNull();
  });

  it('spells the unavailability reason out inside the card', async () => {
    server.use(
      http.get('*/api/providers', () =>
        HttpResponse.json({
          providers: [
            {
              provider: 'claude',
              available: true,
              detail: null,
              capabilities: {
                has_terminal: true,
                launch_option_style: 'cli_flag',
              },
            },
            {
              provider: 'codex',
              available: false,
              detail: "The 'codex' binary for codex was not found on PATH.",
              capabilities: {
                has_terminal: false,
                launch_option_style: 'request_field',
              },
            },
          ],
        }),
      ),
    );
    renderRailPane({ newSession: true });

    // A rail tab has room for a name, not a sentence: the reason stays in the
    // card, where the explanation lived before the tabs moved out.
    const notice = await screen.findByTestId('provider-unavailable-notice');
    expect(screen.getByTestId('composer-card')).toContainElement(notice);
    expect(screen.getByTestId('composer-rail')).not.toContainElement(notice);
  });

  it('carries only the template button in a thread', async () => {
    renderRailPane();

    // The provider is fixed once a session runs, so the tabs are gone — but the
    // prompt-template button rides the rail in BOTH modes, so the strip keeps
    // its height instead of collapsing as the tabs come and go.
    const rail = await screen.findByTestId('composer-rail');
    expect(within(rail).queryByTestId('provider-selector')).toBeNull();
    expect(
      within(rail).getByTestId('prompt-templates-button'),
    ).toBeInTheDocument();
    expect(rail.children).toHaveLength(1);
  });

  it('keeps the rail in the measured overlay, in normal flow above the card', async () => {
    renderRailPane({ newSession: true });

    const overlay = await screen.findByTestId('bottom-overlay');
    const rail = screen.getByTestId('composer-rail');
    const card = screen.getByTestId('composer-card');

    // The overlay's ResizeObserver measures this subtree and drives the body's
    // bottom reserve from it (see "dynamic bottom reserve" above), so the rail
    // only gets counted while it stays a flow descendant: an absolutely
    // positioned rail would leave the last turn hidden underneath it.
    expect(overlay).toContainElement(rail);
    for (
      let node: HTMLElement | null = rail;
      node !== null && node !== overlay;
      node = node.parentElement
    ) {
      expect(node.className).not.toMatch(/(^|\s)(absolute|fixed)(\s|$)/);
    }

    // Rail directly above the card, in the same stack and in that order.
    const stack = rail.parentElement!;
    expect(card.parentElement).toBe(stack);
    expect(Array.from(stack.children)).toEqual([rail, card]);
  });

  it('stacks the notices card above the rail with the overlay gap', async () => {
    renderRailPane({ readOnly: true });

    const overlay = await screen.findByTestId('bottom-overlay');
    const notices = await screen.findByTestId('bottom-notices');
    const rail = screen.getByTestId('composer-rail');

    // The rail+card stack is ONE overlay child, so the overlay's gap falls
    // between the notices card and the rail — never between rail and card.
    expect(overlay.className).toContain('gap-2');
    expect(Array.from(overlay.children)).toEqual([notices, rail.parentElement]);
  });

  it('rests every rail item on the card border, reaching past it only for the open provider tab', async () => {
    renderRailPane({ newSession: true });

    // A rail item draws no bottom border of its own, so the card's top border
    // — and, in a thread, the context-usage fill riding it — stays uncovered.
    // The provider tabs are the one sanctioned exception: that item reaches
    // 1px down (`-mb-px`) so its selected tab can open into the card, which is
    // safe because tabs only exist in new-session mode, where no fill rides
    // the border. Every other item keeps clear of the line.
    const rail = await screen.findByTestId('composer-rail');
    const items = Array.from(rail.children);
    expect(items.length).toBeGreaterThan(0);
    const tabs = screen.getByTestId('provider-selector');
    for (const item of items) {
      expect(item.className).toContain('border-b-0');
      if (item === tabs) {
        expect(item.className).toContain('-mb-px');
      } else {
        expect(item.className).not.toMatch(/(^|\s|:)-m[btxy]?-/);
      }
    }
  });
});

describe('TranscriptPane composer context bar', () => {
  beforeEach(() => {
    useNavStore.setState({
      activeThreadId: MAIN_THREAD_ID,
      focusedSessionId: SESSION_ID,
      preNewSessionFocus: null,
    });
    useLiveStore.setState({
      sending: [],
      localSends: {},
      spawns: [],
      notices: {},
      streamingMessages: {},
      runningSubagents: {},
      contextUsage: {},
      rateLimits: {},
    });
    useComposerStore.setState({
      drafts: {},
      branchOrigin: null,
      newSessionWorkdir: null,
      workdirDialogOpen: false,
    });
  });

  it('fills the context bar proportional to the focused session used_percentage', async () => {
    // Seed a second, unfocused session too: the bar must read the FOCUSED
    // session's key (62), not just whatever entry is present, so an off-by-key
    // selector bug would surface here rather than passing on a single entry.
    useLiveStore.setState({
      contextUsage: { [SESSION_ID]: 62, 'other-session': 9 },
    });

    renderPane();

    const card = await screen.findByTestId('composer-card');
    const fill = within(card).getByTestId('composer-context-fill');
    expect(fill).toHaveStyle({ width: '62%' });
    expect(fill).toHaveAttribute('aria-valuenow', '62');
    // The fill is right-anchored along the card's top edge (it grows leftward
    // from the right so its tip stays next to the `%` readout).
    expect(fill.className).toContain('right-0');
    expect(fill.className).toContain('rounded-tr-md');
    expect(within(card).getByTestId('composer-context-bar')).toHaveTextContent(
      '62%',
    );
  });

  it('exposes a focusable label with an always-present help popover', async () => {
    useLiveStore.setState({ contextUsage: { [SESSION_ID]: 62 } });

    renderPane();

    const card = await screen.findByTestId('composer-card');
    // The `%` readout is a focusable, help-cursored span (it reveals the
    // popover on hover/focus rather than via a native `title` tooltip).
    const label = within(card).getByTestId('composer-context-label');
    expect(label).toHaveTextContent('62%');
    expect(label).toHaveAttribute('tabindex', '0');
    expect(label.className).toContain('cursor-help');
    // The old native title tooltip is gone.
    expect(label).not.toHaveAttribute('title');

    // The custom popover is always in the DOM (CSS-toggled on hover/focus), so
    // it is structurally assertable without simulating a hover.
    const popover = within(card).getByTestId('composer-context-popover');
    expect(popover).toHaveAttribute('role', 'note');
    expect(popover).toHaveTextContent('Context window usage');
  });

  it('omits the fill when the focused session has no context usage', async () => {
    // No `contextUsage` entry for the focused session (e.g. no snapshot yet, or
    // null right after /compact).
    renderPane();

    const card = await screen.findByTestId('composer-card');
    expect(
      within(card).queryByTestId('composer-context-fill'),
    ).not.toBeInTheDocument();
    expect(
      within(card).queryByTestId('composer-context-bar'),
    ).not.toBeInTheDocument();
    // With no usage the whole bar is omitted, so neither the label nor its
    // popover is in the DOM.
    expect(
      within(card).queryByTestId('composer-context-label'),
    ).not.toBeInTheDocument();
    expect(
      within(card).queryByTestId('composer-context-popover'),
    ).not.toBeInTheDocument();
  });

  // A timeline-initiated cross-lane jump records a navigation intent
  // (`activeThreadJumpTarget`) atomically with the active-thread switch. The
  // pane must then land the transcript on the jump target rather than snapping
  // to the newly focused lane's tail — and the jump's own bottom-clamped
  // programmatic scroll must NOT re-arm the stick-to-bottom follow. A plain
  // (navigator / chip / breadcrumb) switch carries no intent and keeps the
  // usual stick-to-bottom jump + armed-stick behavior.
  describe('timeline-jump navigation intent', () => {
    let observeCounts: Map<Element, number>;
    let observations: Map<Element, ResizeObserverCallback>;
    let originalRO: typeof ResizeObserver;

    beforeEach(() => {
      observeCounts = new Map();
      observations = new Map();
      originalRO = globalThis.ResizeObserver;
      class ControllableRO implements ResizeObserver {
        private observedEls = new Set<Element>();
        constructor(private cb: ResizeObserverCallback) {}
        observe(el: Element): void {
          observations.set(el, this.cb);
          observeCounts.set(el, (observeCounts.get(el) ?? 0) + 1);
          this.observedEls.add(el);
        }
        unobserve(el: Element): void {
          observations.delete(el);
          this.observedEls.delete(el);
        }
        disconnect(): void {
          for (const el of this.observedEls) {
            observations.delete(el);
          }
          this.observedEls.clear();
        }
      }
      globalThis.ResizeObserver =
        ControllableRO as unknown as typeof ResizeObserver;
    });

    afterEach(() => {
      globalThis.ResizeObserver = originalRO;
    });

    /** Render the pane and expose a `switchTo(id)` that re-renders with a new
     *  active thread — the workspace's reaction to an active-thread change. */
    function renderTranscript(activeThreadId: number) {
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const client = new ApiClient({ baseUrl: 'http://localhost' });
      const tree = (id: number) => (
        <QueryClientProvider client={queryClient}>
          <ApiProvider client={client}>
            <TranscriptPane
              threads={mockThreads}
              activeThread={mockThreads.find((t) => t.id === id)!}
              readOnly={false}
            />
          </ApiProvider>
        </QueryClientProvider>
      );
      const utils = render(tree(activeThreadId));
      return { ...utils, switchTo: (id: number) => utils.rerender(tree(id)) };
    }

    /** The Panel scroll body, primed with a scrollable geometry jsdom omits. */
    function primeBody(): HTMLElement {
      const body = document.querySelector('.scrollbar-hover') as HTMLElement;
      Object.defineProperty(body, 'scrollHeight', {
        configurable: true,
        get: () => 1000,
      });
      Object.defineProperty(body, 'clientHeight', {
        configurable: true,
        get: () => 400,
      });
      return body;
    }

    it('does not jump to the tail (and leaves stick disarmed) on a switch carrying a jump intent', async () => {
      const { switchTo } = renderTranscript(MAIN_THREAD_ID);
      await screen.findByTestId('bottom-overlay');
      const body = primeBody();
      // The user was reading scrollback, well above the tail (a real scroll so
      // the source lane's own stick is disarmed and a late message load cannot
      // re-glue it to the tail before we switch).
      body.scrollTop = 200;
      fireEvent.scroll(body);

      // The overlay's cross-lane jump: record the intent atomically with the
      // switch, then the workspace re-renders the pane onto the target lane.
      act(() => {
        useNavStore.setState({
          activeThreadId: BRANCH_THREAD_ID,
          activeThreadJumpTarget: {
            threadId: BRANCH_THREAD_ID,
            targetUuid: 'uuid-b2',
          },
        });
        switchTo(BRANCH_THREAD_ID);
      });

      // The thread-change effect skipped its `scrollTop = scrollHeight` jump:
      // the pane stays where the overlay's own scheduleScrollAfterRender will
      // place it, not glued to the tail.
      expect(body.scrollTop).toBe(200);
    });

    it('still jumps to the tail and arms stick on a plain switch with no jump intent', async () => {
      const { switchTo } = renderTranscript(MAIN_THREAD_ID);
      await screen.findByTestId('bottom-overlay');
      const body = primeBody();
      body.scrollTop = 200;
      fireEvent.scroll(body);

      // A navigator / chip switch: setActiveThread carries no intent (and
      // clears any lingering one).
      act(() => {
        useNavStore.getState().setActiveThread(BRANCH_THREAD_ID);
        switchTo(BRANCH_THREAD_ID);
      });

      // The pinned behavior: open the newly focused thread at its tail.
      expect(body.scrollTop).toBe(1000);
    });

    it('does not re-arm stick when the jump landing clamps at the container bottom', async () => {
      const { switchTo } = renderTranscript(MAIN_THREAD_ID);
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(observations.get(overlay)).toBeDefined());
      const body = primeBody();
      body.scrollTop = 200;
      fireEvent.scroll(body);

      act(() => {
        useNavStore.setState({
          activeThreadId: BRANCH_THREAD_ID,
          activeThreadJumpTarget: {
            threadId: BRANCH_THREAD_ID,
            targetUuid: 'uuid-b2',
          },
        });
        switchTo(BRANCH_THREAD_ID);
      });
      // Jump-to-tail skipped: stick is disarmed.
      expect(body.scrollTop).toBe(200);

      // The landing `scrollIntoView({ block: 'start' })` on a near-tail target
      // clamps at the bottom and fires a scroll event within the stick
      // threshold. While the intent is live this must NOT re-arm stick.
      body.scrollTop = 990;
      fireEvent.scroll(body);

      // A subsequent content-growth render (modelled by the overlay's
      // re-measure, which re-sticks the body only when stick is armed) leaves
      // scrollTop unchanged — the pane is not glued to the tail.
      overlay.getBoundingClientRect = () => ({ height: 100 }) as DOMRect;
      act(() => {
        const fire = observations.get(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });
      expect(body.scrollTop).toBe(990);

      // Once the jump is consumed, a genuine user scroll near the bottom
      // re-arms stick normally.
      act(() => useNavStore.getState().clearActiveThreadJumpTarget());
      body.scrollTop = 990;
      fireEvent.scroll(body);
      act(() => {
        const fire = observations.get(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });
      expect(body.scrollTop).toBe(1000);
    });

    it('binds the bottom-overlay measure effect once and does not re-run it on every unrelated render', async () => {
      const { switchTo } = renderTranscript(MAIN_THREAD_ID);
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(observeCounts.get(overlay)).toBeDefined());
      const initialObserveCount = observeCounts.get(overlay)!;

      // Re-render several times without changing whether the overlay is
      // mounted. The measure effect is keyed on the overlay's PRESENCE, not on
      // `bottomContent`'s per-render JSX identity, so it must not tear down and
      // re-observe on these renders.
      act(() => switchTo(MAIN_THREAD_ID));
      act(() => switchTo(MAIN_THREAD_ID));
      act(() => switchTo(MAIN_THREAD_ID));

      expect(observeCounts.get(overlay)).toBe(initialObserveCount);
    });
  });
});
