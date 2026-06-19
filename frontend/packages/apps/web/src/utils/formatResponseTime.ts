/**
 * Format a turn's response time (milliseconds) as a short human-readable
 * duration: `850ms` under a second, `9.4s` under a minute, `1m03s` beyond.
 * Returns `null` when the input is absent or negative so the caller can render
 * nothing.
 */
export function formatResponseTime(ms: number | null): string | null {
  if (ms === null || !Number.isFinite(ms) || ms < 0) {
    return null;
  }
  if (ms < 1000) {
    return `${Math.round(ms)}ms`;
  }
  const totalSeconds = ms / 1000;
  if (totalSeconds < 60) {
    // One decimal under a minute keeps it precise without noise.
    return `${totalSeconds.toFixed(1)}s`;
  }
  // Round to whole seconds first, then split: rounding the minute and second
  // parts independently would emit `1m60s` for e.g. 119_500ms (1m59.5s) instead
  // of carrying into `2m00s`.
  const wholeSeconds = Math.round(totalSeconds);
  const minutes = Math.floor(wholeSeconds / 60);
  const seconds = wholeSeconds % 60;
  return `${minutes}m${String(seconds).padStart(2, '0')}s`;
}
