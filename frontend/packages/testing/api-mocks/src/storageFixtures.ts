import type { StorageResponse, StorageWorktree } from '@delta/wire-gen';
import { MOCK_VERSION } from './releaseFixtures';

/** The worktree base the mock `GET /api/storage` reports. */
export const MOCK_WORKTREE_BASE = '/home/u/.delta/worktrees';

/** The repository every mock worktree under {@link MOCK_WORKTREE_BASE} is cut from. */
const MOCK_WORKTREE_REPO = '/home/u/src/delta';

/**
 * The directories under {@link MOCK_WORKTREE_BASE} that the mock
 * `GET /api/storage/worktrees` lists, sorted by name, one per state the
 * Worktrees block renders: a clean leftover, a leftover with uncommitted
 * changes, a leftover git no longer knows, and one a listed session still
 * works in, on the unmerged branch Delta created for it — so the erase
 * confirmation has a branch to name for each reason it keeps one.
 */
export const mockStorageWorktrees: StorageWorktree[] = [
  {
    path: `${MOCK_WORKTREE_BASE}/x7c1-delta-leftover-clean`,
    in_use: false,
    repo_root: MOCK_WORKTREE_REPO,
    dirty: false,
    branch: { name: 'delta-leftover-clean', created_for_session: false, merged: null },
    launch_branch: null,
  },
  {
    path: `${MOCK_WORKTREE_BASE}/x7c1-delta-leftover-dirty`,
    in_use: false,
    repo_root: MOCK_WORKTREE_REPO,
    dirty: true,
    branch: { name: 'delta-leftover-dirty', created_for_session: false, merged: null },
    launch_branch: null,
  },
  {
    path: `${MOCK_WORKTREE_BASE}/x7c1-delta-leftover-unregistered`,
    in_use: false,
    repo_root: null,
    dirty: null,
    branch: null,
    launch_branch: null,
  },
  {
    path: `${MOCK_WORKTREE_BASE}/x7c1-delta-session-1`,
    in_use: true,
    repo_root: MOCK_WORKTREE_REPO,
    dirty: false,
    branch: { name: 'delta-session-1', created_for_session: true, merged: false },
    launch_branch: null,
  },
];

/** The data directory the mock `GET /api/storage` reports. */
export const MOCK_DATA_DIR = '/home/u/.local/share/io.github.x7c1.delta';

/**
 * The inventory the mock `GET /api/storage` returns: placeholder paths under
 * {@link MOCK_DATA_DIR}, two migration snapshots so mock mode shows the
 * snapshot sub-list (and can delete one and still show the other), and the
 * same version string as `GET /api/version`.
 */
export const mockStorage: StorageResponse = {
  identifier: 'io.github.x7c1.delta',
  version: MOCK_VERSION,
  data_dir: MOCK_DATA_DIR,
  database: { path: `${MOCK_DATA_DIR}/delta.db`, bytes: 3_250_176 },
  snapshots: [
    { path: `${MOCK_DATA_DIR}/delta.db.bak-v3`, bytes: 1_048_576 },
    { path: `${MOCK_DATA_DIR}/delta.db.bak-v5`, bytes: 2_097_152 },
  ],
  updates: { path: `${MOCK_DATA_DIR}/updates`, bytes: 9_437_184 },
  hook_state: `${MOCK_DATA_DIR}/delta-hook-state.json`,
  sessions_dir: `${MOCK_DATA_DIR}/sessions`,
  session_settings: `${MOCK_DATA_DIR}/settings/7878.json`,
  tmux_conf: `${MOCK_DATA_DIR}/tmux.conf`,
  tmux_socket: 'io.github.x7c1.delta',
  worktree_base: MOCK_WORKTREE_BASE,
  transcript_root: '/home/u/.claude/projects',
};
