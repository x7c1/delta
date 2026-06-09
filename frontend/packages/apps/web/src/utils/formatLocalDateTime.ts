/**
 * Format a stored UTC ISO-8601 timestamp as absolute local time in the
 * browser's timezone (`YYYY-MM-DD HH:mm`, or `YYYY-MM-DD HH:mm:ss` when
 * `withSeconds` is set). Returns `null` when the input is absent or unparseable
 * so the caller can render nothing.
 */
export function formatLocalDateTime(
  iso: string | null,
  withSeconds = false,
): string | null {
  if (!iso) {
    return null;
  }
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  const pad = (n: number) => String(n).padStart(2, '0');
  const y = date.getFullYear();
  const mo = pad(date.getMonth() + 1);
  const d = pad(date.getDate());
  const h = pad(date.getHours());
  const mi = pad(date.getMinutes());
  const base = `${y}-${mo}-${d} ${h}:${mi}`;
  return withSeconds ? `${base}:${pad(date.getSeconds())}` : base;
}
