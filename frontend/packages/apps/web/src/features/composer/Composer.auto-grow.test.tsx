/**
 * The textarea auto-grow.
 *
 * Shared fixtures live in Composer.testkit.tsx.
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
import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import { MAIN_THREAD_ID } from '@delta/api-mocks';
import { useComposerStore } from '../../store/composerStore';
import { COMPOSER_MAX_HEIGHT, COMPOSER_MIN_HEIGHT } from './autoGrow';
import { renderComposer, resetComposerState, server } from './Composer.testkit';

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }));
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

describe('Composer', () => {
  beforeEach(resetComposerState);

  it('wires the auto-grow effect: an inline height and overflow style are applied', async () => {
    // jsdom performs no layout, so `scrollHeight` is 0 and the clamp resolves to
    // the min height; we cannot assert real growth here (covered by the
    // autoGrow.test.ts unit test and the stubbed-scrollHeight tests below). What
    // we CAN assert is that the effect ran and drove the textarea's inline
    // geometry — the wiring that makes it auto-grow in a real browser. The
    // measurement is coalesced into an animation frame, so wait for it.
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: 'one\ntwo\nthree' } });
    // The effect set an explicit pixel height and toggled the internal scrollbar.
    await waitFor(() => expect(textarea.style.height).toMatch(/px$/));
    // Below the cap (0 scrollHeight in jsdom) the bar stays hidden.
    expect(textarea.style.overflowY).toBe('hidden');
    // The manual resize handle is gone (auto-grow replaces it).
    expect(textarea.className).toContain('resize-none');
    expect(textarea.className).not.toContain('resize-y');
  });

  it('resets the textarea height after a submit clears the draft', async () => {
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: 'plain message' } });
    expect(textarea.value).toBe('plain message');

    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    // Submit clears the draft; the controlled value empties and the auto-grow
    // effect re-runs (coalesced into a frame), leaving the textarea reset to its
    // (min) height with the scrollbar hidden.
    await waitFor(() => expect(textarea.value).toBe(''));
    await waitFor(() => expect(textarea.style.height).toMatch(/px$/));
    expect(textarea.style.overflowY).toBe('hidden');
  });

  // jsdom performs no layout, so `scrollHeight` is always 0 there; model a real
  // browser's content-height report by deriving it from the current value's line
  // count (each line ~20px over an 8px base). This lets the JS-driven autosize —
  // the part that stays in JS after the perf change — be exercised end to end:
  // it reads `scrollHeight` inside its coalesced frame, clamps via
  // `autoGrowGeometry`, and writes the inline height + overflow.
  function stubScrollHeight(textarea: HTMLTextAreaElement): void {
    const base = 8;
    const perLine = 20;
    Object.defineProperty(textarea, 'scrollHeight', {
      configurable: true,
      get(this: HTMLTextAreaElement) {
        const lines = (this.value.match(/\n/g)?.length ?? 0) + 1;
        return base + lines * perLine;
      },
    });
  }

  it('grows the textarea as multi-line content is typed', async () => {
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    stubScrollHeight(textarea);

    // One short line stays at the min height (content shorter than the min).
    fireEvent.change(textarea, { target: { value: 'one' } });
    await waitFor(() =>
      expect(textarea.style.height).toBe(`${COMPOSER_MIN_HEIGHT}px`),
    );

    // Three lines (8 + 3*20 = 68px) grow past the min, below the cap.
    fireEvent.change(textarea, { target: { value: 'one\ntwo\nthree' } });
    await waitFor(() => expect(textarea.style.height).toBe('68px'));
    expect(textarea.style.overflowY).toBe('hidden');
  });

  it('shrinks the textarea back when content is deleted', async () => {
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    stubScrollHeight(textarea);

    fireEvent.change(textarea, { target: { value: 'a\nb\nc\nd\ne' } });
    await waitFor(() => expect(textarea.style.height).toBe('108px')); // 8 + 5*20

    // Deleting back down to two lines shrinks the height again (auto-grow resets
    // to `auto` before measuring, so it never sticks at the taller value).
    fireEvent.change(textarea, { target: { value: 'a\nb' } });
    await waitFor(() => expect(textarea.style.height).toBe('48px')); // 8 + 2*20
    expect(textarea.style.overflowY).toBe('hidden');
  });

  it('clamps at the max height and switches to internal scrolling', async () => {
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    stubScrollHeight(textarea);

    // Ten lines (8 + 10*20 = 208px) exceed the 160px cap: the height is clamped
    // and the internal scrollbar is enabled.
    fireEvent.change(textarea, {
      target: { value: Array.from({ length: 10 }, (_, i) => `l${i}`).join('\n') },
    });
    await waitFor(() =>
      expect(textarea.style.height).toBe(`${COMPOSER_MAX_HEIGHT}px`),
    );
    expect(textarea.style.overflowY).toBe('auto');
  });

  it('resizes after a programmatic draft change (quote insertion / thread switch)', async () => {
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    stubScrollHeight(textarea);

    // A programmatic draft change — quote insertion or draft restore on a thread
    // switch — mutates the store rather than firing an input event. The textarea
    // must still resize to fit the injected content.
    act(() => {
      useComposerStore.setState({
        drafts: { [MAIN_THREAD_ID]: 'quoted\nline\nhere' },
      });
    });
    await waitFor(() => expect(textarea.value).toBe('quoted\nline\nhere'));
    await waitFor(() => expect(textarea.style.height).toBe('68px')); // 8 + 3*20
  });
});
