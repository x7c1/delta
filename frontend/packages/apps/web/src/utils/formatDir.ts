/**
 * Display a working-directory path with the home directory collapsed to `~`.
 *
 * The transcript records absolute paths (e.g. `/home/alice/repos/x`). For
 * display we collapse a leading home directory to `~` (`~/repos/x`), matching
 * how shells and most tools render paths. The browser has no access to `$HOME`,
 * so this is a heuristic on the path's own leading segment: `/home/<user>`,
 * `/Users/<user>` (macOS), or `/root`. Anything else is returned unchanged.
 */
export function formatDir(cwd: string): string {
  return cwd.replace(/^(?:\/home\/[^/]+|\/Users\/[^/]+|\/root)(?=\/|$)/, '~');
}
