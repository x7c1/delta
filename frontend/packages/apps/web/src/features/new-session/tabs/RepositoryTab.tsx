import { useCallback, useEffect, useMemo, useState } from 'react';
import { useHomeDirQuery, useRepositoriesQuery } from '@delta/api-client';
import { displayBranch } from '@delta/model';
import type { RepositoryEntry } from '@delta/wire-gen';
import { Spinner, cn } from '@delta/ui-kit';
import { useApiClient } from '../../../data/apiContext';
import { useComposerStore } from '../../../store/composerStore';
import { displayPath } from '../../../utils/displayPath';

/**
 * Identity-key prefix the picker uses to flag a "this is a path, not an
 * origin URL" repository entry. Backend returns the absolute clone path as
 * the identity key whenever `origin` is unset; we surface that with a
 * subtle pill in the row so it is clear why this entry is not bundled.
 */
const PATH_KEY_PREFIX = '/';

/**
 * The Repository tab: registered repositories (origin-deduplicated),
 * recency-ordered. Selecting the tab auto-highlights the first (most-recent)
 * repo AND auto-picks its `recently_used_clone_path` (falling back to the
 * first clone) into `composerStore.newSessionWorkdir`, so the panel mounts
 * in a fully-selected state — both a repo row and a clone row read as
 * active — and the user can press Send without first clicking a clone.
 * Clicking a different Repository row replaces the picked clone with that
 * repo's default; picking a different clone from the same repo overrides
 * the default.
 *
 * Per-clone state — branch, launch options, worktree opt-in — will pre-fill
 * once per-session persistence lands. Today the existing override UI under
 * the composer card remains the authoritative knobs.
 */
export function RepositoryTab() {
  const client = useApiClient();
  const repositoriesQuery = useRepositoriesQuery(client, true);
  const setSelected = useComposerStore((state) => state.setNewSessionWorkdir);
  const selectedPath = useComposerStore((state) => state.newSessionWorkdir);
  const setNewSessionSelectedPrUrl = useComposerStore(
    (state) => state.setNewSessionSelectedPrUrl,
  );
  // Display-only home abbreviation. The stored `newSessionWorkdir` stays
  // the absolute path.
  const home = useHomeDirQuery(client, true).data?.path ?? null;

  // Memoised so the empty fallback does not hand a fresh array identity to
  // the effects and memos below on every render while the query is pending.
  const repositories = useMemo(
    () => repositoriesQuery.data?.repositories ?? [],
    [repositoriesQuery.data],
  );

  // Selected repo, by identity_key — drives the expanded clone list.
  // Defaults to the first (most-recent) repo so the panel reads as "pick a
  // clone" out of the gate rather than "first pick a repo, then pick a
  // clone".
  const [selectedRepoKey, setSelectedRepoKey] = useState<string | null>(null);
  useEffect(() => {
    if (repositories.length === 0) {
      return;
    }
    const firstKey = repositories[0].identity_key;
    // Updater form on purpose: React evaluates it against the state as of the
    // flush, not as of the render that queued this effect. The repo rows are
    // clickable the moment they paint, while this seeding effect may still be
    // queued (React defers its passive flush past a commit that overruns the
    // frame budget), so a snapshot-based `selectedRepoKey === null` test would
    // see the pre-click value and snap the selection back to the first repo.
    // Returning `current` unchanged makes the seed a no-op with no re-render.
    setSelectedRepoKey((current) => current ?? firstKey);
  }, [repositories]);

  const selectedRepo = useMemo(
    () =>
      repositories.find((repo) => repo.identity_key === selectedRepoKey) ?? null,
    [repositories, selectedRepoKey],
  );

  // Auto-pick a clone path for `repo` into the composer store. Preference:
  //   1. `recently_used_clone_path` (when it is actually in the clones list)
  //   2. the first clone
  // Skipped when the current selection already belongs to this repo — that
  // means the user explicitly picked a different clone of the same repo and
  // we must not stomp it (e.g. re-clicking the same Repo row, or the
  // mount-time effect firing after a clone was already picked).
  //
  // That guard reads the live store rather than the render-time `selectedPath`
  // snapshot, because this also runs from a passive effect. React defers its
  // passive flush to a later task whenever a commit overruns the scheduler's
  // frame budget, so the clone rows can already be on screen and clicked while
  // the effect that seeds the default is still queued. A captured
  // `selectedPath` would then be evaluated as it stood *before* that click and
  // overwrite the explicit pick with the repo default. Reading at call time
  // makes the outcome independent of when the flush lands, and keeps this
  // callback identity-stable so the seeding effect stops re-running on every
  // selection change.
  const autoPickClone = useCallback(
    (repo: RepositoryEntry) => {
      if (repo.clones.length === 0) {
        return;
      }
      const currentPath = useComposerStore.getState().newSessionWorkdir;
      const alreadyPicked = repo.clones.some(
        (clone) => clone.path === currentPath,
      );
      if (alreadyPicked) {
        return;
      }
      const recent = repo.recently_used_clone_path;
      const recentMatches = repo.clones.some((clone) => clone.path === recent);
      const next = recentMatches ? recent : repo.clones[0].path;
      setSelected(next);
    },
    [setSelected],
  );

  // Mount-time (and selected-repo-change) auto-pick. When the Repository tab
  // first shows or the selected repo changes, write a sensible default clone
  // path into the composer store so the clone row also reads as active and
  // Send is immediately available. The `alreadyPicked` guard inside
  // `autoPickClone` makes this idempotent — if a user has already clicked a
  // clone of the selected repo, this effect is a no-op.
  useEffect(() => {
    if (!selectedRepo) {
      return;
    }
    autoPickClone(selectedRepo);
  }, [selectedRepo, autoPickClone]);

  // Clicking a Repository row both highlights it and routes through the same
  // auto-pick so the picked clone follows the picked repo. Idempotent for a
  // same-repo reclick — `autoPickClone` short-circuits when the existing
  // selection already belongs to the clicked repo.
  const handleRepoClick = useCallback(
    (repo: RepositoryEntry) => {
      setSelectedRepoKey(repo.identity_key);
      autoPickClone(repo);
    },
    [autoPickClone],
  );

  if (repositoriesQuery.isLoading) {
    return <Spinner label="Loading repositories…" />;
  }

  if (repositoriesQuery.isError) {
    return (
      <div
        className="rounded border border-danger/30 bg-danger/10 px-3 py-2 text-caption text-danger"
        data-testid="repository-tab-error"
        role="alert"
      >
        Could not load repositories. The Directory tab still works.
      </div>
    );
  }

  if (repositories.length === 0) {
    return (
      <div
        className="rounded-md border border-dashed border-border-default bg-surface-elevated px-4 py-6 text-secondary text-fg-muted"
        data-testid="repository-tab-empty"
      >
        <p className="font-medium text-fg">No repositories yet.</p>
        <p className="mt-1 text-caption text-fg-subtle">
          Start a session via the Directory tab to register your first repo.
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-3" data-testid="repository-tab">
      <section className="space-y-1" data-testid="repository-tab-repos">
        <h3 className="text-caption font-semibold uppercase tracking-wide text-fg-subtle">
          Repositories
        </h3>
        <ul className="space-y-0.5">
          {repositories.map((repo) => (
            <li key={repo.identity_key}>
              <RepoRow
                repo={repo}
                isSelected={selectedRepoKey === repo.identity_key}
                onSelect={() => handleRepoClick(repo)}
              />
            </li>
          ))}
        </ul>
      </section>

      {selectedRepo && (
        <section className="space-y-1" data-testid="repository-tab-clones">
          <h3 className="text-caption font-semibold uppercase tracking-wide text-fg-subtle">
            Clones
          </h3>
          <ul className="space-y-0.5">
            {selectedRepo.clones.map((clone) => {
              const isPicked = selectedPath === clone.path;
              return (
                <li key={clone.path}>
                  <button
                    type="button"
                    onClick={() => {
                      // Picking a clone here is mutually exclusive with the PR
                      // tab's "selected row" highlight — clear it so at most
                      // one row reads as the active pick across the three tabs.
                      setSelected(clone.path);
                      setNewSessionSelectedPrUrl(null);
                    }}
                    aria-pressed={isPicked}
                    className={cn(
                      'flex w-full min-w-0 items-center justify-between gap-3 rounded px-2 py-1.5 text-left text-caption hover:bg-surface-elevated-hover',
                      isPicked
                        ? 'bg-accent/10 text-accent ring-1 ring-accent-disabled'
                        : 'text-fg',
                    )}
                    title={clone.path}
                    data-testid="repository-tab-clone-row"
                    data-default={
                      clone.path === selectedRepo.recently_used_clone_path
                        ? 'true'
                        : 'false'
                    }
                  >
                    <span className="min-w-0 flex-1 truncate font-mono text-code">
                      {displayPath(clone.path, home)}
                    </span>
                    {clone.last_branch && (
                      <span
                        className="shrink-0 rounded bg-surface-elevated px-1.5 py-0.5 font-mono text-code text-fg-subtle"
                        title={clone.last_branch}
                      >
                        {displayBranch(clone.last_branch)}
                      </span>
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
        </section>
      )}
    </div>
  );
}

interface RepoRowProps {
  repo: RepositoryEntry;
  isSelected: boolean;
  onSelect: () => void;
}

function RepoRow({ repo, isSelected, onSelect }: RepoRowProps) {
  // The identity_key falls back to the path when `origin` was unset — surface
  // that with a subtle pill so the entry is not mistaken for one of the
  // origin-deduplicated repos. A path-shaped key always starts with `/`.
  const isPathKey = repo.identity_key.startsWith(PATH_KEY_PREFIX);
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={isSelected}
      className={cn(
        'flex w-full min-w-0 items-center justify-between gap-3 rounded px-2 py-1.5 text-left hover:bg-surface-elevated-hover',
        isSelected
          ? 'bg-accent/10 text-accent ring-1 ring-accent-disabled'
          : 'text-fg',
      )}
      data-testid="repository-tab-repo-row"
    >
      <div className="min-w-0 flex-1">
        <div className="truncate text-caption font-medium">{repo.display_name}</div>
        <div className="truncate font-mono text-code text-fg-subtle">
          {repo.identity_key}
        </div>
      </div>
      {isPathKey && (
        <span className="shrink-0 rounded bg-surface-elevated px-1.5 py-0.5 text-caption uppercase tracking-wide text-fg-subtle">
          local
        </span>
      )}
      {repo.clones.length > 1 && (
        <span className="shrink-0 rounded bg-surface-elevated px-1.5 py-0.5 text-caption text-fg-muted">
          {repo.clones.length} clones
        </span>
      )}
    </button>
  );
}
