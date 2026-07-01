import { describe, expect, it } from 'vitest';
import {
  computeBudgetLinePercentage,
  formatResetCountdown,
} from './rateLimitReset';

const NOW_MS = 1_700_000_000_000;
const NOW_S = NOW_MS / 1000;

const SEVEN_DAYS_SECONDS = 7 * 24 * 60 * 60;
const FIVE_HOURS_SECONDS = 5 * 60 * 60;

describe('formatResetCountdown', () => {
  it('shows hours and minutes within a day', () => {
    const resetsAt = NOW_S + 2 * 3600 + 13 * 60;
    expect(formatResetCountdown(resetsAt, NOW_MS)).toBe('02h13m');
  });

  it('shows days and hours past a day', () => {
    const resetsAt = NOW_S + 5 * 86400 + 4 * 3600;
    expect(formatResetCountdown(resetsAt, NOW_MS)).toBe('05d04h');
  });

  it('shows minutes under an hour with a zero-padded hours prefix', () => {
    const resetsAt = NOW_S + 13 * 60;
    expect(formatResetCountdown(resetsAt, NOW_MS)).toBe('00h13m');
  });

  it('reads <1m once under a minute or already elapsed', () => {
    expect(formatResetCountdown(NOW_S + 30, NOW_MS)).toBe('<1m');
    expect(formatResetCountdown(NOW_S - 100, NOW_MS)).toBe('<1m');
  });
});

describe('computeBudgetLinePercentage', () => {
  it('returns the current bucket end at mid-window (5h)', () => {
    // 5h window, 3h remaining → 2h elapsed → bucket index 2 → 3/5 = 60%.
    const resetsAt = NOW_S + 3 * 3600;
    const result = computeBudgetLinePercentage(
      resetsAt,
      FIVE_HOURS_SECONDS,
      5,
      NOW_MS,
    );
    expect(result).toBe(60);
  });

  it('returns the current bucket end at mid-window (7d)', () => {
    // 7d window, 5d remaining → 2d elapsed → bucket index 2 → 3/7.
    const resetsAt = NOW_S + 5 * 86400;
    const result = computeBudgetLinePercentage(
      resetsAt,
      SEVEN_DAYS_SECONDS,
      7,
      NOW_MS,
    );
    expect(result).toBeCloseTo((3 / 7) * 100, 5);
  });

  it('returns 1 / bucketCount immediately after a reset (5h)', () => {
    // Window just reset: full duration remains → bucket index 0 → 1/5 = 20%.
    const resetsAt = NOW_S + FIVE_HOURS_SECONDS;
    expect(
      computeBudgetLinePercentage(resetsAt, FIVE_HOURS_SECONDS, 5, NOW_MS),
    ).toBe(20);
  });

  it('returns 100 on the final bucket (7d)', () => {
    // 1d remaining out of 7d → 6d elapsed → bucket index 6 (last) → 7/7 = 100%.
    const resetsAt = NOW_S + 1 * 86400;
    expect(
      computeBudgetLinePercentage(resetsAt, SEVEN_DAYS_SECONDS, 7, NOW_MS),
    ).toBe(100);
  });

  it('stays step-wise stable within a bucket', () => {
    // 5h window, 2h45m remaining → 2h15m elapsed. Continuous fraction would
    // give 45%, but the bucket index is still 2, so the line stays at 60%.
    const resetsAt = NOW_S + 2 * 3600 + 45 * 60;
    expect(
      computeBudgetLinePercentage(resetsAt, FIVE_HOURS_SECONDS, 5, NOW_MS),
    ).toBe(60);
  });

  it('clamps to 100 once the window has fully elapsed', () => {
    // The reset moment is now: 0 seconds remain → last bucket → 100%.
    expect(
      computeBudgetLinePercentage(NOW_S, FIVE_HOURS_SECONDS, 5, NOW_MS),
    ).toBe(100);
    // Already past the reset: also clamped to 100.
    expect(
      computeBudgetLinePercentage(NOW_S - 600, FIVE_HOURS_SECONDS, 5, NOW_MS),
    ).toBe(100);
  });

  it('clamps to 1 / bucketCount when the reset is further out than the window', () => {
    // Defensive: pathological input (elapsed < 0) should not produce a
    // negative bucket index — clamp to the first bucket.
    const resetsAt = NOW_S + FIVE_HOURS_SECONDS + 3600;
    expect(
      computeBudgetLinePercentage(resetsAt, FIVE_HOURS_SECONDS, 5, NOW_MS),
    ).toBe(20);
  });
});
