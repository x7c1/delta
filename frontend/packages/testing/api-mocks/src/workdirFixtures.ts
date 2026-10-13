/** The mock filesystem's `$HOME`, used when a workdir list omits `path`. */
export const MOCK_WORKDIR_HOME = '/home/dev';

/**
 * A tiny static directory tree backing the workdir-picker mock. Each key is a
 * canonical directory; the value lists its immediate subdirectory names. A path
 * absent from this map is treated as "not a directory" (400).
 */
const MOCK_WORKDIR_TREE: Record<string, string[]> = {
  '/': ['home'],
  '/home': ['dev'],
  [MOCK_WORKDIR_HOME]: ['.config', 'projects', 'scratch'],
  '/home/dev/.config': [],
  '/home/dev/projects': ['delta', 'website'],
  '/home/dev/projects/delta': ['backend', 'frontend'],
  '/home/dev/projects/delta/backend': [],
  '/home/dev/projects/delta/frontend': [],
  '/home/dev/projects/website': [],
  '/home/dev/scratch': [],
};

/** The canonical parent of a path in the mock tree, or `null` at the root. */
function workdirParent(path: string): string | null {
  if (path === '/') {
    return null;
  }
  const parent = path.slice(0, path.lastIndexOf('/'));
  return parent === '' ? '/' : parent;
}

/**
 * One level of the mock browse for `path`, or `null` when `path` is not a known
 * directory (mapped to a 400 by the handler). Entries are name-sorted, dirs
 * only, and dot-directories are dropped unless `includeHidden` is set,
 * mirroring the real server.
 */
export function workdirListing(
  path: string,
  includeHidden = false,
): {
  path: string;
  parent: string | null;
  entries: { name: string; path: string }[];
} | null {
  const names = MOCK_WORKDIR_TREE[path];
  if (!names) {
    return null;
  }
  const entries = names
    .filter((name) => includeHidden || !name.startsWith('.'))
    .sort((a, b) => a.localeCompare(b))
    .map((name) => ({
      name,
      path: path === '/' ? `/${name}` : `${path}/${name}`,
    }));
  return { path, parent: workdirParent(path), entries };
}

/** Recently-used working directories for the mock, most-recent first. */
export function recentWorkdirs(): {
  path: string;
  last_used_at: string | null;
}[] {
  return [
    { path: '/home/dev/projects/delta', last_used_at: '2026-01-03T00:00:00Z' },
    { path: '/home/dev/projects/website', last_used_at: '2026-01-02T00:00:00Z' },
    { path: '/home/dev/scratch', last_used_at: null },
  ];
}

/**
 * The mock git repository: `/home/dev/projects/delta` and everything under it
 * is treated as one repo (root `/home/dev/projects/delta`, default branch
 * `main`). Every other directory is non-git, so the worktree toggle's show/hide
 * and the branches endpoint's 400-for-non-git path are both exercisable.
 */
export const MOCK_GIT_REPO_ROOT = '/home/dev/projects/delta';
const MOCK_GIT_DEFAULT_BRANCH = 'main';
const MOCK_GIT_REMOTE_BRANCHES = ['main', 'develop', 'release/1.0'];

/** Whether `path` is inside the mock git repository. */
function isInMockGitRepo(path: string): boolean {
  return path === MOCK_GIT_REPO_ROOT || path.startsWith(`${MOCK_GIT_REPO_ROOT}/`);
}

/**
 * The mock `GET /api/workdir/git` answer for `path`: a `repo_root` +
 * `default_branch` when the path is inside the mock repo, both `null`
 * otherwise. Mirrors the real endpoint, which never errors (it just reports
 * "not a git repo" as `repo_root: null`).
 */
export function gitRepoInfo(path: string): {
  repo_root: string | null;
  default_branch: string | null;
} {
  if (isInMockGitRepo(path)) {
    return {
      repo_root: MOCK_GIT_REPO_ROOT,
      default_branch: MOCK_GIT_DEFAULT_BRANCH,
    };
  }
  return { repo_root: null, default_branch: null };
}

/**
 * The mock `GET /api/workdir/git/branches` answer for `path`, or `null` when the
 * path is not a git repository (mapped to a 400 by the handler, matching the
 * real endpoint that rejects a non-git path).
 */
export function gitBranches(path: string): {
  default_branch: string | null;
  remote_branches: string[];
} | null {
  if (!isInMockGitRepo(path)) {
    return null;
  }
  return {
    default_branch: MOCK_GIT_DEFAULT_BRANCH,
    remote_branches: [...MOCK_GIT_REMOTE_BRANCHES],
  };
}
