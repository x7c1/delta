/**
 * Format a rate-limit window's `resets_at` (Unix epoch seconds) as a compact
 * relative countdown from `now` (defaults to the current time), e.g. `02h13m`
 * or `05d04h`. The footer prefixes it with the `↻` reset glyph.
 *
 * The two largest units are shown, in descending order, zero-padded so the
 * label stays a consistent 6-character width across the common cases — the
 * footer stacks two rows and the fixed column keeps the `↻` glyph aligned:
 *
 * - days + hours when at least a day remains (`05d04h`),
 * - hours + minutes within a day (`02h13m`),
 * - minutes under an hour, with a `00h` prefix so the width matches (`00h13m`),
 * - `<1m` once under a minute (and for an already-elapsed reset), so the row
 *   never reads as a misleading `00m`. This is a brief special-case window
 *   where the consistent column width is intentionally relaxed.
 */
export function formatResetCountdown(
  resetsAt: number,
  now: number = Date.now(),
): string {
  // `resetsAt` is epoch seconds but `now` is milliseconds, so convert `now` to
  // seconds first; the named intermediate keeps both sides of the subtraction in
  // the same unit and avoids any operator-precedence guesswork.
  const nowSeconds = now / 1000;
  const remainingSeconds = Math.floor(resetsAt - nowSeconds);
  if (remainingSeconds < 60) {
    return '<1m';
  }
  const totalMinutes = Math.floor(remainingSeconds / 60);
  const days = Math.floor(totalMinutes / (60 * 24));
  const hours = Math.floor((totalMinutes % (60 * 24)) / 60);
  const minutes = totalMinutes % 60;

  if (days > 0) {
    return `${pad(days)}d${pad(hours)}h`;
  }
  return `${pad(hours)}h${pad(minutes)}m`;
}

/** Zero-pad a sub-100 unit to two digits so the label stays fixed-width. */
function pad(value: number): string {
  return value.toString().padStart(2, '0');
}

/**
 * The share of the rolling window that has already elapsed, as a 0–100
 * percentage. Computed from `resetsAt` (Unix epoch seconds, the right
 * edge of the window) and the window's total length: the moment of reset
 * sits at 0, the moment of the previous reset at 100, and anywhere in
 * between is a linear time fraction.
 *
 * Callers anchor a marker on the rate-limit bar at this position to show
 * "where we are in the window right now" — overlaying the marker on the
 * usage fill makes whether consumption is running ahead of or behind a
 * straight-line burn obvious without any numbers.
 */
export function computeElapsedPercentage(
  resetsAt: number,
  windowDurationSeconds: number,
  now: number = Date.now(),
): number {
  // `resetsAt` is epoch seconds but `now` is milliseconds; convert before
  // the subtraction so both operands share the same unit.
  const nowSeconds = now / 1000;
  const remainingSeconds = resetsAt - nowSeconds;
  const elapsedSeconds = windowDurationSeconds - remainingSeconds;
  const fraction = elapsedSeconds / windowDurationSeconds;
  return Math.min(100, Math.max(0, fraction * 100));
}
