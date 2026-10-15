/**
 * Lane content: mark rendering, active lane highlight, mystery-dot
 * filter, small-dot clustering, and cluster mark size.
 *
 * Shared fixtures live in ThreadTimelineOverlay.testkit.tsx.
 */
import { act, screen, waitFor } from '@testing-library/react';
import {
  beforeEach,
  describe,
  expect,
  it,
} from 'vitest';
import { LANE_LEFT_PAD_PX } from './TimelineLaneRow';
import {
  MARK_CLUSTER_PX,
  MARK_SMALL_PX,
} from './TimelineMarks';
import {
  makeMessage,
  makeThread,
  makeUserText,
  renderOverlay,
  resetGlobals,
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
    // An outline paints nothing unless its style is something other than
    // `none` and its width is non-zero. Check the style first: jsdom
    // reports the initial `outline-width: medium` without applying the
    // spec rule that resolves it to 0 when `outline-style` is `none`.
    expect(
      computed.outlineStyle === 'none' || computed.outlineWidth === '0px',
    ).toBe(true);
    expect(computed.borderTopWidth).toBe('0px');
  });
});
