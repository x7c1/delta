/**
 * Lane content: mark rendering, active lane highlight, mystery-dot
 * filter, small-dot clustering, grid lane layout, and cluster mark size.
 *
 * Shared fixtures live in ThreadTimelineOverlay.testkit.tsx.
 */
import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import {
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { LANE_LEFT_PAD_PX } from './ThreadTimelineOverlay';
import {
  MARK_CLUSTER_PX,
  MARK_SMALL_PX,
} from './TimelineMarks';
import {
  makeMessage,
  makeThread,
  makeUserText,
  playheadLeftPx,
  renderOverlay,
  resetGlobals,
  stubAxisRect,
  timelineExpandedKey,
  waitForPlayheadAt,
} from './ThreadTimelineOverlay.testkit';

describe('ThreadTimelineOverlay mark rendering', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('renders circular marks with role-coded color classes and a data-message-kind attribute', async () => {
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          // A genuine human turn (user role + text block) → `user` kind.
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'hello' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          // An assistant reply → `other` kind.
          makeMessage(1, 1, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'hi' }],
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const marks = await screen.findAllByTestId('thread-timeline-dot');
    expect(marks).toHaveLength(2);
    const userMark = marks.find(
      (m) => m.getAttribute('data-message-uuid') === 'u',
    )!;
    const otherMark = marks.find(
      (m) => m.getAttribute('data-message-uuid') === 'a',
    )!;
    // Circle: rounded-full with equal width/height. The packed-lane overlap
    // problem that drove v3's rectangles is solved at the layout level now
    // (see buildGlobalXMap's minimum-spacing push), so the marks can stay
    // solid-fill with no alpha/ring workaround.
    expect(userMark.className).toContain('rounded-full');
    expect(userMark.style.width).toBe(userMark.style.height);
    // Role-coded color and data attribute (tested via class membership and
    // the data attribute, not literal hex, so the tailwind tokens can move).
    // Solid fill on both — overlap is prevented by the global x map, not by
    // alpha stacking, so the classes carry no alpha suffix or ring outline.
    expect(userMark).toHaveAttribute('data-message-kind', 'user');
    expect(userMark.className).toContain('bg-info');
    expect(userMark.className).not.toContain('bg-info/');
    expect(userMark.className).not.toContain('ring-');
    expect(otherMark).toHaveAttribute('data-message-kind', 'other');
    expect(otherMark.className).toContain('bg-fg-subtle');
    expect(otherMark.className).not.toContain('bg-fg-subtle/');
    expect(otherMark.className).not.toContain('ring-');
  });

  it('renders the main-conversation turns as a larger circle than the auxiliary turns', async () => {
    // One small dot sandwiched between two large dots on either side so
    // each small dot stays a lone single-dot render item (the clustering
    // logic needs 2+ adjacent smalls — see the small-dot clustering tests).
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          // user turn → large
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'hello' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          // tool call → small (sandwiched between u and a, no cluster)
          makeMessage(1, 1, 't', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:01:00Z',
          }),
          // assistant prose → large
          makeMessage(1, 2, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'hi' }],
            created_at: '2026-01-01T00:02:00Z',
          }),
          // meta line → small (lone, between large a and large u2)
          makeMessage(1, 3, 'm', {
            role: 'meta',
            content: [{ type: 'text', text: 'sys' }],
            created_at: '2026-01-01T00:03:00Z',
          }),
          // user turn → large (caps the trailing lone small)
          makeMessage(1, 4, 'u2', {
            role: 'user',
            content: [{ type: 'text', text: 'bye' }],
            created_at: '2026-01-01T00:04:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const marks = await screen.findAllByTestId('thread-timeline-dot');
    const byUuid = new Map(
      marks.map((m) => [m.getAttribute('data-message-uuid'), m]),
    );
    expect(byUuid.get('u')).toHaveAttribute('data-message-size', 'large');
    expect(byUuid.get('a')).toHaveAttribute('data-message-size', 'large');
    expect(byUuid.get('t')).toHaveAttribute('data-message-size', 'small');
    expect(byUuid.get('m')).toHaveAttribute('data-message-size', 'small');
    // The diameter of a "large" mark is greater than that of a "small" one
    // (px values, not classes — the renderer applies them inline).
    const largeDiameter = parseFloat(byUuid.get('u')!.style.width);
    const smallDiameter = parseFloat(byUuid.get('t')!.style.width);
    expect(largeDiameter).toBeGreaterThan(smallDiameter);
    // The delta is subtle but visible — the lane should still read as one
    // timeline, not two layers. Cap at 6 px so a future tweak that goes
    // overboard is caught here.
    expect(largeDiameter - smallDiameter).toBeLessThanOrEqual(6);
  });
});

describe('ThreadTimelineOverlay active lane highlight', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('falls back to the activeThreadId prop highlight when no dot is in view', async () => {
    const threads = [
      makeThread(1),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: 'uuid-a',
        created_at: '2026-01-01T00:05:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map(), activeThreadId: 2 });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    expect(lanes[0]).toHaveAttribute('data-active', 'false');
    expect(lanes[1]).toHaveAttribute('data-active', 'true');
  });

  it('marks the lane containing the playhead-active message regardless of the activeThreadId prop', async () => {
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    const messages = new Map([
      [1, [makeUserText(1, 0, 'a', '2026-01-01T00:00:00Z')]],
      [2, [makeUserText(2, 0, 'b', '2026-01-01T00:02:00Z')]],
    ]);
    // Mount on lane 2: the playhead anchors to lane 2's latest large turn
    // (msg-b), so the prop and the playhead's lane agree at first.
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 2,
      conversationArticles: [{ uuid: 'a' }, { uuid: 'b' }],
    });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    // The lanes render from the `threads` prop alone, and lane 2's highlight
    // is satisfied by the `activeThreadId` prop fallback before a single
    // message has loaded — so neither is evidence that the wheel step below
    // has anything to walk. Wait for the state the step actually needs: both
    // lanes' marks present and the playhead settled on msg-b's x (which is
    // the right end of the axis only once msg-a defines its left end).
    expect(await screen.findAllByTestId('thread-timeline-dot')).toHaveLength(2);
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    expect(lanes[1]).toHaveAttribute('data-active', 'true');
    // Wheel-up one step jumps the playhead back to msg-a on lane 1 (a
    // cross-lane step). `setActiveThread` fires, but the overlay's
    // `activeThreadId` PROP stays 2 here — so the lane-1 highlight can only
    // follow if it derives from the playhead-active message's lane rather
    // than the prop. That is exactly the invariant under test.
    const body = screen.getByTestId('thread-timeline-axis-column');
    act(() => {
      body.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -50,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    await waitFor(() => {
      expect(lanes[0]).toHaveAttribute('data-active', 'true');
    });
    expect(lanes[1]).toHaveAttribute('data-active', 'false');
  });

  // v30 fix 2: the active-row hairline used to be `border-y border-slate-200`
  // (active) / `border-y border-transparent` (inactive). The transparent
  // placeholder kept the active and inactive rows the same height — but it
  // also reserved 1 px of layout on the top and 1 px on the bottom of EVERY
  // row, producing a ~2 px transparent stripe between adjacent rows under
  // `align-items: stretch`. That stripe broke the per-lane playhead column
  // visually even after v28 dropped the `<ul>`'s `gap-y-*`. v30 moves the
  // active hairline to a pair of `inset box-shadow`s — non-layout, so
  // adjacent rows now sit truly edge-to-edge.
  //
  // This test pins the structural facts: no row carries a `border-y`
  // utility (active or inactive), and the active row's label and axis
  // cells carry the shadow-inset class instead.
  it('renders the active hairline via inset box-shadow rather than border-y placeholders (v30)', async () => {
    const threads = [
      makeThread(1),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: 'uuid-a',
        created_at: '2026-01-01T00:05:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map(), activeThreadId: 2 });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    // No lane (active or inactive) reserves layout via `border-y` — those
    // transparent placeholders are exactly what produced the inter-row gap.
    for (const lane of lanes) {
      const cells = lane.querySelectorAll('[data-thread-id]');
      for (const cell of Array.from(cells)) {
        // The lane `<li>` itself is `display: contents`, so the two
        // grid items are the label `<span>` and the axis `<div>`. Both
        // must be free of `border-y` utilities (any direction-y border
        // would reintroduce the layout-reserved stripe).
        if (cell === lane) continue;
        expect(cell.className).not.toMatch(/(^|\s)border-y(\s|$)/);
      }
    }
    // The active lane's two cells both carry the shadow-inset utility that
    // paints the hairline non-destructively. The inactive lane does not.
    const activeLane = lanes.find(
      (l) => l.getAttribute('data-active') === 'true',
    )!;
    const inactiveLane = lanes.find(
      (l) => l.getAttribute('data-active') === 'false',
    )!;
    const activeCells = activeLane.querySelectorAll('[data-thread-id]');
    expect(activeCells.length).toBeGreaterThanOrEqual(2);
    for (const cell of Array.from(activeCells)) {
      // Looking for the shadow utility's class name. We do not pin the
      // exact pixel values (those are an implementation detail of the
      // hairline colour) — only that a `shadow-[inset_...]` class is present.
      expect(cell.className).toMatch(/shadow-\[inset_/);
    }
    const inactiveCells = inactiveLane.querySelectorAll('[data-thread-id]');
    for (const cell of Array.from(inactiveCells)) {
      expect(cell.className).not.toMatch(/shadow-\[inset_/);
    }
  });
});

describe('ThreadTimelineOverlay mystery-dot filter', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('does not render dots for system or other ingest-only rows', async () => {
    // Real sessions emit a handful of `role: "system"` rows on startup
    // (and an occasional `other`) whose stamps land before the first
    // user prompt. The transcript skips them; the timeline must too so
    // they do not surface as mystery dots to the left of the first
    // human-readable message.
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'sys', {
            role: 'system',
            content: [{ type: 'text', text: 'bootstrap' }],
            created_at: '2025-12-31T23:59:00Z',
          }),
          makeMessage(1, 1, 'usr', {
            role: 'user',
            content: [{ type: 'text', text: 'hi' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 2, 'oth', {
            role: 'other',
            content: [{ type: 'text', text: 'misc' }],
            created_at: '2026-01-01T00:00:30Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const marks = await screen.findAllByTestId('thread-timeline-dot');
    const uuids = marks
      .map((m) => m.getAttribute('data-message-uuid'))
      .sort();
    expect(uuids).toEqual(['usr']);
  });
});

describe('ThreadTimelineOverlay small-dot clustering', () => {
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('renders 2+ consecutive small dots as a single cluster mark', async () => {
    // A user turn, three tool calls in a row (each is a "small" auxiliary
    // mark), then an assistant prose reply. The three tool calls must
    // collapse into one cluster mark while the user and assistant turns
    // still render as their own dots.
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'do stuff' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't1', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:10Z',
          }),
          makeMessage(1, 2, 't2', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu2', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:20Z',
          }),
          makeMessage(1, 3, 't3', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu3', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:30Z',
          }),
          makeMessage(1, 4, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'done' }],
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    // The lone large dots (u, a) still render as dots.
    const dots = await screen.findAllByTestId('thread-timeline-dot');
    const dotUuids = dots.map((d) => d.getAttribute('data-message-uuid'));
    expect(dotUuids).toContain('u');
    expect(dotUuids).toContain('a');
    expect(dotUuids).not.toContain('t1');
    expect(dotUuids).not.toContain('t2');
    expect(dotUuids).not.toContain('t3');
    // Exactly one cluster mark, pointing at the first member.
    const clusters = await screen.findAllByTestId(
      'thread-timeline-cluster',
    );
    expect(clusters).toHaveLength(1);
    expect(clusters[0]).toHaveAttribute('data-message-uuid', 't1');
    expect(clusters[0]).toHaveAttribute('data-cluster-member-count', '3');
  });

  it('renders a lone small dot as a regular dot, not a cluster', async () => {
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'hi' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:30Z',
          }),
          makeMessage(1, 2, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'done' }],
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const dots = await screen.findAllByTestId('thread-timeline-dot');
    const dotUuids = dots.map((d) => d.getAttribute('data-message-uuid'));
    expect(dotUuids).toContain('t');
    // No cluster mark when only one consecutive small dot exists.
    expect(screen.queryAllByTestId('thread-timeline-cluster')).toHaveLength(
      0,
    );
  });
});

describe('ThreadTimelineOverlay grid lane layout', () => {
  // The lane container is a CSS Grid with two columns: a `max-content`
  // label column that auto-sizes to the widest label across every lane,
  // and a `1fr` axis column carrying the dots and the playhead. The grid
  // replaces an earlier two-`<ul>` flex layout whose label column was a
  // hard-coded width (wasting space for short names) and whose per-row
  // alignment between the label cell and the axis cell drifted as lanes
  // accumulated because the label cell's padding inflated its height
  // past the axis cell's fixed pixel height.
  //
  // The grid solves both at once: `max-content` shares the widest label
  // across every row, and `align-items: stretch` makes each row's two
  // cells share the row's full height so the active-highlight band and
  // the per-lane playhead segment paint at identical vertical extents
  // on both the label side and the axis side. (`center` was the prior
  // contract; it left the axis cell — which carries an explicit pixel
  // height — measurably shorter than the label cell whose height was
  // governed by font metrics + padding, so the highlight band painted
  // a thinner stripe on the axis side and the per-lane playhead looked
  // disconnected between rows.)
  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('uses CSS Grid with a max-content label column and stretched rows for the lane container', async () => {
    // Structural contract: the lane `<ul>` is a grid with two columns
    // sized `max-content 1fr`, and rows stretch via `align-items:
    // stretch`. The label column being `max-content` is what gives every
    // lane label the same width as the longest one (no hard-coded
    // px gutter that wastes space when names are short). `stretch`
    // (rather than `center`) is what guarantees the two grid items of
    // a single row paint at the same vertical extent — the necessary
    // condition for the active-highlight band and the per-lane playhead
    // segment to read as one continuous block across the row.
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const grid = await screen.findByTestId('thread-timeline-lane-grid');
    expect(grid.style.display).toBe('grid');
    expect(grid.style.gridTemplateColumns).toBe('max-content 1fr');
    expect(grid.style.alignItems).toBe('stretch');
  });

  it('carries no non-zero row-gap class on the lane grid so per-lane playhead spans align edge-to-edge across rows', async () => {
    // Each lane renders its own per-lane playhead `<span>`. Any non-zero
    // row gap on the lane `<ul>` shows as a visible break in the
    // otherwise continuous vertical playhead line — a 2px `gap-y-0.5`
    // gap, for instance, paints a 2px gap between every adjacent
    // playhead segment. Pin the contract: the grid must not carry any
    // `gap-y-*` class except `gap-y-0` (Tailwind's default row-gap is
    // already 0, so the natural shape is to drop the class entirely).
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const grid = await screen.findByTestId('thread-timeline-lane-grid');
    // Reject any `gap-y-<non-zero>` token. `gap-y-0` would still pass,
    // but the natural shape is no class at all.
    expect(grid.className).not.toMatch(/(?:^|\s)gap-y-(?!0(?:\s|$))/);
  });

  it('stretches the lane grid to the full axis content width via width:max-content + minWidth:100% so sticky labels have a containing block to pin against', async () => {
    // The label cell uses `position: sticky; left: 0`, and sticky only
    // moves within its containing block — for a grid item that block is
    // the grid `<ul>` itself. The grid `<ul>` is a block-level child of
    // the horizontal-scroll wrapper, so without an explicit width hint
    // its used width stays equal to the wrapper's content-box width
    // (i.e. the visible viewport), even when the axis grid item
    // overflows it horizontally and triggers the wrapper's scrollbar.
    // A containing block no wider than the viewport leaves `left: 0`
    // with nowhere to slide, so the label scrolls off-screen with the
    // axis — which is the regression a previous grid restructure shipped
    // because it dropped this very width hint.
    //
    // `width: max-content` resolves to the sum of the grid tracks' max-
    // content widths (the axis cells declare an explicit pixel width,
    // so this stretches the `<ul>` to the full scrollable range), and
    // `minWidth: 100%` keeps the `<ul>` at least viewport-wide on short
    // sessions where the axis fits without scroll. jsdom does not run
    // CSS layout, but the inline-style contract is what tells a real
    // browser to stretch — pin both declarations so a future restructure
    // cannot silently regress sticky pinning again.
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const grid = await screen.findByTestId('thread-timeline-lane-grid');
    expect(grid.style.width).toBe('max-content');
    expect(grid.style.minWidth).toBe('100%');
  });

  it('renders one label cell and one axis cell per lane, each promoted to a grid item via display:contents on the <li>', async () => {
    // Each lane is an `<li>` with `display: contents` (the list item is
    // kept for semantics / a11y but stripped from layout), so its inner
    // label `<span>` and axis `<div>` are promoted to direct grid items
    // of the `<ul>` at LAYOUT time. The DOM tree itself still nests the
    // cells under the `<li>` — that is what semantic markup demands —
    // but `display: contents` elides the `<li>` box so the grid
    // measures the cells as if they were direct children. The
    // necessary conditions to verify here are: each lane has exactly
    // one label cell and one axis cell, the `<li>` carries
    // `display: contents`, and the `<li>` is a direct DOM child of the
    // grid (so `display: contents` is enough to promote the cells —
    // no extra wrapper sits between).
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const grid = await screen.findByTestId('thread-timeline-lane-grid');
    const lanes = within(grid).getAllByTestId('thread-timeline-lane');
    expect(lanes).toHaveLength(2);
    for (const lane of lanes) {
      expect(lane.style.display).toBe('contents');
      // The `<li>` is a direct DOM child of the grid `<ul>` — no
      // intermediate wrapper that would defeat `display: contents`.
      expect(lane.parentElement).toBe(grid);
      const label = within(lane).getByTestId('thread-timeline-lane-label');
      // `data-timeline-axis` marks the axis cell of the lane.
      const axisCell = lane.querySelector('[data-timeline-axis]');
      expect(label).not.toBeNull();
      expect(axisCell).not.toBeNull();
      // The label and axis are direct children of the `<li>` (one level
      // deep), so `display: contents` on the `<li>` promotes them
      // straight to grid items at layout time.
      expect(label.parentElement).toBe(lane);
      expect(axisCell?.parentElement).toBe(lane);
    }
  });

  it('shares the label column width across lanes so every label measures the same as the longest one', async () => {
    // JSDOM does not run CSS layout, so we cannot read the resolved
    // pixel width of each label cell directly. The structural contract
    // we CAN pin is: every label sits in the same grid column (the
    // `max-content` column) of the same grid container, and no per-lane
    // explicit width overrides it. That is the necessary and sufficient
    // condition for real browsers to render all labels at the longest
    // label's width.
    const threads = [
      makeThread(1, { title: 'main', created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        title: 'short',
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
      makeThread(3, {
        title: 'a very long subthread title that exceeds the others',
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:02:00Z',
      }),
    ];
    renderOverlay({ threads, messagesByThread: new Map() });
    const grid = await screen.findByTestId('thread-timeline-lane-grid');
    const labels = within(grid).getAllByTestId('thread-timeline-lane-label');
    expect(labels).toHaveLength(3);
    // No label carries an explicit `width` style — width is governed by
    // the grid's `max-content` column. (A regression that pinned a px
    // width per label would defeat the auto-sized-to-longest contract.)
    for (const label of labels) {
      expect(label.style.width).toBe('');
      // Each label sits inside its lane's `<li>` whose `display:
      // contents` promotes the label to a direct grid item at layout
      // time, so all labels share the same `max-content` column.
      const lane = label.closest('[data-testid="thread-timeline-lane"]');
      expect(lane).not.toBeNull();
      expect((lane as HTMLElement).style.display).toBe('contents');
      expect(lane?.parentElement).toBe(grid);
    }
  });

  it('routes horizontal scroll through a single wrapper so the sticky label cells can pin to the left edge', async () => {
    // Vertical scroll lives on the outer body (`overflow-y-auto`);
    // horizontal scroll lives on the axis-column wrapper that hosts the
    // grid. The label cells use `position: sticky; left: 0` to pin to
    // the left edge during a horizontal pan, so a wide axis still leaves
    // the labels readable.
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const body = await screen.findByTestId('thread-timeline-body');
    expect(body.className).toMatch(/\boverflow-y-auto\b/);
    expect(body.className).not.toMatch(/\boverflow-x\b/);
    const axisColumn = await screen.findByTestId(
      'thread-timeline-axis-column',
    );
    expect(axisColumn.className).toMatch(/\boverflow-x-auto\b/);
    const label = (
      await screen.findAllByTestId('thread-timeline-lane-label')
    )[0];
    expect(label.style.position).toBe('sticky');
    expect(label.style.left).toBe('0px');
  });

  it('paints the sticky label with an opaque background via className (bg-surface resting, bg-surface-elevated active) so axis dots cannot peek through during a horizontal pan and the active highlight remains visible', async () => {
    // The sticky label slides over the axis cell horizontally as the
    // wrapper pans. Without an opaque background the axis line and dots
    // would read through the label glyphs, which is illegible. The
    // background MUST come from the className (not from an inline
    // `style.background`): an inline background has higher specificity
    // than a Tailwind class, so an inline `background: surface` would win
    // over an active-state `bg-surface-elevated` class and leave the
    // sticky label white while the axis cell paints `bg-surface-elevated`
    // — breaking the row's visual continuity, which is precisely what
    // {@link applies the active highlight to both grid cells of the
    // active lane} pins on the axis side.
    //
    // The contract is therefore: inactive sticky label paints `bg-surface`
    // (matching the body so axis dots never read through it), active
    // sticky label paints `bg-surface-elevated` (matching the axis cell so
    // the active band reads as one continuous row), and no inline
    // `background` style is set that would override either.
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({
      threads,
      messagesByThread: new Map(),
      activeThreadId: 2,
    });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    const inactiveLabel = within(lanes[0]).getByTestId(
      'thread-timeline-lane-label',
    );
    const activeLabel = within(lanes[1]).getByTestId(
      'thread-timeline-lane-label',
    );
    // Inactive sticky label is opaque surface through the className.
    // `bg-surface(?!-)` matches the resting class but not `bg-surface-elevated`.
    expect(inactiveLabel.className).toMatch(/\bbg-surface(?!-)/);
    expect(inactiveLabel.className).not.toMatch(/\bbg-surface-elevated\b/);
    // Active sticky label is opaque surface-elevated (matching the axis
    // cell's highlight) and does NOT carry the resting bg-surface token
    // — so there is exactly one background class active per cell and the
    // class set unambiguously identifies the visual state.
    expect(activeLabel.className).toMatch(/\bbg-surface-elevated\b/);
    expect(activeLabel.className).not.toMatch(/\bbg-surface(?!-)/);
    // No inline background on either label — the background lives on
    // className alone so the active class always wins. (Reading the
    // style property directly catches both `background` and
    // `background-color` short-hand variants on inline styles.)
    expect(inactiveLabel.style.background).toBe('');
    expect(inactiveLabel.style.backgroundColor).toBe('');
    expect(activeLabel.style.background).toBe('');
    expect(activeLabel.style.backgroundColor).toBe('');
  });

  it('keeps the sticky label visible at the wrapper left edge while the axis cell content scrolls horizontally', async () => {
    // Behavioural pin for the sticky-label contract: when the axis-
    // column wrapper scrolls horizontally, the sticky label MUST stay
    // pinned at x=0 of the wrapper while the axis cell shifts left by
    // the scroll amount. jsdom does not run CSS, so `position: sticky`
    // does not move the label automatically — but it DOES report
    // `scrollLeft` on the scroll container, and the inline style
    // contract (`position: sticky; left: 0`) is what tells a real
    // browser to pin. Assert both halves:
    //
    //   1. The label still carries the sticky positioning contract
    //      after the wrapper has been scrolled (no regression that
    //      drops the style under some state transition).
    //   2. The wrapper's `scrollLeft` advances normally so the axis
    //      cells visibly pan — the wrapper is the only horizontal
    //      scroller, the label rides along sticky-pinned.
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const axisColumn = await screen.findByTestId(
      'thread-timeline-axis-column',
    );
    const label = (
      await screen.findAllByTestId('thread-timeline-lane-label')
    )[0];
    // Simulate the wrapper being scrolled horizontally past zero — e.g.
    // the user has panned a wide session's axis to the right.
    act(() => {
      axisColumn.scrollLeft = 120;
    });
    expect(axisColumn.scrollLeft).toBe(120);
    // The sticky positioning contract is intact: a real browser holds
    // the label at the wrapper's left edge while the axis cell content
    // pans behind it.
    expect(label.style.position).toBe('sticky');
    expect(label.style.left).toBe('0px');
    // The label sits in the same DOM ancestor as the axis cell of the
    // same lane — i.e. inside the scrolling wrapper — so sticky has
    // somewhere to pin. (A regression that moved the label out of the
    // scroll container would defeat sticky entirely.)
    expect(axisColumn.contains(label)).toBe(true);
  });

  it('applies the active highlight to both grid cells of the active lane so the band reads as continuous', async () => {
    // With `display: contents` on the `<li>` the list-item itself has no
    // box, so a highlight applied to the `<li>` would never paint. The
    // active highlight lives on BOTH the label cell AND the axis cell
    // individually, so the two halves of the active lane's grid row line
    // up into one continuous visual band. v30 expresses the top/bottom
    // hairline as an `inset box-shadow` rather than `border-y`, because
    // `border-y border-transparent` (the prior inactive placeholder)
    // reserved 1 px on top and 1 px on bottom of every row and produced
    // a ~2 px transparent stripe between adjacent rows under
    // `align-items: stretch`. The `bg-surface-elevated` background remains the
    // active band's surface; the inset shadow draws its boundary.
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({
      threads,
      messagesByThread: new Map(),
      activeThreadId: 2,
    });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    expect(lanes).toHaveLength(2);
    expect(lanes[0]).toHaveAttribute('data-active', 'false');
    expect(lanes[1]).toHaveAttribute('data-active', 'true');
    const activeLabel = within(lanes[1]).getByTestId(
      'thread-timeline-lane-label',
    );
    const activeAxis = lanes[1].querySelector(
      '[data-timeline-axis]',
    ) as HTMLElement;
    expect(activeLabel).toHaveAttribute('data-active', 'true');
    expect(activeAxis).toHaveAttribute('data-active', 'true');
    // Both cells carry the identical highlight token set so the band
    // reads as continuous across the row.
    expect(activeLabel.className).toMatch(/bg-surface-elevated/);
    expect(activeAxis.className).toMatch(/bg-surface-elevated/);
    // v30: the hairline is an inset box-shadow (non-layout), not a
    // border-y placeholder (which used to reserve a 2 px gap between
    // adjacent rows).
    expect(activeLabel.className).toMatch(/shadow-\[inset_/);
    expect(activeAxis.className).toMatch(/shadow-\[inset_/);
    // The inactive lane's cells do NOT carry the active tokens, so the
    // highlight is per-lane rather than global.
    const inactiveLabel = within(lanes[0]).getByTestId(
      'thread-timeline-lane-label',
    );
    const inactiveAxis = lanes[0].querySelector(
      '[data-timeline-axis]',
    ) as HTMLElement;
    expect(inactiveLabel.className).not.toMatch(/bg-surface-elevated/);
    expect(inactiveAxis.className).not.toMatch(/bg-surface-elevated/);
    // Inactive rows must not carry the inset shadow either — otherwise
    // the active state stops reading as distinct.
    expect(inactiveLabel.className).not.toMatch(/shadow-\[inset_/);
    expect(inactiveAxis.className).not.toMatch(/shadow-\[inset_/);
  });

  it('paints the active-highlight band at matched heights on label and axis by stretching both grid items to the row height', async () => {
    // Each grid item of a lane (the sticky label `<span>` and the axis
    // `<div>` marked `data-timeline-axis`) carries `h-full` plus a
    // `minHeight: LANE_HEIGHT_PX` floor. Combined with the grid
    // container's `align-items: stretch`, this is the necessary and
    // sufficient condition for the two cells of a single row to share
    // the same painted height — so the active-highlight band
    // (`bg-surface-elevated` + `border-y`) appears as one continuous block
    // across the row rather than two stripes of mismatched height. A
    // regression that dropped `h-full` from either side or pinned the
    // axis to a fixed `height` would defeat the stretch and reintroduce
    // the visible mismatch.
    //
    // jsdom does not run CSS layout, so we cannot read the resolved
    // pixel height of each cell. The contract we CAN pin is the inline
    // and class declarations themselves: both cells expose `h-full` in
    // their className and `minHeight: LANE_HEIGHT_PX` (== 18px) inline.
    const LANE_HEIGHT_PX = 18;
    const threads = [
      makeThread(1, { created_at: '2026-01-01T00:00:00Z' }),
      makeThread(2, {
        parent_thread_id: 1,
        root_message_uuid: null,
        created_at: '2026-01-01T00:01:00Z',
      }),
    ];
    renderOverlay({
      threads,
      messagesByThread: new Map(),
      activeThreadId: 2,
    });
    const lanes = await screen.findAllByTestId('thread-timeline-lane');
    for (const lane of lanes) {
      const label = within(lane).getByTestId('thread-timeline-lane-label');
      const axis = lane.querySelector(
        '[data-timeline-axis]',
      ) as HTMLElement;
      expect(label).not.toBeNull();
      expect(axis).not.toBeNull();
      // Both cells declare `h-full` so each row's items grow to the row's
      // stretched height instead of capping at their intrinsic height.
      expect(label.className).toMatch(/(?:^|\s)h-full(?:\s|$)/);
      expect(axis.className).toMatch(/(?:^|\s)h-full(?:\s|$)/);
      // Both cells declare the same `minHeight` floor so an empty axis
      // row still respects `LANE_HEIGHT_PX` rather than collapsing.
      expect(label.style.minHeight).toBe(`${LANE_HEIGHT_PX}px`);
      expect(axis.style.minHeight).toBe(`${LANE_HEIGHT_PX}px`);
      // The axis side must NOT pin a fixed `height` — that would defeat
      // the stretch by forcing the axis cell back to exactly
      // `LANE_HEIGHT_PX` regardless of how tall the row grew.
      expect(axis.style.height).toBe('');
    }
    // The active lane's both cells additionally carry the highlight
    // tokens, so when the row stretches the band paints continuously
    // across both halves at the same height.
    const activeLane = lanes[1];
    expect(activeLane).toHaveAttribute('data-active', 'true');
    const activeLabel = within(activeLane).getByTestId(
      'thread-timeline-lane-label',
    );
    const activeAxis = activeLane.querySelector(
      '[data-timeline-axis]',
    ) as HTMLElement;
    expect(activeLabel.className).toMatch(/bg-surface-elevated/);
    expect(activeAxis.className).toMatch(/bg-surface-elevated/);
  });

  it('keeps row alignment stretched regardless of how many lanes accumulate', async () => {
    // The grid's `align-items: stretch` is the single source of truth for
    // row alignment between the label cell and the axis cell — it
    // guarantees the two cells share the row's full height so the
    // active-highlight band and the per-lane playhead segment paint at
    // identical vertical extents. A regression that swapped it for
    // `center` (the prior contract) would reintroduce the visible
    // mismatch in highlight band height; pin the contract on the grid
    // container itself so the guarantee does not depend on lane count.
    const threads = Array.from({ length: 8 }, (_, i) =>
      makeThread(i + 1, {
        title: `lane ${i + 1}`,
        parent_thread_id: i === 0 ? null : 1,
        root_message_uuid: i === 0 ? null : null,
        created_at: `2026-01-01T00:0${i}:00Z`,
      }),
    );
    renderOverlay({ threads, messagesByThread: new Map() });
    const grid = await screen.findByTestId('thread-timeline-lane-grid');
    expect(grid.style.alignItems).toBe('stretch');
    const lanes = within(grid).getAllByTestId('thread-timeline-lane');
    expect(lanes).toHaveLength(8);
  });

  it('ignores wheel events whose target is a label cell so labels behave like normal page content', async () => {
    // A wheel over a label cell must NOT scrub the timeline. The wheel
    // listener attaches to the axis-column wrapper (which now hosts both
    // the label and the axis cells, because the sticky label needs to
    // share the same scroll container as the axis), so scope
    // discrimination happens by event target.
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
          makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [{ uuid: 'msg-a' }, { uuid: 'msg-b' }],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    // Wheel originating on a label cell has no effect — the wheel
    // bubbles to the axis-column wrapper but the handler returns early
    // when the target sits inside a label cell.
    const label = screen.getAllByTestId('thread-timeline-lane-label')[0];
    act(() => {
      label.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
    // A wheel anywhere else inside the axis-column wrapper DOES scrub —
    // proving the listener is wired but scoped past the labels. One
    // step back from the tail (msg-b) lands on msg-a at x=0.
    const axisColumn = screen.getByTestId('thread-timeline-axis-column');
    act(() => {
      axisColumn.dispatchEvent(
        new WheelEvent('wheel', {
          deltaY: -100,
          bubbles: true,
          cancelable: true,
        }),
      );
    });
    // The wheel handler is a native (`addEventListener`) listener, so the
    // active-index state update it commits is scheduled by React rather than
    // flushed synchronously inside the `act` above — a plain synchronous
    // `expect` here races that flush and can observe the pre-scrub position
    // (`256px`) under CI scheduling. `waitFor` retries the assertion across
    // React's flush boundary, so it settles deterministically on the scrubbed
    // position without weakening what is asserted.
    await waitFor(() =>
      expect(
        playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
      ).toBe(`${LANE_LEFT_PAD_PX}px`),
    );
  });

  it('ignores click events whose target is a label cell', async () => {
    // Same scope contract for clicks: a click on a label is not a scrub
    // intent. The handler attaches to the axis-column wrapper and the
    // same label-target discrimination keeps label clicks out of the
    // jump path.
    stubAxisRect({ left: 0, width: 240 });
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeUserText(1, 0, 'msg-a', '2026-01-01T00:00:00Z'),
          makeUserText(1, 1, 'msg-b', '2026-01-01T00:01:00Z'),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
      conversationArticles: [{ uuid: 'msg-a' }, { uuid: 'msg-b' }],
    });
    await screen.findAllByTestId('thread-timeline-dot');
    await waitForPlayheadAt(`${240 + LANE_LEFT_PAD_PX}px`);
    // A click on a label cell with clientX=0 (where msg-a would land if
    // the axis click handler picked it up) must NOT move the playhead.
    fireEvent.click(screen.getAllByTestId('thread-timeline-lane-label')[0], {
      clientX: 0,
    });
    expect(
      playheadLeftPx(screen.getAllByTestId('thread-timeline-playhead')[0]),
    ).toBe(`${240 + LANE_LEFT_PAD_PX}px`);
  });

  // v30 fix 3: the axis cell reserves a right-side pad mirroring the left
  // pad so the rightmost large dot (6 px diameter centred on x = laneAxisWidth)
  // does not clip into the column's right edge. The axis-cell's resolved
  // width is `LANE_LEFT_PAD_PX + laneAxisWidth + LANE_RIGHT_PAD_PX`, and
  // the inner axis-line `<span>` sits at `left = LANE_LEFT_PAD_PX` with
  // `width = laneAxisWidth`. The trailing pad is therefore
  // `axisCellWidth - axisLineLeft - axisLineWidth`. The structural
  // contract we want to pin is `LANE_LEFT_PAD_PX === LANE_RIGHT_PAD_PX`
  // (symmetric pads). Reading both off the rendered DOM avoids depending
  // on an internal un-exported constant.
  it('reserves symmetric left/right padding on the axis cell so the rightmost dot is not clipped (v30)', async () => {
    const threads = [makeThread(1)];
    renderOverlay({ threads, messagesByThread: new Map() });
    const lane = await screen.findByTestId('thread-timeline-lane');
    const axisCell = lane.querySelector('[data-timeline-axis]') as HTMLElement;
    expect(axisCell).not.toBeNull();
    // The axis line `<span>` is the only direct child of the axis cell
    // with inline `left` AND `width` set in pixels (dots/playhead use
    // either transform or no width). Pick it via that signature.
    const axisLine = Array.from(
      axisCell.querySelectorAll<HTMLElement>('span[aria-hidden="true"]'),
    ).find(
      (el) =>
        /\d/.test(el.style.left ?? '') && /\d/.test(el.style.width ?? ''),
    );
    expect(axisLine).toBeDefined();
    const axisLineLeft = parseFloat(axisLine!.style.left);
    const axisLineWidth = parseFloat(axisLine!.style.width);
    const axisCellWidth = parseFloat(axisCell.style.width);
    const rightPad = axisCellWidth - axisLineLeft - axisLineWidth;
    // Left pad mirrors right pad.
    expect(rightPad).toBe(LANE_LEFT_PAD_PX);
    // And the left pad itself is the exported constant — the axis line
    // is anchored at exactly LANE_LEFT_PAD_PX from the cell's left edge.
    expect(axisLineLeft).toBe(LANE_LEFT_PAD_PX);
  });
});

describe('ThreadTimelineOverlay cluster mark size', () => {
  // v10 dogfooding revealed that the cluster's render size (5 px) was
  // visually indistinguishable from the 6 px main-role dots — the user
  // could not tell a run-of-tool-calls cluster apart from a user/Claude
  // turn. The contract now is: a cluster renders at the SMALL dot
  // diameter exactly, and conveys "cluster-ness" through a thin outline
  // ring instead of size. These tests pin the contract.

  beforeEach(() => {
    resetGlobals();
    window.localStorage.setItem(timelineExpandedKey(), 'true');
  });

  it('renders cluster dots at exactly the small-dot diameter', async () => {
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 'u', {
            role: 'user',
            content: [{ type: 'text', text: 'go' }],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't1', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:10Z',
          }),
          makeMessage(1, 2, 't2', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu2', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:20Z',
          }),
          makeMessage(1, 3, 'a', {
            role: 'assistant',
            content: [{ type: 'text', text: 'done' }],
            created_at: '2026-01-01T00:01:00Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const clusters = await screen.findAllByTestId('thread-timeline-cluster');
    expect(clusters).toHaveLength(1);
    const cluster = clusters[0];
    expect(cluster.style.width).toBe(`${MARK_SMALL_PX}px`);
    expect(cluster.style.height).toBe(`${MARK_SMALL_PX}px`);
    // Pin the literal value too — the v7/v11 contract is "cluster dots
    // stay at 4px exactly, ring-only differentiation". A future tweak to
    // MARK_SMALL_PX should break this assertion so the regression is
    // visible at review time, not in dogfooding.
    expect(cluster.style.width).toBe('4px');
    expect(cluster.style.height).toBe('4px');
    // Cross-check the constant equality so a future "let's bump cluster
    // size again" lands here, not in dogfooding.
    expect(MARK_CLUSTER_PX).toBe(MARK_SMALL_PX);
    expect(MARK_CLUSTER_PX).toBe(4);
  });

  it('renders a cluster with no outline / ring / border so its visual footprint matches a lone small dot', async () => {
    // v16: the v11 outline-based "halo" extended the cluster's painted
    // footprint by 1 px on each side, so a 4 px disc became a 6 px outer
    // disc — visually indistinguishable from the 6 px main-role dots,
    // exactly the regression v11 thought it had fixed by dropping the
    // 5 px fill. v16 drops the outline entirely. The cluster carries no
    // outline / ring / border utility, no shadow, no transform; its
    // visible footprint equals MARK_CLUSTER_PX end-to-end. "Cluster-ness"
    // is purely positional / interactive — the representative x and the
    // `data-cluster-member-count` attribute carry the meaning.
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 't1', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't2', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu2', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:10Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const cluster = (await screen.findAllByTestId('thread-timeline-cluster'))[0];
    // No outline / ring / border utility — these are precisely the
    // Tailwind tokens that would extend the visual footprint beyond the
    // inline width/height of MARK_CLUSTER_PX. A single failed assertion
    // here flags exactly which footprint-expanding utility crept back in.
    expect(cluster.className).not.toMatch(/\boutline\b/);
    expect(cluster.className).not.toMatch(/\boutline-1\b/);
    expect(cluster.className).not.toMatch(/\boutline-/);
    expect(cluster.className).not.toMatch(/\bring(?:-|\b)/);
    expect(cluster.className).not.toMatch(/\bborder(?:-|\b)/);
    expect(cluster.className).not.toMatch(/\bshadow(?:-|\b)/);
    // Pin the fill colour explicitly: a cluster reads as a normal small
    // assistant dot (same fill, same size, no halo).
    expect(cluster.className).toMatch(/\bbg-fg-subtle\b/);
    // No transform-scale either: a 4 px disc * scale-150 would also
    // recreate the "looks 6 px" regression at a different code path.
    expect(cluster.className).not.toMatch(/\bscale-/);
  });

  it('matches the inline width and height of a lone small dot exactly, including no outline contribution', async () => {
    // The cluster's INLINE box is sized to MARK_CLUSTER_PX. The previous
    // v11 contract relied on `outline` (which paints OUTSIDE the box and
    // does not show up in `style.width`/`height`), so a width-equals-4px
    // assertion alone could not catch the regression. This test pins
    // both the inline width/height AND the absence of any
    // footprint-extending utility class, so a future "let's add a tiny
    // ring back" regression cannot slip past the size assertion.
    const threads = [makeThread(1)];
    const messages = new Map([
      [
        1,
        [
          makeMessage(1, 0, 't1', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu1', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:00Z',
          }),
          makeMessage(1, 1, 't2', {
            role: 'assistant',
            content: [
              { type: 'tool_use', id: 'tu2', name: 'Bash', input: {} },
            ],
            created_at: '2026-01-01T00:00:10Z',
          }),
        ],
      ],
    ]);
    renderOverlay({
      threads,
      messagesByThread: messages,
      activeThreadId: 1,
    });
    const cluster = (await screen.findAllByTestId('thread-timeline-cluster'))[0];
    // Pin the literal value so the value of MARK_SMALL_PX in source can
    // never silently bump the cluster footprint either.
    expect(cluster.style.width).toBe(`${MARK_SMALL_PX}px`);
    expect(cluster.style.height).toBe(`${MARK_SMALL_PX}px`);
    expect(MARK_CLUSTER_PX).toBe(MARK_SMALL_PX);
    expect(MARK_CLUSTER_PX).toBe(4);
    // Cross-check that the resolved computed style (jsdom returns the
    // inline width straight back, with no outline applied because no
    // outline class is present) also matches — guarding against a future
    // CSS-cascade rule that re-grows the disc via `width` rather than
    // `outline`.
    const computed = window.getComputedStyle(cluster);
    expect(computed.width).toBe(`${MARK_SMALL_PX}px`);
    expect(computed.height).toBe(`${MARK_SMALL_PX}px`);
    expect(computed.outlineWidth === '' || computed.outlineWidth === '0px').toBe(
      true,
    );
    expect(computed.borderTopWidth === '' || computed.borderTopWidth === '0px').toBe(
      true,
    );
  });
});
