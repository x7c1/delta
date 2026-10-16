/** The units {@link formatAge} picks from, largest first, each in seconds. */
const UNITS: readonly [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 365 * 86_400],
  ['month', 30 * 86_400],
  ['week', 7 * 86_400],
  ['day', 86_400],
  ['hour', 3_600],
  ['minute', 60],
];

const RELATIVE = new Intl.RelativeTimeFormat('en', { numeric: 'always' });

/**
 * How long ago `then` was at `now`, in the largest whole unit (`3 days ago`,
 * `2 months ago`), or `just now` under a minute. A time in the future (a clock
 * skew between the server's file system and the browser) also reads
 * `just now`.
 */
export function formatAge(then: Date, now: Date): string {
  const seconds = Math.floor((now.getTime() - then.getTime()) / 1000);
  for (const [unit, size] of UNITS) {
    if (seconds >= size) {
      return RELATIVE.format(-Math.floor(seconds / size), unit);
    }
  }
  return 'just now';
}

/** The absolute time, for a tooltip beside {@link formatAge}'s figure. */
export function exactTime(then: Date): string {
  return then.toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' });
}
