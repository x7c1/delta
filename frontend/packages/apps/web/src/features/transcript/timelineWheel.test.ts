import { describe, expect, it } from 'vitest';
import {
  WHEEL_DELTA_LINE_PX,
  WHEEL_PER_EVENT_CLAMP_PX,
  normalizeWheelDeltaPx,
  stepsForCumulativePx,
} from './timelineWheel';

describe('ThreadTimelineOverlay wheel calculator', () => {
  it('normalizes pixel-mode |delta| with the per-event clamp', () => {
    expect(normalizeWheelDeltaPx(50, 0)).toBe(50);
    expect(normalizeWheelDeltaPx(-50, 0)).toBe(50);
    // Above the clamp ceiling, contributions are capped.
    expect(normalizeWheelDeltaPx(500, 0)).toBe(WHEEL_PER_EVENT_CLAMP_PX);
  });

  it('normalizes line-mode |delta| by the per-line pixel proxy', () => {
    // 1 line ≈ 40 px, two lines ≈ 80 px (under the clamp).
    expect(normalizeWheelDeltaPx(2, 1)).toBe(2 * WHEEL_DELTA_LINE_PX);
    // 5 lines = 200 px → clamped to 100.
    expect(normalizeWheelDeltaPx(5, 1)).toBe(WHEEL_PER_EVENT_CLAMP_PX);
  });

  it('normalizes page-mode |delta| by the per-page pixel proxy and clamps', () => {
    // Even a single page-mode event is clamped to one notch.
    expect(normalizeWheelDeltaPx(1, 2)).toBe(WHEEL_PER_EVENT_CLAMP_PX);
  });

  it('maps cumulative |delta| to the staircase step count', () => {
    expect(stepsForCumulativePx(0)).toBe(1);
    // The first acceleration bucket sits strictly above one notch's worth
    // of clamped |delta| (WHEEL_PER_EVENT_CLAMP_PX = 100), so a single
    // slow notch (cum=100, the maximum after one event) still walks just
    // one step — the user can land on the immediate prev/next message.
    expect(stepsForCumulativePx(100)).toBe(1);
    expect(stepsForCumulativePx(199)).toBe(1);
    expect(stepsForCumulativePx(200)).toBe(2);
    expect(stepsForCumulativePx(399)).toBe(2);
    expect(stepsForCumulativePx(400)).toBe(3);
    expect(stepsForCumulativePx(699)).toBe(3);
    expect(stepsForCumulativePx(700)).toBe(5);
    expect(stepsForCumulativePx(1099)).toBe(5);
    expect(stepsForCumulativePx(1100)).toBe(8);
    expect(stepsForCumulativePx(10_000)).toBe(8);
  });

  it('guarantees at least one step for any nonzero accumulator value', () => {
    // Regression: a single slow wheel notch (any |delta| up to the
    // per-event clamp of 100 px) must always advance exactly one step so
    // the user can always land on the immediate prev/next message. The
    // first acceleration bucket sits strictly above that ceiling.
    expect(stepsForCumulativePx(1)).toBe(1);
    expect(stepsForCumulativePx(WHEEL_PER_EVENT_CLAMP_PX)).toBe(1);
  });
});
