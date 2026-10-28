import { http, HttpResponse, type RequestHandler } from 'msw';
import type { LatestReleaseResponse } from '@delta/wire-gen';
import { MOCK_VERSION } from './releaseFixtures';

/** The version and newer-release routes. */
export function releaseHandlers(): RequestHandler[] {
  return [
    // Delta workspace version for the navigator footer. The real server owns
    // the format contract (`v<version>` on a release build,
    // `v<version>+dev.<sha>` on a local one); the mock returns the fixed
    // `MOCK_VERSION` so mock-mode e2e can assert on it without depending on
    // the host's git sha.
    http.get('*/api/version', () => HttpResponse.json({ version: MOCK_VERSION })),
    // No newer release by default, and the browser version's offer (none),
    // so the mock app's footer shows only the version; a test that wants the
    // update notice or the Update control overrides this handler with
    // `MOCK_NEWER_RELEASE` and the offer it needs.
    http.get('*/api/latest-release', () =>
      HttpResponse.json({
        newer: null,
        offer: 'none',
        download: null,
        installs: false,
        install: null,
      } satisfies LatestReleaseResponse),
    ),
    // The browser version may not update: the server refuses, as the real one
    // does for the CLI. A test of the Update control overrides this.
    http.post('*/api/latest-release/download', () =>
      HttpResponse.json(
        {
          error: 'updating is offered only in the desktop app',
          code: 'update_cli_launcher',
        },
        { status: 409 },
      ),
    ),
    // Nor install or restart: the CLI's refusal, and nothing is installed.
    // Tests of the Install and Restart controls override these.
    http.post('*/api/latest-release/install', () =>
      HttpResponse.json(
        {
          error: 'updating is offered only in the desktop app',
          code: 'update_cli_launcher',
        },
        { status: 409 },
      ),
    ),
    http.post('*/api/latest-release/restart', () =>
      HttpResponse.json(
        {
          error: 'no update has been installed to restart into',
          code: 'update_not_installed',
        },
        { status: 409 },
      ),
    ),
  ];
}
