/** The units {@link formatBytes} steps through, each 1024 times the previous. */
const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

/**
 * Humanise a byte count for display (`3250176` → `3.1 MB`), in powers of 1024,
 * the units `du -h` and `ls -lh` use. Below 1 KB the exact count is shown
 * (`512 B`); above it, one decimal place.
 */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  // Step on the rounded figure, so `1048575` reads `1.0 MB`, not `1024.0 KB`.
  while (Number(value.toFixed(1)) >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${value.toFixed(1)} ${UNITS[unit]}`;
}

/** The exact byte count, for a tooltip beside {@link formatBytes}' figure. */
export function exactBytes(bytes: number): string {
  return `${bytes.toLocaleString('en-US')} bytes`;
}
