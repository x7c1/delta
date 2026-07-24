/**
 * Rolling window (ms) over which wheel-event |delta| magnitudes accumulate
 * so a vigorous spin advances more steps than a leisurely turn. Each event's
 * normalized contribution sticks around for this duration; once nothing
 * fires for longer than the window the accumulator resets to 0 on the next
 * event, so an unrelated later flick always starts fresh at the slowest
 * step.
 *
 * Tuned to ~250 ms: short enough that two deliberate but slow turns stay
 * independent (each at the lowest step), long enough that a multi-notch
 * spin compounds into the higher staircase buckets while the user's fingers
 * are still in motion. Exported so a test can drive the window timing
 * without sleeping wall-clock time.
 */
export const WHEEL_VELOCITY_WINDOW_MS = 250;

/**
 * Upper bound (px) on a single wheel event's |delta| contribution to the
 * accumulator. Trackpads emit many small pixel-mode events per flick (often
 * 5–20 px each); without per-event clamping a single inertial burst would
 * pile up hundreds of px and explode straight into the top staircase
 * bucket. The clamp sits at one mouse-wheel notch (~100 px on Linux /
 * Chrome) so a single notch always contributes at most one notch's worth
 * of acceleration regardless of the source device.
 */
export const WHEEL_PER_EVENT_CLAMP_PX = 100;

/**
 * Cool-down window (ms) between consecutive step commits emitted by the
 * wheel handler. Suppresses output (not input — the rolling-window
 * accumulator keeps feeding through cooldown), so a trackpad's continuous
 * pixel-mode event stream cannot fire more than one step per cooldown
 * tick while a typical mouse-wheel cadence (notches 150+ ms apart)
 * passes through unthrottled.
 *
 * Why the cooldown is needed in addition to the per-event clamp: the
 * clamp assumes "1 wheel notch = 1 wheel event", which holds for
 * traditional mouse wheels but NOT for macOS trackpads. A gentle trackpad
 * gesture emits a continuous stream of small pixel-mode events (~5–20 px
 * each, ~5–10 ms apart, plus inertial residue after the finger lifts);
 * each individual event sits below the clamp, so the clamp does not
 * protect the accumulator and the per-event 1-step commit fires on every
 * event. The cooldown gates the output side instead — the accumulator
 * and staircase keep working unchanged, only the commit rate is capped.
 *
 * 100 ms = 10 steps/sec ceiling. Mouse-wheel cadence is typically
 * 150+ ms between notches in normal use, so realistic mouse-wheel
 * scrubbing passes through unthrottled. Exported so tests can drive the
 * timing explicitly and a future tuning PR has one knob.
 */
export const WHEEL_STEP_COOLDOWN_MS = 100;

/**
 * `WheelEvent.deltaMode` indicates the unit of `deltaY` / `deltaX`. Pixel
 * mode (0) is the trackpad / high-resolution-mouse default and needs no
 * conversion; line mode (1) and page mode (2) report small integer counts
 * that must be scaled to a pixel-equivalent magnitude before clamping so
 * cross-device behaviour stays consistent. The multipliers are deliberate
 * approximations — one line ≈ 40 px, one page ≈ 800 px — matching the
 * staircase's notch-sized thresholds.
 */
export const WHEEL_DELTA_LINE_PX = 40;
export const WHEEL_DELTA_PAGE_PX = 800;

/**
 * Velocity → step-count staircase, encoded as descending-threshold entries
 * (highest bucket first so a top-down walk picks the first match). Each
 * entry maps "cumulative |delta| at least this large within the rolling
 * window" → "number of large-message steps to take on this wheel event".
 *
 * The first acceleration bucket sits strictly above one notch's worth of
 * accumulated |delta| ({@link WHEEL_PER_EVENT_CLAMP_PX} = 100 px), so a
 * single leisurely notch ALWAYS lands in the slowest bucket (1 step) — the
 * user can land on the immediate prev/next message. Acceleration only kicks
 * in once a second notch arrives inside the rolling window (cum ≥ 200), at
 * which point the staircase compounds: 2 / 3 / 5 / 8 steps at the 200 / 400
 * / 700 / 1100 px thresholds. A sustained vigorous spin still traverses a
 * long session in a handful of turns, but the bug where the very first
 * notch already jumped two messages is gone.
 *
 * Exported so tests can assert the calculator's behaviour against the
 * same thresholds the live UI uses, without duplicating magic numbers.
 */
export const WHEEL_STEP_STAIRCASE: ReadonlyArray<{
  readonly minCumulativePx: number;
  readonly steps: number;
}> = [
  { minCumulativePx: 1100, steps: 8 },
  { minCumulativePx: 700, steps: 5 },
  { minCumulativePx: 400, steps: 3 },
  { minCumulativePx: 200, steps: 2 },
  { minCumulativePx: 0, steps: 1 },
];

/**
 * Convert a raw `WheelEvent.deltaY` magnitude in the event's native
 * `deltaMode` to a pixel-equivalent magnitude, clamped to
 * {@link WHEEL_PER_EVENT_CLAMP_PX}. The conversion lets line / page-mode
 * scrolls compete on the same staircase as pixel-mode events; the clamp
 * bounds a single trackpad event so an inertial burst cannot explode the
 * accumulator.
 *
 * Exported for unit testing — the wheel handler is the only runtime caller.
 */
export function normalizeWheelDeltaPx(
  deltaMagnitude: number,
  deltaMode: number,
): number {
  const abs = Math.abs(deltaMagnitude);
  let scaled: number;
  if (deltaMode === 1) {
    scaled = abs * WHEEL_DELTA_LINE_PX;
  } else if (deltaMode === 2) {
    scaled = abs * WHEEL_DELTA_PAGE_PX;
  } else {
    scaled = abs;
  }
  return Math.min(scaled, WHEEL_PER_EVENT_CLAMP_PX);
}

/**
 * Map a cumulative |delta| (px, within the rolling window) to a step count
 * by walking {@link WHEEL_STEP_STAIRCASE} from the top bucket down — the
 * first entry whose threshold the cumulative value meets wins. Always
 * returns at least 1: any nonzero wheel input deserves at least one step,
 * so the user can always land on the immediate prev/next message with a
 * single slow notch (with the staircase's first acceleration bucket sitting
 * strictly above one notch's clamped contribution, a leisurely turn never
 * trips a higher bucket). Exported for unit testing.
 */
export function stepsForCumulativePx(cumulativePx: number): number {
  for (const entry of WHEEL_STEP_STAIRCASE) {
    if (cumulativePx >= entry.minCumulativePx) {
      return Math.max(1, entry.steps);
    }
  }
  // Defensive: the last entry's threshold is 0 so the loop above always
  // returns; keep an explicit fallback so a future edit that drops the
  // 0-threshold entry still degrades gracefully to a single step.
  return 1;
}
