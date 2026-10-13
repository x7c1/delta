/**
 * The dynamic bottom reserve: the body's bottom padding follows the measured
 * height of the composer overlay.
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
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MAIN_THREAD_ID, mockThreads } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import { ApiProvider } from '../../data/apiContext';
import { useComposerStore } from '../../store/composerStore';
import { TranscriptPane } from './TranscriptPane';
import {
  bodyEl,
  installControllableResizeObserver,
  lookupObserver,
  renderPane,
  resetTranscriptPaneState,
  restoreResizeObserver,
  server,
} from './TranscriptPane.testkit';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('TranscriptPane', () => {
  beforeEach(resetTranscriptPaneState);

  describe('dynamic bottom reserve (composer auto-grow follow)', () => {
    beforeEach(installControllableResizeObserver);
    afterEach(restoreResizeObserver);

    it('creates a ResizeObserver for the bottom overlay and drives padding-bottom from its measured height', async () => {
      renderPane();
      await waitFor(() =>
        expect(screen.getByTestId('bottom-overlay')).toBeInTheDocument(),
      );
      const overlay = screen.getByTestId('bottom-overlay');

      // The overlay-measuring observer is watching the overlay node itself.
      await waitFor(() => expect(lookupObserver(overlay)).not.toBeNull());
      const fire = lookupObserver(overlay)!;

      // Stub the overlay's measured height, then fire the observer as a real
      // resize would. The body's padding-bottom = measured height + the overlay
      // inset gap (12px fallback in jsdom, which computes no custom-property) +
      // the 192px reading gap that keeps the last turn off the composer.
      overlay.getBoundingClientRect = () =>
        ({ height: 120 }) as DOMRect;
      act(() => fire([], fire as unknown as ResizeObserver));

      await waitFor(() =>
        expect(bodyEl().style.paddingBottom).toBe('324px'),
      );
    });

    it('grows the reserve when the overlay grows (composer auto-grow), keeping the tail above it', async () => {
      renderPane();
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(lookupObserver(overlay)).not.toBeNull());

      overlay.getBoundingClientRect = () => ({ height: 80 }) as DOMRect;
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });
      await waitFor(() => expect(bodyEl().style.paddingBottom).toBe('284px'));

      // The composer grows (more lines typed): the overlay is taller, so the
      // reserve grows in lockstep — the last turn stays clear of the input.
      overlay.getBoundingClientRect = () => ({ height: 200 }) as DOMRect;
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });
      await waitFor(() => expect(bodyEl().style.paddingBottom).toBe('404px'));
    });

    it('re-sticks the body to the bottom when the overlay grows while sticking', async () => {
      renderPane();
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(lookupObserver(overlay)).not.toBeNull());

      // Make the body look scrollable and pinned at the bottom (sticking). jsdom
      // reports 0 for layout, so define the scroll geometry by hand.
      const body = bodyEl();
      Object.defineProperty(body, 'scrollHeight', {
        configurable: true,
        get: () => 1000,
      });
      Object.defineProperty(body, 'clientHeight', {
        configurable: true,
        get: () => 400,
      });
      // Start pinned to the bottom so stickRef stays true.
      body.scrollTop = 600;
      fireEvent.scroll(body);

      overlay.getBoundingClientRect = () => ({ height: 150 }) as DOMRect;
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });

      // The reserve grew (overlay 150 + 12 inset); the measurement re-stuck the
      // body to the new bottom (scrollTop := scrollHeight) so the tail stays
      // visible just above the grown composer.
      await waitFor(() =>
        expect(body.style.paddingBottom).toBe('354px'),
      );
      expect(body.scrollTop).toBe(1000);
    });

    it('does not move the body when the user has scrolled up (not sticking)', async () => {
      renderPane();
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(lookupObserver(overlay)).not.toBeNull());

      const body = bodyEl();
      Object.defineProperty(body, 'scrollHeight', {
        configurable: true,
        get: () => 1000,
      });
      Object.defineProperty(body, 'clientHeight', {
        configurable: true,
        get: () => 400,
      });
      // Scrolled well up: far from the bottom, so stickRef goes false.
      body.scrollTop = 100;
      fireEvent.scroll(body);

      overlay.getBoundingClientRect = () => ({ height: 150 }) as DOMRect;
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });

      // Reading scrollback is not yanked to the bottom; only the reserve updates.
      await waitFor(() =>
        expect(body.style.paddingBottom).toBe('354px'),
      );
      expect(body.scrollTop).toBe(100);
    });

    it('holds the reserve and skips the re-scroll while a branch is being composed', async () => {
      // Selecting text for "Branch from selected text" sets branchOrigin and
      // makes the overlay taller (the banner renders inside it). Folding that
      // into the reserve and re-scrolling would shift the transcript the instant
      // text is selected — moving the very selection the user is adjusting. So
      // while a branch is pending, the overlay's resize is ignored: the reserve
      // stays put and the body is not re-stuck to the bottom.
      renderPane();
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(lookupObserver(overlay)).not.toBeNull());

      // Establish a baseline reserve before any branch is pending.
      overlay.getBoundingClientRect = () => ({ height: 80 }) as DOMRect;
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });
      await waitFor(() => expect(bodyEl().style.paddingBottom).toBe('284px'));

      // Pin the body to the bottom (sticking) so a re-scroll would be visible.
      const body = bodyEl();
      Object.defineProperty(body, 'scrollHeight', {
        configurable: true,
        get: () => 1000,
      });
      Object.defineProperty(body, 'clientHeight', {
        configurable: true,
        get: () => 400,
      });
      body.scrollTop = 600;
      fireEvent.scroll(body);

      // A branch is now pending: the banner grows the overlay.
      act(() =>
        useComposerStore.setState({
          branchOrigin: {
            parentThreadId: MAIN_THREAD_ID,
            semanticParentUuid: 'm-user',
            locatorQuote: 'What is a delta?',
          },
        }),
      );
      overlay.getBoundingClientRect = () => ({ height: 200 }) as DOMRect;
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });

      // The grown banner does NOT grow the reserve and does NOT re-stick the
      // body: the banner floats over the transcript tail instead of pushing it.
      expect(body.style.paddingBottom).toBe('284px');
      expect(body.scrollTop).toBe(600);
    });

    it('applies a re-render that grows the overlay in that same commit, before the observer reports it', async () => {
      // Focusing a closed session adds the read-only notice above the composer
      // in the same commit that swaps the transcript. The observer would report
      // the taller overlay only at the next rendering step, and its update would
      // commit later still — so the reserve is re-measured after every commit,
      // and lands with the change that caused it.
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const client = new ApiClient({ baseUrl: 'http://localhost' });
      const active = mockThreads.find((t) => t.id === MAIN_THREAD_ID)!;
      const tree = (readOnly: boolean) => (
        <QueryClientProvider client={queryClient}>
          <ApiProvider client={client}>
            <TranscriptPane
              threads={mockThreads}
              activeThread={active}
              readOnly={readOnly}
            />
          </ApiProvider>
        </QueryClientProvider>
      );
      const { rerender } = render(tree(false));
      const overlay = await screen.findByTestId('bottom-overlay');
      await waitFor(() => expect(lookupObserver(overlay)).not.toBeNull());
      overlay.getBoundingClientRect = () => ({ height: 80 }) as DOMRect;
      act(() => {
        const fire = lookupObserver(overlay)!;
        fire([], fire as unknown as ResizeObserver);
      });
      await waitFor(() => expect(bodyEl().style.paddingBottom).toBe('284px'));

      // The notice makes the overlay taller; the observer is NOT fired.
      overlay.getBoundingClientRect = () => ({ height: 136 }) as DOMRect;
      act(() => rerender(tree(true)));

      expect(screen.getByTestId('readonly-notice')).toBeInTheDocument();
      expect(bodyEl().style.paddingBottom).toBe('340px');
    });
  });
});
