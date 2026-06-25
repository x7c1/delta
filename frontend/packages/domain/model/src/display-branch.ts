/**
 * Match a `delta-`-prefixed UUID v7 / v4 branch name (the canonical 36-char
 * hyphenated body), case-insensitive. Delta spawns each session's worktree
 * branch as `delta-<session-id>` where `<session-id>` is a UUID — outside
 * delta the prefix tags the branch as "managed by delta", but inside the UI
 * the prefix is noise and the 36-char UUID is unreadable. {@link displayBranch}
 * recognises only this exact shape; any other name (including `delta-` followed
 * by something that is not a UUID, or a name that contains `delta-` mid-string)
 * passes through unchanged.
 */
const DELTA_UUID_BRANCH = /^delta-([0-9a-f]{8})-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/**
 * Shorten a delta-managed branch name for display.
 *
 * The backend, the on-disk worktree path, and the `branch_at_launch` value
 * stored in the database all keep the full `delta-<uuid>` name — only the UI
 * presentation flows through this helper. The first 8 hex characters of the
 * UUID are kept (matching the git short-sha convention used elsewhere in the
 * UI) so distinct delta-spawned sessions stay distinguishable at a glance.
 *
 * Any branch name that does not match the canonical shape — including
 * user-created branches, plain names like `main` or `feat/foo`, names that
 * have already been shortened, and names with surrounding whitespace — is
 * returned untouched. The caller should still expose the original name via
 * a hover-reveal (HTML `title` or equivalent) so the full identifier remains
 * recoverable.
 */
export function displayBranch(name: string): string {
  const match = DELTA_UUID_BRANCH.exec(name);
  if (match === null) {
    return name;
  }
  return match[1];
}
