/**
 * The last `count` path segments of an absolute path, joined by `/`
 * (e.g. `/a/b/c/d/e` → `d/e`). Used to identify a session by its working
 * directory compactly. Returns '' for an empty path or `/`.
 */
export function pathTail(path: string, count = 2): string {
  const parts = path.split('/').filter(Boolean);
  return parts.slice(-count).join('/');
}
