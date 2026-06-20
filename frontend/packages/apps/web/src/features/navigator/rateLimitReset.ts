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
