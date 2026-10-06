import { useState } from 'react';
import {
  ApiError,
  useRemoveStorageWorktreeMutation,
  useStorageWorktreesQuery,
} from '@delta/api-client';
import type { StorageWorktree } from '@delta/wire-gen';
import { Button, Spinner } from '@delta/ui-kit';
import { useApiClient } from '../../../data/apiContext';
import { ConfirmPanel } from './ConfirmPanel';
import { baseName, CopyButton, errorMessage, PathText } from './StorageParts';

/** What a worktree is, as its row says it. */
type WorktreeState = 'in_use' | 'clean' | 'dirty' | 'unregistered';

function stateOf(worktree: StorageWorktree): WorktreeState {
  if (worktree.in_use) {
    return 'in_use';
  }
  if (worktree.repo_root === null || worktree.dirty === null) {
    return 'unregistered';
  }
  return worktree.dirty ? 'dirty' : 'clean';
}

const STATE_TEXT: Record<WorktreeState, string> = {
  in_use: 'In use by a listed session',
  clean: 'Clean',
  dirty: 'Has uncommitted changes',
  unregistered: 'Not registered with git',
};

/**
 * Storage block: the directories under the worktree base.
 *
 * Every directory is listed, the ones a listed session still works in marked
 * and offered no control — removing the session is how those go. The rest are
 * leftovers: kept because they held work when their session was removed, or
 * left by versions that did not clean up. A clean leftover is removed after a
 * plain confirmation; one with changes, or one git no longer knows, only after
 * the user types its directory name, because its contents are lost. `active`
 * gates the fetch to while the Storage category is shown.
 */
export function WorktreesBlock({ active }: { active: boolean }) {
  const client = useApiClient();
  const worktreesQuery = useStorageWorktreesQuery(client, active);

  return (
    <section className="flex flex-col gap-2" data-testid="storage-worktrees">
      <div>
        <h4 className="text-secondary font-medium text-fg">Worktrees</h4>
        <p className="text-caption text-fg-muted">
          The directories under the worktree base. One a listed session works
          in goes with that session&apos;s Remove, unless it holds work. The
          others are left over and can be removed here; their branches stay.
        </p>
      </div>
      {worktreesQuery.isPending ? (
        <div className="flex justify-center py-2">
          <Spinner label="loading worktrees" />
        </div>
      ) : worktreesQuery.isError ? (
        <div className="flex items-center gap-2 text-caption text-fg-muted">
          <p>Could not load the worktrees.</p>
          <Button size="sm" variant="secondary" onClick={() => worktreesQuery.refetch()}>
            Retry
          </Button>
        </div>
      ) : worktreesQuery.data.worktrees.length === 0 ? (
        <p className="text-caption text-fg-subtle" data-testid="storage-no-worktrees">
          No worktrees.
        </p>
      ) : (
        <ul className="flex flex-col gap-2" data-testid="storage-worktree-list">
          {worktreesQuery.data.worktrees.map((worktree) => (
            <WorktreeRow key={worktree.path} worktree={worktree} />
          ))}
        </ul>
      )}
    </section>
  );
}

/** The line a refused removal shows, by the server's code. */
function refusalText(error: unknown): string {
  if (error instanceof ApiError) {
    switch (error.code) {
      case 'worktree_in_use':
        return 'A listed session works in it now. Remove that session instead.';
      case 'worktree_dirty':
        return 'It has uncommitted changes now. To remove it and lose them, type its name above.';
      case 'worktree_not_registered':
        return 'Git no longer knows it. To remove it and lose its contents, type its name above.';
      case 'worktree_outside_base':
        return 'It is no longer under the worktree base.';
    }
  }
  return errorMessage(error);
}

function WorktreeRow({ worktree }: { worktree: StorageWorktree }) {
  const client = useApiClient();
  const remove = useRemoveStorageWorktreeMutation(client);
  const [confirming, setConfirming] = useState(false);
  const [typed, setTyped] = useState('');
  const state = stateOf(worktree);
  const name = baseName(worktree.path);
  // Destroying work needs the name typed; a clean worktree goes through git's
  // own unforced removal, which still refuses if work appeared since.
  const force = state === 'dirty' || state === 'unregistered';

  const cancel = () => {
    setConfirming(false);
    setTyped('');
    remove.reset();
  };

  return (
    <li
      className="flex flex-col gap-1 rounded-lg border border-border-default px-3 py-2"
      data-testid="storage-worktree"
    >
      <div className="flex items-center gap-2">
        <PathText value={worktree.path} />
        <CopyButton value={worktree.path} label={`worktree path ${name}`} />
        {state !== 'in_use' && !confirming && (
          <Button
            size="sm"
            variant="ghost"
            className="shrink-0"
            onClick={() => setConfirming(true)}
            aria-label={`Remove worktree ${name}`}
          >
            Remove
          </Button>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-x-3 text-caption text-fg-muted">
        <span data-testid="storage-worktree-state" className={state === 'dirty' ? 'text-warning' : undefined}>
          {STATE_TEXT[state]}
        </span>
        {worktree.repo_root !== null && (
          <span className="min-w-0 truncate" title={worktree.repo_root}>
            Repository <span className="font-mono">{worktree.repo_root}</span>
          </span>
        )}
      </div>
      {confirming && (
        <ConfirmPanel
          confirmLabel={force ? 'Remove and lose changes' : 'Remove worktree'}
          onConfirm={() => remove.mutate({ path: worktree.path, force })}
          onCancel={cancel}
          // A refusal refreshes the list: a session that started in it since
          // leaves nothing to confirm, a worktree that gained changes asks for its name.
          confirmDisabled={state === 'in_use' || (force && typed !== name)}
          pending={remove.isPending}
          error={remove.isError ? refusalText(remove.error) : null}
          testId="storage-worktree-confirm"
        >
          {force ? (
            <>
              <p>
                {state === 'dirty'
                  ? 'This worktree has uncommitted or untracked changes. Removing it deletes them, and they cannot be recovered.'
                  : 'Git no longer knows this directory, so Delta cannot tell whether it holds work. Removing it deletes everything in it, and it cannot be recovered.'}
              </p>
              <label className="flex flex-col gap-1 text-fg-muted">
                <span>
                  Type <span className="font-mono text-fg">{name}</span> to confirm.
                </span>
                <input
                  type="text"
                  value={typed}
                  onChange={(event) => setTyped(event.target.value)}
                  aria-label="Directory name to confirm"
                  autoComplete="off"
                  spellCheck={false}
                  className="rounded border border-border-default bg-surface px-2 py-1 font-mono text-code text-fg focus:border-accent-hover focus:outline-none"
                />
              </label>
            </>
          ) : (
            <p>
              Remove the worktree <span className="font-mono">{name}</span>? It
              has no uncommitted changes; its branch is kept.
            </p>
          )}
        </ConfirmPanel>
      )}
    </li>
  );
}
