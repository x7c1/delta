import { useState } from 'react';
import {
  ApiError,
  useEraseEverythingMutation,
  useStorageWorktreesQuery,
} from '@delta/api-client';
import type { StorageWorktree } from '@delta/wire-gen';
import { Button } from '@delta/ui-kit';
import { useApiClient } from '../../../data/apiContext';
import { ConfirmPanel } from './ConfirmPanel';
import { useShowErased } from './ErasedGate';
import { baseName, errorMessage } from './StorageParts';

/** The word typed to confirm, as the dirty-worktree removal types a name. */
const CONFIRM_WORD = 'erase';

/**
 * The worktrees erasing will keep: those with changes, and those git no longer
 * knows, whose contents Delta cannot vouch for. A clean one goes, whether a
 * session works in it or not.
 */
function keptWorktrees(worktrees: StorageWorktree[]): StorageWorktree[] {
  return worktrees.filter((worktree) => worktree.dirty === true || worktree.repo_root === null);
}

/** The line a failed erase shows. */
function failureText(error: unknown): string {
  if (error instanceof ApiError && error.code === 'erase_in_progress') {
    return 'An erase is already running.';
  }
  return `Could not finish erasing: ${errorMessage(error)}. Sessions may already be closed or removed; try again to erase the rest.`;
}

/**
 * Storage block, the last one and set apart: erase everything Delta created
 * on this machine that holds no work, then stop Delta.
 *
 * Before the user confirms it says what will stay — the worktrees
 * {@link keptWorktrees} picks, by directory name, from the Worktrees block's
 * own query — and the confirmation needs the word `erase` typed. On success
 * the whole app is replaced by the report (see {@link useShowErased}).
 * `active` gates the worktree fetch to while the Storage category is shown.
 */
export function EraseBlock({ active }: { active: boolean }) {
  const client = useApiClient();
  const showErased = useShowErased();
  const worktreesQuery = useStorageWorktreesQuery(client, active);
  const erase = useEraseEverythingMutation(client, showErased);
  const [confirming, setConfirming] = useState(false);
  const [typed, setTyped] = useState('');
  const kept = keptWorktrees(worktreesQuery.data?.worktrees ?? []);

  const cancel = () => {
    setConfirming(false);
    setTyped('');
    erase.reset();
  };

  return (
    <section
      className="mt-6 flex flex-col gap-2 rounded-lg border border-danger/30 bg-danger/5 px-3 py-3"
      data-testid="storage-erase"
    >
      <div className="flex flex-col gap-1">
        <h4 className="text-secondary font-medium text-danger">Erase everything</h4>
        <p className="text-caption text-fg-muted">
          Delta closes every session, removes its worktrees and branches that
          hold no work, deletes its own files, and stops. What stays:
          worktrees with changes and branches that are not merged, the app
          itself, and Claude Code&apos;s and Codex&apos;s own files under{' '}
          <span className="font-mono">~/.claude</span> and{' '}
          <span className="font-mono">~/.codex</span>, which Delta does not own.
        </p>
      </div>
      {/* The desktop app quits as soon as the erase is done, so this list is
          the only place its user learns what stays: say when it is not known
          yet rather than show nothing. */}
      {worktreesQuery.isPending ? (
        <p className="text-caption text-fg-muted">Checking which worktrees will stay…</p>
      ) : worktreesQuery.isError ? (
        <p className="text-caption text-danger" data-testid="storage-erase-kept-error">
          Could not list the worktrees that will stay: {errorMessage(worktreesQuery.error)}
        </p>
      ) : kept.length === 0 ? (
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
      {confirming ? (
        <ConfirmPanel
          confirmLabel="Erase everything and stop Delta"
          onConfirm={() => erase.mutate()}
          onCancel={cancel}
          confirmDisabled={typed !== CONFIRM_WORD}
          pending={erase.isPending}
          error={erase.isError ? failureText(erase.error) : null}
          testId="storage-erase-confirm"
        >
          <p>
            Every session&apos;s conversation in Delta goes for good, and Delta
            stops. Worktrees and branches holding work stay on disk.
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
              className="rounded border border-border-default bg-surface px-2 py-1 font-mono text-code text-fg focus:border-accent-hover focus:outline-none"
            />
          </label>
        </ConfirmPanel>
      ) : (
        <div>
          <Button size="sm" onClick={() => setConfirming(true)}>
            Erase everything…
          </Button>
        </div>
      )}
    </section>
  );
}
