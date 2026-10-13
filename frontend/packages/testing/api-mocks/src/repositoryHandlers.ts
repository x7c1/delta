import { http, HttpResponse, type RequestHandler } from 'msw';
import type {
  CloneRepositoryRequest,
  CreateCloneRootRequest,
  CloneRoot,
  CloneRootsResponse,
  PullRequestsResponse,
  RepositoriesResponse,
} from '@delta/wire-gen';
import {
  mockAuthorPullRequests,
  mockRepositories,
  mockReviewerPullRequests,
} from './repositoryFixtures';
import { workdirListing } from './workdirFixtures';
import type { HandlerContext } from './handlerContext';

/**
 * Decode a URL-safe base64 token used by the clone-root DELETE path segment,
 * mirroring the server-side decoder. Returns `null` for any non-base64url
 * byte or invalid UTF-8. The implementation uses `atob` after re-padding and
 * substituting the URL-safe variants.
 */
function decodeBase64Url(token: string): string | null {
  // Restore the standard base64 alphabet and re-pad to a multiple of 4 so
  // `atob` can parse it.
  const base64 = token.replace(/-/g, '+').replace(/_/g, '/');
  const padded = base64.padEnd(base64.length + ((4 - (base64.length % 4)) % 4), '=');
  try {
    const binary = atob(padded);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

/**
 * Where a clone lands: `<clone_root>/<repo_name>`, the server's one rule with
 * no fallback naming.
 */
function cloneDestination(cloneRoot: string, repoName: string): string {
  // Trailing slashes go, so a bare `/` root yields `/<name>` — the same join
  // the server does, and the same one the PR tab predicts.
  return `${cloneRoot.replace(/\/+$/, '')}/${repoName}`;
}

/**
 * Whether the mock filesystem already has a directory at `path`. Standing in
 * for the real server's destination-exists check, using the same little tree the
 * workdir picker browses.
 */
function mockDirectoryExists(path: string): boolean {
  return workdirListing(path) !== null;
}

/**
 * The repository routes: the Repository and PR tabs, cloning, and the clone
 * roots.
 */
export function repositoryHandlers({
  store,
  clonedRepositoryEntries,
}: HandlerContext): RequestHandler[] {
  return [
    // Registered repositories for the Repository tab. Mirrors the real
    // endpoint's shape; the default list seeds two entries (one
    // origin-deduplicated repo with two clones, one path-keyed single
    // clone) so the picker's clone-expansion + path-key affordance are
    // exercisable.
    http.get('*/api/repositories', () => {
      const responseBody: RepositoriesResponse = {
        repositories: [...mockRepositories(), ...clonedRepositoryEntries()],
      };
      return HttpResponse.json(responseBody);
    }),

    // Clone a repository into a registered clone root. Reproduces the real
    // server's refusals (an unregistered root, an occupied destination) and
    // otherwise answers 202 having done nothing: the mock has no filesystem, so
    // a clone "lands" only when a scripted `repository_clone_completed` event is
    // applied — see `applyEvent`.
    http.post('*/api/repositories/clone', async ({ request }) => {
      const payload = (await request.json()) as CloneRepositoryRequest;
      const cloneRoot = payload?.clone_root ?? '';
      if (!store.cloneRoots.some((root) => root.path === cloneRoot)) {
        return HttpResponse.json(
          {
            error: `not a registered clone root: ${cloneRoot}`,
            code: 'clone_root_not_registered',
          },
          { status: 400 },
        );
      }
      const destination = cloneDestination(cloneRoot, payload.repo_name);
      // The mock's stand-in for the filesystem: the workdir tree plus whatever
      // this session has already cloned. Either counts as "already there".
      const occupied =
        mockDirectoryExists(destination) ||
        store.clonedRepos.some((entry) => entry.path === destination);
      if (occupied) {
        return HttpResponse.json(
          {
            error: `clone destination already exists: ${destination}`,
            code: 'clone_dest_exists',
          },
          { status: 409 },
        );
      }
      return new HttpResponse(null, { status: 202 });
    }),

    // Pull requests for the PR tab. Each lens carries its own canned
    // list (the reviewer fixture pairs a clone-having row with a
    // no-clone row so the inline clone panel is exercisable; the author
    // fixture seeds one of the user's own drafts). An unknown lens is a
    // 400, mirroring the server.
    http.get('*/api/prs', ({ request }) => {
      const url = new URL(request.url);
      const lens = url.searchParams.get('lens');
      const pull_requests =
        lens === 'reviewer'
          ? mockReviewerPullRequests()
          : lens === 'author'
            ? mockAuthorPullRequests()
            : null;
      if (pull_requests === null) {
        return HttpResponse.json(
          { error: `unknown lens '${lens ?? ''}'` },
          { status: 400 },
        );
      }
      const responseBody: PullRequestsResponse = {
        gh_available: true,
        // A repository cloned during this session has a local clone from here
        // on, exactly as the real endpoint would report after the clone landed.
        pull_requests: pull_requests.map((pr) => ({
          ...pr,
          has_local_clone:
            pr.has_local_clone ||
            store.clonedRepos.some(
              (entry) => entry.key === `${pr.repo_owner}/${pr.repo_name}`,
            ),
        })),
      };
      return HttpResponse.json(responseBody);
    }),

    // Clone roots: list, create, delete. The mock reproduces the real server's
    // contract — newest-first ordering, trailing-slash trim, duplicate-path 409
    // with the stable `clone_root_duplicate` code, and an idempotent delete on
    // an unknown path.
    http.get('*/api/clone-roots', () => {
      const clone_roots = [...store.cloneRoots].sort((a, b) =>
        b.created_at.localeCompare(a.created_at) || a.path.localeCompare(b.path),
      );
      const body: CloneRootsResponse = {
        clone_roots: clone_roots.map((root) => ({ path: root.path })),
      };
      return HttpResponse.json(body);
    }),

    http.post('*/api/clone-roots', async ({ request }) => {
      const payload = (await request.json()) as CreateCloneRootRequest;
      const rawPath = typeof payload?.path === 'string' ? payload.path.trim() : '';
      // Only the bare `/` is exempt from the stripping: it is the one all-slash
      // spelling the contract takes as a deliberate root, while `'//'` and
      // `'///'` strip down to nothing, blank like `''` and `'   '`. Same rule
      // as the server's `create_clone_root`.
      const canonical = rawPath === '/' ? '/' : rawPath.replace(/\/+$/, '');
      if (canonical.length === 0) {
        return HttpResponse.json(
          { error: 'a clone root must have a non-blank `path`' },
          { status: 400 },
        );
      }
      if (!canonical.startsWith('/')) {
        return HttpResponse.json(
          { error: 'a clone root `path` must be absolute (start with `/`)' },
          { status: 400 },
        );
      }
      if (store.cloneRoots.some((r) => r.path === canonical)) {
        return HttpResponse.json(
          { error: `clone root already registered: ${canonical}`, code: 'clone_root_duplicate' },
          { status: 409 },
        );
      }
      store.cloneRoots.push({
        path: canonical,
        // Stored only for the newest-first list ordering; stripped from the wire.
        created_at: new Date().toISOString(),
      });
      const row: CloneRoot = { path: canonical };
      return HttpResponse.json(row, { status: 201 });
    }),

    http.delete('*/api/clone-roots/:path_b64', ({ params }) => {
      const token = String(params.path_b64 ?? '');
      const decoded = decodeBase64Url(token);
      if (decoded === null) {
        return HttpResponse.json(
          { error: 'malformed clone-root path token' },
          { status: 400 },
        );
      }
      // Idempotent: an unknown path is still a 204.
      store.cloneRoots = store.cloneRoots.filter((r) => r.path !== decoded);
      return new HttpResponse(null, { status: 204 });
    }),
  ];
}
