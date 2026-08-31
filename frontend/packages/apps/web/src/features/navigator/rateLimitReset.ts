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

/**
 * Format the instant (epoch ms) at which a restored rate-limit reading was
 * last observed, as an absolute local date and time (`Aug 27, 2026, 10:32 PM`).
 *
 * Absolute rather than relative on purpose: the row already carries a relative
 * reading in the `↻` countdown, and two relative durations pointing in
 * opposite directions read as a puzzle. The question a de-emphasized row
 * raises — "how old is this number?" — is answered most directly by naming the
 * moment it was taken.
 */
export function formatObservedAt(observedAtMs: number): string {
  return new Date(observedAtMs).toLocaleString(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  });
}

/** Zero-pad a sub-100 unit to two digits so the label stays fixed-width. */
function pad(value: number): string {
  return value.toString().padStart(2, '0');
}

/**
 * Position of the "budget line" — how much of the window the user may spend
 * up to and including the current bucket, as a 0–100 percentage anchored to
 * the right edge of the bar (i.e. distance from the reset edge, leftward).
 *
 * The window is split into `bucketCount` equal buckets (7 days for the 7d
 * window, 5 hours for the 5h window) and the line steps one bucket to the
 * left each time the clock crosses a boundary. Right after a reset the line
 * sits at `1 / bucketCount` from the right — the first bucket's worth of
 * budget is fair game today; on the final bucket the line reaches the left
 * edge (100%) — the entire window is fair game.
 *
 * Callers overlay this marker on the usage fill. Since the fill also grows
 * leftward from the reset edge, the invariant is intuitive: fill INSIDE
 * (right of) the line = spending within this bucket's share; fill CROSSING
 * (left of) the line = spending ahead of the per-bucket pace.
 */
export function computeBudgetLinePercentage(
  resetsAt: number,
  windowDurationSeconds: number,
  bucketCount: number,
  now: number = Date.now(),
): number {
  // `resetsAt` is epoch seconds but `now` is milliseconds; convert before
  // the subtraction so both operands share the same unit.
  const nowSeconds = now / 1000;
  const remainingSeconds = resetsAt - nowSeconds;
  const elapsedSeconds = windowDurationSeconds - remainingSeconds;
  const bucketSize = windowDurationSeconds / bucketCount;
  // Clamp to a valid bucket index even for pathological inputs (past reset,
  // or `resetsAt` further out than the window) so the caller never sees a
  // negative or over-100 marker position.
  const rawIndex = Math.floor(elapsedSeconds / bucketSize);
  const bucketIndex = Math.min(bucketCount - 1, Math.max(0, rawIndex));
  return ((bucketIndex + 1) / bucketCount) * 100;
}
