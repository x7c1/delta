import { describe, expect, it } from 'vitest';
import { formatAge } from './formatAge';

const NOW = new Date('2026-10-16T12:00:00Z');

/** `seconds` before {@link NOW}. */
function ago(seconds: number): Date {
  return new Date(NOW.getTime() - seconds * 1000);
}

describe('formatAge', () => {
  it('reads the largest whole unit', () => {
    expect(formatAge(ago(30), NOW)).toBe('just now');
    expect(formatAge(ago(60), NOW)).toBe('1 minute ago');
    expect(formatAge(ago(5 * 3_600 + 59), NOW)).toBe('5 hours ago');
    expect(formatAge(ago(3 * 86_400), NOW)).toBe('3 days ago');
    expect(formatAge(ago(13 * 86_400), NOW)).toBe('1 week ago');
    expect(formatAge(ago(65 * 86_400), NOW)).toBe('2 months ago');
    expect(formatAge(ago(800 * 86_400), NOW)).toBe('2 years ago');
  });

  it('reads a time in the future as just now', () => {
    expect(formatAge(ago(-120), NOW)).toBe('just now');
  });
});
