import { http, HttpResponse, type RequestHandler } from 'msw';
import type {
  PrunePreviewResponse,
  PruneSessionsRequest,
  PruneSessionsResponse,
  PruneStatus,
  RemoveWorktreeRequest,
  DeleteSnapshotRequest,
  EraseResponse,
  StorageWorktreesResponse,
} from '@delta/wire-gen';
import { mockStorage } from './storageFixtures';
import type { HandlerContext } from './handlerContext';

/** Both prune statuses: what a prune request that names none means. */
const PRUNE_STATUSES: PruneStatus[] = ['ended', 'failed'];

function isPruneStatus(value: string): value is PruneStatus {
  return (PRUNE_STATUSES as string[]).includes(value);
}

/**
 * The storage routes (inventory, worktree and snapshot removal, erase) and the
 * bulk session removal.
 */
export function storageHandlers({
  store,
  isLive,
  pruneMatches,
  emitServerEvent,
}: HandlerContext): RequestHandler[] {
  return [
    http.get('*/api/storage', () =>
      HttpResponse.json({ ...mockStorage, snapshots: store.snapshots }),
    ),

    // The directories under the worktree base, and removing one. The refusals
    // mirror the server's, each with its stable code, so the Storage block's
    // confirmation paths are drivable in mock mode.
    http.get('*/api/storage/worktrees', () => {
      const body: StorageWorktreesResponse = { worktrees: store.storageWorktrees };
      return HttpResponse.json(body);
    }),
    http.delete('*/api/storage/worktrees', async ({ request }) => {
      const body = (await request.json()) as RemoveWorktreeRequest;
      const force = body.force ?? false;
      const worktree = store.storageWorktrees.find((w) => w.path === body.path);
      const refuse = (error: string, code: string) =>
        HttpResponse.json({ error: `${error}: ${body.path}`, code }, { status: 409 });
      if (!worktree) {
        return refuse(
          'not a directory directly under the worktree base',
          'worktree_outside_base',
        );
      }
      if (worktree.in_use) {
        return refuse('a listed session still works in this worktree', 'worktree_in_use');
      }
      if (worktree.dirty === true && !force) {
        return refuse(
          'the worktree has uncommitted or untracked changes',
          'worktree_dirty',
        );
      }
      if (worktree.repo_root === null && !force) {
        return refuse(
          'git does not know this directory as a worktree',
          'worktree_not_registered',
        );
      }
      store.storageWorktrees = store.storageWorktrees.filter((w) => w !== worktree);
      return new HttpResponse(null, { status: 204 });
    }),

    // Delete a migration snapshot: only one the inventory lists.
    http.delete('*/api/storage/snapshots', async ({ request }) => {
      const body = (await request.json()) as DeleteSnapshotRequest;
      if (!store.snapshots.some((snapshot) => snapshot.path === body.path)) {
        return HttpResponse.json(
          { error: `not a listed migration snapshot: ${body.path}` },
          { status: 404 },
        );
      }
      store.snapshots = store.snapshots.filter((snapshot) => snapshot.path !== body.path);
      return new HttpResponse(null, { status: 204 });
    }),

    // Erase everything: every session goes, and of the worktrees only the
    // clean ones git knows; the rest are kept with the reason the server
    // gives. A second erase is refused, as the real server refuses one while
    // it stops.
    http.post('*/api/storage/erase', () => {
      if (store.erased) {
        return HttpResponse.json(
          { error: 'an erase is already in progress', code: 'erase_in_progress' },
          { status: 409 },
        );
      }
      store.erased = true;
      const response: EraseResponse = {
        removed: {
          sessions: store.sessions.length,
          worktrees: [],
          branches: [],
          data_dir: mockStorage.data_dir,
        },
        kept: [],
      };
      for (const worktree of store.storageWorktrees) {
        const keepsWorktree = worktree.dirty === true || worktree.repo_root === null;
        if (keepsWorktree) {
          response.kept.push({
            session_id: null,
            kind: 'worktree',
            target: worktree.path,
            reason: worktree.repo_root === null ? 'not_registered' : 'dirty',
            detail: null,
          });
        } else {
          response.removed.worktrees.push(worktree.path);
        }
        // Only the branch Delta created for a listed session goes, once its
        // worktree has gone and it is merged; the real erase reports a kept
        // branch against its session, which the mock does not model.
        for (const branch of [worktree.branch, worktree.launch_branch]) {
          if (branch === null) {
            continue;
          }
          if (!keepsWorktree && branch.created_for_session && branch.merged === true) {
            response.removed.branches.push(branch.name);
          } else if (!keepsWorktree && branch.created_for_session) {
            response.kept.push({
              session_id: null,
              kind: 'branch',
              target: branch.name,
              reason: 'unmerged',
              detail: null,
            });
          }
        }
      }
      store.storageWorktrees = store.storageWorktrees.filter(
        (worktree) => !response.removed.worktrees.includes(worktree.path),
      );
      store.sessions = [];
      store.sends = [];
      store.messagesByThread = {};
      store.snapshots = [];
      return HttpResponse.json(response);
    }),

    // The bulk session removal and its preview. Like the single removal, each
    // removed session rides on its emitted `session_removed`.
    http.get('*/api/sessions/prune', ({ request }) => {
      const params = new URL(request.url).searchParams;
      const days = Number(params.get('older_than_days'));
      const statusParam = params.get('statuses');
      const statuses =
        statusParam === null
          ? PRUNE_STATUSES
          : statusParam.split(',').filter((value) => value.length > 0);
      if (
        params.get('older_than_days') === null ||
        !Number.isInteger(days) ||
        days < 0 ||
        !statuses.every(isPruneStatus)
      ) {
        return HttpResponse.json({ error: 'malformed prune query' }, { status: 400 });
      }
      const ids = pruneMatches(days, statuses as PruneStatus[])
        .filter((entry) => !isLive(entry))
        .map((entry) => entry.session.id);
      const body: PrunePreviewResponse = { count: ids.length, session_ids: ids };
      return HttpResponse.json(body);
    }),
    http.post('*/api/sessions/prune', async ({ request }) => {
      const body = (await request.json()) as PruneSessionsRequest;
      if (!Number.isInteger(body.older_than_days) || body.older_than_days < 0) {
        return HttpResponse.json({ error: 'malformed prune request' }, { status: 422 });
      }
      const response: PruneSessionsResponse = {
        removed: 0,
        removed_ids: [],
        skipped: [],
        kept: [],
      };
      for (const entry of pruneMatches(body.older_than_days, body.statuses ?? PRUNE_STATUSES)) {
        if (isLive(entry)) {
          response.skipped.push({
            session_id: entry.session.id,
            reason: entry.open ? 'open' : 'starting',
            detail: null,
          });
          continue;
        }
        response.removed_ids.push(entry.session.id);
        emitServerEvent({ kind: 'session_removed', session_id: entry.session.id });
      }
      response.removed = response.removed_ids.length;
      return HttpResponse.json(response);
    }),
  ];
}
