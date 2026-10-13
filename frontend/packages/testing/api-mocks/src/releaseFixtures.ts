import type { NewerRelease } from '@delta/wire-gen';

/**
 * The version string the mock `GET /api/version` returns. Fixed rather than
 * derived from the workspace `package.json` so mock-mode e2e can assert on it
 * verbatim without importing build metadata into the mock package.
 */
export const MOCK_VERSION = 'v0.0.0-mock';

/**
 * A newer published release, for tests that override the mock
 * `GET /api/latest-release` (which answers `{ newer: null, offer: 'none' }` by default, so the
 * mock app shows no update notice unless a test asks for one).
 */
export const MOCK_NEWER_RELEASE: NewerRelease = {
  version: 'v0.0.1',
  url: 'https://github.com/x7c1/delta/releases/tag/v0.0.1',
};
