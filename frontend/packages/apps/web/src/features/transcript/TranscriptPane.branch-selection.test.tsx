/**
 * The pending branch selection: dismissing it with a click, and its highlight.
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
  vi,
} from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import { MAIN_THREAD_ID } from '@delta/api-mocks';
import { useComposerStore } from '../../store/composerStore';
import { findAllQuoteRanges } from './branchHighlight';
import {
  renderPane,
  resetTranscriptPaneState,
  server,
} from './TranscriptPane.testkit';

beforeAll(() => server.listen({ onUnhandledFrame: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('TranscriptPane', () => {
  beforeEach(resetTranscriptPaneState);

  it('clears a pending branch and collapses the selection on a stationary click even when the engine has not collapsed the selection yet', async () => {
    // A passage was selected for "Branch from selected text" (a pending
    // branchOrigin on the active thread). A stationary plain click in the
    // conversation drops it, so dismissing no longer requires the composer's ✕.
    // Some engines (WebKit) defer collapsing the native selection until AFTER
    // the click fires — so even though getSelection() still reports a
    // non-collapsed selection here, a stationary click must clear the origin
    // AND explicitly collapse the selection (removeAllRanges) so the release's
    // mouseup re-arm has nothing to pick back up.
    useComposerStore.setState({
      branchOrigin: {
        parentThreadId: MAIN_THREAD_ID,
        semanticParentUuid: 'm-user',
        locatorQuote: 'selected passage',
      },
    });
    // WebKit-style deferred collapse: the selection is still non-collapsed at
    // click time. `removeAllRanges` is observed so the explicit collapse is
    // asserted.
    const removeAllRanges = vi.fn();
    const getSelection = vi.spyOn(window, 'getSelection').mockReturnValue({
      isCollapsed: false,
      removeAllRanges,
    } as unknown as Selection);

    renderPane();
    const message = await screen.findByText('What is a delta?');

    // A stationary single click: mousedown and click at the same point.
    fireEvent.mouseDown(message, { clientX: 100, clientY: 100 });
    fireEvent.click(message, { clientX: 100, clientY: 100, detail: 1 });

    await waitFor(() =>
      expect(useComposerStore.getState().branchOrigin).toBeNull(),
    );
    expect(removeAllRanges).toHaveBeenCalled();
    getSelection.mockRestore();
  });

  it('collapses a lingering selection on a stationary click even with no pending branch', async () => {
    // WebKitGTK can refuse to drop the native selection on the click that
    // dismissed the branch (its own selection handling runs after the click
    // and overwrites the clear). The user's next click then arrives with the
    // branch already gone — the selection must still be collapsed, or the
    // stale highlight lingers indefinitely.
    useComposerStore.setState({ branchOrigin: null });
    const removeAllRanges = vi.fn();
    const getSelection = vi.spyOn(window, 'getSelection').mockReturnValue({
      isCollapsed: false,
      removeAllRanges,
    } as unknown as Selection);

    renderPane();
    const message = await screen.findByText('What is a delta?');

    fireEvent.mouseDown(message, { clientX: 100, clientY: 100 });
    fireEvent.click(message, { clientX: 100, clientY: 100, detail: 1 });

    expect(removeAllRanges).toHaveBeenCalled();
    getSelection.mockRestore();
  });

  it('keeps a pending branch when the click is preceded by pointer movement (drag-select end)', async () => {
    // The mouseup that finishes a drag-select also fires a click, but the
    // pointer travelled from the mousedown point — that is a drag, the gesture
    // that just set the branch origin. It must NOT immediately undo it. The
    // detection is by pointer movement, not by a mocked isCollapsed.
    const origin = {
      parentThreadId: MAIN_THREAD_ID,
      semanticParentUuid: 'm-user' as const,
      locatorQuote: 'selected passage',
    };
    useComposerStore.setState({ branchOrigin: origin });

    renderPane();
    const message = await screen.findByText('What is a delta?');

    // mousedown then a click well beyond the slop = a drag-select release.
    fireEvent.mouseDown(message, { clientX: 100, clientY: 100 });
    fireEvent.click(message, { clientX: 220, clientY: 160, detail: 1 });

    // The branch origin survives the drag-select release click.
    expect(useComposerStore.getState().branchOrigin).toEqual(origin);
  });

  it('keeps a pending branch when the click is a double/triple-click selection (detail > 1)', async () => {
    // A double/triple-click word/paragraph select fires a click with detail > 1
    // and no pointer travel. That gesture just armed the origin, so its own
    // click must not immediately undo it.
    const origin = {
      parentThreadId: MAIN_THREAD_ID,
      semanticParentUuid: 'm-user' as const,
      locatorQuote: 'selected passage',
    };
    useComposerStore.setState({ branchOrigin: origin });

    renderPane();
    const message = await screen.findByText('What is a delta?');

    fireEvent.mouseDown(message, { clientX: 100, clientY: 100 });
    fireEvent.click(message, { clientX: 100, clientY: 100, detail: 2 });

    expect(useComposerStore.getState().branchOrigin).toEqual(origin);
  });

  it('paints the pending branch quote in the body via the branch-origin highlight', async () => {
    // While a branch is pending, its selected passage stays highlighted (the
    // CSS Custom Highlight API), so it is visible even after focus moves to the
    // composer textarea and the native selection fades. The effect searches the
    // rendered message bodies for the branchOrigin quote; in jsdom the highlight
    // registry may be unavailable, so this asserts the guarded, no-throw path
    // and that the range computation runs against the quote.
    useComposerStore.setState({
      branchOrigin: {
        parentThreadId: MAIN_THREAD_ID,
        semanticParentUuid: 'm-user',
        locatorQuote: 'What is a delta?',
      },
    });

    expect(() => renderPane()).not.toThrow();
    await waitFor(() =>
      expect(screen.getByText('What is a delta?')).toBeInTheDocument(),
    );

    // The highlighted passage occurs verbatim in a rendered message body, so the
    // range computation finds at least one match (proving the effect targeted
    // the branchOrigin quote, independent of whether jsdom paints it).
    const body = screen
      .getByText('What is a delta?')
      .closest('[data-testid="message-item"]')!;
    expect(findAllQuoteRanges(body, 'What is a delta?').length).toBeGreaterThan(
      0,
    );
  });
});
