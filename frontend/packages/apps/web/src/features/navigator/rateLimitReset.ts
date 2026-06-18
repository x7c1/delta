/**
 * Format a rate-limit window's `resets_at` (Unix epoch seconds) as a compact
 * relative countdown from `now` (defaults to the current time), e.g. `02h13m`
 * or `5d04h`. The footer prefixes it with the `↻` reset glyph.
 *
 * The two largest non-zero units are shown, in descending order, so the label
 * stays terse while still conveying scale:
 *
 * - days + hours when at least a day remains (`5d04h`),
 * - hours + minutes within a day (`02h13m`),
 * - minutes only under an hour (`13m`),
 * - `<1m` once under a minute (and for an already-elapsed reset), so the row
 *   never reads as a misleading `00m`.
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
    return `${days}d${pad(hours)}h`;
  }
  if (hours > 0) {
    return `${pad(hours)}h${pad(minutes)}m`;
  }
  return `${minutes}m`;
}

/** Zero-pad a sub-100 unit to two digits so the label stays fixed-width. */
function pad(value: number): string {
  return value.toString().padStart(2, '0');
}
