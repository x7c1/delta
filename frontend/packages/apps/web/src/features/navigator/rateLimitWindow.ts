/**
 * Turning a rate-limit window's *duration* into the two things the footer row
 * needs: its label and its budget-line bucket count.
 *
 * A window arrives identified by how long it is, not by a name — Claude reports
 * a fixed 5-hour and 7-day pair while Codex reports anonymous windows carrying
 * an explicit duration, and a UI that hardcoded `5h` / `7d` rows could only ever
 * render the first provider's. Deriving both properties from the duration means
 * a window the app has never seen (a 24-hour one, a 30-minute one) renders
 * correctly with no change here, and Claude's rows come out byte-identical to
 * the hardcoded ones they replace.
 */

const MINUTE_SECONDS = 60;
const HOUR_SECONDS = 60 * MINUTE_SECONDS;
const DAY_SECONDS = 24 * HOUR_SECONDS;

/**
 * The label shown at the head of a window's row: the duration in its largest
 * whole unit (`7d`, `5h`, `30m`).
 *
 * `null` when the provider reported a window without saying how long it is —
 * the window's percentage is still real and worth showing, so the row renders
 * with {@link UNKNOWN_WINDOW_LABEL} rather than being hidden or given an
 * invented duration.
 */
export function windowLabel(durationSeconds: number | null): string | null {
  if (durationSeconds === null || durationSeconds <= 0) return null;
  if (durationSeconds >= DAY_SECONDS) {
    return `${Math.round(durationSeconds / DAY_SECONDS)}d`;
  }
  if (durationSeconds >= HOUR_SECONDS) {
    return `${Math.round(durationSeconds / HOUR_SECONDS)}h`;
  }
  return `${Math.max(1, Math.round(durationSeconds / MINUTE_SECONDS))}m`;
}

/**
 * Placeholder label for a window whose duration the provider did not report.
 * An em dash, so the fixed-width label column still reads as "a window, length
 * unknown" instead of implying a duration the server never sent.
 */
export const UNKNOWN_WINDOW_LABEL = '—';

/**
 * How many buckets the budget line splits this window into: one per unit of the
 * window's own natural scale — 7 for a 7-day window (a day each), 5 for a
 * 5-hour one (an hour each). That is what makes the pacing marker mean "this
 * much is fair game so far today / this hour" rather than an arbitrary fraction.
 *
 * At least 1, so a window shorter than its own unit still yields a valid
 * single-bucket line rather than a division by zero. Takes a duration, never
 * `null`: a window without one is drawn with no budget line at all, so the
 * caller has already ruled that case out.
 */
export function windowBucketCount(durationSeconds: number): number {
  const unit =
    durationSeconds >= DAY_SECONDS
      ? DAY_SECONDS
      : durationSeconds >= HOUR_SECONDS
        ? HOUR_SECONDS
        : MINUTE_SECONDS;
  return Math.max(1, Math.round(durationSeconds / unit));
}
