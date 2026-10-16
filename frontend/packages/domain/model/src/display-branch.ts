/**
 * Delta names each session's worktree branch `delta-<session-id>`. Outside
 * Delta the prefix tags the branch as managed by Delta; inside the UI it is
 * noise and the full UUID is unreadable.
 */
const DELTA_UUID_BRANCH = /^delta-([0-9a-f]{8})-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/**
 * Shorten a Delta-managed branch name for display; any other name is shown as
 * is.
 *
 * Only the presentation changes: the backend, the worktree path, and the
 * stored `branch_at_launch` keep the full name. The caller should still expose
 * the full name on hover (HTML `title` or equivalent) so it stays recoverable.
 */
export function displayBranch(name: string): string {
  const match = DELTA_UUID_BRANCH.exec(name);
  if (match === null) {
    return name;
  }
  return match[1];
}
