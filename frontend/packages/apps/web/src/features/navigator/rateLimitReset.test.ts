import { describe, expect, it } from 'vitest';
import {
  computeElapsedPercentage,
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

describe('computeElapsedPercentage', () => {
  it('returns the linear time fraction at mid-window', () => {
    // 7d window, 5d remaining → 2d elapsed → 2/7 ≈ 28.571…
    const resetsAt = NOW_S + 5 * 86400;
    const result = computeElapsedPercentage(
      resetsAt,
      SEVEN_DAYS_SECONDS,
      NOW_MS,
    );
    expect(result).toBeCloseTo((2 / 7) * 100, 5);
  });

  it('returns 0 immediately after a reset', () => {
    // Window just reset: full duration remains.
    const resetsAt = NOW_S + SEVEN_DAYS_SECONDS;
    expect(
      computeElapsedPercentage(resetsAt, SEVEN_DAYS_SECONDS, NOW_MS),
    ).toBe(0);
  });

  it('clamps to 100 once the window has fully elapsed', () => {
    // The reset moment is now: 0 seconds remain → 100% elapsed.
    expect(computeElapsedPercentage(NOW_S, FIVE_HOURS_SECONDS, NOW_MS)).toBe(
      100,
    );
    // Already past the reset: also clamped to 100.
    expect(
      computeElapsedPercentage(NOW_S - 600, FIVE_HOURS_SECONDS, NOW_MS),
    ).toBe(100);
  });

  it('clamps to 0 if the reset is somehow further out than the window', () => {
    // Defensive: pathological inputs should not produce a negative percentage.
    const resetsAt = NOW_S + SEVEN_DAYS_SECONDS + 3600;
    expect(
      computeElapsedPercentage(resetsAt, SEVEN_DAYS_SECONDS, NOW_MS),
    ).toBe(0);
  });
});
