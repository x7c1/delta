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
import { MAIN_THREAD_ID, createHandlers, mockThreads } from '@delta/api-mocks';
import { ApiClient } from '@delta/api-client';
import type { Thread } from '@delta/wire-gen';
import { ApiProvider } from '../../data/apiContext';
import { useComposerStore } from '../../store/composerStore';
import { useLiveStore } from '../../store/liveStore';
import { useNavStore } from '../../store/navStore';
import { useSettingsStore } from '../../store/settingsStore';
import { Composer, composerDraftKey, type ComposerMode } from './Composer';
import { ComposerDraftTargetProvider } from './composerDraftTarget';
import { ComposerRail } from './ComposerRail';
import {
  HOVER_CLOSE_DELAY_MS,
  HOVER_OPEN_DELAY_MS,
  PromptTemplateButton,
} from './PromptTemplateButton';
import { ProviderTabs } from './ProviderTabs';

const server = setupServer(...createHandlers());

/**
 * Every request the app fired, as `METHOD /path`. Insertion is a purely local
 * edit, so the specs below assert against this log that choosing a template
 * costs exactly one GET and never a send.
 */
const requestLog: string[] = [];

beforeAll(() => {
  server.listen({ onUnhandledFrame: 'error' });
  server.events.on('request:start', ({ request }) => {
    requestLog.push(`${request.method} ${new URL(request.url).pathname}`);
  });
});
afterEach(() => server.resetHandlers());
afterAll(() => server.close());

const mainThread = mockThreads.find((t) => t.id === MAIN_THREAD_ID) as Thread;

/** The seeded fixture bodies, as the mock server serves them. */
const SHORT_TEMPLATE_TEXT =
  'Once CI is green, merge the PR and then update the plan doc.';
const MULTILINE_TEMPLATE_FIRST_LINE =
  "Review the diff on this branch with a critic's eye.";

const THREAD_MODE: ComposerMode = {
  kind: 'thread',
  activeThread: mainThread,
  readOnly: false,
  spawning: false,
};

/**
 * The composer the way {@link TranscriptPane} assembles it: the rail (with the
 * template button in its left slot) above the card, both inside the provider
 * that shares the textarea between them. Rendering the real pair is the point —
 * the whole feature is a control OUTSIDE the card editing the draft INSIDE it.
 */
function renderComposer(mode: ComposerMode = THREAD_MODE) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const client = new ApiClient({ baseUrl: 'http://localhost' });
  return render(
    <QueryClientProvider client={queryClient}>
      <ApiProvider client={client}>
        <ComposerDraftTargetProvider draftKey={composerDraftKey(mode)}>
          <ComposerRail
            templateSlot={<PromptTemplateButton />}
            providerTabs={mode.kind === 'new-session' ? <ProviderTabs /> : null}
          />
          <Composer mode={mode} />
        </ComposerDraftTargetProvider>
      </ApiProvider>
    </QueryClientProvider>,
  );
}

/** Serve an empty registry, standing in for a user who has registered none. */
function useEmptyRegistry() {
  server.use(
    http.get('*/api/prompt-templates', () =>
      HttpResponse.json({ prompt_templates: [] }),
    ),
  );
}

/** Open the popover and wait for the seeded list to land. */
async function openPopover(): Promise<HTMLElement> {
  fireEvent.click(screen.getByTestId('prompt-templates-button'));
  await screen.findByTestId('prompt-template-option-1');
  return screen.getByTestId('prompt-templates-popover');
}

/** Put the caret at `offset` of the textarea, the way a click into it would. */
function placeCaret(textarea: HTMLTextAreaElement, offset: number) {
  textarea.focus();
  textarea.setSelectionRange(offset, offset);
}

/**
 * Back to a freshly loaded app: no draft, no session activity, Settings closed
 * on its default category, and an empty request log for the traffic assertions.
 */
function resetStores() {
  requestLog.length = 0;
  useNavStore.setState({ activeThreadId: MAIN_THREAD_ID, settingsOpen: false });
  useSettingsStore.setState({ activeCategory: 'launch-options' });
  useLiveStore.setState({
    sending: [],
    localSends: {},
    spawns: [],
    notices: {},
    unread: {},
  });
  useComposerStore.setState({
    drafts: {},
    branchOrigin: null,
    newSessionWorkdir: null,
    newSessionLaunchOptionIds: [],
    newSessionWorktreeEnabled: false,
    newSessionWorktreeStartPoint: { kind: 'head' },
    newSessionProvider: 'claude',
    newSessionProviderSeeded: false,
  });
}

describe('PromptTemplateButton', () => {
  beforeEach(resetStores);

  it('sits leftmost on the rail in a thread, where it is the only item', () => {
    renderComposer();

    const rail = screen.getByTestId('composer-rail');
    const button = screen.getByTestId('prompt-templates-button');
    expect(rail).toContainElement(button);
    expect(rail.firstElementChild).toContainElement(button);
    // A thread has no provider tabs, so the button alone is what keeps the rail
    // from collapsing to zero height.
    expect(rail.children).toHaveLength(1);
  });

  it('sits before the provider tabs on the new-session rail', () => {
    renderComposer({ kind: 'new-session' });

    const rail = screen.getByTestId('composer-rail');
    const button = screen.getByTestId('prompt-templates-button');
    const tabs = screen.getByTestId('provider-selector');
    expect(rail.firstElementChild).toContainElement(button);
    expect(
      button.compareDocumentPosition(tabs) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it('renders even when the registry is empty, and offers the way to fill it', async () => {
    useEmptyRegistry();
    renderComposer();

    // The button is the only discoverable entry point, so it must never depend
    // on there being something to insert.
    const button = screen.getByTestId('prompt-templates-button');
    expect(button).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(button);

    expect(
      await screen.findByTestId('prompt-templates-popover-empty'),
    ).toHaveTextContent('No prompt templates yet.');
    expect(screen.getByTestId('prompt-templates-manage')).toBeInTheDocument();
    expect(
      screen.queryByTestId('prompt-templates-popover-list'),
    ).not.toBeInTheDocument();
  });

  it('opens on a spinner while the list is still in flight', async () => {
    // The panel appears on the click, not on the response — and while the
    // request is out it shows the spinner rather than an empty registry (the
    // two are a very different message) with the footer already usable. Focus
    // waits for a row to exist, which is why the focus effect is gated on the
    // query rather than on the open state.
    let resolveList: () => void = () => {};
    const listGate = new Promise<void>((resolve) => {
      resolveList = resolve;
    });
    server.use(
      http.get('*/api/prompt-templates', async () => {
        await listGate;
        return HttpResponse.json({
          prompt_templates: [
            {
              id: 1,
              label: 'Merge when green',
              text: SHORT_TEMPLATE_TEXT,
              created_at: '2026-01-01T00:00:00Z',
              updated_at: '2026-01-01T00:00:00Z',
            },
          ],
        });
      }),
    );
    renderComposer();

    fireEvent.click(screen.getByTestId('prompt-templates-button'));

    const popover = await screen.findByTestId('prompt-templates-popover');
    expect(within(popover).getByRole('status')).toHaveTextContent(
      'loading prompt templates',
    );
    expect(
      within(popover).queryByTestId('prompt-templates-popover-list'),
    ).not.toBeInTheDocument();
    expect(
      within(popover).queryByTestId('prompt-templates-popover-empty'),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId('prompt-templates-manage')).toBeInTheDocument();

    resolveList();

    expect(await screen.findByTestId('prompt-template-option-1')).toHaveFocus();
    expect(within(popover).queryByRole('status')).not.toBeInTheDocument();
  });

  it('opens a label-only list with the first item focused and previewed in full', async () => {
    renderComposer();

    const popover = await openPopover();
    expect(screen.getByTestId('prompt-templates-button')).toHaveAttribute(
      'aria-expanded',
      'true',
    );
    expect(popover).toHaveAttribute('role', 'menu');

    // The list carries labels and nothing else: the multi-line fixture's body
    // is nowhere in it, however long the template runs.
    const list = within(popover).getByTestId('prompt-templates-popover-list');
    expect(within(list).getAllByRole('menuitem').map((i) => i.textContent)).toEqual([
      'Merge when green',
      'Review checklist',
    ]);
    expect(list).not.toHaveTextContent(MULTILINE_TEMPLATE_FIRST_LINE);
    expect(list).not.toHaveTextContent(SHORT_TEMPLATE_TEXT);

    // The first item takes focus, and the preview shows ITS body in full.
    expect(screen.getByTestId('prompt-template-option-1')).toHaveFocus();
    const preview = within(popover).getByTestId(
      'prompt-templates-popover-preview',
    );
    expect(preview).toHaveTextContent(SHORT_TEMPLATE_TEXT);
  });

  it('opens upward, anchored to the rail so it can never outgrow the card', async () => {
    renderComposer();

    const popover = await openPopover();

    // Upward, over the cards stacked above the composer: the composer sits at
    // the bottom of the screen, so a downward panel would fall off it.
    expect(popover.className).toMatch(/(^|\s)bottom-full(\s|$)/);

    // And its width cap is measured against the RAIL. `max-w-full` on an
    // absolutely positioned panel resolves against the nearest POSITIONED
    // ancestor, so that ancestor has to be the rail, which spans exactly the
    // composer card. The button's own box is a few pixels wide: anchored
    // there the cap would bound nothing, leaving only the viewport to hold the
    // panel in — and the viewport ignores the navigator and terminal panes
    // flanking this one, so a wide panel would spill out of the pane and under
    // them. jsdom lays nothing out, so the class chain is the only place this
    // invariant can be pinned; without it, moving `relative` back onto the
    // button's box would silently restore that overflow.
    expect(popover.className).toMatch(/(^|\s)absolute(\s|$)/);
    expect(popover.className).toMatch(/(^|\s)max-w-full(\s|$)/);
    let anchor: HTMLElement | null = popover.parentElement;
    while (
      anchor !== null &&
      !/(^|\s)(relative|absolute|fixed|sticky)(\s|$)/.test(anchor.className)
    ) {
      anchor = anchor.parentElement;
    }
    expect(anchor).toBe(screen.getByTestId('composer-rail'));
  });

  it('moves focus and the preview with the arrow keys', async () => {
    renderComposer();
    const popover = await openPopover();
    const preview = within(popover).getByTestId(
      'prompt-templates-popover-preview',
    );

    fireEvent.keyDown(screen.getByTestId('prompt-template-option-1'), {
      key: 'ArrowDown',
    });
    expect(screen.getByTestId('prompt-template-option-2')).toHaveFocus();
    // The whole multi-paragraph body is previewed — including the blank line
    // and the bulleted middle, which no list row could have shown.
    expect(preview.textContent).toContain(MULTILINE_TEMPLATE_FIRST_LINE);
    expect(preview.textContent).toContain('\n\nCheck, in order:');

    fireEvent.keyDown(screen.getByTestId('prompt-template-option-2'), {
      key: 'ArrowUp',
    });
    expect(screen.getByTestId('prompt-template-option-1')).toHaveFocus();
    expect(preview).toHaveTextContent(SHORT_TEMPLATE_TEXT);
  });

  it('closes on Escape and hands the caret back to the textarea', async () => {
    useComposerStore.getState().setDraft(MAIN_THREAD_ID, 'hello world');
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    placeCaret(textarea, 5);

    await openPopover();
    fireEvent.keyDown(document, { key: 'Escape' });

    await waitFor(() =>
      expect(
        screen.queryByTestId('prompt-templates-popover'),
      ).not.toBeInTheDocument(),
    );
    expect(textarea).toHaveFocus();
    expect(textarea.selectionStart).toBe(5);
    expect(useComposerStore.getState().drafts[MAIN_THREAD_ID]).toBe(
      'hello world',
    );
  });

  it('closes on a click outside and hands the caret back to the textarea', async () => {
    useComposerStore.getState().setDraft(MAIN_THREAD_ID, 'hello world');
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    placeCaret(textarea, 5);

    await openPopover();
    fireEvent.pointerDown(document.body);

    await waitFor(() =>
      expect(
        screen.queryByTestId('prompt-templates-popover'),
      ).not.toBeInTheDocument(),
    );
    expect(textarea).toHaveFocus();
    expect(textarea.selectionStart).toBe(5);
  });

  it('closes again when the trigger itself is clicked a second time', async () => {
    useComposerStore.getState().setDraft(MAIN_THREAD_ID, 'hello world');
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    placeCaret(textarea, 5);

    await openPopover();
    const button = screen.getByTestId('prompt-templates-button');
    // The real sequence: the press lands INSIDE the button's container, so the
    // dismiss-on-outside-press listener must leave the panel open and let the
    // click below be what toggles it. Were the press treated as an outside
    // one, the two would fight and the click would reopen what it just closed.
    fireEvent.pointerDown(button);
    fireEvent.click(button);

    await waitFor(() =>
      expect(
        screen.queryByTestId('prompt-templates-popover'),
      ).not.toBeInTheDocument(),
    );
    expect(button).toHaveAttribute('aria-expanded', 'false');
    expect(textarea).toHaveFocus();
    expect(textarea.selectionStart).toBe(5);
  });

  it('splices the chosen template into the draft at the caret and sends nothing', async () => {
    useComposerStore.getState().setDraft(MAIN_THREAD_ID, 'before after');
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    placeCaret(textarea, 'before '.length);

    await openPopover();
    fireEvent.click(screen.getByTestId('prompt-template-option-2'));

    await waitFor(() =>
      expect(
        screen.queryByTestId('prompt-templates-popover'),
      ).not.toBeInTheDocument(),
    );

    const draft = useComposerStore.getState().drafts[MAIN_THREAD_ID];
    expect(draft.startsWith('before ')).toBe(true);
    expect(draft.endsWith('after')).toBe(true);
    const inserted = draft.slice('before '.length, draft.length - 'after'.length);
    expect(inserted.startsWith(MULTILINE_TEMPLATE_FIRST_LINE)).toBe(true);
    // Verbatim: the body's own blank line survives and no separator was added
    // around it.
    expect(inserted).toContain('\n\nCheck, in order:');
    expect(draft).toBe(`before ${inserted}after`);

    // Focus and caret come back to the textarea, right after what was inserted,
    // so typing continues where the template ends.
    expect(textarea).toHaveFocus();
    expect(textarea.selectionStart).toBe('before '.length + inserted.length);
    expect(textarea.selectionEnd).toBe(textarea.selectionStart);

    // Nothing was sent: the only traffic is the list the popover read.
    expect(requestLog).toEqual(['GET /api/prompt-templates']);
  });

  it('replaces the selected text rather than inserting beside it', async () => {
    useComposerStore.getState().setDraft(MAIN_THREAD_ID, 'keep DROP keep');
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    textarea.focus();
    textarea.setSelectionRange(5, 9);

    await openPopover();
    fireEvent.click(screen.getByTestId('prompt-template-option-1'));

    await waitFor(() =>
      expect(useComposerStore.getState().drafts[MAIN_THREAD_ID]).toBe(
        `keep ${SHORT_TEMPLATE_TEXT} keep`,
      ),
    );
  });

  it('appends at the end when the textarea has never been focused', async () => {
    // A restored draft the user has not clicked into reports caret 0, which
    // must NOT be read as "insert at the start".
    useComposerStore.getState().setDraft(MAIN_THREAD_ID, 'restored draft');
    renderComposer();

    await openPopover();
    fireEvent.click(screen.getByTestId('prompt-template-option-1'));

    await waitFor(() =>
      expect(useComposerStore.getState().drafts[MAIN_THREAD_ID]).toBe(
        `restored draft${SHORT_TEMPLATE_TEXT}`,
      ),
    );
  });

  it('opens Settings on the prompt-templates category from the footer', async () => {
    renderComposer();
    await openPopover();

    fireEvent.click(screen.getByTestId('prompt-templates-manage'));

    await waitFor(() =>
      expect(
        screen.queryByTestId('prompt-templates-popover'),
      ).not.toBeInTheDocument(),
    );
    expect(useNavStore.getState().settingsOpen).toBe(true);
    expect(useSettingsStore.getState().activeCategory).toBe('prompt-templates');
  });

  it('offers the same footer from an empty registry', async () => {
    useEmptyRegistry();
    renderComposer();
    fireEvent.click(screen.getByTestId('prompt-templates-button'));

    const manage = await screen.findByTestId('prompt-templates-manage');
    fireEvent.click(manage);

    expect(useNavStore.getState().settingsOpen).toBe(true);
    expect(useSettingsStore.getState().activeCategory).toBe('prompt-templates');
  });

  it('reports a failed load without losing the way into the registry', async () => {
    server.use(
      http.get('*/api/prompt-templates', () =>
        HttpResponse.json({ error: 'boom' }, { status: 500 }),
      ),
    );
    renderComposer();
    fireEvent.click(screen.getByTestId('prompt-templates-button'));

    expect(
      await screen.findByTestId('prompt-templates-popover-error'),
    ).toBeInTheDocument();
    expect(screen.getByTestId('prompt-templates-manage')).toBeInTheDocument();
  });
});

describe('PromptTemplatePopover height', () => {
  beforeEach(resetStores);

  it('keeps one box whichever template is previewed', async () => {
    // One template of a single word, one of a hundred lines: were the height
    // the preview's, the upward-opening panel's top edge (and with it the
    // label column) would jump between the two hovers.
    const longText = Array.from(
      { length: 100 },
      (_, i) => `Line ${i + 1} of a very long template.`,
    ).join('\n');
    server.use(
      http.get('*/api/prompt-templates', () =>
        HttpResponse.json({
          prompt_templates: [
            {
              id: 1,
              label: 'Short',
              text: 'Go.',
              created_at: '2026-01-01T00:00:00Z',
              updated_at: '2026-01-01T00:00:00Z',
            },
            {
              id: 2,
              label: 'Long',
              text: longText,
              created_at: '2026-01-01T00:00:00Z',
              updated_at: '2026-01-01T00:00:00Z',
            },
          ],
        }),
      ),
    );
    renderComposer();
    const popover = await openPopover();
    const body = within(popover).getByTestId('prompt-templates-popover-body');
    const list = within(popover).getByTestId('prompt-templates-popover-list');
    const preview = within(popover).getByTestId(
      'prompt-templates-popover-preview',
    );
    const boxes = () =>
      [popover, body, list, preview].map((node) => node.className);

    fireEvent.mouseEnter(screen.getByTestId('prompt-template-option-1'));
    expect(preview).toHaveTextContent('Go.');
    const shortBoxes = boxes();

    fireEvent.mouseEnter(screen.getByTestId('prompt-template-option-2'));
    expect(preview).toHaveTextContent('Line 100 of a very long template.');
    expect(boxes()).toEqual(shortBoxes);

    // The height is a fixed one on the row both columns share, and neither
    // column nor the panel caps itself by its content (a `max-h` would let the
    // box follow the text up to the cap).
    expect(body.className).toMatch(/(^|\s)h-\[/);
    for (const className of boxes()) {
      expect(className).not.toMatch(/(^|\s)max-h-/);
    }
  });
});

describe('PromptTemplateButton on hover', () => {
  beforeEach(() => {
    resetStores();
    // Only the timer queue: the msw and React Query plumbing awaits promises,
    // which must keep resolving on their own.
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  /** Let `ms` of fake time pass, flushing the renders it triggers. */
  async function advance(ms: number) {
    await act(() => vi.advanceTimersByTimeAsync(ms));
  }

  /** Rest the pointer on the button until the popover opens, list loaded. */
  async function hoverOpen() {
    fireEvent.pointerEnter(screen.getByTestId('prompt-templates-button'));
    await advance(HOVER_OPEN_DELAY_MS);
    // A few more ticks for the list request to land.
    await advance(50);
    expect(screen.getByTestId('prompt-template-option-1')).toBeInTheDocument();
  }

  const popoverShown = () => screen.queryByTestId('prompt-templates-popover');

  it('opens after the intent delay and leaves focus in the textarea', async () => {
    useComposerStore.getState().setDraft(MAIN_THREAD_ID, 'hello world');
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    placeCaret(textarea, 5);

    fireEvent.pointerEnter(screen.getByTestId('prompt-templates-button'));
    await advance(HOVER_OPEN_DELAY_MS - 10);
    expect(popoverShown()).not.toBeInTheDocument();

    await advance(10);
    expect(popoverShown()).toBeInTheDocument();
    expect(screen.getByTestId('prompt-templates-button')).toHaveAttribute(
      'aria-expanded',
      'true',
    );

    // Even once the rows exist, the caret stays where the user was typing.
    await advance(50);
    expect(screen.getByTestId('prompt-template-option-1')).toBeInTheDocument();
    expect(textarea).toHaveFocus();
    expect(textarea.selectionStart).toBe(5);

    // The hovered row still drives the preview.
    fireEvent.mouseEnter(screen.getByTestId('prompt-template-option-2'));
    expect(
      screen.getByTestId('prompt-templates-popover-preview').textContent,
    ).toContain(MULTILINE_TEMPLATE_FIRST_LINE);
  });

  it('does not open when the pointer only passes over the button', async () => {
    renderComposer();
    const button = screen.getByTestId('prompt-templates-button');

    fireEvent.pointerEnter(button);
    await advance(HOVER_OPEN_DELAY_MS / 2);
    fireEvent.pointerLeave(button);
    await advance(HOVER_OPEN_DELAY_MS * 4);

    expect(popoverShown()).not.toBeInTheDocument();
  });

  it('closes after the grace delay once the pointer has left', async () => {
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    placeCaret(textarea, 0);
    await hoverOpen();

    fireEvent.pointerLeave(screen.getByTestId('prompt-templates-button'));
    await advance(HOVER_CLOSE_DELAY_MS - 10);
    expect(popoverShown()).toBeInTheDocument();

    await advance(10);
    expect(popoverShown()).not.toBeInTheDocument();
    expect(textarea).toHaveFocus();
  });

  it('stays open while the pointer travels from the button into the popover', async () => {
    renderComposer();
    await hoverOpen();

    // Out of the button into the gap above it, then into the popover. The gap
    // is outside the container, so this is a real leave and re-enter.
    fireEvent.pointerOut(screen.getByTestId('prompt-templates-button'), {
      relatedTarget: document.body,
    });
    await advance(HOVER_CLOSE_DELAY_MS / 2);
    fireEvent.pointerOver(screen.getByTestId('prompt-template-option-2'), {
      relatedTarget: document.body,
    });
    await advance(HOVER_CLOSE_DELAY_MS * 4);

    expect(popoverShown()).toBeInTheDocument();
  });

  it('inserts a clicked row at the caret as it stands at the click', async () => {
    useComposerStore.getState().setDraft(MAIN_THREAD_ID, 'after');
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    placeCaret(textarea, 0);
    await hoverOpen();

    // The textarea kept focus, so the user typed on after it opened; a caret
    // captured at open (offset 0) would be stale by now.
    fireEvent.change(textarea, { target: { value: 'before after' } });
    placeCaret(textarea, 'before '.length);
    expect(popoverShown()).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('prompt-template-option-1'));

    expect(popoverShown()).not.toBeInTheDocument();
    expect(useComposerStore.getState().drafts[MAIN_THREAD_ID]).toBe(
      `before ${SHORT_TEMPLATE_TEXT}after`,
    );
    expect(textarea).toHaveFocus();
    expect(textarea.selectionStart).toBe(
      'before '.length + SHORT_TEMPLATE_TEXT.length,
    );
  });

  it('pins on a click: focus moves in, and leaving no longer closes it', async () => {
    useComposerStore.getState().setDraft(MAIN_THREAD_ID, 'hello world');
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    placeCaret(textarea, 5);
    await hoverOpen();
    const button = screen.getByTestId('prompt-templates-button');

    fireEvent.pointerDown(button);
    fireEvent.click(button);
    await advance(0);

    expect(popoverShown()).toBeInTheDocument();
    expect(screen.getByTestId('prompt-template-option-1')).toHaveFocus();

    fireEvent.pointerLeave(button);
    await advance(HOVER_CLOSE_DELAY_MS * 4);
    expect(popoverShown()).toBeInTheDocument();

    // From here it is the click-opened popover: arrows and Escape as ever.
    fireEvent.keyDown(screen.getByTestId('prompt-template-option-1'), {
      key: 'ArrowDown',
    });
    expect(screen.getByTestId('prompt-template-option-2')).toHaveFocus();
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(popoverShown()).not.toBeInTheDocument();
    expect(textarea).toHaveFocus();
    expect(textarea.selectionStart).toBe(5);
  });

  it('closes on Escape and on a click outside, leaving focus alone', async () => {
    renderComposer();
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    placeCaret(textarea, 0);

    await hoverOpen();
    fireEvent.keyDown(textarea, { key: 'Escape' });
    expect(popoverShown()).not.toBeInTheDocument();
    expect(textarea).toHaveFocus();

    fireEvent.pointerLeave(screen.getByTestId('prompt-templates-button'));
    await hoverOpen();
    fireEvent.pointerDown(document.body);
    expect(popoverShown()).not.toBeInTheDocument();
  });

  it('ignores touch, where a tap stays a plain click', async () => {
    renderComposer();
    const button = screen.getByTestId('prompt-templates-button');

    fireEvent.pointerEnter(button, { pointerType: 'touch' });
    await advance(HOVER_OPEN_DELAY_MS * 4);
    expect(popoverShown()).not.toBeInTheDocument();

    fireEvent.click(button);
    await advance(50);
    expect(screen.getByTestId('prompt-template-option-1')).toHaveFocus();
  });
});

/**
 * Inserting is a local draft edit, so it must be blind to what the session is
 * doing. Each case below is one row of the operation × session-state matrix,
 * built out of the store state that actually distinguishes it:
 *
 * - **new-session** — no session exists yet; the draft keys off the sentinel.
 * - **open + idle** — a live thread, nothing in flight.
 * - **open + mid-turn** — a submit is in flight, so Send is disabled. The
 *   template button must not be.
 * - **closed** — the read-only "Send to resume this closed session…" composer.
 * - **resuming** — closed AND a submit in flight: the resume the backend is
 *   servicing, with further sends deferred behind it.
 * - **spawning** — the session is listed and focusable but its launch has not
 *   registered, so the server refuses a send and the composer offers none.
 */
describe('PromptTemplateButton across session states', () => {
  const inFlightSend = {
    id: 'local-sess-1-1',
    target: {
      kind: 'thread' as const,
      sessionId: mainThread.session_id,
      threadId: mainThread.id,
    },
    text: 'in flight',
    status: 'sending' as const,
    createdAt: 0,
  };

  const cases = [
    {
      name: 'new-session',
      mode: { kind: 'new-session' } as ComposerMode,
      sending: [],
    },
    { name: 'open + idle', mode: THREAD_MODE, sending: [] },
    { name: 'open + mid-turn', mode: THREAD_MODE, sending: [inFlightSend] },
    {
      name: 'closed',
      mode: {
        kind: 'thread',
        activeThread: mainThread,
        readOnly: true,
        spawning: false,
      } as ComposerMode,
      sending: [],
    },
    {
      name: 'resuming',
      mode: {
        kind: 'thread',
        activeThread: mainThread,
        readOnly: true,
        spawning: false,
      } as ComposerMode,
      sending: [inFlightSend],
    },
    {
      name: 'spawning',
      mode: {
        kind: 'thread',
        activeThread: mainThread,
        readOnly: true,
        spawning: true,
      } as ComposerMode,
      sending: [],
    },
  ];

  beforeEach(resetStores);

  for (const testCase of cases) {
    it(`inserts into the draft while ${testCase.name}`, async () => {
      useLiveStore.setState({
        sending: testCase.sending,
        localSends: {},
        spawns: [],
        notices: {},
        unread: {},
      });
      const draftKey = composerDraftKey(testCase.mode);
      useComposerStore.getState().setDraft(draftKey, 'ab');
      renderComposer(testCase.mode);

      const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
      placeCaret(textarea, 1);
      await openPopover();
      fireEvent.click(screen.getByTestId('prompt-template-option-1'));

      await waitFor(() =>
        expect(useComposerStore.getState().drafts[draftKey]).toBe(
          `a${SHORT_TEMPLATE_TEXT}b`,
        ),
      );
      expect(textarea).toHaveFocus();
      expect(textarea.selectionStart).toBe(1 + SHORT_TEMPLATE_TEXT.length);
      // Only the list fetch — no send, no spawn, whatever the session is doing.
      expect(requestLog.filter((entry) => !entry.startsWith('GET'))).toEqual([]);
      expect(requestLog).toContain('GET /api/prompt-templates');
      expect(useLiveStore.getState().spawns).toHaveLength(0);
    });
  }
});
