import { useCallback, useEffect, useRef, useState } from 'react';
import {
  ApiError,
  useRemoveStorageWorktreeMutation,
  useStorageWorktreesQuery,
} from '@delta/api-client';
import type { StorageWorktree } from '@delta/wire-gen';
import { Button, Spinner } from '@delta/ui-kit';
import { useApiClient } from '../../../data/apiContext';
import { ConfirmPanel } from './ConfirmPanel';
import { exactTime, formatAge } from './formatAge';
import { baseName, CopyButton, errorMessage, PathText } from './StorageParts';

/**
 * What git says of a worktree, as its row says it: whether removing it can
 * lose work. Whether a session uses it is said by the group it is listed in.
 */
type WorktreeState = 'clean' | 'dirty' | 'unregistered';

function stateOf(worktree: StorageWorktree): WorktreeState {
  if (worktree.repo_root === null || worktree.dirty === null) {
    return 'unregistered';
  }
  return worktree.dirty ? 'dirty' : 'clean';
}

const STATE_TEXT: Record<WorktreeState, string> = {
  clean: 'No uncommitted changes',
  dirty: 'Has uncommitted changes',
  unregistered: 'Not registered with git, so changes are unknown',
};

/**
 * `worktrees` newest first by modification time, those without one last;
 * ties keep the server's name order. The timestamps are ISO-8601 UTC, which
 * compare correctly as text.
 */
function newestFirst(worktrees: StorageWorktree[]): StorageWorktree[] {
  return [...worktrees].sort((a, b) => {
    if (a.modified_at === b.modified_at) {
      return 0;
    }
    if (a.modified_at === null) {
      return 1;
    }
    if (b.modified_at === null) {
      return -1;
    }
    return a.modified_at < b.modified_at ? 1 : -1;
  });
}

/**
 * Storage block: the directories under the worktree base.
 *
 * Every directory is listed, in two groups, each newest first: the
 * leftovers, first because they are what the user comes here to act on, and
 * the ones a listed session still works in, offered no control — removing the
 * session is how those go. Leftovers were kept because they held work when
 * their session was removed, or left by versions that did not clean up. A
 * clean leftover is removed after a plain confirmation; one with changes, or
 * one git no longer knows, only after the user types its directory name,
 * because its contents are lost. `active` gates the fetch to while the
 * Storage category is shown.
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
        <WorktreeGroups worktrees={worktreesQuery.data.worktrees} />
      )}
    </section>
  );
}

/** The listed worktrees, leftovers first, a group left out when it is empty. */
function WorktreeGroups({ worktrees }: { worktrees: StorageWorktree[] }) {
  // The paths whose removal panel is open. Such a row stays among the
  // leftovers even when a refusal's refresh finds a session in it: moving it
  // to the other group would mount it anew there, dropping the panel, the
  // refusal it shows, and the focus.
  const [held, setHeld] = useState<ReadonlySet<string>>(() => new Set());
  const hold = useCallback((path: string, holding: boolean) => {
    setHeld((current) => {
      if (current.has(path) === holding) {
        return current;
      }
      const next = new Set(current);
      if (holding) {
        next.add(path);
      } else {
        next.delete(path);
      }
      return next;
    });
  }, []);
  const sorted = newestFirst(worktrees);
  const isLeftover = (worktree: StorageWorktree) => !worktree.in_use || held.has(worktree.path);
  const groups = [
    {
      key: 'leftover',
      heading: 'Leftovers',
      worktrees: sorted.filter(isLeftover),
    },
    {
      key: 'in-use',
      heading: 'In use by a listed session',
      worktrees: sorted.filter((worktree) => !isLeftover(worktree)),
    },
  ];
  return (
    <div className="flex flex-col gap-3" data-testid="storage-worktree-list">
      {groups
        .filter((group) => group.worktrees.length > 0)
        .map((group) => (
          <section
            key={group.key}
            className="flex flex-col gap-2"
            aria-labelledby={`storage-worktrees-${group.key}`}
            data-testid={`storage-worktrees-${group.key}`}
          >
            <h5
              id={`storage-worktrees-${group.key}`}
              className="text-caption font-medium text-fg-muted"
            >
              {group.heading}{' '}
              <span className="tabular-nums text-fg-subtle">({group.worktrees.length})</span>
            </h5>
            <ul className="flex flex-col gap-2">
              {group.worktrees.map((worktree) => (
                <WorktreeRow key={worktree.path} worktree={worktree} hold={hold} />
              ))}
            </ul>
          </section>
        ))}
    </div>
  );
}

/** How long ago the worktree's directory last changed, the time in a tooltip. */
function Age({ modifiedAt }: { modifiedAt: string }) {
  const then = new Date(modifiedAt);
  return (
    <span data-testid="storage-worktree-age" className="shrink-0" title={exactTime(then)}>
      Modified {formatAge(then, new Date())}
    </span>
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

function WorktreeRow({
  worktree,
  hold,
}: {
  worktree: StorageWorktree;
  /** Keeps the row in its group while its removal panel is open. */
  hold: (path: string, holding: boolean) => void;
}) {
  const client = useApiClient();
  const remove = useRemoveStorageWorktreeMutation(client);
  const [confirming, setConfirming] = useState(false);
  const [typed, setTyped] = useState('');
  useEffect(() => {
    if (!confirming) {
      return;
    }
    hold(worktree.path, true);
    return () => hold(worktree.path, false);
  }, [confirming, hold, worktree.path]);
  // Wraps the Remove button alone: the row's first control is its Copy button.
  const openerRef = useRef<HTMLSpanElement>(null);
  const state = stateOf(worktree);
  const removable = !worktree.in_use;
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
        {removable && !confirming && (
          <span ref={openerRef} className="contents">
            <Button
              size="sm"
              variant="ghost"
              className="shrink-0"
              onClick={() => setConfirming(true)}
              aria-label={`Remove worktree ${name}`}
            >
              Remove
            </Button>
          </span>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-x-3 text-caption text-fg-muted">
        <span data-testid="storage-worktree-state" className={state === 'dirty' ? 'text-warning' : undefined}>
          {STATE_TEXT[state]}
        </span>
        {worktree.modified_at !== null && <Age modifiedAt={worktree.modified_at} />}
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
          confirmDisabled={!removable || (force && typed !== name)}
          pending={remove.isPending}
          error={remove.isError ? refusalText(remove.error) : null}
          testId="storage-worktree-confirm"
          returnFocusTo={openerRef}
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
