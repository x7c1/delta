import { describe, expect, it } from 'vitest';
import { formatResetCountdown } from './rateLimitReset';

const NOW_MS = 1_700_000_000_000;
const NOW_S = NOW_MS / 1000;

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
