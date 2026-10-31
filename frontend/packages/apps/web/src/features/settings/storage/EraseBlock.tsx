import { useRef, useState } from 'react';
import {
  ApiError,
  useEraseEverythingMutation,
  useStorageWorktreesQuery,
} from '@delta/api-client';
import type { StorageWorktree, StorageWorktreeBranch } from '@delta/wire-gen';
import { Button } from '@delta/ui-kit';
import { useApiClient } from '../../../data/apiContext';
import { ConfirmPanel } from './ConfirmPanel';
import { useShowErased } from './ErasedGate';
import { baseName, errorMessage } from './StorageParts';

/** The word typed to confirm, as the dirty-worktree removal types a name. */
const CONFIRM_WORD = 'erase';

/**
 * Whether erasing keeps `worktree`: it has changes, or git no longer knows
 * it, so Delta cannot vouch for its contents. A clean one goes, whether a
 * session works in it or not.
 */
function keepsWorktree(worktree: StorageWorktree): boolean {
  return worktree.dirty === true || worktree.repo_root === null;
}

/** A branch erasing leaves in place, with why, after "because". */
interface KeptBranch {
  name: string;
  worktree: string;
  reason: string;
}

/**
 * Why erasing keeps `branch` of `worktree` — the one checked out in it, or
 * the one Delta cut for its session before the session switched away from
 * it — or `null` when it deletes it. Erasing removes each session as
 * removing it alone would: a branch goes only when it is the one Delta
 * created for a session it still lists, its worktree goes, and git counts it
 * as merged.
 */
function branchKeepReason(
  worktree: StorageWorktree,
  branch: StorageWorktreeBranch,
): string | null {
  if (keepsWorktree(worktree)) {
    return 'its worktree has changes and stays';
  }
  if (!branch.created_for_session) {
    return worktree.in_use
      ? 'Delta did not create it'
      : 'no session Delta lists owns it';
  }
  return branch.merged === false ? 'it is not merged' : null;
}

/** The branches erasing will keep, from the Worktrees block's own listing. */
function keptBranches(worktrees: StorageWorktree[]): KeptBranch[] {
  return worktrees.flatMap((worktree) =>
    [worktree.branch, worktree.launch_branch].flatMap((branch) => {
      const reason = branch === null ? null : branchKeepReason(worktree, branch);
      return branch === null || reason === null
        ? []
        : [{ name: branch.name, worktree: worktree.path, reason }];
    }),
  );
}

/** The line a failed erase shows. */
function failureText(error: unknown): string {
  if (error instanceof ApiError && error.code === 'erase_in_progress') {
    return 'An erase is already running.';
  }
  return `Could not finish erasing: ${errorMessage(error)}. Sessions may already be closed or removed; try again to erase the rest.`;
}

/**
 * The Erase everything category, a child of Storage in the Settings menu:
 * erase everything Delta created on this machine that holds no work, then
 * stop Delta.
 *
 * Before the user confirms it says what will stay — the worktrees
 * {@link keepsWorktree} picks, by directory name, and the branches
 * {@link keptBranches} picks, each with why, both from the Worktrees block's
 * own query — and the confirmation needs the word `erase` typed. On success
 * the whole app is replaced by the report (see {@link useShowErased}).
 * `active` gates the worktree fetch to while the category is shown.
 */
export function EraseBlock({ active }: { active: boolean }) {
  const client = useApiClient();
  const showErased = useShowErased();
  const worktreesQuery = useStorageWorktreesQuery(client, active);
  const erase = useEraseEverythingMutation(client, showErased);
  const [confirming, setConfirming] = useState(false);
  const [typed, setTyped] = useState('');
  const openerRef = useRef<HTMLDivElement>(null);
  const worktrees = worktreesQuery.data?.worktrees ?? [];
  const kept = worktrees.filter(keepsWorktree);
  const branches = keptBranches(worktrees);

  const cancel = () => {
    setConfirming(false);
    setTyped('');
    erase.reset();
  };

  return (
    <section
      className="flex flex-col gap-2 rounded-lg border border-danger/30 bg-danger/5 px-3 py-3"
      data-testid="storage-erase"
    >
      <div className="flex flex-col gap-1">
        <h3 className="text-secondary font-semibold text-danger">Erase everything</h3>
        <p className="text-caption text-fg-muted">
          Delta closes every session, removes its worktrees and branches that
          hold no work, deletes its own files, and stops. What stays:
          worktrees with changes, branches that are not merged or that Delta
          did not create, the app itself, and Claude Code&apos;s and
          Codex&apos;s own files under{' '}
          <span className="font-mono">~/.claude</span> and{' '}
          <span className="font-mono">~/.codex</span>, which Delta does not own.
        </p>
      </div>
      {/* The desktop app quits as soon as the erase is done, so these lists
          are the only place its user learns what stays: say when it is not
          known yet rather than show nothing. */}
      {worktreesQuery.isPending ? (
        <p className="text-caption text-fg-muted">Checking which worktrees and branches will stay…</p>
      ) : worktreesQuery.isError ? (
        // This category shows no other view of the worktrees, so the retry
        // has to be here.
        <div className="flex items-center gap-2">
          <p className="text-caption text-danger" data-testid="storage-erase-kept-error">
            Could not list the worktrees and branches that will stay:{' '}
            {errorMessage(worktreesQuery.error)}
          </p>
          <Button size="sm" variant="secondary" onClick={() => worktreesQuery.refetch()}>
            Retry
          </Button>
        </div>
      ) : (
        <>
          {kept.length === 0 ? (
            <p className="text-caption text-fg-muted" data-testid="storage-erase-kept">
              No worktree will stay: none has changes.
            </p>
          ) : (
            <div className="text-caption text-fg-muted" data-testid="storage-erase-kept">
              <p>These worktrees may hold work and will stay:</p>
              <ul className="flex flex-col gap-0.5">
                {kept.map((worktree) => (
                  <li key={worktree.path} className="font-mono text-code" title={worktree.path}>
                    {baseName(worktree.path)}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {branches.length === 0 ? (
            <p className="text-caption text-fg-muted" data-testid="storage-erase-kept-branches">
              No branch of Delta&apos;s worktrees will stay.
            </p>
          ) : (
            <div className="text-caption text-fg-muted" data-testid="storage-erase-kept-branches">
              <p>These branches will stay:</p>
              <ul className="flex flex-col gap-0.5">
                {branches.map((branch) => (
                  <li key={`${branch.worktree}:${branch.name}`} title={branch.worktree}>
                    <span className="font-mono text-code text-fg">{branch.name}</span>, because{' '}
                    {branch.reason}.
                  </li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}
      {confirming ? (
        <ConfirmPanel
          confirmLabel="Erase everything and stop Delta"
          onConfirm={() => erase.mutate()}
          onCancel={cancel}
          confirmDisabled={typed !== CONFIRM_WORD}
          pending={erase.isPending}
          error={erase.isError ? failureText(erase.error) : null}
          testId="storage-erase-confirm"
          returnFocusTo={openerRef}
        >
          <p>
            Every session&apos;s conversation in Delta goes for good, and Delta
            stops. The worktrees and branches listed above stay on disk.
          </p>
          <label className="flex flex-col gap-1 text-fg-muted">
            <span>
              Type <span className="font-mono text-fg">{CONFIRM_WORD}</span> to confirm.
            </span>
            <input
              type="text"
              value={typed}
              onChange={(event) => setTyped(event.target.value)}
              aria-label="Word to confirm"
              autoComplete="off"
              spellCheck={false}
              className="rounded-sm border border-border-default bg-surface px-2 py-1 font-mono text-code text-fg focus:border-accent-hover focus:outline-hidden"
            />
          </label>
        </ConfirmPanel>
      ) : (
        <div ref={openerRef}>
          <Button size="sm" onClick={() => setConfirming(true)}>
            Erase everything…
          </Button>
        </div>
      )}
    </section>
  );
}
