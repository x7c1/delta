import { describe, expect, it } from 'vitest';
import { pullRequestUrl } from './pull-request-url';

describe('pullRequestUrl', () => {
  it('builds the GitHub PR URL from an `org/repo` label and the number', () => {
    expect(pullRequestUrl('x7c1/delta', 138)).toBe(
      'https://github.com/x7c1/delta/pull/138',
    );
  });

  it('returns null when the session has no repository label', () => {
    // A session launched outside a git repo, or a row that predates the
    // `repository_display_name` column: nothing names a GitHub repository.
    expect(pullRequestUrl(null, 138)).toBeNull();
  });

  it('returns null for a basename-shaped label', () => {
    // The backend falls back to the working-tree basename when the clone has
    // no `origin`. A basename is not a GitHub repository, so linking to
    // `github.com/delta/pull/138` would be an invented URL.
    expect(pullRequestUrl('delta', 138)).toBeNull();
  });

  it('returns null for a label with more than two segments', () => {
    // A self-hosted-style `host/org/repo` label would otherwise produce a URL
    // pointing at the wrong repository.
    expect(pullRequestUrl('git.example.com/x7c1/delta', 138)).toBeNull();
  });

  it('returns null for a label carrying path traversal or whitespace', () => {
    expect(pullRequestUrl('../x7c1/delta', 138)).toBeNull();
    expect(pullRequestUrl('x7c1 /delta', 138)).toBeNull();
    // These two do have exactly two segments, so the `<org>/<repo>` shape
    // alone accepts them — but a browser resolves
    // `github.com/../delta/pull/138` to a wholly different repository's URL,
    // which is the invented link this helper exists to withhold.
    expect(pullRequestUrl('../delta', 138)).toBeNull();
    expect(pullRequestUrl('x7c1/..', 138)).toBeNull();
  });

  it('returns null for a number no pull request could have', () => {
    expect(pullRequestUrl('x7c1/delta', 0)).toBeNull();
    expect(pullRequestUrl('x7c1/delta', -1)).toBeNull();
    expect(pullRequestUrl('x7c1/delta', 1.5)).toBeNull();
  });
});
