import { http, HttpResponse, type RequestHandler } from 'msw';
import type {
  GitBranchesResponse,
  GitRepoResponse,
  WorkdirListResponse,
  WorkdirRecentResponse,
} from '@delta/wire-gen';
import {
  gitBranches,
  gitRepoInfo,
  MOCK_WORKDIR_HOME,
  recentWorkdirs,
  workdirListing,
} from './workdirFixtures';
import type { HandlerContext } from './handlerContext';

/**
 * The working-directory routes: the picker's browse and recent list, the git
 * probes, and opening a session's cwd in an external tool.
 */
export function workdirHandlers({ store }: HandlerContext): RequestHandler[] {
  return [
    // Open the given path in an external tool (VS Code today). The mock has
    // no real subprocess to spawn, so it just answers `204` on any allowlist
    // hit — a path that appears in a session's cwd or in any stored message's
    // cwd. Anything else answers `400` with the stable code, mirroring the
    // real server's allowlist check.
    http.post('*/api/open-cwd', async ({ request }) => {
      const payload = (await request.json()) as {
        path?: string;
        handler?: string | null;
      };
      const path = typeof payload?.path === 'string' ? payload.path.trim() : '';
      if (path === '') {
        return HttpResponse.json(
          { error: '`path` must be a non-blank string' },
          { status: 400 },
        );
      }
      if (
        typeof payload.handler === 'string' &&
        payload.handler !== 'vscode'
      ) {
        return HttpResponse.json(
          {
            error: `unknown open-cwd handler: ${payload.handler}`,
            code: 'open_cwd_unknown_handler',
          },
          { status: 400 },
        );
      }
      const known = new Set<string>();
      for (const entry of store.sessions) {
        known.add(entry.session.cwd);
      }
      for (const msgs of Object.values(store.messagesByThread)) {
        for (const m of msgs) {
          if (typeof m.cwd === 'string') {
            known.add(m.cwd);
          }
        }
      }
      if (!known.has(path)) {
        return HttpResponse.json(
          {
            error: `path is not in the known-cwd allowlist: ${path}`,
            code: 'open_cwd_path_not_allowed',
          },
          { status: 400 },
        );
      }
      return new HttpResponse(null, { status: 204 });
    }),

    // Browse one level of the (mock) filesystem for the new-session picker.
    // An omitted `path` lists $HOME; an unknown path is a 400 and the special
    // `/forbidden` path a 403, exercising the inline-error path. `hidden=true`
    // also lists dot-directories, as on the real server.
    http.get('*/api/workdir/list', ({ request }) => {
      const url = new URL(request.url);
      const path = url.searchParams.get('path') ?? MOCK_WORKDIR_HOME;
      if (path === '/forbidden') {
        return HttpResponse.json(
          { error: 'permission denied' },
          { status: 403 },
        );
      }
      const includeHidden = url.searchParams.get('hidden') === 'true';
      const listing = workdirListing(path, includeHidden);
      if (!listing) {
        return HttpResponse.json(
          { error: 'not a directory' },
          { status: 400 },
        );
      }
      const responseBody: WorkdirListResponse = listing;
      return HttpResponse.json(responseBody);
    }),

    http.get('*/api/workdir/recent', () => {
      const responseBody: WorkdirRecentResponse = {
        workdirs: recentWorkdirs(),
      };
      return HttpResponse.json(responseBody);
    }),

    // Whether the queried directory is a git repository, for the new-session
    // worktree option. A non-git path is not an error — it reports
    // `repo_root: null` — but `path` is required: a missing or blank one is the
    // same 400 the real endpoint's `require_path` returns, and the value is
    // trimmed before it is resolved. A path under the mock repo reports its root
    // and default branch, so the worktree toggle's show/hide is exercisable.
    http.get('*/api/workdir/git', ({ request }) => {
      const url = new URL(request.url);
      const path = url.searchParams.get('path')?.trim() ?? '';
      if (path.length === 0) {
        return HttpResponse.json(
          { error: 'a `path` query parameter is required' },
          { status: 400 },
        );
      }
      const responseBody: GitRepoResponse = gitRepoInfo(path);
      return HttpResponse.json(responseBody);
    }),

    // The repository's remote branches (the lazily-fetched start-point list). A
    // non-git path is a 400, exactly as the real endpoint rejects it, so the
    // picker's inline-error path is exercisable; a missing or blank `path` is
    // the same 400 for the same reason as above.
    http.get('*/api/workdir/git/branches', ({ request }) => {
      const url = new URL(request.url);
      const path = url.searchParams.get('path')?.trim() ?? '';
      if (path.length === 0) {
        return HttpResponse.json(
          { error: 'a `path` query parameter is required' },
          { status: 400 },
        );
      }
      const branches = gitBranches(path);
      if (!branches) {
        return HttpResponse.json(
          { error: 'not a git repository' },
          { status: 400 },
        );
      }
      const responseBody: GitBranchesResponse = branches;
      return HttpResponse.json(responseBody);
    }),
  ];
}
