import { describe, expect, it } from 'vitest';
import { formatLocalDateTime } from './formatLocalDateTime';

describe('formatLocalDateTime', () => {
  it('formats a UTC ISO-8601 timestamp as local `YYYY-MM-DD HH:mm`', () => {
    const iso = '2026-06-08T14:30:00Z';
    const date = new Date(iso);
    const pad = (n: number) => String(n).padStart(2, '0');
    const expected = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(
      date.getDate(),
    )} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
    expect(formatLocalDateTime(iso)).toBe(expected);
  });

  it('returns null for a missing timestamp', () => {
    expect(formatLocalDateTime(null)).toBeNull();
  });

  it('returns null for an unparseable timestamp', () => {
    expect(formatLocalDateTime('not-a-date')).toBeNull();
  });
});
