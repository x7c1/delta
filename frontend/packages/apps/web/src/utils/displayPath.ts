/**
 * Abbreviate an absolute path for display by collapsing the home directory to
 * `~` (e.g. `/home/alice/p` → `~/p`). Returns the path unchanged when `home`
 * is unknown or the path is not under it. Display-only — never use the result
 * as a value sent to the server.
 */
export function displayPath(path: string, home: string | null): string {
  if (!home) return path;
  if (path === home) return '~';
  if (path.startsWith(home + '/')) return '~' + path.slice(home.length);
  return path;
}
