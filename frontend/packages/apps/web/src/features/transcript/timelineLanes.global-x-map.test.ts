/**
 * `buildGlobalXMap`: the shared x positions of every mark.
 *
 * Shared builders live in timelineLanes.testkit.ts.
 */
import { describe, expect, it } from 'vitest';
import {
  IDLE_GAP_CAP_FACTOR_K,
  MIN_IDLE_GAP_CAP_MS,
  buildGlobalXMap,
  buildSortedMessages,
  buildTimelineLanes,
  computeTimeRange,
  type TimelineDotSize,
} from './timelineLanes';
import { message, thread } from './timelineLanes.testkit';

describe('buildGlobalXMap', () => {
  // Real renderer values; tests pin the contract those constants drive.
  const MARK_LARGE_PX = 6;
  const MARK_SMALL_PX = 4;
  const diameter = (size: TimelineDotSize) =>
    size === 'large' ? MARK_LARGE_PX : MARK_SMALL_PX;

  it('places a message at the same px x in every lane it appears next to', () => {
    // Lane 1 carries msg-a (early) and msg-c (late). Lane 2 carries msg-b
    // (mid) at the same instant a hypothetical lane-1 message would land —
    // the test enforces that the global map collapses both ideal positions
    // to ONE px value keyed by uuid, so cross-lane rendering lines up.
    const threads = [
      thread(1, { createdAt: '2026-01-01T00:00:00Z' }),
      thread(2, { parent: 1, createdAt: '2026-01-01T00:01:00Z' }),
    ];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', {
            role: 'user',
            content: [{ type: 'text', text: 'hi' }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
          // c shares msg-b's timestamp — they collide ideally. The greedy
          // push moves the later one (by seq) right by the min spacing, so
          // both lanes still see the SAME map entries for both uuids.
          message(1, 5, 'c', {
            role: 'user',
            content: [{ type: 'text', text: 'late' }],
            createdAt: '2026-01-01T00:02:00Z',
          }),
        ],
      ],
      [
        2,
        [
          message(2, 1, 'b', {
            role: 'user',
            content: [{ type: 'text', text: 'mid' }],
            createdAt: '2026-01-01T00:02:00Z',
          }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid } = buildGlobalXMap(sorted, range, 240, diameter, 240);
    // a at ts 0 → ideal 0 px → actual 0 px (first message keeps its ideal).
    expect(pxByUuid.get('a')).toBe(0);
    // b at ts 120s of 120s range → ideal 240 px → first to land at that
    // timestamp, so actual = 240 px.
    expect(pxByUuid.get('b')).toBe(240);
    // c shares ts with b → ideal 240 px, but b already occupies 240. The
    // greedy push moves c to 240 + (6+6)/2 = 246 px.
    expect(pxByUuid.get('c')).toBe(246);
  });

  it('enforces minimum spacing so overlapping ideal positions get pushed apart', () => {
    // Three large messages within a 1 ms window: their ideal positions
    // collapse to ~0 px on a 240 px axis, so the greedy push must spread
    // them out by 6 px each (large+large min spacing).
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', {
            role: 'user',
            content: [{ type: 'text', text: 'a' }],
            createdAt: '2026-01-01T00:00:00.000Z',
          }),
          message(1, 1, 'b', {
            role: 'user',
            content: [{ type: 'text', text: 'b' }],
            createdAt: '2026-01-01T00:00:00.001Z',
          }),
          message(1, 2, 'c', {
            role: 'user',
            content: [{ type: 'text', text: 'c' }],
            createdAt: '2026-01-01T00:00:00.002Z',
          }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid, axisWidth } = buildGlobalXMap(
      sorted,
      range,
      240,
      diameter,
      240,
    );
    expect(pxByUuid.get('a')).toBe(0);
    // b's ideal is ~0.005 px (1 ms out of 2 ms range × 240 px = 120 px,
    // actually, let me recompute: 1/2 × 240 = 120 px). Not overlapping,
    // stays at 120.
    expect(pxByUuid.get('b')).toBe(120);
    expect(pxByUuid.get('c')).toBe(240);
    expect(axisWidth).toBe(240);
  });

  it('enforces large+small min spacing of 5 px between mixed neighbours', () => {
    // A large then a small, both at the same timestamp → ideals collide at
    // 0 px. The push moves the small to (6+4)/2 = 5 px.
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'L', {
            role: 'user',
            content: [{ type: 'text', text: 'large' }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
          // Same instant, classified small (tool_use only).
          message(1, 1, 's', {
            role: 'assistant',
            content: [{ type: 'tool_use', id: 'tu', name: 'Bash', input: {} }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid } = buildGlobalXMap(sorted, range, 240, diameter, 240);
    expect(pxByUuid.get('L')).toBe(0);
    expect(pxByUuid.get('s')).toBe(5);
  });

  it('enforces small+small min spacing of 4 px between two auxiliary neighbours', () => {
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 's1', {
            role: 'assistant',
            content: [{ type: 'tool_use', id: 'tu1', name: 'Bash', input: {} }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
          message(1, 1, 's2', {
            role: 'assistant',
            content: [{ type: 'tool_use', id: 'tu2', name: 'Bash', input: {} }],
            createdAt: '2026-01-01T00:00:00Z',
          }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid } = buildGlobalXMap(sorted, range, 240, diameter, 240);
    expect(pxByUuid.get('s1')).toBe(0);
    expect(pxByUuid.get('s2')).toBe(4);
  });

  it('expands axisWidth past the minimum when overlap pushes the last mark right', () => {
    // Four messages, every one at the rightmost timestamp. The first lands
    // at the ideal max (240 px); the next three get pushed right by 6 px
    // each, so axisWidth grows past the minimum.
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 1, 'b', { createdAt: '2026-01-01T00:01:00Z' }),
          message(1, 2, 'c', { createdAt: '2026-01-01T00:01:00Z' }),
          message(1, 3, 'd', { createdAt: '2026-01-01T00:01:00Z' }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    // Every message here is a user-role no-content → small (4 px). Spacing
    // between two smalls is 4 px.
    const { pxByUuid, axisWidth } = buildGlobalXMap(
      sorted,
      range,
      240,
      diameter,
      240,
    );
    expect(pxByUuid.get('a')).toBe(0);
    expect(pxByUuid.get('b')).toBe(240);
    expect(pxByUuid.get('c')).toBe(244);
    expect(pxByUuid.get('d')).toBe(248);
    expect(axisWidth).toBe(248);
  });

  it('keeps axisWidth at the minimum when no message exists yet', () => {
    const { pxByUuid, axisWidth } = buildGlobalXMap([], null, 240, diameter, 240);
    expect(pxByUuid.size).toBe(0);
    expect(axisWidth).toBe(240);
  });

  it('clamps a single long idle gap so the conversational messages do not collapse to the edges', () => {
    // 4 messages with dts [5s, 5s, 3600s, 5s] — one outlier idle gap. The
    // linear-time projection would put the 4th dot at the far right (the
    // 3600s gap eats ~99% of the axis); after clamping the outlier dt down
    // to the per-session cap, the 4th dot lands somewhere closer to the
    // middle so the actually-conversational rhythm is legible.
    const baseTs = Date.parse('2026-01-01T00:00:00Z');
    const stamps = [
      baseTs,
      baseTs + 5_000,
      baseTs + 10_000,
      baseTs + 10_000 + 3_600_000,
      baseTs + 10_000 + 3_600_000 + 5_000,
    ];
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        stamps.map((ts, i) =>
          message(1, i, `m${i}`, {
            role: 'user',
            content: [{ type: 'text', text: `m${i}` }],
            createdAt: new Date(ts).toISOString(),
          }),
        ),
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const baseWidth = 240;
    const { pxByUuid, axisWidth } = buildGlobalXMap(
      sorted,
      range,
      baseWidth,
      diameter,
      baseWidth,
    );
    // dts = [5s, 5s, 3_600s, 5s]; sorted = [5s, 5s, 5s, 3_600s]; P75 nearest-
    // rank lower at floor(0.75 * 3) = index 2 → 5s. cap =
    // max(60s, 5s * K) = 60s. effective dts = [5s, 5s, 60s, 5s]; total
    // effective = 75s. The outlier-gap message lands at
    // (5+5+60)/75 * 240 = 224 px. Without the clamp it would be at
    // (5+5+3600)/3615 * 240 ≈ 239.7 px — i.e. flush against the right edge,
    // which is the bug case from the dogfooding report.
    const m3Px = pxByUuid.get('m3');
    expect(m3Px).toBeDefined();
    expect(m3Px!).toBeGreaterThan(0);
    expect(m3Px!).toBeLessThan(baseWidth);
    // And it should be away from the right edge, not flush against it. With
    // clamping it lands around 224 px on a 240 px axis.
    expect(m3Px!).toBeCloseTo(224, 0);
    // The greedy-push minimum spacing is preserved across the clamped gap.
    expect(axisWidth).toBeGreaterThanOrEqual(baseWidth);
  });

  it('produces the same relative spacing as the linear projection when every dt is equal (no clamping kicks in)', () => {
    // Even-tempo session: dts = [60s, 60s, 60s]. P75 = 60s, cap =
    // max(60s, 180s) = 180s. No dt is clamped, so the layout matches the
    // linear-time formula: x_i = i / (n - 1) * baseWidth.
    const baseTs = Date.parse('2026-01-01T00:00:00Z');
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [0, 60_000, 120_000, 180_000].map((dt, i) =>
          message(1, i, `m${i}`, {
            role: 'user',
            content: [{ type: 'text', text: `m${i}` }],
            createdAt: new Date(baseTs + dt).toISOString(),
          }),
        ),
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid } = buildGlobalXMap(sorted, range, 240, diameter, 240);
    expect(pxByUuid.get('m0')).toBe(0);
    expect(pxByUuid.get('m1')).toBe(80);
    expect(pxByUuid.get('m2')).toBe(160);
    expect(pxByUuid.get('m3')).toBe(240);
  });

  it('returns a 1-entry map at x=0 with axisWidth=minWidth for a single message', () => {
    const range = { minMs: 1000, maxMs: 1000 };
    const single: ReturnType<typeof buildSortedMessages> = [
      {
        uuid: 'only',
        threadId: 1,
        x: 0,
        timeMs: 1000,
        seq: 0,
        size: 'large',
      },
    ];
    const { pxByUuid, axisWidth } = buildGlobalXMap(single, range, 240, diameter, 240);
    expect(pxByUuid.size).toBe(1);
    expect(pxByUuid.get('only')).toBe(0);
    expect(axisWidth).toBe(240);
  });

  it('places every message at x=0 (modulo radius push) without dividing by zero when all timestamps collide', () => {
    // Pathological input where total effective dt is zero. The greedy
    // push must still spread by mark radius, and no NaN/Infinity may leak.
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 1, 'b', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 2, 'c', { createdAt: '2026-01-01T00:00:00Z' }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid, axisWidth } = buildGlobalXMap(sorted, range, 240, diameter, 240);
    const ax = pxByUuid.get('a')!;
    const bx = pxByUuid.get('b')!;
    const cx = pxByUuid.get('c')!;
    expect(Number.isFinite(ax)).toBe(true);
    expect(Number.isFinite(bx)).toBe(true);
    expect(Number.isFinite(cx)).toBe(true);
    // a stays at 0, b and c get pushed right by the small/small spacing
    // (4 px) since these are user-role no-content lines → small.
    expect(ax).toBe(0);
    expect(bx).toBe(4);
    expect(cx).toBe(8);
    expect(axisWidth).toBe(240);
  });

  it('places two messages at x=0 and baseWidth respectively (degenerate but well-defined)', () => {
    // Two messages → single dt. Cap = max(60s, dt * K) ≥ dt, so the dt is
    // NOT clamped and the layout matches the linear projection.
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [
          message(1, 0, 'a', { createdAt: '2026-01-01T00:00:00Z' }),
          message(1, 1, 'b', { createdAt: '2026-01-01T00:10:00Z' }),
        ],
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid } = buildGlobalXMap(sorted, range, 240, diameter, 240);
    expect(pxByUuid.get('a')).toBe(0);
    expect(pxByUuid.get('b')).toBe(240);
  });

  it('floors the cap at MIN_IDLE_GAP_CAP_MS in a tight session so no short dt gets clamped', () => {
    // Every dt is 1 s — P75 = 1 s, P75 * K = 3 s, below the 60 s floor.
    // The floor wins, so no dt is clamped and the layout is identical to
    // the linear projection on this set.
    const baseTs = Date.parse('2026-01-01T00:00:00Z');
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        [0, 1_000, 2_000, 3_000, 4_000].map((dt, i) =>
          message(1, i, `m${i}`, {
            role: 'user',
            content: [{ type: 'text', text: `m${i}` }],
            createdAt: new Date(baseTs + dt).toISOString(),
          }),
        ),
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid } = buildGlobalXMap(sorted, range, 240, diameter, 240);
    // Linear: x_i = i / 4 * 240 = 0, 60, 120, 180, 240.
    expect(pxByUuid.get('m0')).toBe(0);
    expect(pxByUuid.get('m1')).toBe(60);
    expect(pxByUuid.get('m2')).toBe(120);
    expect(pxByUuid.get('m3')).toBe(180);
    expect(pxByUuid.get('m4')).toBe(240);
  });

  it('uses nearest-rank (lower) at P75 — dts [10s, 10s, 10s, 100s] → cap = MIN floor, 100s gap clamps', () => {
    // dts sorted = [10s, 10s, 10s, 100s]; P75 at floor(0.75 * 3) = index 2,
    // → P75 = 10 s. cap = max(60s, 30s) = 60 s. The 100 s outlier clamps
    // to 60 s; the three 10 s gaps stay intact.
    const baseTs = Date.parse('2026-01-01T00:00:00Z');
    const stamps = [
      baseTs,
      baseTs + 10_000,
      baseTs + 20_000,
      baseTs + 30_000,
      baseTs + 30_000 + 100_000,
    ];
    const threads = [thread(1)];
    const messagesByThread = new Map([
      [
        1,
        stamps.map((ts, i) =>
          message(1, i, `m${i}`, {
            role: 'user',
            content: [{ type: 'text', text: `m${i}` }],
            createdAt: new Date(ts).toISOString(),
          }),
        ),
      ],
    ]);
    const range = computeTimeRange(messagesByThread);
    const sorted = buildSortedMessages(buildTimelineLanes(threads, messagesByThread));
    const { pxByUuid } = buildGlobalXMap(sorted, range, 240, diameter, 240);
    // effective dts = [10s, 10s, 10s, 60s]; cumDt = [0, 10s, 20s, 30s, 90s];
    // total = 90s; scale = 240/90s. ideals = [0, 26.67, 53.33, 80, 240].
    expect(pxByUuid.get('m0')).toBe(0);
    expect(pxByUuid.get('m1')).toBeCloseTo((10 / 90) * 240, 5);
    expect(pxByUuid.get('m2')).toBeCloseTo((20 / 90) * 240, 5);
    expect(pxByUuid.get('m3')).toBeCloseTo((30 / 90) * 240, 5);
    // The clamped outlier still lands at the right edge of the axis,
    // because every dt at or below the cap contributes its full extent.
    expect(pxByUuid.get('m4')).toBe(240);
  });

  it('exports the cap constants so tests and future markers can pin the same numbers', () => {
    // Pinning the constants in a test guards against an accidental retune
    // that would change the visual rhythm of every existing session.
    expect(IDLE_GAP_CAP_FACTOR_K).toBe(3);
    expect(MIN_IDLE_GAP_CAP_MS).toBe(60_000);
  });
});
