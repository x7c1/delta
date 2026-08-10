import { describe, expect, it } from 'vitest';
import { windowBucketCount, windowLabel } from './rateLimitWindow';

const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

describe('windowLabel', () => {
  it('reproduces Claude\'s two fixed windows from their durations alone', () => {
    // The rows these replace were hardcoded `5h` / `7d`. Deriving them from the
    // duration must be byte-identical, or the change is visible to Claude users.
    expect(windowLabel(5 * HOUR)).toBe('5h');
    expect(windowLabel(7 * DAY)).toBe('7d');
  });

  it('labels a window in its largest whole unit', () => {
    expect(windowLabel(DAY)).toBe('1d');
    expect(windowLabel(30 * DAY)).toBe('30d');
    expect(windowLabel(HOUR)).toBe('1h');
    expect(windowLabel(23 * HOUR)).toBe('23h');
    expect(windowLabel(30 * MINUTE)).toBe('30m');
  });

  it('has no label for a window whose length the provider did not report', () => {
    // `null` is the caller's cue to show the "length unknown" placeholder
    // rather than invent a duration.
    expect(windowLabel(null)).toBeNull();
    expect(windowLabel(0)).toBeNull();
    expect(windowLabel(-1)).toBeNull();
  });

  it('never labels a sub-minute window as 0m', () => {
    // A window shorter than its own smallest unit still reads as one unit,
    // which is at least honest about the order of magnitude.
    expect(windowLabel(30)).toBe('1m');
  });
});

describe('windowBucketCount', () => {
  it('paces each window in its own natural unit', () => {
    // 7 daily buckets for the 7-day window, 5 hourly ones for the 5-hour
    // window — the counts the footer used to hardcode next to each row.
    expect(windowBucketCount(7 * DAY)).toBe(7);
    expect(windowBucketCount(5 * HOUR)).toBe(5);
    expect(windowBucketCount(DAY)).toBe(1);
    expect(windowBucketCount(45 * MINUTE)).toBe(45);
  });

  it('is at least one bucket, so the pacing math can never divide by zero', () => {
    expect(windowBucketCount(0)).toBe(1);
    expect(windowBucketCount(10)).toBe(1);
  });
});
