/**
 * An `<org>/<repo>` repository identity label: exactly two non-empty segments,
 * neither of which may itself contain a slash or whitespace.
 *
 * The backend's `repository_display_name` is either such a label (derived from
 * the clone's `origin` URL) or a plain working-tree basename (a clone with no
 * `origin` configured), and the two are indistinguishable by anything but
 * shape — so the shape is what decides whether a URL can be built at all.
 */
const ORG_REPO = /^[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+$/;

/**
 * A segment made of nothing but dots (`.`, `..`). {@link ORG_REPO}'s character
 * class admits them, but they are relative-path steps rather than names:
 * `../delta` would build `https://github.com/../delta/pull/1`, which a browser
 * normalizes into some other repository's URL. Checked separately so the label
 * shape itself stays one readable expression.
 */
const DOT_ONLY_SEGMENT = /^\.+$/;

/**
 * The GitHub web URL of the pull request a session was opened from, or `null`
 * when one cannot be formed.
 *
 * Delta's PR flow is `github.com`-only (the backend's PR listing pins that
 * host), and a PR-picked session's `repository_display_name` names the very
 * repository the PR lives in — so the URL is rebuilt from the label and the
 * number rather than stored alongside them.
 *
 * Returns `null` when `repositoryDisplayName` is `null` (the session was not
 * launched inside a git repository, or the row predates the column) or is not
 * `<org>/<repo>`-shaped — the backend falls back to the working-tree basename
 * for a clone with no `origin`, and a basename names no GitHub repository —
 * and likewise for a number no pull request could carry. The caller renders the
 * bare `#<number>` as plain text in those cases: a wrong link is worse than no
 * link.
 */
export function pullRequestUrl(
  repositoryDisplayName: string | null,
  pullRequestNumber: number,
): string | null {
  if (repositoryDisplayName === null || !ORG_REPO.test(repositoryDisplayName)) {
    return null;
  }
  const segments = repositoryDisplayName.split('/');
  if (segments.some((segment) => DOT_ONLY_SEGMENT.test(segment))) {
    return null;
  }
  if (!Number.isInteger(pullRequestNumber) || pullRequestNumber <= 0) {
    return null;
  }
  return `https://github.com/${repositoryDisplayName}/pull/${pullRequestNumber}`;
}
